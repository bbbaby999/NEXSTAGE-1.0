//! Biblioteca de jogos por perfil (adição manual de executáveis).

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i64,
    pub title: String,
    pub platform: String,
    pub executable_path: String,
    pub launch_args: String,
    pub cover: Option<String>,
    pub favorite: bool,
    /// Quando entrou nos favoritos (ordem do carrossel Favoritos da Home); vazio em favoritos antigos.
    pub favorited_at: Option<String>,
    pub playtime_seconds: i64,
    pub last_played_at: Option<String>,
    pub added_at: String,
    pub background: Option<String>,
    pub bg_x: i64,
    pub bg_y: i64,
    /// Logo (SteamGridDB ou arquivo): aparece no lugar do título na página do jogo.
    pub logo: Option<String>,
    pub steam_id: Option<String>,
    pub release_date: Option<String>,
    pub release_iso: Option<String>,
    pub genre: Option<String>,
    pub genres: Vec<String>,
    pub completed: bool,
    pub completed_at: Option<String>,
    pub iso_path: Option<String>,
    pub rom_path: Option<String>,
    pub emulator_id: Option<i64>,
    pub emulator_name: Option<String>,
    /// Jogo do RetroAchievements associado (por hash ou confirmado pelo usuário).
    pub ra_game_id: Option<i64>,
    /// Origem ("manual", "iso", "folder", "steam"...).
    pub source: String,
    /// AppID da Steam (jogo importado da Steam ou vinculado a ela).
    pub steam_app_id: Option<i64>,
    /// Pronto para jogar neste PC: jogo da Steam instalado pelo cliente Steam; demais, executável ou
    /// arquivo do jogo (ISO/ROM) presente no disco. Calculado na listagem (só leitura).
    pub installed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInput {
    pub title: String,
    pub platform: String,
    pub executable_path: String,
    #[serde(default)]
    pub launch_args: String,
    pub cover: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub bg_x: Option<i64>,
    #[serde(default)]
    pub bg_y: Option<i64>,
    /// Ausente = mantém a logo atual; `null` = remove; texto = nova logo.
    #[serde(default, deserialize_with = "present")]
    pub logo: Option<Option<String>>,
    /// Caminho do .iso importado (nunca copiado, movido ou modificado).
    #[serde(default)]
    pub iso_path: Option<String>,
}

/// Campo presente no JSON (mesmo `null`) vira `Some(..)`; ausente fica `None` (pelo `default`).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

const SELECT: &str = "SELECT g.id,g.title,g.platform,g.executable_path,g.launch_args,g.cover,
    pg.favorite,pg.playtime_seconds,pg.last_played_at,pg.added_at,
    g.background,g.bg_x,g.bg_y,json_extract(gi.data,'$.externalId'),
    json_extract(gi.data,'$.releaseDate'),json_extract(gi.data,'$.genres[0]'),json_extract(gi.data,'$.genres'),EXISTS(SELECT 1 FROM zerei_entries z WHERE z.profile_id=pg.profile_id AND z.game_id=g.id AND z.status='completed'),
    (SELECT MAX(z.completed_at) FROM zerei_entries z WHERE z.profile_id=pg.profile_id AND z.game_id=g.id AND z.status='completed'),g.iso_path,g.rom_path,g.emulator_id,em.name,rl.ra_game_id,g.source,
    COALESCE(sl.appid, CASE WHEN g.source='steam' THEN CAST(g.external_id AS INTEGER) END),g.logo,pg.favorited_at
    FROM profile_games pg JOIN games g ON g.id = pg.game_id
    LEFT JOIN game_info gi ON gi.game_id = g.id
    LEFT JOIN emulators em ON em.id = g.emulator_id
    LEFT JOIN ra_links rl ON rl.game_id = g.id AND rl.status IN ('hash','manual')
    LEFT JOIN steam_links sl ON sl.game_id = g.id";

/// Converte datas da Steam ("21 de fev. de 2019", "Feb 21, 2019", "2019") em "AAAA-MM-DD" para ordenar.
pub fn parse_release(s: &str) -> Option<String> {
    let (mut year, mut month, mut day) = (None, None, None);
    for tok in s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        if tok.chars().all(|c| c.is_ascii_digit()) {
            match tok.len() {
                4 => year = tok.parse::<i32>().ok().filter(|y| (1970..=2100).contains(y)),
                1 | 2 if day.is_none() => day = tok.parse::<u32>().ok().filter(|d| (1..=31).contains(d)),
                _ => {}
            }
        } else if month.is_none() && tok.chars().count() >= 3 {
            let p: String = tok.chars().take(3).collect();
            month = match p.as_str() {
                "jan" => Some(1), "fev" | "feb" => Some(2), "mar" => Some(3), "abr" | "apr" => Some(4),
                "mai" | "may" => Some(5), "jun" => Some(6), "jul" => Some(7), "ago" | "aug" => Some(8),
                "set" | "sep" => Some(9), "out" | "oct" => Some(10), "nov" => Some(11), "dez" | "dec" => Some(12),
                _ => None,
            };
        }
    }
    year.map(|y| format!("{y:04}-{:02}-{:02}", month.unwrap_or(1), day.unwrap_or(1)))
}

fn map(r: &Row) -> rusqlite::Result<Game> {
    let release_date: Option<String> = r.get(14)?;
    Ok(Game {
        release_iso: release_date.as_deref().and_then(parse_release),
        release_date,
        completed: r.get::<_, i64>(17)? != 0,
        completed_at: r.get(18)?,
        iso_path: r.get(19)?,
        rom_path: r.get(20)?,
        emulator_id: r.get(21)?,
        emulator_name: r.get(22)?,
        ra_game_id: r.get(23)?,
        source: r.get(24)?,
        steam_app_id: r.get(25)?,
        installed: false,
        genre: r.get(15)?,
        genres: r
            .get::<_, Option<String>>(16)?
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default(),
        id: r.get(0)?,
        title: r.get(1)?,
        platform: r.get(2)?,
        executable_path: r.get(3)?,
        launch_args: r.get(4)?,
        cover: crate::images::resolve(r.get(5)?),
        favorite: r.get::<_, i64>(6)? != 0,
        favorited_at: r.get(27)?,
        playtime_seconds: r.get(7)?,
        last_played_at: r.get(8)?,
        added_at: r.get(9)?,
        background: crate::images::resolve(r.get(10)?),
        bg_x: r.get(11)?,
        bg_y: r.get(12)?,
        logo: crate::images::resolve(r.get(26)?),
        steam_id: r.get(13)?,
    })
}

/// Marca `installed` em cada jogo. Os manifestos da Steam são lidos uma vez por chamada.
fn mark_installed(games: &mut [Game]) {
    let steam = if games.iter().any(|g| g.steam_app_id.is_some()) { crate::steam::local::local_apps() } else { Default::default() };
    for g in games.iter_mut() {
        let steam_ok = g.steam_app_id.and_then(|a| steam.get(&a)).map(|a| a.installed).unwrap_or(false);
        let steam_only = g.source == "steam" && g.executable_path.trim().is_empty();
        let file_ok = || {
            let exists = |p: &Option<String>| p.as_deref().map(|p| !p.trim().is_empty() && Path::new(p).exists()).unwrap_or(false);
            exists(&Some(g.executable_path.clone())) || exists(&g.iso_path) || exists(&g.rom_path)
                || crate::msstore::installed(&g.executable_path)
        };
        g.installed = steam_ok || (!steam_only && file_ok());
    }
}

fn validate(i: &mut GameInput, allow_no_exe: bool) -> AppResult<()> {
    i.title = i.title.trim().to_string();
    i.platform = i.platform.trim().to_string();
    i.executable_path = i.executable_path.trim().to_string();
    i.launch_args = i.launch_args.trim().to_string();
    let t = i.title.chars().count();
    if t == 0 || t > 80 {
        return Err(AppError::Validation("Título: 1 a 80 caracteres.".into()));
    }
    if i.platform.is_empty() {
        i.platform = "PC".into();
    }
    // Padroniza o valor gravado (evita duplicatas por diferença de escrita). Valores antigos
    // sem equivalente claro (ex.: "RetroArch") são preservados como estão.
    if let Some(c) = canonical_platform(&i.platform) {
        i.platform = c.into();
    }
    // Jogo importado por ISO pode ficar sem emulador por enquanto (configurado depois).
    if i.executable_path.is_empty() && !allow_no_exe {
        return Err(AppError::Validation("Selecione o executável do jogo.".into()));
    }
    // Jogo da Microsoft Store / Xbox: `shell:AppsFolder\<AUMID>` (ver `msstore`), não é um arquivo.
    if crate::msstore::is_app(&i.executable_path) {
        if !crate::msstore::installed(&i.executable_path) {
            return Err(AppError::Validation("Este jogo da Microsoft Store / Xbox não está instalado neste PC.".into()));
        }
    } else if !i.executable_path.is_empty() && !Path::new(&i.executable_path).is_file() {
        return Err(AppError::Validation(
            "O executável selecionado não existe neste computador.".into(),
        ));
    }
    if i.launch_args.len() > 1000 {
        return Err(AppError::Validation("Argumentos muito longos (máx. 1000).".into()));
    }
    Ok(())
}

/// Formatos de arquivo de jogo aceitos no "Adicionar jogo" (iniciados por um emulador). O .iso vai para
/// `iso_path`; os demais para `rom_path`, como na importação por pasta.
const GAME_FILE_EXTS: [&str; 8] = ["iso", "bin", "cue", "nsp", "xci", "nup", "pkg", "vpk"];

/// O caminho já pertence a um jogo do perfil? Compara caminhos normalizados (maiúsculas, barras).
/// `file`: compara com arquivos de jogo (ISO/ROM); senão, com executáveis de jogos sem arquivo.
fn in_library(conn: &Connection, profile_id: i64, path: &str, file: bool) -> AppResult<bool> {
    let key = super::scan::norm_key(path);
    let sql = if file {
        "SELECT g.iso_path,g.rom_path FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1"
    } else {
        "SELECT g.executable_path,NULL FROM profile_games pg JOIN games g ON g.id=pg.game_id
         WHERE pg.profile_id=?1 AND g.iso_path IS NULL AND g.rom_path IS NULL"
    };
    let mut st = conn.prepare(sql)?;
    let hit = st
        .query_map([profile_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))?
        .filter_map(Result::ok)
        .any(|(a, b)| [a, b].iter().flatten().any(|s| super::scan::norm_key(s) == key));
    Ok(hit)
}

/// Valida o arquivo do jogo sem tocar nele: formato, existência, leitura e duplicidade na biblioteca do perfil.
/// Devolve se é um .iso.
fn validate_game_file(conn: &Connection, profile_id: i64, file: &str) -> AppResult<bool> {
    let p = Path::new(file);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !GAME_FILE_EXTS.contains(&ext.as_str()) {
        return Err(AppError::Validation("Formato não suportado. Escolha um arquivo .exe, .iso, .bin, .cue, .nsp, .xci, .nup, .pkg ou .vpk.".into()));
    }
    if !p.is_file() {
        return Err(AppError::Validation("O arquivo do jogo não existe ou está inacessível.".into()));
    }
    std::fs::File::open(p).map_err(|_| {
        AppError::Validation("Não foi possível ler o arquivo do jogo (sem permissão ou em uso por outro programa).".into())
    })?;
    if in_library(conn, profile_id, file, true)? {
        return Err(AppError::Validation("Este jogo já está na sua biblioteca.".into()));
    }
    Ok(ext == "iso")
}

/// Troca o local do arquivo (ISO/ROM) de um jogo importado — ex.: o arquivo foi movido de pasta.
/// Só referencia o novo caminho (nada é copiado ou movido) e grava na mesma coluna que o jogo já usa.
/// Jogos sem arquivo (executável/Steam) não são alterados.
fn relocate_file(conn: &Connection, profile_id: i64, game_id: i64, new_file: &str) -> AppResult<()> {
    let (iso, rom): (Option<String>, Option<String>) =
        conn.query_row("SELECT iso_path,rom_path FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let column = match (&iso, &rom) {
        (_, Some(r)) => { if r == new_file { return Ok(()); } "rom_path" }
        (Some(i), None) => { if i == new_file { return Ok(()); } "iso_path" }
        (None, None) => return Ok(()),
    };
    let p = Path::new(new_file);
    if !p.is_file() {
        return Err(AppError::Validation("O arquivo do jogo não existe ou está inacessível.".into()));
    }
    std::fs::File::open(p).map_err(|_| {
        AppError::Validation("Não foi possível ler o arquivo do jogo (sem permissão ou em uso por outro programa).".into())
    })?;
    let key = super::scan::norm_key(new_file);
    let mut st = conn.prepare(
        "SELECT g.iso_path,g.rom_path FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1 AND g.id<>?2",
    )?;
    let taken = st
        .query_map(params![profile_id, game_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))?
        .filter_map(Result::ok)
        .any(|(a, b)| [a, b].iter().flatten().any(|s| super::scan::norm_key(s) == key));
    if taken {
        return Err(AppError::Validation("Este arquivo já pertence a outro jogo da sua biblioteca.".into()));
    }
    conn.execute(&format!("UPDATE games SET {column}=?1 WHERE id=?2"), params![new_file, game_id])?;
    Ok(())
}

fn fetch(conn: &Connection, profile_id: i64, game_id: i64) -> AppResult<Game> {
    let mut g = conn.query_row(
        &format!("{SELECT} WHERE pg.profile_id=?1 AND pg.game_id=?2"),
        params![profile_id, game_id],
        map,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            AppError::NotFound("Jogo não encontrado na biblioteca.".into())
        }
        other => other.into(),
    })?;
    mark_installed(std::slice::from_mut(&mut g));
    Ok(g)
}

#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn list_library(db: State<Db>, profile_id: i64) -> AppResult<Vec<Game>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE pg.profile_id=?1 ORDER BY g.title COLLATE NOCASE"
    ))?;
    let mut rows = stmt
        .query_map([profile_id], map)?
        .collect::<Result<Vec<_>, _>>()?;
    mark_installed(&mut rows);
    Ok(rows)
}

#[tauri::command]
pub fn add_game(db: State<Db>, profile_id: i64, input: GameInput) -> AppResult<Game> {
    add(&db, profile_id, input)
}

fn add(db: &Db, profile_id: i64, mut input: GameInput) -> AppResult<Game> {
    let iso = input.iso_path.clone().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    validate(&mut input, iso.is_some())?;
    input.cover = crate::images::store(input.cover.take())?;
    input.background = crate::images::store(input.background.take())?;
    let logo = crate::images::store(input.logo.take().flatten())?;
    let mut conn = db.conn()?;
    let is_iso = match &iso {
        Some(p) => validate_game_file(&conn, profile_id, p)?,
        None => {
            if in_library(&conn, profile_id, &input.executable_path, false)? {
                return Err(AppError::Validation("Este jogo já está na sua biblioteca.".into()));
            }
            false
        }
    };
    let (iso_col, rom_col) = if is_iso { (iso.clone(), None) } else { (None, iso.clone()) };
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO games(title,platform,executable_path,launch_args,cover,background,bg_x,bg_y,iso_path,rom_path,source,logo)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            input.title,
            input.platform,
            input.executable_path,
            input.launch_args,
            input.cover,
            input.background,
            input.bg_x.unwrap_or(50).clamp(0, 100),
            input.bg_y.unwrap_or(50).clamp(0, 100),
            iso_col,
            rom_col,
            match (&iso, is_iso) { (Some(_), true) => "iso", (Some(_), false) => "rom", _ => "manual" },
            logo
        ],
    )?;
    let game_id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO profile_games(profile_id,game_id) VALUES(?1,?2)",
        params![profile_id, game_id],
    )?;
    tx.commit()?;
    fetch(&conn, profile_id, game_id)
}

#[tauri::command]
pub fn update_game(
    db: State<Db>,
    profile_id: i64,
    game_id: i64,
    mut input: GameInput,
) -> AppResult<Game> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    // `input.iso_path`: novo local do arquivo do jogo (ISO/ROM), se o usuário trocou (ver `relocate_file`).
    // ISO/ROM ou jogo da Steam (abre pelo cliente Steam) podem ficar sem executável
    let no_exe_ok: bool = conn
        .query_row("SELECT COALESCE(iso_path, rom_path) IS NOT NULL OR source='steam' OR EXISTS(SELECT 1 FROM steam_links WHERE game_id=?1) FROM games WHERE id=?1", [game_id], |r| r.get(0))
        .unwrap_or(false);
    validate(&mut input, no_exe_ok)?;
    let old_platform: Option<String> = conn.query_row("SELECT platform FROM games WHERE id=?1", [game_id], |r| r.get(0)).ok();
    input.cover = crate::images::store(input.cover.take())?;
    input.background = crate::images::store(input.background.take())?;
    // logo: só muda quando o formulário a envia (ausente = mantém)
    let (set_logo, logo) = match input.logo.take() {
        Some(v) => (true, crate::images::store(v)?),
        None => (false, None),
    };
    let old_logo: Option<String> = conn.query_row("SELECT logo FROM games WHERE id=?1", [game_id], |r| r.get(0)).ok().flatten();
    let n = conn.execute(
        "UPDATE games SET title=?1,platform=?2,executable_path=?3,launch_args=?4,cover=?5,
         background=?6,bg_x=?7,bg_y=?8,logo=CASE WHEN ?11 THEN ?12 ELSE logo END
         WHERE id=?9 AND id IN (SELECT game_id FROM profile_games WHERE profile_id=?10)",
        params![
            input.title,
            input.platform,
            input.executable_path,
            input.launch_args,
            input.cover,
            input.background,
            input.bg_x.unwrap_or(50).clamp(0, 100),
            input.bg_y.unwrap_or(50).clamp(0, 100),
            game_id,
            profile_id,
            set_logo,
            logo
        ],
    )?;
    if n == 0 {
        return Err(AppError::NotFound("Jogo não encontrado na biblioteca.".into()));
    }
    // logo escolhida ou removida pelo usuário: a busca automática não mexe mais neste jogo
    if set_logo && logo != old_logo {
        conn.execute("INSERT OR REPLACE INTO logo_auto(game_id, found) VALUES(?1, 1)", [game_id])?;
    }
    if let Some(new_file) = input.iso_path.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        relocate_file(&conn, profile_id, game_id, new_file)?;
    }
    // plataforma trocada: a nota do Metacritic passa a ser a da versão escolhida (cache, sem rede)
    if old_platform.as_deref() != Some(input.platform.as_str()) {
        super::metacritic::on_platform_changed(&conn, game_id, &input.platform)?;
    }
    fetch(&conn, profile_id, game_id)
}

#[tauri::command]
pub fn remove_game(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM play_sessions WHERE profile_id=?1 AND game_id=?2",
        params![profile_id, game_id],
    )?;
    tx.execute(
        "DELETE FROM profile_games WHERE profile_id=?1 AND game_id=?2",
        params![profile_id, game_id],
    )?;
    tx.execute(
        "DELETE FROM games WHERE id=?1 AND id NOT IN (SELECT game_id FROM profile_games)",
        [game_id],
    )?;
    tx.commit()?;
    Ok(())
}

#[tauri::command]
pub fn set_favorite(db: State<Db>, profile_id: i64, game_id: i64, favorite: bool) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        // a data de entrada nos favoritos define a ordem do carrossel Favoritos (mais recente primeiro)
        "UPDATE profile_games SET favorite=?1,
         favorited_at=CASE WHEN ?1=1 THEN COALESCE(CASE WHEN favorite=1 THEN favorited_at END, strftime('%Y-%m-%d %H:%M:%f','now')) ELSE NULL END
         WHERE profile_id=?2 AND game_id=?3",
        params![favorite as i64, profile_id, game_id],
    )?;
    Ok(())
}

/// Garante que o jogo pertence à biblioteca do perfil.
pub fn ensure_owned(conn: &Connection, profile_id: i64, game_id: i64) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM profile_games WHERE profile_id=?1 AND game_id=?2",
        params![profile_id, game_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        Err(AppError::NotFound("Jogo não encontrado na biblioteca.".into()))
    } else {
        Ok(())
    }
}

/// Plataformas oficiais do NEXSTAGE, na ordem exibida na interface.
pub const PLATFORMS: [&str; 15] = [
    "PC", "XBOX", "Xbox 360", "Xbox Series", "Playstation 1", "Playstation 2", "Playstation 3", "Playstation 4",
    "Playstation 5", "PSP", "Ps Vita", "Nintendo Switch", "Nintendo Switch 2", "Super Nintendo", "Nintendinho",
];

/// Converte nomes antigos/variações para o valor oficial. `None` = sem equivalente seguro.
/// (Emuladores com sistema único mapeiam para o sistema: PCSX2→PS2, RPCS3→PS3, PPSSPP→PSP;
/// Steam / Epic / Game Pass já eram jogos de PC.)
pub fn canonical_platform(s: &str) -> Option<&'static str> {
    let k = s.trim().to_lowercase();
    Some(match k.as_str() {
        "pc" | "steam" | "epic games" | "epic" | "xbox / game pass" | "xbox/game pass" | "game pass" | "pc game pass" | "windows" => "PC",
        "xbox" | "xbox one" | "xbox original" => "XBOX",
        "xbox series" | "xbox series x" | "xbox series s" | "xbox series x|s" | "xbox series x/s" | "xsx" => "Xbox Series",
        "xbox 360" | "x360" => "Xbox 360",
        "playstation 1" | "playstation" | "ps1" | "psx" | "psone" | "ps one" | "playstation one" => "Playstation 1",
        "playstation 2" | "ps2" | "pcsx2" => "Playstation 2",
        "playstation 3" | "ps3" | "rpcs3" => "Playstation 3",
        "playstation 4" | "ps4" => "Playstation 4",
        "playstation 5" | "ps5" => "Playstation 5",
        "psp" | "ppsspp" | "playstation portable" => "PSP",
        "ps vita" | "psvita" | "playstation vita" | "vita" => "Ps Vita",
        "nintendo switch" | "switch" => "Nintendo Switch",
        "nintendo switch 2" | "switch 2" | "switch2" | "ns2" | "lichii" => "Nintendo Switch 2",
        "super nintendo" | "snes" | "super nes" | "super famicom" => "Super Nintendo",
        "nintendinho" | "nes" | "famicom" | "nintendo 8 bits" => "Nintendinho",
        _ => return None,
    })
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn favorite_date_is_set_kept_and_cleared() {
        let p = std::env::temp_dir().join(format!("nexstage-fav-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let db = Db::open(&p).unwrap();
        let c = db.conn().unwrap();
        c.execute_batch(
            "INSERT INTO profiles(id,username,display_name) VALUES(1,'ana','Ana');
             INSERT INTO games(id,title,executable_path) VALUES(1,'Jogo','x');
             INSERT INTO profile_games(profile_id,game_id) VALUES(1,1);",
        ).unwrap();
        let fav = |c: &Connection| -> (bool, Option<String>) {
            let g = c.prepare(&format!("{SELECT} WHERE pg.profile_id=1")).unwrap().query_map([], map).unwrap().next().unwrap().unwrap();
            (g.favorite, g.favorited_at)
        };
        let set = |c: &Connection, on: bool| {
            c.execute(
                "UPDATE profile_games SET favorite=?1,
                 favorited_at=CASE WHEN ?1=1 THEN COALESCE(CASE WHEN favorite=1 THEN favorited_at END, strftime('%Y-%m-%d %H:%M:%f','now')) ELSE NULL END
                 WHERE profile_id=1 AND game_id=1",
                [on as i64],
            ).unwrap();
        };
        assert_eq!(fav(&c), (false, None));
        set(&c, true);
        let (on, at) = fav(&c);
        assert!(on && at.is_some(), "favoritar grava a data (coluna lida na posição certa)");
        set(&c, true);
        assert_eq!(fav(&c).1, at, "favoritar de novo mantém a data original");
        set(&c, false);
        assert_eq!(fav(&c), (false, None), "desfavoritar limpa a data");
        drop(c);
        drop(db);
        let _ = std::fs::remove_file(&p);
    }


    fn game(source: &str, exe: &str, iso: Option<String>, appid: Option<i64>) -> Game {
        Game {
            id: 1, title: "x".into(), platform: "PC".into(), executable_path: exe.into(), launch_args: String::new(), cover: None, favorite: false, favorited_at: None,
            playtime_seconds: 0, last_played_at: None, added_at: String::new(), background: None, bg_x: 50, bg_y: 50, logo: None, steam_id: None,
            release_date: None, release_iso: None, genre: None, genres: vec![], completed: false, completed_at: None, iso_path: iso, rom_path: None,
            emulator_id: None, emulator_name: None, ra_game_id: None, source: source.into(), steam_app_id: appid, installed: false,
        }
    }

    #[test]
    fn installed_status() {
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        let mut v = vec![
            game("manual", &exe, None, None),                           // executável existe
            game("manual", r"Z:\nao\existe.exe", None, None),         // executável sumiu
            game("iso", "", Some(exe.clone()), None),                   // arquivo do jogo existe
            game("steam", "", None, Some(999_999_999)),                 // Steam, sem manifesto
        ];
        mark_installed(&mut v);
        assert_eq!(v.iter().map(|g| g.installed).collect::<Vec<_>>(), vec![true, false, true, false]);
    }

    #[test]
    fn maps_old_values_and_keeps_unknown() {
        assert_eq!(canonical_platform("Steam"), Some("PC"));
        assert_eq!(canonical_platform("Xbox Series X"), Some("Xbox Series"));
        assert_eq!(canonical_platform("xbox one"), Some("XBOX"));
        assert_eq!(canonical_platform("  pcsx2 "), Some("Playstation 2"));
        assert_eq!(canonical_platform("PPSSPP"), Some("PSP"));
        assert_eq!(canonical_platform("Ps Vita"), Some("Ps Vita"));
        assert_eq!(canonical_platform("Switch 2"), Some("Nintendo Switch 2"));
        assert_eq!(canonical_platform("RetroArch"), None);
        assert!(PLATFORMS.iter().all(|p| canonical_platform(p) == Some(*p)));
    }
}

#[cfg(test)]
mod add_tests {
    //! "Adicionar jogo" de ponta a ponta num banco temporário: formatos, coluna gravada, emulador e duplicidade.
    use super::*;
    use crate::commands::scan;
    use std::path::PathBuf;

    fn input(title: &str, platform: &str, exe: &str, file: Option<&Path>) -> GameInput {
        GameInput {
            title: title.into(), platform: platform.into(), executable_path: exe.into(), launch_args: String::new(),
            cover: None, background: None, bg_x: None, bg_y: None, logo: None, iso_path: file.map(|p| p.to_string_lossy().to_string()),
        }
    }

    #[test]
    fn logo_absent_keeps_null_removes_value_sets() {
        let base = r#""title":"Celeste","platform":"PC","executablePath":"C:/c.exe","cover":null"#;
        let parse = |extra: &str| serde_json::from_str::<GameInput>(&format!("{{{base}{extra}}}")).unwrap().logo;
        assert_eq!(parse(""), None, "formulário antigo sem o campo: mantém a logo");
        assert_eq!(parse(r#","logo":null"#), Some(None), "remover");
        assert_eq!(parse(r#","logo":"https://cdn2.steamgriddb.com/logo/a.png""#), Some(Some("https://cdn2.steamgriddb.com/logo/a.png".to_string())));
        // gravada no cadastro e devolvida na listagem
        let dir = std::env::temp_dir().join(format!("nexstage-logo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        db.conn().unwrap().execute_batch("INSERT INTO profiles(id, username, display_name) VALUES(1,'a','A');").unwrap();
        let exe = dir.join("celeste.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let mut i = input("Celeste", "PC", &exe.to_string_lossy(), None);
        i.logo = Some(Some(String::from("https://cdn2.steamgriddb.com/logo/a.png")));
        let g = add(&db, 1, i).unwrap();
        assert_eq!(g.logo.as_deref(), Some("https://cdn2.steamgriddb.com/logo/a.png"));
    }

    #[test]
    fn adds_every_supported_format_once() {
        let dir = std::env::temp_dir().join(format!("nexstage-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Pasta Ação")).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        db.conn().unwrap().execute_batch(
            "INSERT INTO profiles(id, username, display_name) VALUES(1,'a','A');
             INSERT INTO emulators(id,name,exe_path,platforms,formats,args) VALUES
               (7,'Switch','C:/e.exe','[\"Nintendo Switch\"]','[\"nsp\",\"xci\"]','-f -g \"{rom}\"');",
        ).unwrap();
        let mk = |n: &str, d: &[u8]| { let p = dir.join("Pasta Ação").join(n); std::fs::write(&p, d).unwrap(); p };

        // EXE (jogo de PC): detectado como PC; adicionado; o mesmo caminho de novo é recusado
        let exe = std::env::current_exe().unwrap();
        let it = scan::inspect(&db, 1, &exe.to_string_lossy()).unwrap();
        assert_eq!((it.kind.as_str(), it.platform.as_deref()), ("windows", Some("PC")));
        let g = add(&db, 1, input("Meu Jogo", "PC", &exe.to_string_lossy(), None)).unwrap();
        assert_eq!((g.source.as_str(), g.iso_path.as_deref(), g.rom_path.as_deref()), ("manual", None, None));
        assert!(add(&db, 1, input("De novo", "PC", &exe.to_string_lossy().to_uppercase(), None)).is_err());

        // ISO → iso_path; duplicado (mesmo caminho em maiúsculas) é recusado
        let iso = mk("Jogo [EUA].iso", &vec![0u8; 40 * 2048]);
        let g = add(&db, 1, input("Jogo", "Playstation 2", "", Some(&iso))).unwrap();
        assert_eq!((g.source.as_str(), g.iso_path.is_some(), g.rom_path.is_none()), ("iso", true, true));
        let up = PathBuf::from(iso.to_string_lossy().to_uppercase());
        assert!(add(&db, 1, input("Jogo", "Playstation 2", "", Some(&up))).is_err());
        assert_eq!(scan::inspect(&db, 1, &iso.to_string_lossy()).unwrap().status, "duplicate");

        // XCI → rom_path; o emulador compatível (plataforma + formato) é sugerido antes de importar
        let mut x = vec![0u8; 0x200]; x[0x100..0x104].copy_from_slice(b"HEAD");
        let xci = mk("Zelda (v1.0).xci", &x);
        let it = scan::inspect(&db, 1, &xci.to_string_lossy()).unwrap();
        assert_eq!((it.platform.as_deref(), it.status.as_str(), it.assigned), (Some("Nintendo Switch"), "new", Some(7)));
        let g = add(&db, 1, input("Zelda", "Nintendo Switch", "", Some(&xci))).unwrap();
        assert_eq!((g.source.as_str(), g.rom_path.is_some(), g.iso_path.is_none()), ("rom", true, true));
        assert_eq!(scan::inspect(&db, 1, &xci.to_string_lossy()).unwrap().status, "duplicate");

        // BIN, CUE, NSP, NUP, PKG, VPK também são aceitos
        for (n, d) in [("a.bin", &b"x"[..]), ("b.cue", b"FILE \"a.bin\" BINARY"), ("c.nsp", b"PFS0...."), ("d.nup", b"x"),
                       ("e.pkg", &[0x7F, b'P', b'K', b'G', 0, 0, 0, 1]), ("f.vpk", b"PK\x03\x04 sce_sys")] {
            let p = mk(n, d);
            add(&db, 1, input(n, "PC", "", Some(&p))).unwrap_or_else(|e| panic!("{n}: {e:?}"));
        }
        // formato fora da lista: recusado nos dois caminhos
        let txt = mk("leia.txt", b"oi");
        assert!(add(&db, 1, input("x", "PC", "", Some(&txt))).is_err());
        assert!(scan::inspect(&db, 1, &txt.to_string_lossy()).is_err());
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Onde o jogo está no disco: o arquivo do jogo (ISO/ROM) ou o executável; atalho (.lnk) → o arquivo
/// original; jogo da Steam sem executável → a pasta de instalação da Steam.
fn game_location(conn: &Connection, game_id: i64) -> AppResult<std::path::PathBuf> {
    use std::path::PathBuf;
    let (exe, file): (String, Option<String>) = conn
        .query_row("SELECT executable_path, COALESCE(NULLIF(trim(iso_path),''), NULLIF(trim(rom_path),'')) FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| AppError::NotFound("Jogo não encontrado.".into()))?;
    let raw = file.unwrap_or(exe).trim().to_string();
    if raw.is_empty() || raw.to_lowercase().starts_with("steam://") {
        if let Some(appid) = crate::steam::store::appid_of(conn, game_id)? {
            if let Some(dir) = crate::steam::local::install_dir(appid) {
                return Ok(dir);
            }
            return Err(AppError::NotFound("Este jogo da Steam não está instalado neste PC.".into()));
        }
        return Err(AppError::NotFound("Este jogo não tem um arquivo ou pasta cadastrado.".into()));
    }
    if let Some(aumid) = crate::msstore::aumid(&raw) {
        return crate::msstore::package_root(aumid)
            .ok_or_else(|| AppError::NotFound("Este jogo da Microsoft Store / Xbox não está instalado neste PC.".into()));
    }
    let mut p = PathBuf::from(&raw);
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
        // atalho: o arquivo original (leitura nativa; o Shell do Windows como alternativa)
        p = match crate::lnk::read(&p) {
            Some(t) => t.path,
            None => match crate::msstore::resolve_lnk(&p) {
                Ok(s) if crate::msstore::aumid(&s.target).is_some() => {
                    return crate::msstore::package_root(crate::msstore::aumid(&s.target).unwrap_or_default())
                        .ok_or_else(|| AppError::NotFound("Este jogo da Microsoft Store / Xbox não está instalado neste PC.".into()));
                }
                Ok(s) => PathBuf::from(s.target),
                Err(_) => p, // mostra o próprio atalho
            },
        };
    }
    if p.exists() {
        Ok(p)
    } else {
        Err(AppError::NotFound("O arquivo deste jogo não foi encontrado no disco. Ele pode ter sido movido ou apagado.".into()))
    }
}

/// "Abrir pasta de arquivos": abre o Explorador na pasta do jogo, com o arquivo selecionado.
#[tauri::command]
pub fn reveal_game_folder(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<()> {
    let p = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        game_location(&conn, game_id)?
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let s = p.to_string_lossy().replace('/', "\\");
        let mut cmd = std::process::Command::new("explorer");
        if p.is_dir() { cmd.arg(&s); } else { cmd.raw_arg(format!("/select,\"{s}\"")); }
        cmd.spawn().map_err(|e| AppError::Other(format!("Não foi possível abrir o Explorador de Arquivos: {e}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        Err(AppError::Other("Disponível só no Windows.".into()))
    }
}
