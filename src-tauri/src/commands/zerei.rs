//! Zerei! — acompanhamento de jogos (zerados, jogando, pausados, lista de desejos...), independente da biblioteca principal.
//!
//! - Pesquisa por nome no IGDB (credenciais em Configurações → Fontes de dados) e no RAWG (chave em
//!   Pesquisa de imagens). Sem nenhuma das duas, usa a pesquisa pública (Steam/Wikipédia/libretro).
//! - Capas, logos e artes vêm do SteamGridDB (chave opcional), com a capa do IGDB/fundo do RAWG de reserva.
//! - Cada registro pertence a um perfil e tem um identificador único (`uid`, UUID v4) que não muda nunca:
//!   é ele que a importação de CSV e uma futura sincronização (Google Sheets / NEXSTAGE CLOUD) usam para
//!   reconhecer o mesmo registro em outro lugar. `updated_at` marca a última alteração e as exclusões ficam
//!   em `zerei_deleted` (uid + data), para que uma sincronização saiba o que apagar do outro lado.
//! - Duplicados: o mesmo jogo (nome normalizado ou mesmo ID do IGDB/RAWG) na mesma plataforma não é
//!   cadastrado duas vezes — a menos que o usuário peça explicitamente um novo registro de conclusão
//!   (rejogou/zerou de novo), que ganha o número seguinte (`run`).
//! - As imagens escolhidas são baixadas para a pasta de imagens (`images.rs`): funcionam offline e entram
//!   no backup junto com o banco. O endereço original da capa fica em `cover_url` (vai para o CSV).

use super::info;
use super::webshots::{self, SourceRef, WebShot};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::sources::{self, igdb, Source};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, State};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS zerei_entries (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    uid              TEXT NOT NULL UNIQUE,
    profile_id       INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    title            TEXT NOT NULL,
    title_key        TEXT NOT NULL,
    platform         TEXT NOT NULL COLLATE NOCASE,
    run              INTEGER NOT NULL DEFAULT 1,
    cover            TEXT,
    cover_url        TEXT,
    logo             TEXT,
    background       TEXT,
    year             INTEGER,
    platforms        TEXT NOT NULL DEFAULT '[]',
    igdb_id          INTEGER,
    rawg_id          INTEGER,
    sgdb_id          INTEGER,
    status           TEXT NOT NULL DEFAULT 'not_started',
    completion_type  TEXT,
    difficulty       TEXT,
    started_at       TEXT,
    completed_at     TEXT,
    hours            REAL,
    rating           REAL,
    notes            TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (profile_id, title_key, platform, run)
);
CREATE INDEX IF NOT EXISTS idx_zerei_profile ON zerei_entries(profile_id, status);
-- Exclusões lembradas para uma futura sincronização (o outro lado também precisa apagar).
CREATE TABLE IF NOT EXISTS zerei_deleted (
    uid         TEXT PRIMARY KEY,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    deleted_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_zerei_updated ON zerei_entries(profile_id, updated_at);
-- Estado de cada destino de sincronização por perfil ('sheets' = Google Sheets, 'cloud' = NEXSTAGE CLOUD).
-- `remote_ref` = identificador do outro lado (ex.: ID da planilha); credenciais NÃO ficam aqui (cofre/DPAPI).
CREATE TABLE IF NOT EXISTS zerei_sync (
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    target        TEXT NOT NULL,
    remote_ref    TEXT,
    last_sync_at  TEXT,
    PRIMARY KEY (profile_id, target)
);
"#;

/// Prefixo do erro de duplicado (o frontend oferece "registrar nova conclusão").
pub const DUPLICATE: &str = "ZEREI_DUPLICATE::";
const STATUSES: [&str; 5] = ["not_started", "playing", "paused", "completed", "wishlist"];
const COMPLETION: [&str; 2] = ["campaign", "full"];
const DIFFICULTY: [&str; 5] = ["very_easy", "easy", "normal", "hard", "very_hard"];
const RAWG: &str = "https://api.rawg.io/api";
const SGDB: &str = "https://www.steamgriddb.com/api/v2";

// ============================================================================================= tipos

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZereiHit {
    pub title: String,
    pub year: Option<i32>,
    pub platforms: Vec<String>,
    pub cover: Option<String>,
    /// Arte horizontal (fundo do RAWG), quando houver.
    pub background: Option<String>,
    pub igdb_id: Option<i64>,
    pub rawg_id: Option<i64>,
    /// "IGDB", "RAWG", "Steam"...
    pub sources: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZereiSearch {
    pub hits: Vec<ZereiHit>,
    /// Nenhuma das fontes principais (IGDB/RAWG) está configurada: resultados da pesquisa pública.
    pub fallback: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZereiArt {
    pub sgdb_id: Option<i64>,
    pub covers: Vec<WebShot>,
    pub logos: Vec<WebShot>,
    pub backgrounds: Vec<WebShot>,
    /// Chave do SteamGridDB configurada.
    pub sgdb: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZereiEntry {
    pub id: i64,
    pub uid: String,
    pub title: String,
    pub platform: String,
    pub run: i64,
    pub cover: Option<String>,
    pub logo: Option<String>,
    pub background: Option<String>,
    pub year: Option<i64>,
    pub platforms: Vec<String>,
    pub igdb_id: Option<i64>,
    pub rawg_id: Option<i64>,
    pub sgdb_id: Option<i64>,
    pub status: String,
    pub completion_type: Option<String>,
    pub difficulty: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub hours: Option<f64>,
    pub rating: Option<f64>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ZereiInput {
    pub title: String,
    pub platform: String,
    pub cover: Option<String>,
    pub logo: Option<String>,
    pub background: Option<String>,
    pub year: Option<i64>,
    pub platforms: Vec<String>,
    pub igdb_id: Option<i64>,
    pub rawg_id: Option<i64>,
    pub sgdb_id: Option<i64>,
    pub status: String,
    pub completion_type: Option<String>,
    pub difficulty: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub hours: Option<f64>,
    pub rating: Option<f64>,
    pub notes: String,
}

// ============================================================================================= regras

/// Identificador único do registro (UUID v4).
pub fn new_uid() -> String {
    let mut b = [0u8; 16];
    if getrandom::getrandom(&mut b).is_err() {
        // sem gerador do sistema (improvável): relógio + contador ainda dão um valor único local
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = n.to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

fn valid_uid(s: &str) -> bool {
    s.len() == 36 && s.chars().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// Chave de comparação do nome: minúsculas, sem acentos/pontuação ("Marvel's Spider-Man" = "marvels spider man").
pub fn title_key(s: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in s.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '\'' | '’' | '`' | '™' | '®' | '©' => continue,
            c => c,
        };
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        } else {
            space = true;
        }
    }
    out
}

/// Nome de plataforma vindo do IGDB/RAWG → nome oficial do NEXSTAGE quando houver equivalente.
pub fn platform_name(s: &str) -> String {
    let t = s.trim();
    let base = t.split(" (").next().unwrap_or(t).trim(); // "PC (Microsoft Windows)" → "PC"
    if let Some(p) = super::games::canonical_platform(base).or_else(|| super::games::canonical_platform(t)) {
        return p.to_string();
    }
    match base.to_lowercase().as_str() {
        "nintendo entertainment system" => "Nintendinho".into(),
        "super nintendo entertainment system" => String::from("Super Nintendo"),
        "xbox series x|s" | "xbox series s/x" => String::from("Xbox Series"),
        "playstation vita" => String::from("Ps Vita"),
        _ => t.to_string(),
    }
}

fn uniq_platforms(list: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in list.into_iter().map(|p| platform_name(&p)).filter(|p| !p.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&p)) {
            out.push(p);
        }
    }
    out
}

/// Data "AAAA-MM-DD" válida (mês/dia dentro do calendário).
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (s[..4].parse::<i32>(), s[5..7].parse::<u32>(), s[8..].parse::<u32>()) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1970..=2100).contains(&y) && (1..=days).contains(&d)
}

fn opt_key(v: Option<String>, allowed: &[&str], msg: &str) -> AppResult<Option<String>> {
    match v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if allowed.contains(&s.as_str()) => Ok(Some(s)),
        Some(_) => Err(AppError::Validation(msg.into())),
    }
}

fn opt_date(v: Option<String>, msg: &str) -> AppResult<Option<String>> {
    let v = v.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    if let Some(d) = &v {
        if !valid_date(d) {
            return Err(AppError::Validation(msg.into()));
        }
    }
    Ok(v)
}

/// Valida e normaliza a entrada (sem imagens).
pub fn normalize(mut i: ZereiInput) -> AppResult<ZereiInput> {
    i.title = i.title.trim().chars().take(200).collect();
    i.platform = i.platform.trim().chars().take(60).collect();
    if title_key(&i.title).is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    if i.platform.is_empty() {
        return Err(AppError::Validation("Escolha a plataforma.".into()));
    }
    if !STATUSES.contains(&i.status.as_str()) {
        return Err(AppError::Validation("Status inválido.".into()));
    }
    i.completion_type = opt_key(i.completion_type, &COMPLETION, "Tipo de conclusão inválido.")?;
    i.difficulty = opt_key(i.difficulty, &DIFFICULTY, "Dificuldade inválida.")?;
    i.started_at = opt_date(i.started_at, "Data de início inválida.")?;
    i.completed_at = opt_date(i.completed_at, "Data em que zerou inválida.")?;
    if let (Some(s), Some(c)) = (&i.started_at, &i.completed_at) {
        if s > c {
            return Err(AppError::Validation("A data de início é posterior à data em que zerou.".into()));
        }
    }
    if let Some(h) = i.hours {
        if !h.is_finite() || !(0.0..=100_000.0).contains(&h) {
            return Err(AppError::Validation("Tempo jogado inválido.".into()));
        }
        i.hours = Some((h * 10.0).round() / 10.0);
    }
    if let Some(r) = i.rating {
        if !r.is_finite() || !(0.0..=10.0).contains(&r) {
            return Err(AppError::Validation("A nota deve ser de 0 a 10.".into()));
        }
        i.rating = Some((r * 10.0).round() / 10.0);
    }
    i.notes = i.notes.trim().chars().take(5000).collect();
    i.platforms = uniq_platforms(std::mem::take(&mut i.platforms)).into_iter().take(40).collect();
    i.year = i.year.filter(|y| (1950..=2100).contains(y));
    Ok(i)
}

fn row(r: &Row) -> rusqlite::Result<ZereiEntry> {
    let plats: String = r.get("platforms")?;
    Ok(ZereiEntry {
        id: r.get("id")?,
        uid: r.get("uid")?,
        title: r.get("title")?,
        platform: r.get("platform")?,
        run: r.get("run")?,
        // Arquivo local apagado (ex.: limpeza de uma versão antiga que não conhecia o Zerei!): a capa
        // volta para o endereço de origem (ao salvar o registro ela é baixada de novo); logo e fundo
        // ficam vazios em vez de quebrados.
        cover: crate::images::resolve_existing(r.get("cover")?).or(r.get("cover_url")?),
        logo: crate::images::resolve_existing(r.get("logo")?),
        background: crate::images::resolve_existing(r.get("background")?),
        year: r.get("year")?,
        platforms: serde_json::from_str(&plats).unwrap_or_default(),
        igdb_id: r.get("igdb_id")?,
        rawg_id: r.get("rawg_id")?,
        sgdb_id: r.get("sgdb_id")?,
        status: r.get("status")?,
        completion_type: r.get("completion_type")?,
        difficulty: r.get("difficulty")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        hours: r.get("hours")?,
        rating: r.get("rating")?,
        notes: r.get("notes")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

fn get(conn: &Connection, profile_id: i64, id: i64) -> AppResult<ZereiEntry> {
    conn.query_row("SELECT * FROM zerei_entries WHERE id=?1 AND profile_id=?2", params![id, profile_id], row)
        .optional()?
        .ok_or_else(|| AppError::NotFound("Registro não encontrado.".into()))
}

/// Registros do mesmo jogo na mesma plataforma (exceto `skip`): mesmo nome normalizado ou mesmo ID externo.
/// Devolve (id, run) em ordem de conclusão.
pub fn same_game(conn: &Connection, profile_id: i64, i: &ZereiInput, skip: Option<i64>) -> AppResult<Vec<(i64, i64)>> {
    let mut st = conn.prepare(
        "SELECT id, run FROM zerei_entries
         WHERE profile_id=?1 AND platform=?2 AND id<>?3
           AND (title_key=?4 OR (?5 IS NOT NULL AND igdb_id=?5) OR (?6 IS NOT NULL AND rawg_id=?6))
         ORDER BY run",
    )?;
    let r = st
        .query_map(params![profile_id, i.platform, skip.unwrap_or(0), title_key(&i.title), i.igdb_id, i.rawg_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(r)
}

/// Próximo número de conclusão livre para o jogo/plataforma (considera também a chave do nome).
fn next_run(conn: &Connection, profile_id: i64, i: &ZereiInput, dups: &[(i64, i64)]) -> AppResult<i64> {
    let by_key: i64 = conn.query_row(
        "SELECT COALESCE(MAX(run),0) FROM zerei_entries WHERE profile_id=?1 AND platform=?2 AND title_key=?3",
        params![profile_id, i.platform, title_key(&i.title)],
        |r| r.get(0),
    )?;
    Ok(dups.iter().map(|d| d.1).max().unwrap_or(0).max(by_key) + 1)
}

fn dup_error(dups: &[(i64, i64)]) -> AppError {
    AppError::Validation(format!("{DUPLICATE}{}::{}", dups[0].0, dups.len()))
}

fn insert(conn: &Connection, profile_id: i64, i: &ZereiInput, run: i64, uid: &str, cover_url: Option<&str>) -> AppResult<i64> {
    let plats = serde_json::to_string(&i.platforms).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO zerei_entries(uid,profile_id,title,title_key,platform,run,cover,cover_url,logo,background,year,platforms,igdb_id,rawg_id,sgdb_id,
           status,completion_type,difficulty,started_at,completed_at,hours,rating,notes)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            uid, profile_id, i.title, title_key(&i.title), i.platform, run, i.cover, cover_url, i.logo, i.background, i.year, plats,
            i.igdb_id, i.rawg_id, i.sgdb_id, i.status, i.completion_type, i.difficulty, i.started_at, i.completed_at, i.hours, i.rating, i.notes
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn update(conn: &Connection, profile_id: i64, id: i64, i: &ZereiInput, run: i64, cover_url: Option<&str>) -> AppResult<()> {
    let plats = serde_json::to_string(&i.platforms).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "UPDATE zerei_entries SET title=?3,title_key=?4,platform=?5,run=?6,cover=?7,cover_url=?8,logo=?9,background=?10,year=?11,platforms=?12,
           igdb_id=?13,rawg_id=?14,sgdb_id=?15,status=?16,completion_type=?17,difficulty=?18,started_at=?19,completed_at=?20,hours=?21,rating=?22,
           notes=?23,updated_at=datetime('now')
         WHERE id=?1 AND profile_id=?2",
        params![
            id, profile_id, i.title, title_key(&i.title), i.platform, run, i.cover, cover_url, i.logo, i.background, i.year, plats,
            i.igdb_id, i.rawg_id, i.sgdb_id, i.status, i.completion_type, i.difficulty, i.started_at, i.completed_at, i.hours, i.rating, i.notes
        ],
    )?;
    Ok(())
}

/// Grava (já com as imagens resolvidas). `new_run` = o usuário confirmou outro registro de conclusão.
pub fn write(conn: &Connection, profile_id: i64, id: Option<i64>, i: &ZereiInput, new_run: bool, cover_url: Option<&str>) -> AppResult<ZereiEntry> {
    let dups = same_game(conn, profile_id, i, id)?;
    match id {
        None => {
            if !dups.is_empty() && !new_run {
                return Err(dup_error(&dups));
            }
            let run = if dups.is_empty() { 1 } else { next_run(conn, profile_id, i, &dups)? };
            let id = insert(conn, profile_id, i, run, &new_uid(), cover_url)?;
            get(conn, profile_id, id)
        }
        Some(id) => {
            let old = get(conn, profile_id, id)?;
            let moved = title_key(&old.title) != title_key(&i.title) || !old.platform.eq_ignore_ascii_case(&i.platform);
            let run = if !moved {
                old.run
            } else if dups.is_empty() {
                1
            } else if new_run {
                next_run(conn, profile_id, i, &dups)?
            } else {
                return Err(dup_error(&dups));
            };
            update(conn, profile_id, id, i, run, cover_url)?;
            get(conn, profile_id, id)
        }
    }
}

// ============================================================================================= biblioteca

/// Liga registros do Zerei! a jogos da biblioteca (`game_id`, só local: não vai para o CSV/sincronização).
/// A coluna é acrescentada aqui (e não no `SCHEMA`) para que a conversão das marcas antigas rode uma vez,
/// inclusive em bancos que ganham a tabela do Zerei! agora: o antigo "Marcar como concluído" da
/// biblioteca vira um registro "zerado" no Zerei!.
pub fn migrate(conn: &Connection) -> AppResult<()> {
    let has: bool = conn.prepare("SELECT 1 FROM pragma_table_info('zerei_entries') WHERE name='game_id'")?.exists([])?;
    if !has {
        conn.execute_batch("ALTER TABLE zerei_entries ADD COLUMN game_id INTEGER REFERENCES games(id) ON DELETE SET NULL;")?;
        let old: Vec<(i64, i64, Option<String>)> = conn
            .prepare("SELECT profile_id, game_id, completed_at FROM profile_games WHERE completed=1")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (profile_id, game_id, at) in old {
            let day = at.and_then(|s| s.get(..10).map(String::from)).filter(|d| valid_date(d));
            // um jogo com dados que o Zerei! não aceita não impede a abertura do banco
            let _ = mark_game(conn, profile_id, game_id, day);
        }
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_zerei_game ON zerei_entries(game_id);")?;
    Ok(())
}

/// "Marcar como zerado" (página do jogo): o jogo da biblioteca entra no Zerei! como zerado. Se já houver
/// registro do mesmo jogo (ligado a ele ou com o mesmo nome/plataforma), ele é marcado como zerado e
/// ligado ao jogo, mantendo o que o usuário já preencheu; senão, um registro novo é criado com nome,
/// plataforma, imagens e o tempo jogado no NEXSTAGE. Devolve o id do registro.
pub fn mark_game(conn: &Connection, profile_id: i64, game_id: i64, day: Option<String>) -> AppResult<i64> {
    let (title, platform, cover, logo, background, secs): (String, String, Option<String>, Option<String>, Option<String>, i64) = conn
        .query_row(
            "SELECT g.title, g.platform, g.cover, g.logo, g.background, pg.playtime_seconds
             FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1 AND pg.game_id=?2",
            params![profile_id, game_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Jogo não encontrado na biblioteca.".into()))?;
    let day = match day {
        Some(d) => d,
        None => conn.query_row("SELECT date('now','localtime')", [], |r| r.get(0))?,
    };
    let hours = (secs > 0).then(|| (secs as f64 / 360.0).round() / 10.0);
    let input = normalize(ZereiInput {
        title,
        platform: platform_name(&platform),
        cover: cover.clone(),
        logo: logo.clone(),
        background: background.clone(),
        status: "completed".into(),
        completion_type: Some("campaign".into()),
        completed_at: Some(day.clone()),
        hours,
        ..Default::default()
    })?;
    let linked: Option<i64> = conn
        .query_row("SELECT id FROM zerei_entries WHERE profile_id=?1 AND game_id=?2 ORDER BY run DESC LIMIT 1", params![profile_id, game_id], |r| r.get(0))
        .optional()?;
    let target = match linked {
        Some(id) => Some(id),
        None => same_game(conn, profile_id, &input, None)?.last().map(|d| d.0),
    };
    match target {
        Some(id) => {
            conn.execute(
                "UPDATE zerei_entries SET game_id=?3, status='completed', completion_type=COALESCE(completion_type,'campaign'),
                   completed_at=COALESCE(completed_at,?4), hours=COALESCE(hours,?5), cover=COALESCE(cover,?6), logo=COALESCE(logo,?7),
                   background=COALESCE(background,?8), updated_at=datetime('now')
                 WHERE id=?1 AND profile_id=?2",
                params![id, profile_id, game_id, day, hours, cover, logo, background],
            )?;
            Ok(id)
        }
        None => {
            let url = cover.as_deref().filter(|c| c.starts_with("https://"));
            let id = insert(conn, profile_id, &input, 1, &new_uid(), url)?;
            conn.execute("UPDATE zerei_entries SET game_id=?2 WHERE id=?1", params![id, game_id])?;
            Ok(id)
        }
    }
}

#[tauri::command]
pub fn zerei_mark_game(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<ZereiEntry> {
    let conn = db.conn()?;
    let id = mark_game(&conn, profile_id, game_id, None)?;
    get(&conn, profile_id, id)
}

// ============================================================================================= rede

fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION"), " (desktop game launcher)"))
        .build()
        .map_err(|_| AppError::Other("Falha de rede.".into()))
}

async fn get_json(req: reqwest::RequestBuilder) -> Option<Value> {
    req.send().await.ok()?.error_for_status().ok()?.json().await.ok()
}

async fn rawg_search(c: reqwest::Client, key: String, term: String) -> Vec<ZereiHit> {
    let req = c.get(format!("{RAWG}/games")).query(&[("key", key.as_str()), ("search", term.as_str()), ("page_size", "15"), ("search_precise", "true")]);
    let Some(v) = get_json(req).await else { return vec![] };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["esrb_rating"]["slug"] != "adults-only")
        .filter_map(|g| {
            Some(ZereiHit {
                title: g["name"].as_str()?.to_string(),
                year: g["released"].as_str().and_then(webshots::year_in),
                platforms: uniq_platforms(g["platforms"].as_array().into_iter().flatten().filter_map(|p| p["platform"]["name"].as_str().map(String::from))),
                cover: None,
                background: g["background_image"].as_str().map(String::from),
                igdb_id: None,
                rawg_id: g["id"].as_i64(),
                sources: vec!["RAWG".into()],
            })
        })
        .collect()
}

/// Junta o mesmo jogo do IGDB e do RAWG (nome equivalente e mesmo ano) e ordena pelo nome mais parecido.
pub fn merge(igdb: Vec<ZereiHit>, rawg: Vec<ZereiHit>, term: &str) -> Vec<ZereiHit> {
    let mut out = igdb;
    for r in rawg {
        let found = out.iter_mut().find(|h| {
            h.rawg_id.is_none() && info::name_score(&h.title, &r.title) >= 1.0 && (h.year.is_none() || r.year.is_none() || h.year == r.year)
        });
        match found {
            Some(h) => {
                h.rawg_id = r.rawg_id;
                h.background = h.background.take().or(r.background);
                h.year = h.year.or(r.year);
                h.platforms = uniq_platforms(h.platforms.drain(..).chain(r.platforms));
                h.sources.push("RAWG".into());
            }
            None => out.push(r),
        }
    }
    let score = |h: &ZereiHit| -((info::name_score(term, &h.title) * 1000.0) as i64);
    out.sort_by_key(score); // estável: empates mantêm a ordem das fontes
    out.truncate(24);
    out
}

fn source_label(s: &SourceRef) -> String {
    match s.source.as_str() {
        "steam" => "Steam",
        "libretro" => "libretro",
        "wikipedia" => "Wikipédia",
        "steamgriddb" => "SteamGridDB",
        "rawg" => "RAWG",
        o => o,
    }
    .to_string()
}

/// Pesquisa o jogo pelo nome (nada é gravado).
#[tauri::command]
pub async fn zerei_search(app: AppHandle, db: State<'_, Db>, term: String) -> AppResult<ZereiSearch> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let creds = {
        let conn = db.conn()?;
        sources::active(&conn, Source::Igdb)?.and_then(|c| serde_json::from_str::<igdb::Creds>(&c).ok())
    };
    let rawg_key = crate::apikeys::get(&app)("rawg_key");
    if creds.is_none() && rawg_key.is_none() {
        // sem IGDB/RAWG: pesquisa pública (Steam, Wikipédia, libretro e SteamGridDB, se houver chave)
        let hits = webshots::search_games(app.clone(), term.clone(), String::new()).await.unwrap_or_default();
        let hits = hits
            .into_iter()
            .map(|h| {
                let mut srcs: Vec<String> = Vec::new();
                for s in h.sources.iter().map(source_label) {
                    if !srcs.contains(&s) {
                        srcs.push(s);
                    }
                }
                ZereiHit {
                    title: h.title,
                    year: h.year,
                    platforms: uniq_platforms(h.platform.map(|p| p.split(", ").map(String::from).collect::<Vec<_>>()).unwrap_or_default()),
                    cover: h.cover,
                    background: None,
                    igdb_id: None,
                    rawg_id: None,
                    sources: srcs,
                }
            })
            .collect();
        return Ok(ZereiSearch { hits, fallback: true });
    }
    let c = http()?;
    // as duas fontes em paralelo; uma que falha só não contribui
    let ig = creds.map(|cr| {
        let t = term.clone();
        tauri::async_runtime::spawn(async move { igdb::search_titles(&cr, &t, 15).await })
    });
    let rw = rawg_key.map(|k| tauri::async_runtime::spawn(rawg_search(c.clone(), k, term.clone())));
    let mut first_err = None;
    let igdb_hits: Vec<ZereiHit> = match ig {
        Some(h) => match h.await {
            Ok(Ok(list)) => list
                .into_iter()
                .map(|c| ZereiHit {
                    title: c.name,
                    year: c.year.map(|y| y as i32),
                    platforms: uniq_platforms(c.platforms),
                    cover: c.image.map(|u| u.replace("/t_cover_small/", "/t_cover_big/")),
                    background: None,
                    igdb_id: Some(c.id),
                    rawg_id: None,
                    sources: vec!["IGDB".into()],
                })
                .collect(),
            Ok(Err(e)) => {
                first_err = Some(e);
                vec![]
            }
            Err(_) => vec![],
        },
        None => vec![],
    };
    let rawg_hits = match rw {
        Some(h) => h.await.unwrap_or_default(),
        None => vec![],
    };
    let hits = merge(igdb_hits, rawg_hits, &term);
    if hits.is_empty() {
        if let Some(e) = first_err {
            return Err(e);
        }
    }
    Ok(ZereiSearch { hits, fallback: false })
}

fn sgdb(c: &reqwest::Client, key: &str, path: &str) -> reqwest::RequestBuilder {
    c.get(format!("{SGDB}{path}")).bearer_auth(key)
}

/// Jogo do SteamGridDB pelo nome: nome equivalente (preferindo o mesmo ano); senão o primeiro resultado.
async fn sgdb_find_id(c: &reqwest::Client, key: &str, title: &str, year: Option<i32>) -> Option<i64> {
    let v = get_json(sgdb(c, key, &format!("/search/autocomplete/{}", webshots::penc(title.trim())))).await;
    let list: Vec<(i64, String, Option<i32>)> = v
        .as_ref()
        .and_then(|v| v["data"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|g| Some((g["id"].as_i64()?, g["name"].as_str()?.to_string(), g["release_date"].as_i64().map(webshots::unix_year))))
        .collect();
    list.iter()
        .filter(|g| info::name_score(title, &g.1) >= 1.0)
        .max_by_key(|g| (year.is_some() && g.2 == year) as u8)
        .or(list.first())
        .map(|g| g.0)
}

/// Capas, logos e artes do jogo escolhido no SteamGridDB (o jogo de lá é achado pelo nome e ano).
#[tauri::command]
pub async fn zerei_artwork(app: AppHandle, title: String, year: Option<i32>, sgdb_id: Option<i64>) -> AppResult<ZereiArt> {
    let Some(key) = crate::apikeys::get(&app)("sgdb_key") else { return Ok(ZereiArt::default()) };
    let c = http()?;
    let id = match sgdb_id {
        Some(id) => Some(id),
        None => sgdb_find_id(&c, &key, &title, year).await,
    };
    let Some(id) = id else { return Ok(ZereiArt { sgdb: true, ..Default::default() }) };
    let page = Some(format!("https://www.steamgriddb.com/game/{id}"));
    let mut art = ZereiArt { sgdb_id: Some(id), sgdb: true, ..Default::default() };
    let reqs = [
        (format!("/grids/game/{id}?dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false"), "cover", 18usize),
        (format!("/logos/game/{id}?types=static&nsfw=false&humor=false"), "logo", 12),
        (format!("/heroes/game/{id}?types=static&nsfw=false&humor=false"), "background", 12),
    ];
    let handles: Vec<_> = reqs.iter().map(|(p, _, _)| tauri::async_runtime::spawn(get_json(sgdb(&c, &key, p)))).collect();
    for (h, (_, kind, max)) in handles.into_iter().zip(reqs.iter()) {
        let Ok(Some(v)) = h.await else { continue };
        for g in v["data"].as_array().into_iter().flatten().take(*max) {
            if let (Some(url), Some(th)) = (g["url"].as_str(), g["thumb"].as_str()) {
                let s = WebShot { url: url.into(), thumb: th.into(), source: "steamgriddb".into(), page: page.clone(), title: "SteamGridDB".into(), kind: (*kind).into() };
                match *kind {
                    "cover" => art.covers.push(s),
                    "logo" => art.logos.push(s),
                    _ => art.backgrounds.push(s),
                }
            }
        }
    }
    Ok(art)
}

/// Jogo do SteamGridDB para o usuário escolher antes das logos.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogoGame {
    pub id: i64,
    pub name: String,
    pub year: Option<i32>,
    pub cover: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogoGames {
    /// Chave do SteamGridDB configurada.
    pub sgdb: bool,
    pub games: Vec<LogoGame>,
}

/// Lista de jogos do SteamGridDB para um nome (escolha manual do jogo certo antes das logos).
/// Mesma pesquisa da "Pesquisa de imagens" (autocomplete + primeira capa de cada jogo).
#[tauri::command]
pub async fn sgdb_games(app: AppHandle, term: String) -> AppResult<LogoGames> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let Some(key) = crate::apikeys::get(&app)("sgdb_key") else { return Ok(LogoGames { sgdb: false, games: vec![] }) };
    let hits = webshots::sgdb_search(webshots::client()?, key, term).await;
    let games = hits
        .into_iter()
        .filter_map(|h| {
            let id = h.sources.iter().find(|s| s.source == "steamgriddb")?.id.parse().ok()?;
            Some(LogoGame { id, name: h.title, year: h.year, cover: h.cover })
        })
        .collect();
    Ok(LogoGames { sgdb: true, games })
}

// ============================================================================================= registros

#[tauri::command]
pub fn zerei_list(db: State<Db>, profile_id: i64) -> AppResult<Vec<ZereiEntry>> {
    let conn = db.conn()?;
    let mut st = conn.prepare("SELECT * FROM zerei_entries WHERE profile_id=?1 ORDER BY updated_at DESC, id DESC")?;
    let r = st.query_map([profile_id], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(r)
}

/// Imagem escolhida → arquivo local (offline e no backup). Se o download falhar, guarda o endereço.
async fn keep_image(v: Option<String>) -> AppResult<Option<String>> {
    let Some(v) = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else { return Ok(None) };
    if v.starts_with("https://") && webshots::allowed_image_url(&v) {
        if let Ok((bytes, mime)) = webshots::download_image(&v).await {
            if let Ok(r) = crate::images::store_bytes(&bytes, &mime) {
                return Ok(Some(r));
            }
        }
    }
    crate::images::store(Some(v))
}

/// Cadastra (`id` vazio) ou edita um registro. Duplicado → erro `ZEREI_DUPLICATE::<id>::<quantos>`,
/// e com `new_run` grava como um novo registro de conclusão.
#[tauri::command]
pub async fn zerei_save(db: State<'_, Db>, profile_id: i64, id: Option<i64>, input: ZereiInput, new_run: Option<bool>) -> AppResult<ZereiEntry> {
    let mut i = normalize(input)?;
    let new_run = new_run == Some(true);
    let old_cover: Option<(Option<String>, Option<String>)> = {
        // confere perfil e duplicado antes de baixar imagens
        let conn = db.conn()?;
        conn.query_row("SELECT 1 FROM profiles WHERE id=?1", [profile_id], |_| Ok(())).optional()?.ok_or_else(|| AppError::NotFound("Perfil não encontrado.".into()))?;
        if id.is_none() && !new_run {
            let dups = same_game(&conn, profile_id, &i, None)?;
            if !dups.is_empty() {
                return Err(dup_error(&dups));
            }
        }
        match id {
            Some(id) => conn.query_row("SELECT cover, cover_url FROM zerei_entries WHERE id=?1 AND profile_id=?2", params![id, profile_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?,
            None => None,
        }
    };
    let picked = i.cover.clone().filter(|c| c.starts_with("https://"));
    i.cover = keep_image(i.cover).await?;
    i.logo = keep_image(i.logo).await?;
    i.background = keep_image(i.background).await?;
    // endereço de origem da capa: o escolhido agora, ou o anterior se a capa não mudou
    let cover_url = picked.or_else(|| old_cover.filter(|(c, _)| *c == i.cover).and_then(|(_, u)| u));
    let conn = db.conn()?;
    write(&conn, profile_id, id, &i, new_run, cover_url.as_deref())
}

#[tauri::command]
pub fn zerei_delete(db: State<Db>, profile_id: i64, id: i64) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let uid: Option<String> = tx.query_row("SELECT uid FROM zerei_entries WHERE id=?1 AND profile_id=?2", params![id, profile_id], |r| r.get(0)).optional()?;
    let Some(uid) = uid else { return Err(AppError::NotFound("Registro não encontrado.".into())) };
    tx.execute("DELETE FROM zerei_entries WHERE id=?1", [id])?;
    tx.execute("INSERT OR REPLACE INTO zerei_deleted(uid,profile_id) VALUES(?1,?2)", params![uid, profile_id])?;
    tx.commit()?;
    Ok(())
}

// ============================================================================================= sincronização

/// Destinos de sincronização previstos.
pub const SYNC_TARGETS: [&str; 2] = ["sheets", "cloud"];

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZereiDeleted {
    pub uid: String,
    pub deleted_at: String,
}

/// Pacote de alterações para um sincronizador: o que mudou e o que foi apagado desde `since`.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZereiChanges {
    pub target: String,
    /// Último envio a este destino (ausente = primeira sincronização: vem tudo).
    pub since: Option<String>,
    /// Momento desta leitura; o sincronizador o confirma com `mark_synced` depois de enviar.
    pub until: String,
    pub entries: Vec<ZereiEntry>,
    pub deleted: Vec<ZereiDeleted>,
    pub remote_ref: Option<String>,
}

fn check_target(target: &str) -> AppResult<()> {
    if SYNC_TARGETS.contains(&target) { Ok(()) } else { Err(AppError::Validation("Destino de sincronização inválido.".into())) }
}

/// Alterações desde a última sincronização com `target` (registros criados/alterados e exclusões).
/// `>=`: alterações no mesmo segundo da última sincronização são reenviadas (o `uid` torna isso idempotente).
pub fn changes(conn: &Connection, profile_id: i64, target: &str) -> AppResult<ZereiChanges> {
    check_target(target)?;
    let state: Option<(Option<String>, Option<String>)> = conn
        .query_row("SELECT remote_ref, last_sync_at FROM zerei_sync WHERE profile_id=?1 AND target=?2", params![profile_id, target], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let (remote_ref, since) = state.unwrap_or((None, None));
    let until: String = conn.query_row("SELECT datetime('now')", [], |r| r.get(0))?;
    let since_v = since.clone().unwrap_or_default();
    let mut st = conn.prepare("SELECT * FROM zerei_entries WHERE profile_id=?1 AND updated_at>=?2 ORDER BY updated_at, id")?;
    let entries = st.query_map(params![profile_id, since_v], row)?.collect::<Result<Vec<_>, _>>()?;
    let mut st = conn.prepare("SELECT uid, deleted_at FROM zerei_deleted WHERE profile_id=?1 AND deleted_at>=?2 ORDER BY deleted_at")?;
    let deleted = st
        .query_map(params![profile_id, since_v], |r| Ok(ZereiDeleted { uid: r.get(0)?, deleted_at: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ZereiChanges { target: target.into(), since, until, entries, deleted, remote_ref })
}

/// Confirma um envio bem-sucedido (o próximo `changes` parte de `until`).
pub fn mark_synced(conn: &Connection, profile_id: i64, target: &str, until: &str, remote_ref: Option<&str>) -> AppResult<()> {
    check_target(target)?;
    conn.execute(
        "INSERT INTO zerei_sync(profile_id,target,remote_ref,last_sync_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(profile_id,target) DO UPDATE SET last_sync_at=excluded.last_sync_at, remote_ref=COALESCE(excluded.remote_ref, zerei_sync.remote_ref)",
        params![profile_id, target, remote_ref, until],
    )?;
    Ok(())
}

/// Alterações pendentes para um destino (base para a sincronização com Google Sheets / NEXSTAGE CLOUD).
#[tauri::command]
pub fn zerei_changes(db: State<Db>, profile_id: i64, target: String) -> AppResult<ZereiChanges> {
    let conn = db.conn()?;
    changes(&conn, profile_id, &target)
}

#[tauri::command]
pub fn zerei_mark_synced(db: State<Db>, profile_id: i64, target: String, until: String, remote_ref: Option<String>) -> AppResult<()> {
    let conn = db.conn()?;
    mark_synced(&conn, profile_id, &target, &until, remote_ref.as_deref())
}

// ============================================================================================= CSV

/// Colunas do CSV (ordem da exportação). A importação lê pelo nome do cabeçalho, em qualquer ordem.
pub const CSV_COLUMNS: [&str; 20] = [
    "uid", "title", "platform", "run", "status", "completion_type", "difficulty", "started_at", "completed_at", "hours",
    "rating", "notes", "year", "platforms", "igdb_id", "rawg_id", "sgdb_id", "cover_url", "created_at", "updated_at",
];

fn csv_field(s: &str) -> String {
    // fórmulas de planilha (=, +, -, @) viram texto: o CSV pode ser aberto no Excel/Sheets
    let s = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r', ';']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn num(v: Option<f64>) -> String {
    v.map(|x| if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x}") }).unwrap_or_default()
}

pub fn to_csv(conn: &Connection, profile_id: i64) -> AppResult<String> {
    let mut st = conn.prepare("SELECT * FROM zerei_entries WHERE profile_id=?1 ORDER BY title_key, platform, run")?;
    let rows = st
        .query_map([profile_id], |r| {
            let e = row(r)?;
            let url: Option<String> = r.get("cover_url")?;
            Ok((e, url))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = String::from("\u{feff}"); // BOM: o Excel reconhece UTF-8 (acentos)
    out.push_str(&CSV_COLUMNS.join(","));
    out.push_str("\r\n");
    for (e, url) in rows {
        let o = |v: &Option<String>| v.clone().unwrap_or_default();
        let i = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
        let vals = [
            e.uid, e.title, e.platform, e.run.to_string(), e.status, o(&e.completion_type), o(&e.difficulty), o(&e.started_at),
            o(&e.completed_at), num(e.hours), num(e.rating), e.notes, i(e.year), e.platforms.join("|"), i(e.igdb_id), i(e.rawg_id),
            i(e.sgdb_id), url.unwrap_or_default(), e.created_at, e.updated_at,
        ];
        out.push_str(&vals.iter().map(|v| csv_field(v)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    Ok(out)
}

/// Lê um CSV (vírgula ou ponto e vírgula, aspas no padrão RFC 4180, quebras de linha dentro de aspas).
pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().next().unwrap_or("");
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == sep {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            if row.iter().any(|f| !f.trim().is_empty()) {
                rows.push(std::mem::take(&mut row));
            }
            row.clear();
        } else {
            field.push(c);
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(row);
    }
    rows
}

/// Aceita as chaves internas e os nomes em português ("Zerado", "100%", "Difícil"...).
fn status_key(s: &str) -> String {
    let k = title_key(s);
    match k.as_str() {
        "nao iniciado" | "not started" | "backlog" | "library" | "biblioteca" | "quero zerar" | "sin empezar" | "pendiente"
        | "pas commence" | "a faire" | "nicht begonnen" | "non iniziato" | "da iniziare" => "not_started",
        "jogando" | "playing" | "jugando" | "en cours" | "en train de jouer" | "spiele" | "am spielen" | "aktiv" | "in corso" | "giocando" => "playing",
        "pausado" | "paused" | "on hold" | "abandonado" | "abandoned" | "dropped" | "en pausa" | "en pause" | "abandonne"
        | "pausiert" | "abgebrochen" | "in pausa" | "abbandonato" => "paused",
        "zerado" | "zerei" | "completed" | "concluido" | "finished" | "beaten" | "completado" | "terminado" | "termine" | "fini"
        | "abgeschlossen" | "durchgespielt" | "beendet" | "completato" | "finito" => "completed",
        "lista de desejos" | "desejado" | "quero jogar" | "wishlist" | "wish list" | "lista de deseos" | "liste de souhaits"
        | "wunschliste" | "lista dei desideri" => "wishlist",
        _ => return s.trim().to_lowercase().replace(' ', "_"),
    }
    .into()
}

fn completion_key(s: &str) -> String {
    match title_key(s).as_str() {
        "zerado" | "campanha" | "campanha concluida" | "campaign" | "main story" => "campaign".into(),
        "platinado" | "platina" | "platinum" | "100" | "full" | "completionist" => "full".into(),
        _ => s.trim().to_lowercase(),
    }
}

fn difficulty_key(s: &str) -> String {
    match title_key(s).as_str() {
        "muito facil" | "very easy" => "very_easy".into(),
        "facil" | "easy" => "easy".into(),
        "normal" | "media" | "medio" => "normal".into(),
        "dificil" | "hard" => "hard".into(),
        "muito dificil" | "very hard" => "very_hard".into(),
        _ => s.trim().to_lowercase().replace(' ', "_"),
    }
}

/// Data do CSV: "AAAA-MM-DD", "DD/MM/AAAA" ou, com `mdy`, "MM/DD/AAAA" (planilhas em inglês dos EUA).
/// Aceita também "." e "-" como separador ("05.02.2023", alemão).
fn csv_date_in(s: &str, mdy: bool) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let p: Vec<&str> = s.split(['/', '.', '-']).collect();
    if p.len() == 3 && p[2].trim().len() == 4 {
        let (d, m) = if mdy { (p[1], p[0]) } else { (p[0], p[1]) };
        return Some(format!("{}-{:0>2}-{:0>2}", p[2].trim(), m.trim(), d.trim()));
    }
    Some(s.chars().take(10).collect())
}

/// Ordem das datas do arquivo: alguma data com "dia" > 12 na 2ª posição (05/25/2023) = mês/dia/ano.
/// Na dúvida (todas ≤ 12), dia/mês/ano.
fn dates_are_mdy<'a>(values: impl Iterator<Item = &'a str>) -> bool {
    let (mut dmy, mut mdy) = (false, false);
    for v in values {
        let p: Vec<u32> = v.trim().split(['/', '.']).filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() == 3 {
            dmy |= p[0] > 12;
            mdy |= p[1] > 12;
        }
    }
    mdy && !dmy
}

/// Rótulos (no idioma da interface) das anotações que a importação acrescenta aos comentários.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportLabels {
    pub how_to: String,
    pub difficulty: String,
    pub genre: String,
    pub started: String,
}

impl Default for ImportLabels {
    fn default() -> Self {
        Self { how_to: "Como zerar".into(), difficulty: "Dificuldade".into(), genre: "Gênero".into(), started: "Data de início informada".into() }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportError {
    pub line: usize,
    /// Mensagem fixa (a interface traduz).
    pub message: String,
}

fn csv_num(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    (!s.is_empty()).then(|| s.parse().ok()).flatten()
}

/// Número no começo do texto: "7 -🔥WOOOOO!" → 7; "8,5/10" → 8.5.
fn lead_num(s: &str) -> Option<f64> {
    let n: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    csv_num(n.trim_end_matches(['.', ',']))
}

/// Horas: "99:30:00" / "12:30" (h:m[:s]) ou número ("20,5").
fn csv_hours(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.contains(':') {
        let p: Vec<f64> = s.split(':').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
        return match p.as_slice() {
            [h, m] => Some(h + m / 60.0),
            [h, m, sec] => Some(h + m / 60.0 + sec / 3600.0),
            _ => None,
        };
    }
    csv_num(s)
}

fn yes(s: &str) -> bool {
    matches!(title_key(s).as_str(), "sim" | "s" | "yes" | "y" | "true" | "1" | "x" | "verdadeiro" | "si" | "oui" | "ja" | "vrai" | "wahr" | "vero")
}

/// Nomes de coluna aceitos (comparados sem acento/pontuação/maiúsculas): o export do NEXSTAGE e
/// planilhas/exports comuns em português e inglês. A primeira coluna que casar vale.
/// Inclui português, inglês, espanhol, francês, alemão e italiano (os alfabetos latinos do app).
const ALIASES: [(&str, &[&str]); 25] = [
    ("uid", &["uid", "id"]),
    ("title", &["title", "titulo", "nome do jogo", "nome", "jogo", "game", "name", "nombre", "nombre del juego", "juego",
        "titre", "nom", "nom du jeu", "jeu", "titel", "spiel", "spielname", "nome del gioco", "gioco", "titolo"]),
    ("platform", &["platform", "plataforma", "console", "sistema", "plateforme", "plattform", "piattaforma", "konsole"]),
    ("run", &["run", "conclusao numero"]),
    ("status", &["status", "situacao", "estado", "statut", "stato"]),
    ("completion_type", &["completion type", "tipo de conclusao", "tipo de finalizacion", "type de completion"]),
    ("difficulty", &["difficulty", "dificuldade", "dificuldade pessoal", "dificultad", "difficulte", "schwierigkeit", "difficolta"]),
    ("started_at", &["started at", "inicio", "data de inicio", "comecou em", "start date", "fecha de inicio", "date de debut",
        "startdatum", "begonnen am", "data di inizio"]),
    ("completed_at", &["completed at", "conclusao", "data de conclusao", "data em que zerou", "zerado em", "zerei em", "finished", "completed",
        "completion date", "fecha de finalizacion", "fecha de completado", "terminado el", "date de fin", "termine le",
        "abgeschlossen am", "beendet am", "data di completamento", "completato il"]),
    ("hours", &["hours", "horas jogadas", "horas", "tempo jogado", "tempo jogado horas", "tempo total jogado horas", "tempo total jogado", "playtime",
        "horas jugadas", "tiempo jugado", "heures", "temps de jeu", "spielzeit", "stunden", "ore giocate", "ore", "tempo di gioco"]),
    ("rating", &["rating", "nota", "nota pessoal", "avaliacao", "score", "puntuacion", "calificacion", "valoracion", "note",
        "bewertung", "punteggio", "voto"]),
    ("notes", &["notes", "observacoes", "comentarios", "comentarios e observacoes", "review", "anotacoes", "notas", "observaciones",
        "commentaires", "remarques", "kommentare", "notizen", "commenti", "annotazioni"]),
    ("year", &["year", "ano", "ano de lancamento", "lancamento", "annee", "jahr", "anno"]),
    ("platforms", &["platforms", "plataformas", "plateformes", "plattformen", "piattaforme"]),
    ("igdb_id", &["igdb id", "igdb"]),
    ("rawg_id", &["rawg id", "rawg"]),
    ("sgdb_id", &["sgdb id", "steamgriddb id"]),
    ("cover_url", &["cover url", "url da capa", "capa", "cover", "portada", "couverture", "titelbild", "copertina"]),
    // colunas extras de outras planilhas/apps (viram status, tipo de conclusão ou comentários)
    ("x_platinum", &["platinado", "platinum", "platina", "platino", "platine", "platin", "platinato"]),
    ("x_wishlist", &["lista de desejos", "wishlist", "lista de deseos", "liste de souhaits", "wunschliste", "lista dei desideri"]),
    ("x_dropped", &["abandonado", "dropped", "abandoned", "abandonne", "abgebrochen", "abbandonato"]),
    ("x_done", &["concluido", "zerado", "beaten", "completado", "termine", "durchgespielt", "completato"]),
    ("x_howto", &["como zerar", "como zerei", "objetivo", "how to beat", "como terminar", "comment finir", "objectif", "ziel", "obiettivo"]),
    ("x_genre", &["genero", "genre", "genere"]),
    ("x_type", &["tipo", "type", "typ"]),
];

/// Índice da coluna de cada campo de `ALIASES` (pelo cabeçalho).
fn map_header(head: &[String]) -> Vec<Option<usize>> {
    let keys: Vec<String> = head.iter().map(|h| title_key(h.trim_start_matches('\u{feff}'))).collect();
    let mut used = vec![false; keys.len()];
    ALIASES
        .iter()
        .map(|(_, names)| {
            let i = names.iter().find_map(|n| keys.iter().enumerate().position(|(i, k)| !used[i] && k == n))?;
            used[i] = true;
            Some(i)
        })
        .collect()
}

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZereiImport {
    pub added: usize,
    pub updated: usize,
    pub duplicates: usize,
    /// Duplicados que completaram campos vazios do registro já existente (capa, nota, datas...).
    pub completed: usize,
    /// Linhas recusadas e o motivo (no máximo 20).
    pub errors: Vec<ImportError>,
}

/// Completa só os campos VAZIOS de um registro existente com os dados de `i` (importação de duplicado).
/// Devolve se algo mudou. Nome, plataforma, status e tudo o que já estiver preenchido ficam como estão.
fn fill_empty_fields(conn: &Connection, profile_id: i64, id: i64, i: &ZereiInput, url: Option<&str>) -> AppResult<bool> {
    let old = get(conn, profile_id, id)?;
    let (cover, cover_url, logo, background): (Option<String>, Option<String>, Option<String>, Option<String>) =
        conn.query_row("SELECT cover, cover_url, logo, background FROM zerei_entries WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    let m = ZereiInput {
        cover: cover.clone().or_else(|| url.map(String::from)),
        logo,
        background,
        year: old.year.or(i.year),
        platforms: if old.platforms.is_empty() { i.platforms.clone() } else { old.platforms.clone() },
        igdb_id: old.igdb_id.or(i.igdb_id),
        rawg_id: old.rawg_id.or(i.rawg_id),
        sgdb_id: old.sgdb_id.or(i.sgdb_id),
        completion_type: if old.status == "completed" { old.completion_type.clone().or(i.completion_type.clone()) } else { old.completion_type.clone() },
        difficulty: old.difficulty.clone().or(i.difficulty.clone()),
        started_at: old.started_at.clone().or(i.started_at.clone()),
        completed_at: old.completed_at.clone().or(i.completed_at.clone()),
        hours: old.hours.or(i.hours),
        rating: old.rating.or(i.rating),
        notes: if old.notes.is_empty() { i.notes.clone() } else { old.notes.clone() },
        title: old.title.clone(),
        platform: old.platform.clone(),
        status: old.status.clone(),
    };
    // início não pode ficar depois da conclusão ao juntar datas de fontes diferentes
    let m = ZereiInput { started_at: m.started_at.clone().filter(|s| m.completed_at.as_ref().map_or(true, |c| s <= c)), ..m };
    let new_url = cover_url.clone().or_else(|| url.map(String::from));
    let changed = m.cover != cover || m.year != old.year || m.platforms != old.platforms || m.igdb_id != old.igdb_id || m.rawg_id != old.rawg_id
        || m.sgdb_id != old.sgdb_id || m.completion_type != old.completion_type || m.difficulty != old.difficulty || m.started_at != old.started_at
        || m.completed_at != old.completed_at || m.hours != old.hours || m.rating != old.rating || m.notes != old.notes || new_url != cover_url;
    if changed {
        update(conn, profile_id, id, &m, old.run, new_url.as_deref())?;
    }
    Ok(changed)
}

/// Importa: mesmo `uid` deste perfil → atualiza; jogo+plataforma+conclusão já existente → ignorado
/// (duplicado); o resto é cadastrado. Capas por endereço (https) ficam como endereço.
pub fn import_csv(conn: &mut Connection, profile_id: i64, text: &str, labels: &ImportLabels) -> AppResult<ZereiImport> {
    let rows = parse_csv(text);
    let Some((head, body)) = rows.split_first() else { return Err(AppError::Validation("Arquivo CSV vazio.".into())) };
    let idx = map_header(head);
    let pos = |name: &str| ALIASES.iter().position(|(k, _)| *k == name).unwrap();
    if idx[pos("title")].is_none() || idx[pos("platform")].is_none() {
        return Err(AppError::Validation("CSV inválido: não encontrei as colunas de nome do jogo e de plataforma.".into()));
    }
    // "Avaliação" de 0 a 5 (estrelas, comum em outros apps) vira a escala de 0 a 10 do NEXSTAGE
    // ordem das datas do arquivo inteiro (dia/mês ou mês/dia)
    let mdy = dates_are_mdy(body.iter().flat_map(|r| [idx[pos("started_at")], idx[pos("completed_at")]].into_iter().flatten().filter_map(move |c| r.get(c).map(|s| s.as_str()))));
    let stars = idx[pos("rating")].filter(|&c| title_key(&head[c]) == "avaliacao").is_some_and(|c| {
        let vals: Vec<f64> = body.iter().filter_map(|r| r.get(c)).filter_map(|v| lead_num(v)).collect();
        !vals.is_empty() && vals.iter().all(|v| *v <= 5.0)
    });
    let mut rep = ZereiImport::default();
    let tx = conn.transaction()?;
    for (n, r) in body.iter().enumerate() {
        let f = |name: &str| -> String { idx[pos(name)].and_then(|i| r.get(i)).map(|s| s.trim().to_string()).unwrap_or_default() };
        let opt = |s: String| (!s.is_empty()).then_some(s);
        let int = |s: String| s.trim().parse::<i64>().ok();
        if title_key(&f("title")).is_empty() && r.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        // status: coluna própria, ou as colunas Sim/Não de outros apps; sem nada = zerado (lista de zerados)
        let mut status = opt(f("status")).map(|s| status_key(&s));
        let mut completion = opt(f("completion_type")).map(|s| completion_key(&s)).filter(|c| COMPLETION.contains(&c.as_str()));
        if yes(&f("x_platinum")) {
            status = Some("completed".into());
            completion = Some("full".into());
        } else if status.is_none() {
            status = Some(if yes(&f("x_wishlist")) { "wishlist" } else if yes(&f("x_dropped")) { "paused" } else { "completed" }.into());
        }
        let status = status.unwrap_or_default();
        // o que não tem campo equivalente vai para os comentários (nada se perde)
        let raw_diff = f("difficulty");
        let difficulty = opt(raw_diff.clone()).map(|s| difficulty_key(&s)).filter(|d| DIFFICULTY.contains(&d.as_str()));
        let base = f("notes");
        let mut notes = base.strip_prefix('\'').filter(|s| s.starts_with(['=', '+', '-', '@'])).map(String::from).unwrap_or(base);
        let mut extra = Vec::new();
        if let Some(h) = opt(f("x_howto")) {
            extra.push(format!("{}: {h}", labels.how_to));
        }
        if difficulty.is_none() && !raw_diff.is_empty() {
            extra.push(format!("{}: {raw_diff}", labels.difficulty));
        }
        let completed_at = csv_date_in(&f("completed_at"), mdy);
        let mut started_at = csv_date_in(&f("started_at"), mdy);
        if let (Some(s), Some(c)) = (&started_at, &completed_at) {
            if s > c && valid_date(s) && valid_date(c) {
                // datas trocadas na planilha: o jogo entra mesmo assim; a data de início fica nos comentários
                extra.push(format!("{}: {}", labels.started, f("started_at")));
                started_at = None;
            }
        }
        let genre: Vec<String> = [f("x_genre"), f("x_type")].into_iter().filter(|s| !s.is_empty()).collect();
        if !genre.is_empty() {
            extra.push(format!("{}: {}", labels.genre, genre.join(" · ")));
        }
        if !extra.is_empty() {
            notes = [notes, extra.join("\n")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
        }
        let rating = lead_num(&f("rating")).map(|v| if stars { v * 2.0 } else { v });
        let input = ZereiInput {
            title: f("title"),
            platform: platform_name(&f("platform")),
            completion_type: if status == "completed" { completion } else { None },
            status,
            difficulty,
            started_at,
            completed_at,
            hours: csv_hours(&f("hours")),
            rating,
            notes,
            year: int(f("year")),
            platforms: f("platforms").split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            igdb_id: int(f("igdb_id")),
            rawg_id: int(f("rawg_id")),
            sgdb_id: int(f("sgdb_id")),
            cover: None,
            logo: None,
            background: None,
        };
        let line = n + 2;
        let mut i = match normalize(input) {
            Ok(i) => i,
            Err(e) => {
                if rep.errors.len() < 20 {
                    rep.errors.push(ImportError { line, message: e.to_string() });
                }
                continue;
            }
        };
        let url = opt(f("cover_url")).filter(|u| webshots::allowed_image_url(u));
        let uid = f("uid");
        // mesmo registro deste perfil (exportado daqui antes): atualiza, mantendo as imagens locais
        let existing: Option<(i64, i64, Option<String>, Option<String>, Option<String>, Option<String>)> = if valid_uid(&uid) {
            tx.query_row(
                "SELECT id, run, cover, cover_url, logo, background FROM zerei_entries WHERE uid=?1 AND profile_id=?2",
                params![uid, profile_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()?
        } else {
            None
        };
        let res: AppResult<()> = (|| {
            if let Some((id, run, cover, old_url, logo, bg)) = existing {
                let dups = same_game(&tx, profile_id, &i, Some(id))?;
                let run = if dups.iter().any(|d| d.1 == run) { next_run(&tx, profile_id, &i, &dups)? } else { run };
                let url = url.clone().or(old_url);
                i.cover = cover.or_else(|| url.clone());
                i.logo = logo;
                i.background = bg;
                update(&tx, profile_id, id, &i, run, url.as_deref())?;
                rep.updated += 1;
                return Ok(());
            }
            let dups = same_game(&tx, profile_id, &i, None)?;
            let want = int(f("run")).filter(|r| *r >= 1).unwrap_or(1);
            if dups.iter().any(|d| d.1 == want) || (!dups.is_empty() && int(f("run")).is_none()) {
                // mesmo jogo já registrado: só completa o que estiver vazio nele (nada é sobrescrito)
                let target = dups.iter().find(|d| d.1 == want).unwrap_or(&dups[0]).0;
                if fill_empty_fields(&tx, profile_id, target, &i, url.as_deref())? {
                    rep.completed += 1;
                } else {
                    rep.duplicates += 1;
                }
                return Ok(());
            }
            // uid de outro perfil/computador já em uso aqui: o registro ganha um novo
            let taken = valid_uid(&uid) && tx.query_row("SELECT 1 FROM zerei_entries WHERE uid=?1", [&uid], |_| Ok(())).optional()?.is_some();
            let uid = if valid_uid(&uid) && !taken { uid.to_lowercase() } else { new_uid() };
            i.cover = url.clone();
            insert(&tx, profile_id, &i, want, &uid, url.as_deref())?;
            tx.execute("DELETE FROM zerei_deleted WHERE uid=?1", [&uid])?;
            rep.added += 1;
            Ok(())
        })();
        if let Err(e) = res {
            if rep.errors.len() < 20 {
                rep.errors.push(ImportError { line, message: e.to_string() });
            }
        }
    }
    tx.commit()?;
    Ok(rep)
}

#[tauri::command]
pub fn zerei_export_csv(db: State<Db>, profile_id: i64, path: String) -> AppResult<usize> {
    let conn = db.conn()?;
    let csv = to_csv(&conn, profile_id)?;
    std::fs::write(&path, csv.as_bytes())?;
    Ok(conn.query_row("SELECT COUNT(*) FROM zerei_entries WHERE profile_id=?1", [profile_id], |r| r.get::<_, i64>(0))? as usize)
}

#[tauri::command]
pub fn zerei_import_csv(db: State<Db>, profile_id: i64, path: String, labels: Option<ImportLabels>) -> AppResult<ZereiImport> {
    let meta = std::fs::metadata(&path)?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err(AppError::Validation("Arquivo CSV grande demais (máx. 20 MB).".into()));
    }
    let bytes = std::fs::read(&path)?;
    // UTF-8; se não for (CSV salvo pelo Excel em ANSI), lê como Windows-1252/Latin-1
    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| bytes.iter().map(|&b| b as char).collect());
    let mut conn = db.conn()?;
    conn.query_row("SELECT 1 FROM profiles WHERE id=?1", [profile_id], |_| Ok(())).optional()?.ok_or_else(|| AppError::NotFound("Perfil não encontrado.".into()))?;
    import_csv(&mut conn, profile_id, &text, &labels.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE profiles(id INTEGER PRIMARY KEY); INSERT INTO profiles VALUES(1),(2);").unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    fn input(title: &str, platform: &str) -> ZereiInput {
        normalize(ZereiInput { title: title.into(), platform: platform.into(), status: "completed".into(), ..Default::default() }).unwrap()
    }

    /// Banco com biblioteca: dois jogos, um marcado como "concluído" no esquema antigo.
    fn library() -> Connection {
        let c = conn();
        c.execute_batch(
            "CREATE TABLE games(id INTEGER PRIMARY KEY, title TEXT, platform TEXT, cover TEXT, logo TEXT, background TEXT);
             CREATE TABLE profile_games(profile_id INTEGER, game_id INTEGER, playtime_seconds INTEGER NOT NULL DEFAULT 0,
               completed INTEGER NOT NULL DEFAULT 0, completed_at TEXT);
             INSERT INTO games VALUES(1,'Alien: Isolation','PC','nximg:a.png',NULL,NULL),(2,'Marvel''s Spider Man 2','Playstation 5',NULL,NULL,NULL);
             INSERT INTO profile_games VALUES(1,1,5400,1,'2026-11-06 17:32:41'),(1,2,0,0,NULL);",
        )
        .unwrap();
        c
    }

    #[test]
    fn old_completed_marks_become_beaten_entries_once() {
        let c = library();
        migrate(&c).unwrap();
        let (status, day, hours, game, cover): (String, String, f64, i64, String) = c
            .query_row("SELECT status, completed_at, hours, game_id, cover FROM zerei_entries WHERE title_key='alien isolation'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!((status.as_str(), day.as_str(), hours, game, cover.as_str()), ("completed", "2026-11-06", 1.5, 1, "nximg:a.png"));
        migrate(&c).unwrap(); // de novo: não duplica
        let n: i64 = c.query_row("SELECT COUNT(*) FROM zerei_entries", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "só o jogo marcado vira registro");
    }

    #[test]
    fn marking_reuses_existing_entry_of_same_game() {
        let c = library();
        migrate(&c).unwrap();
        // registro antigo do Zerei! (lista de desejos), com nota, para o mesmo jogo com outra grafia
        let mut w = input("Marvel's Spider-Man 2", "PlayStation 5");
        w.status = String::from("wishlist");
        w.notes = String::from("presente");
        let old = write(&c, 1, None, &w, false, None).unwrap();
        let id = mark_game(&c, 1, 2, Some(String::from("2026-11-14"))).unwrap();
        assert_eq!(id, old.id, "o registro existente é reaproveitado");
        let e = get(&c, 1, id).unwrap();
        assert_eq!((e.status.as_str(), e.completed_at.as_deref(), e.notes.as_str()), ("completed", Some("2026-11-14"), "presente"));
        assert_eq!(mark_game(&c, 1, 2, None).unwrap(), id, "marcar de novo usa o registro ligado");
        assert!(mark_game(&c, 2, 2, None).is_err(), "jogo de outro perfil");
    }

    #[test]
    fn keys_platforms_and_uids() {
        assert_eq!(title_key("Marvel's Spider-Man 2™"), "marvels spider man 2");
        assert_eq!(title_key("  Pokémon: Edição Ouro "), "pokemon edicao ouro");
        assert_eq!(platform_name("PC (Microsoft Windows)"), "PC");
        assert_eq!(platform_name("PlayStation 4"), "Playstation 4");
        assert_eq!(platform_name("Super Nintendo Entertainment System"), "Super Nintendo");
        assert_eq!(platform_name("Sega Saturn"), "Sega Saturn");
        assert_eq!(uniq_platforms(vec![String::from("PC"), String::from("PC (Microsoft Windows)"), String::from("macOS")]), vec!["PC", "macOS"]);
        let (a, b) = (new_uid(), new_uid());
        assert!(valid_uid(&a) && a != b && &a[14..15] == "4", "{a}");
    }

    #[test]
    fn validates_input() {
        assert!(valid_date("2024-02-29") && !valid_date("2023-02-29") && !valid_date("2024-13-01") && !valid_date("24-01-01"));
        let bad = |f: fn(&mut ZereiInput)| {
            let mut i = ZereiInput { title: "Doom".into(), platform: "PC".into(), status: "playing".into(), ..Default::default() };
            f(&mut i);
            normalize(i).is_err()
        };
        assert!(bad(|i| i.rating = Some(10.5)));
        assert!(bad(|i| i.rating = Some(-1.0)));
        assert!(bad(|i| i.hours = Some(-2.0)));
        assert!(bad(|i| i.status = "done".into()));
        assert!(bad(|i| i.title = " - ".into()));
        assert!(bad(|i| i.platform = "".into()));
        assert!(bad(|i| i.completed_at = Some("31/12/2024".into())));
        assert!(bad(|i| i.difficulty = Some("insane".into())));
        assert!(bad(|i| i.completion_type = Some("half".into())));
        assert!(bad(|i| { i.started_at = Some("2024-05-02".into()); i.completed_at = Some("2024-05-01".into()); }));
        assert!(!bad(|i| i.rating = Some(7.5)));
        assert!(!bad(|i| i.status = "wishlist".into()));
        assert_eq!(status_key("Lista de desejos"), "wishlist");
        assert_eq!((completion_key("Platinado").as_str(), completion_key("Zerado").as_str(), completion_key("100%").as_str()), ("full", "campaign", "full"));
        assert!(!bad(|i| { i.difficulty = Some("hard".into()); i.completion_type = Some("full".into()); i.started_at = Some("2024-01-01".into()); }));
        let ok = normalize(ZereiInput { title: "Doom".into(), platform: "PC".into(), status: "paused".into(), hours: Some(12.345), completed_at: Some(" ".into()), ..Default::default() }).unwrap();
        assert_eq!((ok.hours, ok.completed_at), (Some(12.3), None));
    }

    #[test]
    fn blocks_duplicates_but_allows_new_completion_runs() {
        let c = conn();
        let a = write(&c, 1, None, &input("Hollow Knight", "PC"), false, None).unwrap();
        assert_eq!(a.run, 1);
        // mesmo jogo (nome escrito diferente) na mesma plataforma: recusado
        let e = write(&c, 1, None, &input("hollow knight™", "pc"), false, None).unwrap_err().to_string();
        assert!(e.starts_with(&format!("{DUPLICATE}{}::1", a.id)), "{e}");
        // outra plataforma ou outro perfil: permitido
        assert_eq!(write(&c, 1, None, &input("Hollow Knight", "Nintendo Switch"), false, None).unwrap().run, 1);
        assert_eq!(write(&c, 2, None, &input("Hollow Knight", "PC"), false, None).unwrap().run, 1);
        // nova conclusão confirmada: run 2, 3...
        assert_eq!(write(&c, 1, None, &input("Hollow Knight", "PC"), true, None).unwrap().run, 2);
        assert_eq!(write(&c, 1, None, &input("Hollow Knight", "PC"), true, None).unwrap().run, 3);
        // mesmo ID do IGDB com outro nome também conta como o mesmo jogo
        let mut x = input("Celeste", "PC");
        x.igdb_id = Some(26226);
        write(&c, 1, None, &x, false, None).unwrap();
        let mut y = input("Celeste (Definitive)", "PC");
        y.igdb_id = Some(26226);
        assert!(write(&c, 1, None, &y, false, None).is_err());
    }

    #[test]
    fn editing_keeps_run_and_detects_collisions() {
        let c = conn();
        let a = write(&c, 1, None, &input("Celeste", "PC"), false, None).unwrap();
        let b = write(&c, 1, None, &input("Celeste", "Nintendo Switch"), false, None).unwrap();
        let mut edit = input("Celeste", "PC");
        edit.rating = Some(9.0);
        edit.notes = String::from("Capítulo 9 ✓");
        let e = write(&c, 1, Some(a.id), &edit, false, None).unwrap();
        assert_eq!((e.run, e.rating, e.notes.as_str(), e.uid.as_str()), (1, Some(9.0), "Capítulo 9 ✓", a.uid.as_str()), "uid nunca muda");
        // mudar a versão do Switch para PC colide com o registro do PC
        assert!(write(&c, 1, Some(b.id), &input("Celeste", "PC"), false, None).is_err());
        assert_eq!(write(&c, 1, Some(b.id), &input("Celeste", "PC"), true, None).unwrap().run, 2);
        // outro perfil não edita
        assert!(write(&c, 2, Some(a.id), &input("Celeste", "PC"), false, None).is_err());
    }

    #[test]
    fn merges_igdb_and_rawg() {
        let h = |t: &str, y: Option<i32>, src: &str, id: i64| ZereiHit {
            title: t.into(), year: y, platforms: vec!["PC".into()], cover: None, background: None,
            igdb_id: (src == "IGDB").then_some(id), rawg_id: (src == "RAWG").then_some(id), sources: vec![src.into()],
        };
        let m = merge(vec![h("DOOM", Some(2016), "IGDB", 1), h("DOOM", Some(1993), "IGDB", 2)], vec![h("Doom", Some(2016), "RAWG", 9), h("Doom Eternal", Some(2020), "RAWG", 10)], "doom");
        assert_eq!(m.len(), 3);
        let d16 = m.iter().find(|x| x.igdb_id == Some(1)).unwrap();
        assert_eq!((d16.rawg_id, d16.sources.len()), (Some(9), 2));
        assert!(m.iter().find(|x| x.igdb_id == Some(2)).unwrap().rawg_id.is_none());
    }

    #[test]
    fn csv_roundtrip_and_import_rules() {
        let mut c = conn();
        let mut a = input("Hollow Knight, Voidheart", "PC");
        a.notes = "=SOMA(1)\n\"Pantheon\" feito, enfim".into();
        a.rating = Some(9.5);
        a.hours = Some(62.0);
        a.difficulty = Some("hard".into());
        a.completion_type = Some("full".into());
        a.started_at = Some("2024-01-02".into());
        a.completed_at = Some("2024-03-04".into());
        let e = write(&c, 1, None, &a, false, Some("https://cdn2.steamgriddb.com/grid/x.png")).unwrap();
        let csv = to_csv(&c, 1).unwrap();
        assert!(csv.contains("'=SOMA(1)"), "fórmula neutralizada");
        // reimportar no mesmo perfil: atualiza pelo uid (nada duplicado)
        let r = import_csv(&mut c, 1, &csv, &ImportLabels::default()).unwrap();
        assert_eq!((r.added, r.updated, r.duplicates), (0, 1, 0), "{r:?}");
        let back = get(&c, 1, e.id).unwrap();
        assert_eq!((back.notes.as_str(), back.rating, back.hours), (a.notes.as_str(), Some(9.5), Some(62.0)));
        assert_eq!((back.difficulty.as_deref(), back.completion_type.as_deref()), (Some("hard"), Some("full")));
        // em outro perfil: uid já usado aqui → cadastra com um novo
        let r = import_csv(&mut c, 2, &csv, &ImportLabels::default()).unwrap();
        assert_eq!(r.added, 1);
        let other = &zerei_rows(&c, 2)[0];
        assert_ne!(other.uid, e.uid);
        assert_eq!(other.cover.as_deref(), Some("https://cdn2.steamgriddb.com/grid/x.png"));
        // planilha feita à mão (ponto e vírgula, nomes em português, data brasileira, duplicado)
        let hand = "title;platform;status;completion_type;difficulty;completed_at;hours;rating\n\
                    Celeste;PlayStation 4;Zerado;100%;Difícil;05/06/2023;20,5;10\n\
                    Celeste;Playstation 4;Zerado;;;;;\n\
                    Doom;PC;Zerado;;;;;11\n";
        let r = import_csv(&mut c, 1, hand, &ImportLabels::default()).unwrap();
        assert_eq!((r.added, r.duplicates, r.errors.len()), (1, 1, 1), "{r:?}");
        assert_eq!(r.errors[0].line, 4);
        let cel = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Celeste").unwrap();
        assert_eq!((cel.platform.as_str(), cel.completed_at.as_deref(), cel.hours, cel.difficulty.as_deref(), cel.completion_type.as_deref()),
            ("Playstation 4", Some("2023-06-05"), Some(20.5), Some("hard"), Some("full")));
        assert!(import_csv(&mut c, 1, "nome,coisa\nx,y", &ImportLabels::default()).is_err());
    }

    #[test]
    fn imports_spreadsheets_and_other_apps() {
        let mut c = conn();
        // planilha do Google Sheets em português (colunas extras, tabelas laterais, horas h:m:s, nota com emoji)
        let sheet = "Coluna 1,Nome do jogo,Plataforma,Genero,Tipo,Inicio,Conclusão,Horas Jogadas,Dificuldade,Nota,Como Zerar!,Observações,,N°,Nome do Jogo,PLataforma\n\
                     1,Jogo Alfa,PS2,🗺️ Aventura,🕹️ Plataforma,,,09:00:00,A+,7 -🔥WOOOOO!,Conclua todas as missões,,,1,Outro Jogo,PC\n\
                     2,Jogo Beta,PC,⚔️ RPG,,14/04/2023,05/02/2023,99:30:00,SS,\"6 -😒 I MEAN, IT'S OK\",Derrote o chefe,Tem dois finais,,2,X,PC\n";
        let r = import_csv(&mut c, 1, sheet, &ImportLabels::default()).unwrap();
        assert_eq!((r.added, r.errors.len()), (2, 0), "{r:?}");
        // Beta: início depois da conclusão → entra sem a data de início, que vai para os comentários
        let b = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Jogo Beta").unwrap();
        assert_eq!((b.started_at, b.completed_at.as_deref(), b.hours), (None, Some("2023-02-05"), Some(99.5)));
        assert!(b.notes.starts_with("Tem dois finais") && b.notes.contains("Data de início informada: 14/04/2023"), "{}", b.notes);
        let a = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Jogo Alfa").unwrap();
        assert_eq!((a.platform.as_str(), a.status.as_str(), a.hours, a.rating), ("Playstation 2", "completed", Some(9.0), Some(7.0)));
        assert_eq!(a.difficulty, None);
        assert!(a.notes.contains("Como zerar: Conclua todas as missões") && a.notes.contains("Dificuldade: A+") && a.notes.contains("Gênero: 🗺️ Aventura"), "{}", a.notes);
        // export de outro app (Sim/Não, status em inglês, avaliação em estrelas, uuid próprio)
        let other = "\u{feff}ID,Título,Plataforma,Status,Concluído,Lista de Desejos,Platinado,Avaliação,Data de Conclusão,Tempo Total Jogado (horas),Review,URL da Capa,IGDB ID\n\
                     0ee85a12-4ddb-4095-8a5e-7359872d9187,\"Jogo Gama\",\"PC (Microsoft Windows)\",completed,Sim,Não,Sim,5,2026-10-06,18,\"Muito bom,\nrecomendo\",https://images.igdb.com/igdb/image/upload/t_cover_big/x.jpg,912\n\
                     ddd5b3b5-24d6-4589-9c1e-84488047e2d7,\"Jogo Delta\",\"Xbox Series X|S\",library,Não,Não,Não,4,,,\"\",,\n\
                     0cc3722f-898a-4651-a8a1-40cd44b54bec,\"Jogo Alfa\",\"PlayStation 2\",wishlist,Não,Não,Não,,,,\"\",,\n";
        let r = import_csv(&mut c, 1, other, &ImportLabels::default()).unwrap();
        // Alfa/PS2 já existe e a linha não traz nada novo → duplicado ignorado
        assert_eq!((r.added, r.duplicates, r.completed, r.errors.len()), (2, 1, 0, 0), "{r:?}");
        // o mesmo jogo vindo de outra fonte com mais dados completa só os campos vazios
        let more = "Título,Plataforma,Platinado,Avaliação,URL da Capa,IGDB ID,Review\nJogo Alfa,PS2,Sim,2,https://images.igdb.com/igdb/image/upload/t_cover_big/a.jpg,77,Nota nova\n";
        let r = import_csv(&mut c, 1, more, &ImportLabels::default()).unwrap();
        assert_eq!((r.completed, r.duplicates), (1, 0), "{r:?}");
        let a = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Jogo Alfa" && x.platform == "Playstation 2").unwrap();
        assert_eq!((a.completion_type.as_deref(), a.igdb_id, a.rating), (Some("full"), Some(77), Some(7.0)), "nota já preenchida não muda");
        assert!(a.cover.as_deref().unwrap().ends_with("/a.jpg") && a.notes.contains("Como zerar"), "comentário existente mantido");
        let g = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Jogo Gama").unwrap();
        assert_eq!((g.uid.as_str(), g.platform.as_str(), g.completion_type.as_deref(), g.rating, g.hours), ("0ee85a12-4ddb-4095-8a5e-7359872d9187", "PC", Some("full"), Some(10.0), Some(18.0)));
        assert_eq!((g.completed_at.as_deref(), g.igdb_id, g.notes.as_str()), (Some("2026-10-06"), Some(912), "Muito bom,\nrecomendo"));
        let d = zerei_rows(&c, 1).into_iter().find(|x| x.title == "Jogo Delta").unwrap();
        assert_eq!((d.status.as_str(), d.platform.as_str(), d.rating, d.completion_type), ("not_started", "Xbox Series", Some(8.0), None));
        assert_eq!((csv_hours("00:10:20").map(|h| (h * 100.0).round() / 100.0), lead_num("10 - 😍 Melhor"), csv_hours("12:30")), (Some(0.17), Some(10.0), Some(12.5)));
    }

    #[test]
    fn imports_other_languages_and_us_dates() {
        let mut c = conn();
        let es = "Nombre del juego;Plataforma;Estado;Fecha de finalización;Horas jugadas;Puntuación;Comentarios\nJuego Uno;PS4;Terminado;25/12/2023;10,5;9;Genial\nJuego Dos;PC;Jugando;;;;\n";
        let fr = "Titre,Plateforme,Statut,Note,Platine,Commentaires\nJeu Trois,Switch,En pause,8,Non,\nJeu Quatre,PC,Fini,7,Oui,Bien\n";
        let de = "Spielname;Plattform;Status;Abgeschlossen am;Spielzeit\nSpiel Fünf;Xbox 360;Durchgespielt;24.12.2022;12:30\n";
        let us = "Game,Platform,Status,Completion Date,Hours\nUS One,PC,Completed,05/25/2023,3\nUS Two,PC,Completed,01/02/2023,4\n";
        let jp = "title,platform,status\nゼルダの伝説,Nintendo Switch,completed\nゼルダの伝説,nintendo switch,completed\n";
        let labels = ImportLabels { how_to: String::from("How to beat"), difficulty: String::from("Difficulty"), genre: String::from("Genre"), started: String::from("Start date given") };
        for (text, added) in [(es, 2), (fr, 2), (de, 1), (us, 2), (jp, 1)] {
            let r = import_csv(&mut c, 1, text, &labels).unwrap();
            assert_eq!((r.added, r.errors.len()), (added, 0), "{text}\n{r:?}");
        }
        let rows = zerei_rows(&c, 1);
        let by = |t: &str| rows.iter().find(|x| x.title == t).unwrap();
        assert_eq!((by("Juego Uno").status.as_str(), by("Juego Uno").completed_at.as_deref(), by("Juego Uno").hours), ("completed", Some("2023-12-25"), Some(10.5)));
        assert_eq!(by("Juego Dos").status, "playing");
        assert_eq!((by("Jeu Trois").status.as_str(), by("Jeu Trois").rating), ("paused", Some(8.0)));
        assert_eq!(by("Jeu Quatre").completion_type.as_deref(), Some("full"));
        assert_eq!((by("Spiel Fünf").completed_at.as_deref(), by("Spiel Fünf").hours), (Some("2022-12-24"), Some(12.5)));
        // 05/25 só pode ser mês/dia → o arquivo inteiro é lido como americano (01/02 = 2 de janeiro)
        assert_eq!((by("US One").completed_at.as_deref(), by("US Two").completed_at.as_deref()), (Some("2023-05-25"), Some("2023-01-02")));
        assert!(dates_are_mdy(["05/25/2023"].into_iter()) && !dates_are_mdy(["25/05/2023", "01/02/2023"].into_iter()) && !dates_are_mdy(["01/02/2023"].into_iter()));
        // rótulos das anotações no idioma da interface
        let r = import_csv(&mut c, 1, "Game,Platform,Difficulty,How to beat\nNotes Game,PC,SS,Beat the boss\n", &labels).unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(by_title(&c, "Notes Game").notes, "How to beat: Beat the boss\nDifficulty: SS");
    }

    fn by_title(c: &Connection, t: &str) -> ZereiEntry {
        zerei_rows(c, 1).into_iter().find(|x| x.title == t).unwrap()
    }

    /// Diagnóstico com arquivos reais (banco em memória): NEXSTAGE_CSV="a.csv|b.csv" cargo test real_csv -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_csv() {
        let mut c = conn();
        for p in std::env::var("NEXSTAGE_CSV").expect("defina NEXSTAGE_CSV").split('|') {
            let text = std::fs::read_to_string(p).unwrap();
            let r = import_csv(&mut c, 1, &text, &ImportLabels::default()).unwrap();
            println!("{p}\n  novos {} | atualizados {} | duplicados {} | erros {}", r.added, r.updated, r.duplicates, r.errors.len());
            r.errors.iter().for_each(|e| println!("  linha {}: {}", e.line, e.message));
        }
        let rows = zerei_rows(&c, 1);
        let count = |s: &str| rows.iter().filter(|x| x.status == s).count();
        println!("total {} | zerados {} | platinados {} | jogando {} | desejos {} | não iniciados {}", rows.len(), count("completed"),
            rows.iter().filter(|x| x.completion_type.as_deref() == Some("full")).count(), count("playing"), count("wishlist"), count("not_started"));
        let mut plats: Vec<String> = rows.iter().map(|x| x.platform.clone()).collect();
        plats.sort();
        plats.dedup();
        println!("plataformas: {plats:?}");
    }

    fn zerei_rows(c: &Connection, p: i64) -> Vec<ZereiEntry> {
        let mut st = c.prepare("SELECT * FROM zerei_entries WHERE profile_id=?1").unwrap();
        let r = st.query_map([p], row).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        r
    }

    #[test]
    fn sync_changes_since_last_push() {
        let c = conn();
        let a = write(&c, 1, None, &input("Doom", "PC"), false, None).unwrap();
        write(&c, 2, None, &input("Quake", "PC"), false, None).unwrap();
        assert!(changes(&c, 1, "dropbox").is_err());
        // primeira sincronização: tudo do perfil (e só dele)
        let ch = changes(&c, 1, "sheets").unwrap();
        assert_eq!((ch.since.as_deref(), ch.entries.len(), ch.deleted.len()), (None, 1, 0));
        mark_synced(&c, 1, "sheets", "2000-01-01 00:00:00", Some("planilha-123")).unwrap();
        // simula o tempo passando: registros antigos ficam antes da última sincronização
        c.execute("UPDATE zerei_entries SET updated_at='1999-01-01 00:00:00'", []).unwrap();
        let ch = changes(&c, 1, "sheets").unwrap();
        assert_eq!((ch.entries.len(), ch.remote_ref.as_deref()), (0, Some("planilha-123")));
        // edição e exclusão aparecem no próximo pacote; o outro destino continua vendo tudo
        let mut e = input("Doom", "PC");
        e.rating = Some(8.0);
        write(&c, 1, Some(a.id), &e, false, None).unwrap();
        let b = write(&c, 1, None, &input("Hexen", "PC"), false, None).unwrap();
        c.execute("DELETE FROM zerei_entries WHERE id=?1", [b.id]).unwrap();
        c.execute("INSERT INTO zerei_deleted(uid,profile_id) VALUES(?1,1)", [&b.uid]).unwrap();
        let ch = changes(&c, 1, "sheets").unwrap();
        assert_eq!(ch.entries.iter().map(|x| x.uid.as_str()).collect::<Vec<_>>(), vec![a.uid.as_str()]);
        assert_eq!(ch.deleted.iter().map(|x| x.uid.as_str()).collect::<Vec<_>>(), vec![b.uid.as_str()]);
        assert_eq!(changes(&c, 1, "cloud").unwrap().entries.len(), 1);
        // confirmar sem remote_ref mantém o anterior
        mark_synced(&c, 1, "sheets", "2100-01-01 00:00:00", None).unwrap();
        let ch = changes(&c, 1, "sheets").unwrap();
        assert_eq!((ch.entries.len(), ch.deleted.len(), ch.remote_ref.as_deref()), (0, 0, Some("planilha-123")));
    }

    #[test]
    fn delete_leaves_tombstone() {
        let c = conn();
        let a = write(&c, 1, None, &input("Doom", "PC"), false, None).unwrap();
        c.execute("DELETE FROM zerei_entries WHERE id=?1", [a.id]).unwrap();
        c.execute("INSERT INTO zerei_deleted(uid,profile_id) VALUES(?1,1)", [&a.uid]).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM zerei_deleted", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // e o mesmo jogo pode ser cadastrado de novo (run 1)
        assert_eq!(write(&c, 1, None, &input("Doom", "PC"), false, None).unwrap().run, 1);
    }
}

// ============================================================================================= imagens e lote

/// Primeira imagem de cada tipo encontrada para um jogo.
#[derive(Default)]
struct FirstArt {
    sgdb_id: Option<i64>,
    cover: Option<String>,
    logo: Option<String>,
    background: Option<String>,
}

/// Capa, logo e fundo automáticos (o PRIMEIRO resultado de cada): SteamGridDB, se houver chave; capa e
/// fundo que faltarem vêm da pesquisa do Zerei! (IGDB/RAWG ou a pesquisa pública).
async fn first_art(app: &AppHandle, title: &str, year: Option<i32>, sgdb_id: Option<i64>, need: [bool; 3]) -> FirstArt {
    let mut art = FirstArt::default();
    if let (Some(key), Ok(c)) = (crate::apikeys::get(app)("sgdb_key"), http()) {
        let id = match sgdb_id {
            Some(id) => Some(id),
            None => sgdb_find_id(&c, &key, title, year).await,
        };
        if let Some(id) = id {
            art.sgdb_id = Some(id);
            let first = |v: Option<Value>| v.and_then(|v| v["data"].as_array()?.first()?["url"].as_str().map(String::from));
            if need[0] {
                art.cover = first(get_json(sgdb(&c, &key, &format!("/grids/game/{id}?dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false"))).await);
            }
            if need[1] {
                art.logo = first(get_json(sgdb(&c, &key, &format!("/logos/game/{id}?types=static&nsfw=false&humor=false"))).await);
            }
            if need[2] {
                art.background = first(get_json(sgdb(&c, &key, &format!("/heroes/game/{id}?types=static&nsfw=false&humor=false"))).await);
            }
        }
    }
    if (need[0] && art.cover.is_none()) || (need[2] && art.background.is_none()) {
        use tauri::Manager;
        if let Ok(r) = zerei_search(app.clone(), app.state::<Db>(), title.to_string()).await {
            let hit = r.hits.iter().find(|h| info::name_score(title, &h.title) >= 1.0).or(r.hits.first());
            if let Some(h) = hit {
                if need[0] && art.cover.is_none() {
                    art.cover = h.cover.clone();
                }
                if need[2] && art.background.is_none() {
                    art.background = h.background.clone();
                }
            }
        }
    }
    art
}

static REFRESH_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static REFRESH_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RefreshProgress {
    pub done: usize,
    pub total: usize,
    pub updated: usize,
    pub title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    /// Jogos que precisavam de alguma imagem (os já completos são pulados).
    pub total: usize,
    /// Jogos que receberam ao menos uma imagem.
    pub updated: usize,
    /// Jogos em que nada foi encontrado.
    pub not_found: usize,
    pub canceled: bool,
}

/// "Atualizar imagens": procura capa, logo e fundo (primeiro resultado de cada) e grava como arquivo.
/// - `ids` vazio/ausente: todos os jogos do perfil; senão só os escolhidos.
/// - `overwrite = false`: só preenche o que falta (imagens existentes, escolhidas pelo usuário, ficam);
///   `true` (seleção): troca as três pelas encontradas (o que não for encontrado continua como estava).
/// Progresso no evento `zerei-images-progress`; `zerei_refresh_cancel` interrompe entre um jogo e outro.
#[tauri::command]
pub async fn zerei_refresh_images(app: AppHandle, db: State<'_, Db>, profile_id: i64, ids: Option<Vec<i64>>, overwrite: bool) -> AppResult<RefreshReport> {
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    if REFRESH_RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Validation("A atualização de imagens já está em andamento.".into()));
    }
    REFRESH_CANCEL.store(false, Ordering::SeqCst);
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            REFRESH_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _running = Running;

    type Item = (i64, String, Option<i32>, Option<i64>, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Item> = {
        let conn = db.conn()?;
        let mut st = conn.prepare("SELECT id, title, year, sgdb_id, cover, logo, background FROM zerei_entries WHERE profile_id=?1 ORDER BY title")?;
        let all = st
            .query_map([profile_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
            .collect::<Result<Vec<Item>, _>>()?;
        match &ids {
            Some(ids) if !ids.is_empty() => all.into_iter().filter(|r| ids.contains(&r.0)).collect(),
            _ => all,
        }
    };
    // o que cada jogo precisa: arquivo apagado conta como "sem imagem"
    let missing = |v: &Option<String>| crate::images::resolve_existing(v.clone()).is_none();
    let work: Vec<(Item, [bool; 3])> = rows
        .into_iter()
        .map(|r| {
            let need = if overwrite { [true; 3] } else { [missing(&r.4), missing(&r.5), missing(&r.6)] };
            (r, need)
        })
        .filter(|(_, n)| n.iter().any(|x| *x))
        .collect();
    let total = work.len();
    let (mut updated, mut not_found) = (0, 0);
    for (i, ((id, title, year, sgdb_id, ..), need)) in work.into_iter().enumerate() {
        if REFRESH_CANCEL.load(Ordering::SeqCst) {
            return Ok(RefreshReport { total, updated, not_found, canceled: true });
        }
        let _ = app.emit("zerei-images-progress", RefreshProgress { done: i, total, updated, title: title.clone() });
        let art = first_art(&app, &title, year, sgdb_id, need).await;
        // baixa e guarda como arquivo (offline e no backup); falha no download = imagem não encontrada
        let cover_src = art.cover.clone();
        let cover = if need[0] { store_found(art.cover).await } else { None };
        let logo = if need[1] { store_found(art.logo).await } else { None };
        let background = if need[2] { store_found(art.background).await } else { None };
        if cover.is_none() && logo.is_none() && background.is_none() {
            not_found += 1;
            continue;
        }
        let conn = db.conn()?;
        conn.execute(
            "UPDATE zerei_entries SET
               cover = COALESCE(?3, cover), cover_url = CASE WHEN ?3 IS NOT NULL THEN ?4 ELSE cover_url END,
               logo = COALESCE(?5, logo), background = COALESCE(?6, background),
               sgdb_id = COALESCE(sgdb_id, ?7), updated_at = datetime('now')
             WHERE id=?1 AND profile_id=?2",
            params![id, profile_id, cover, cover_src, logo, background, art.sgdb_id],
        )?;
        updated += 1;
    }
    let _ = app.emit("zerei-images-progress", RefreshProgress { done: total, total, updated, title: String::new() });
    Ok(RefreshReport { total, updated, not_found, canceled: false })
}

async fn store_found(u: Option<String>) -> Option<String> {
    keep_image(Some(u?)).await.ok().flatten()
}

#[tauri::command]
pub fn zerei_refresh_cancel() {
    REFRESH_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Exclui vários registros (como `zerei_delete`, numa transação só).
#[tauri::command]
pub fn zerei_bulk_delete(db: State<Db>, profile_id: i64, ids: Vec<i64>) -> AppResult<usize> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let mut n = 0;
    for id in ids {
        let uid: Option<String> = tx.query_row("SELECT uid FROM zerei_entries WHERE id=?1 AND profile_id=?2", params![id, profile_id], |r| r.get(0)).optional()?;
        let Some(uid) = uid else { continue };
        tx.execute("DELETE FROM zerei_entries WHERE id=?1", [id])?;
        tx.execute("INSERT OR REPLACE INTO zerei_deleted(uid,profile_id) VALUES(?1,?2)", params![uid, profile_id])?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// Move vários registros para outra lista (status). Ao virar "zerado", ganha tipo "Zerado" e a data de
/// hoje se ainda não tiver; ao sair de "zerado", o tipo de conclusão é limpo (a data fica no histórico).
#[tauri::command]
pub fn zerei_bulk_status(db: State<Db>, profile_id: i64, ids: Vec<i64>, status: String) -> AppResult<usize> {
    if !STATUSES.contains(&status.as_str()) {
        return Err(AppError::Validation("Status inválido.".into()));
    }
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let mut n = 0;
    for id in ids {
        n += tx.execute(
            "UPDATE zerei_entries SET status=?3,
               completion_type = CASE WHEN ?3='completed' THEN COALESCE(completion_type,'campaign') ELSE NULL END,
               completed_at = CASE WHEN ?3='completed' THEN COALESCE(completed_at, date('now','localtime')) ELSE completed_at END,
               updated_at = datetime('now')
             WHERE id=?1 AND profile_id=?2",
            params![id, profile_id, status],
        )?;
    }
    tx.commit()?;
    Ok(n)
}
