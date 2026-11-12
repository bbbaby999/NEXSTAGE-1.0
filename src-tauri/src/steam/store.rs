//! Cache local da Steam (por perfil) e ligação com a biblioteca do NEXSTAGE.
//!
//! - `steam_owned`: jogos da conta (GetOwnedGames) ou, sem chave, os instalados no PC. O tempo de jogo
//!   da Steam fica AQUI, como dado externo: nunca altera `profile_games.playtime_seconds` (histórico local).
//! - `steam_links`: jogo da biblioteca ↔ AppID (criado só pela importação confirmada ou pelo usuário).
//! - `steam_ach_schema` / `steam_ach_user` / `steam_stats`: conquistas e estatísticas públicas.

use crate::error::AppResult;
use crate::retro::link::norm_title;
use crate::retro::util::{fmt_epoch, int, text};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const PLATFORM: &str = "steam";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS steam_owned (
    profile_id         INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    appid              INTEGER NOT NULL,
    name               TEXT NOT NULL,
    icon_hash          TEXT,
    playtime_forever   INTEGER,           -- minutos (NULL = Steam não informou)
    playtime_2weeks    INTEGER,
    last_played        INTEGER,           -- epoch (rtime_last_played)
    recent             INTEGER NOT NULL DEFAULT 0,
    has_stats          INTEGER NOT NULL DEFAULT 0,
    source             TEXT NOT NULL DEFAULT 'api',   -- 'api' | 'local'
    updated_at         TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, appid)
);
CREATE TABLE IF NOT EXISTS steam_links (
    game_id  INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    appid    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_steam_links_app ON steam_links(appid);
CREATE TABLE IF NOT EXISTS steam_ach_schema (
    appid        INTEGER NOT NULL,
    apiname      TEXT NOT NULL,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    icon         TEXT,
    icon_gray    TEXT,
    hidden       INTEGER NOT NULL DEFAULT 0,
    global_pct   REAL,
    sort         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (appid, apiname)
);
CREATE TABLE IF NOT EXISTS steam_ach_user (
    profile_id   INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    appid        INTEGER NOT NULL,
    apiname      TEXT NOT NULL,
    unlocked_at  INTEGER,                 -- epoch; NULL = pendente
    PRIMARY KEY (profile_id, appid, apiname)
);
CREATE TABLE IF NOT EXISTS steam_stats (
    profile_id   INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    appid        INTEGER NOT NULL,
    stats        TEXT,                    -- JSON [{name, label, value}]
    error        TEXT,                    -- 'private' | 'nostats' | mensagem
    fetched_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, appid)
);
"#;

/// Endereço atual da CDN para imagens da Comunidade Steam (ícones de jogos e de conquistas).
/// A Web API ainda devolve o caminho antigo (`steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/…`),
/// que dá 404 para jogos mais novos; o caminho `community_assets` atende os antigos e os novos.
/// Aplicado na leitura, corrige também o que já está no cache.
const COMMUNITY_ASSETS: &str = "https://shared.akamai.steamstatic.com/community_assets/images/apps/";

pub fn media_url(url: &str) -> String {
    const OLD_PATH: &str = "/steamcommunity/public/images/apps/";
    match url.find(OLD_PATH) {
        Some(i) if url.starts_with("https://") && is_steam_host(&url[8..i]) => format!("{COMMUNITY_ASSETS}{}", &url[i + OLD_PATH.len()..]),
        _ => url.to_string(),
    }
}

fn is_steam_host(h: &str) -> bool {
    h == "steamcdn-a.akamaihd.net" || h == "media.steampowered.com" || h.ends_with(".steamstatic.com")
}

/// Ícone pequeno do jogo (hash `img_icon_url` do GetOwnedGames).
pub fn app_icon_url(appid: i64, hash: &str) -> String {
    format!("{COMMUNITY_ASSETS}{appid}/{hash}.jpg")
}

pub fn cover_url(appid: i64) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg")
}
pub fn header_url(appid: i64) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg")
}
pub fn store_url(appid: i64) -> String {
    format!("https://store.steampowered.com/app/{appid}/")
}

// ---------------------------------------------------------------- biblioteca da conta

/// Substitui a lista da conta (GetOwnedGames). Mantém a marcação "recente" calculada depois.
pub fn store_owned(conn: &Connection, profile_id: i64, games: &[Value]) -> AppResult<usize> {
    conn.execute("DELETE FROM steam_owned WHERE profile_id=?1", [profile_id])?;
    let mut n = 0;
    for g in games {
        let appid = int(&g["appid"]);
        if appid <= 0 {
            continue;
        }
        let name = text(&g["name"]);
        let opt = |k: &str| g.get(k).filter(|v| !v.is_null()).map(int);
        conn.execute(
            "INSERT OR REPLACE INTO steam_owned(profile_id, appid, name, icon_hash, playtime_forever, playtime_2weeks, last_played, has_stats, source)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'api')",
            params![
                profile_id, appid, if name.is_empty() { format!("App {appid}") } else { name },
                Some(text(&g["img_icon_url"])).filter(|s| !s.is_empty()),
                opt("playtime_forever"), opt("playtime_2weeks"), opt("rtime_last_played").filter(|v| *v > 0),
                (g["has_community_visible_stats"].as_bool().unwrap_or(false)) as i64
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

/// Marca os jogados nas últimas 2 semanas (GetRecentlyPlayedGames).
pub fn store_recent(conn: &Connection, profile_id: i64, games: &[Value]) -> AppResult<()> {
    conn.execute("UPDATE steam_owned SET recent=0 WHERE profile_id=?1", [profile_id])?;
    for g in games {
        conn.execute(
            "UPDATE steam_owned SET recent=1, playtime_2weeks=COALESCE(?3, playtime_2weeks), playtime_forever=COALESCE(?4, playtime_forever)
             WHERE profile_id=?1 AND appid=?2",
            params![profile_id, int(&g["appid"]), g.get("playtime_2weeks").map(int), g.get("playtime_forever").map(int)],
        )?;
    }
    Ok(())
}

/// Sem chave: lista os jogos instalados no PC (cliente Steam local). Não substitui a lista da conta.
pub fn store_local(conn: &Connection, profile_id: i64, apps: &[super::local::LocalApp]) -> AppResult<()> {
    let has_api: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM steam_owned WHERE profile_id=?1 AND source='api')", [profile_id], |r| r.get(0))?;
    if has_api {
        return Ok(());
    }
    conn.execute("DELETE FROM steam_owned WHERE profile_id=?1 AND source='local'", [profile_id])?;
    for a in apps {
        conn.execute(
            "INSERT OR REPLACE INTO steam_owned(profile_id, appid, name, source) VALUES(?1, ?2, ?3, 'local')",
            params![profile_id, a.appid, if a.name.is_empty() { format!("App {}", a.appid) } else { a.name.clone() }],
        )?;
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InLibrary {
    pub game_id: i64,
    pub title: String,
    /// "linked" (já vinculado ao AppID) | "appid" (mesmo AppID nas informações do jogo) | "title" (mesmo nome)
    pub how: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OwnedRow {
    pub appid: i64,
    pub name: String,
    pub icon_url: Option<String>,
    pub cover_url: String,
    pub header_url: String,
    pub playtime_forever: Option<i64>,
    pub playtime_2weeks: Option<i64>,
    pub last_played: Option<String>,
    pub recent: bool,
    pub has_stats: bool,
    pub source: String,
    /// "installed" | "updating" | "installing" | "not_installed" | "unknown" (cliente Steam não encontrado)
    pub install: String,
    pub in_library: Option<InLibrary>,
}

/// Jogos da biblioteca do perfil, indexados por AppID e por título normalizado.
struct LibIndex {
    by_link: HashMap<i64, (i64, String)>,
    by_info: HashMap<i64, (i64, String)>,
    by_title: HashMap<Vec<String>, Vec<(i64, String)>>,
}

fn lib_index(conn: &Connection, profile_id: i64) -> AppResult<LibIndex> {
    let mut st = conn.prepare(
        "SELECT g.id, g.title, g.platform, sl.appid, CASE WHEN g.source='steam' THEN CAST(g.external_id AS INTEGER) END,
                CAST(json_extract(gi.data,'$.externalId') AS INTEGER)
         FROM profile_games pg JOIN games g ON g.id=pg.game_id
         LEFT JOIN steam_links sl ON sl.game_id=g.id LEFT JOIN game_info gi ON gi.game_id=g.id
         WHERE pg.profile_id=?1",
    )?;
    let mut idx = LibIndex { by_link: HashMap::new(), by_info: HashMap::new(), by_title: HashMap::new() };
    let rows = st.query_map([profile_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<i64>>(3)?, r.get::<_, Option<i64>>(4)?, r.get::<_, Option<i64>>(5)?))
    })?;
    for row in rows {
        let (id, title, platform, link, src, info) = row?;
        if let Some(a) = link.or(src) {
            idx.by_link.insert(a, (id, title.clone()));
        } else if let Some(a) = info.filter(|a| *a > 0) {
            idx.by_info.entry(a).or_insert((id, title.clone()));
        }
        if platform == "PC" && link.or(src).is_none() {
            idx.by_title.entry(norm_title(&title)).or_default().push((id, title));
        }
    }
    Ok(idx)
}

fn find_in_library(idx: &LibIndex, appid: i64, name: &str) -> Option<InLibrary> {
    if let Some((id, t)) = idx.by_link.get(&appid) {
        return Some(InLibrary { game_id: *id, title: t.clone(), how: "linked".into() });
    }
    if let Some((id, t)) = idx.by_info.get(&appid) {
        return Some(InLibrary { game_id: *id, title: t.clone(), how: "appid".into() });
    }
    let key = norm_title(name);
    match idx.by_title.get(&key) {
        // só um jogo com o mesmo nome: sugestão (o usuário confirma na prévia); vários = ambíguo, ignora
        Some(v) if v.len() == 1 && !key.is_empty() => Some(InLibrary { game_id: v[0].0, title: v[0].1.clone(), how: "title".into() }),
        _ => None,
    }
}

pub fn list_owned(conn: &Connection, profile_id: i64, local: &HashMap<i64, super::local::LocalApp>, client_found: bool) -> AppResult<Vec<OwnedRow>> {
    let idx = lib_index(conn, profile_id)?;
    let mut st = conn.prepare(
        "SELECT appid, name, icon_hash, playtime_forever, playtime_2weeks, last_played, recent, has_stats, source
         FROM steam_owned WHERE profile_id=?1 ORDER BY name COLLATE NOCASE",
    )?;
    let rows = st
        .query_map([profile_id], |r| {
            let appid: i64 = r.get(0)?;
            let name: String = r.get(1)?;
            let icon: Option<String> = r.get(2)?;
            Ok(OwnedRow {
                appid,
                icon_url: icon.map(|h| app_icon_url(appid, &h)),
                cover_url: cover_url(appid),
                header_url: header_url(appid),
                playtime_forever: r.get(3)?,
                playtime_2weeks: r.get(4)?,
                last_played: r.get::<_, Option<i64>>(5)?.map(fmt_epoch),
                recent: r.get::<_, i64>(6)? != 0,
                has_stats: r.get::<_, i64>(7)? != 0,
                source: r.get(8)?,
                install: String::new(),
                in_library: None,
                name,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|mut o| {
            o.install = match local.get(&o.appid) {
                Some(a) => a.state.clone(),
                None if client_found => "not_installed".into(),
                None => "unknown".into(),
            };
            o.in_library = find_in_library(&idx, o.appid, &o.name);
            o
        })
        .collect())
}

// ---------------------------------------------------------------- prévia e importação

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub appid: i64,
    pub name: String,
    /// "new" | "existing" (mesmo AppID) | "title" (mesmo nome: vincular só se o usuário aceitar)
    pub status: String,
    pub game_id: Option<i64>,
    pub game_title: Option<String>,
    /// Códigos do que muda: "link", "playtime", "cover", "launch" (a interface traduz).
    pub updates: Vec<String>,
}

pub fn preview(conn: &Connection, profile_id: i64, appids: &[i64]) -> AppResult<Vec<PreviewItem>> {
    let idx = lib_index(conn, profile_id)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for &appid in appids {
        if !seen.insert(appid) {
            continue;
        }
        let name: Option<String> = conn
            .query_row("SELECT name FROM steam_owned WHERE profile_id=?1 AND appid=?2", params![profile_id, appid], |r| r.get(0))
            .optional()?;
        let Some(name) = name else { continue }; // só jogos da lista desta conta/PC
        match find_in_library(&idx, appid, &name) {
            None => out.push(PreviewItem { appid, name, status: "new".into(), game_id: None, game_title: None, updates: vec![] }),
            Some(l) => {
                let has_cover: bool = conn.query_row("SELECT cover IS NOT NULL AND cover<>'' FROM games WHERE id=?1", [l.game_id], |r| r.get(0)).unwrap_or(true);
                let mut updates = Vec::new();
                if l.how != "linked" {
                    updates.push("link".into());
                    updates.push("launch".into());
                }
                updates.push("playtime".into());
                if !has_cover {
                    updates.push("cover".into());
                }
                out.push(PreviewItem {
                    appid,
                    name,
                    status: if l.how == "title" { "title" } else { "existing" }.into(),
                    game_id: Some(l.game_id),
                    game_title: Some(l.title),
                    updates,
                });
            }
        }
    }
    Ok(out)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: usize,
    pub linked: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Executa a importação confirmada. Jogos novos entram como "PC · Steam" (sem executável: abrem pelo
/// cliente Steam). Existentes só ganham o vínculo/capa; o histórico local não é tocado.
pub fn import(conn: &Connection, profile_id: i64, appids: &[i64], link_title_matches: bool) -> AppResult<ImportReport> {
    let items = preview(conn, profile_id, appids)?;
    let mut rep = ImportReport::default();
    for it in items {
        match it.status.as_str() {
            "new" => {
                conn.execute(
                    "INSERT INTO games(title, platform, executable_path, launch_args, cover, source, external_id) VALUES(?1, 'PC', '', '', ?2, 'steam', ?3)",
                    params![it.name.chars().take(80).collect::<String>(), cover_url(it.appid), it.appid.to_string()],
                )?;
                let gid = conn.last_insert_rowid();
                conn.execute("INSERT INTO profile_games(profile_id, game_id) VALUES(?1, ?2)", params![profile_id, gid])?;
                conn.execute("INSERT OR REPLACE INTO steam_links(game_id, appid) VALUES(?1, ?2)", params![gid, it.appid])?;
                ensure_info(conn, gid, it.appid, &it.name)?;
                rep.added += 1;
            }
            "title" if !link_title_matches => rep.skipped += 1,
            _ => {
                let gid = it.game_id.unwrap_or(0);
                if it.updates.iter().any(|u| u == "link") {
                    conn.execute("INSERT OR REPLACE INTO steam_links(game_id, appid) VALUES(?1, ?2)", params![gid, it.appid])?;
                    ensure_info(conn, gid, it.appid, &it.name)?;
                    rep.linked += 1;
                } else {
                    rep.updated += 1;
                }
                if it.updates.iter().any(|u| u == "cover") {
                    conn.execute("UPDATE games SET cover=?2 WHERE id=?1 AND (cover IS NULL OR cover='')", params![gid, cover_url(it.appid)])?;
                }
            }
        }
    }
    Ok(rep)
}

/// Informações mínimas do jogo (fonte Steam), sem apagar o que já foi buscado/editado.
fn ensure_info(conn: &Connection, game_id: i64, appid: i64, name: &str) -> AppResult<()> {
    if crate::commands::info::load(conn, game_id)?.is_some() {
        return Ok(());
    }
    let info = crate::commands::info::GameInfo {
        source: "steam".into(),
        external_id: Some(appid.to_string()),
        matched_name: name.into(),
        store_url: Some(store_url(appid)),
        header_image: Some(header_url(appid)),
        ..Default::default()
    };
    crate::commands::info::store(conn, game_id, &info)
}

/// Vincular/desvincular manualmente um jogo da biblioteca a um AppID.
pub fn set_link(conn: &Connection, profile_id: i64, game_id: i64, appid: Option<i64>) -> AppResult<()> {
    crate::commands::games::ensure_owned(conn, profile_id, game_id)?;
    match appid {
        Some(a) if a > 0 => {
            conn.execute("INSERT OR REPLACE INTO steam_links(game_id, appid) VALUES(?1, ?2)", params![game_id, a])?;
        }
        _ => {
            let is_steam: bool = conn.query_row("SELECT source='steam' AND executable_path='' FROM games WHERE id=?1", [game_id], |r| r.get(0))?;
            if is_steam {
                return Err(crate::error::AppError::Validation("Este jogo foi importado da Steam e não tem executável próprio: remova o jogo em vez de desvincular.".into()));
            }
            conn.execute("DELETE FROM steam_links WHERE game_id=?1", [game_id])?;
        }
    }
    Ok(())
}

/// AppID do jogo da biblioteca (vínculo ou jogo importado da Steam).
pub fn appid_of(conn: &Connection, game_id: i64) -> AppResult<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT COALESCE((SELECT appid FROM steam_links WHERE game_id=?1), CASE WHEN g.source='steam' THEN CAST(g.external_id AS INTEGER) END)
             FROM games g WHERE g.id=?1",
            [game_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten()
        .filter(|a| *a > 0))
}

// ---------------------------------------------------------------- conquistas e estatísticas

pub fn store_schema(conn: &Connection, appid: i64, game: &Value) -> AppResult<Vec<(String, String)>> {
    let achs = game["availableGameStats"]["achievements"].as_array().cloned().unwrap_or_default();
    conn.execute("DELETE FROM steam_ach_schema WHERE appid=?1", [appid])?;
    for (i, a) in achs.iter().enumerate() {
        let api = text(&a["name"]);
        if api.is_empty() {
            continue;
        }
        conn.execute(
            "INSERT OR REPLACE INTO steam_ach_schema(appid, apiname, name, description, icon, icon_gray, hidden, sort) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                appid, api, Some(text(&a["displayName"])).filter(|s| !s.is_empty()).unwrap_or_else(|| api.clone()),
                text(&a["description"]), Some(text(&a["icon"])).filter(|s| s.starts_with("https://")),
                Some(text(&a["icongray"])).filter(|s| s.starts_with("https://")), int(&a["hidden"]), i as i64
            ],
        )?;
    }
    let labels = game["availableGameStats"]["stats"]
        .as_array()
        .map(|s| s.iter().map(|x| (text(&x["name"]), text(&x["displayName"]))).collect())
        .unwrap_or_default();
    Ok(labels)
}

pub fn store_global(conn: &Connection, appid: i64, list: &[Value]) -> AppResult<()> {
    for a in list {
        let pct = match &a["percent"] {
            Value::String(s) => s.parse::<f64>().ok(),
            v => v.as_f64(),
        };
        conn.execute("UPDATE steam_ach_schema SET global_pct=?3 WHERE appid=?1 AND apiname=?2", params![appid, text(&a["name"]), pct])?;
    }
    Ok(())
}

pub fn store_player(conn: &Connection, profile_id: i64, appid: i64, list: &[Value]) -> AppResult<()> {
    conn.execute("DELETE FROM steam_ach_user WHERE profile_id=?1 AND appid=?2", params![profile_id, appid])?;
    for a in list {
        let done = int(&a["achieved"]) == 1;
        let t = int(&a["unlocktime"]);
        conn.execute(
            "INSERT OR REPLACE INTO steam_ach_user(profile_id, appid, apiname, unlocked_at) VALUES(?1,?2,?3,?4)",
            params![profile_id, appid, text(&a["apiname"]), if done { Some(t.max(1)) } else { None }],
        )?;
    }
    Ok(())
}

pub fn store_stats(conn: &Connection, profile_id: i64, appid: i64, stats: Option<&Value>, error: Option<&str>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO steam_stats(profile_id, appid, stats, error) VALUES(?1,?2,?3,?4)
         ON CONFLICT(profile_id, appid) DO UPDATE SET stats=COALESCE(excluded.stats, steam_stats.stats), error=excluded.error, fetched_at=datetime('now')",
        params![profile_id, appid, stats.map(|s| s.to_string()), error],
    )?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchRow {
    pub apiname: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub hidden: bool,
    pub global_pct: Option<f64>,
    pub unlocked_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameData {
    pub appid: i64,
    pub owned: Option<OwnedRow>,
    pub achievements: Vec<AchRow>,
    pub unlocked: usize,
    pub total: usize,
    pub stats: Vec<Value>,
    /// "private" | "nostats" | outra mensagem (dados indisponíveis — nada é inventado)
    pub error: Option<String>,
    pub fetched_at: Option<String>,
}

pub fn game_data(conn: &Connection, profile_id: i64, appid: i64, local: &HashMap<i64, super::local::LocalApp>, client_found: bool) -> AppResult<GameData> {
    let owned = list_owned(conn, profile_id, local, client_found)?.into_iter().find(|o| o.appid == appid);
    let mut st = conn.prepare(
        "SELECT s.apiname, s.name, s.description, s.icon, s.icon_gray, s.hidden, s.global_pct, u.unlocked_at
         FROM steam_ach_schema s LEFT JOIN steam_ach_user u ON u.appid=s.appid AND u.apiname=s.apiname AND u.profile_id=?1
         WHERE s.appid=?2 ORDER BY (u.unlocked_at IS NULL), u.unlocked_at DESC, s.sort",
    )?;
    let achievements = st
        .query_map(params![profile_id, appid], |r| {
            let at: Option<i64> = r.get(7)?;
            let (icon, gray): (Option<String>, Option<String>) = (r.get(3)?, r.get(4)?);
            Ok(AchRow {
                apiname: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                icon: if at.is_some() { icon } else { gray.or(icon) }.map(|u| media_url(&u)),
                hidden: r.get::<_, i64>(5)? != 0,
                global_pct: r.get(6)?,
                unlocked_at: at.filter(|t| *t > 1).map(fmt_epoch).or_else(|| at.map(|_| String::new())),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (stats, error, fetched_at): (Option<String>, Option<String>, Option<String>) = conn
        .query_row("SELECT stats, error, fetched_at FROM steam_stats WHERE profile_id=?1 AND appid=?2", params![profile_id, appid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .unwrap_or((None, None, None));
    let unlocked = achievements.iter().filter(|a| a.unlocked_at.is_some()).count();
    Ok(GameData {
        appid,
        owned,
        total: achievements.len(),
        unlocked,
        achievements,
        stats: stats.and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok()).unwrap_or_default(),
        error,
        fetched_at,
    })
}

// ---------------------------------------------------------------- central de conquistas (todos os jogos)

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchEntry {
    pub appid: i64,
    pub game_name: String,
    pub apiname: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub hidden: bool,
    pub global_pct: Option<f64>,
    /// `None` = pendente; "" = obtida sem data informada pela Steam.
    pub unlocked_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchGame {
    pub appid: i64,
    pub name: String,
    pub icon_url: Option<String>,
    pub unlocked: i64,
    pub total: i64,
    pub last_unlock: Option<String>,
    pub library_game_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchTotals {
    pub unlocked: i64,
    pub total: i64,
    pub games: i64,
    pub perfect: i64,
    /// Jogos da conta com conquistas e tempo de jogo cujas conquistas ainda não foram buscadas
    /// (chegam aos poucos nas próximas sincronizações).
    pub pending_games: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchOverview {
    pub totals: AchTotals,
    pub recent: Vec<AchEntry>,
    pub games: Vec<AchGame>,
}

/// Nome do jogo: o da conta Steam; sem ele (jogo fora de `steam_owned`), "App <id>".
const GAME_NAME: &str = "COALESCE((SELECT o.name FROM steam_owned o WHERE o.profile_id=u.profile_id AND o.appid=u.appid), 'App ' || u.appid)";

const ENTRY_COLS: &str = "u.appid, s.apiname, s.name, s.description, s.icon, s.icon_gray, s.hidden, s.global_pct, u.unlocked_at";

fn entry(r: &rusqlite::Row) -> rusqlite::Result<AchEntry> {
    let at: Option<i64> = r.get(9)?;
    let (icon, gray): (Option<String>, Option<String>) = (r.get(5)?, r.get(6)?);
    Ok(AchEntry {
        appid: r.get(1)?,
        game_name: r.get(0)?,
        apiname: r.get(2)?,
        name: r.get(3)?,
        description: r.get(4)?,
        icon: if at.is_some() { icon } else { gray.or(icon) }.map(|u| media_url(&u)),
        hidden: r.get::<_, i64>(7)? != 0,
        global_pct: r.get(8)?,
        unlocked_at: at.filter(|t| *t > 1).map(fmt_epoch).or_else(|| at.map(|_| String::new())),
    })
}

/// Visão geral das conquistas Steam do perfil (só cache; funciona offline).
pub fn ach_overview(conn: &Connection, profile_id: i64) -> AppResult<AchOverview> {
    let mut st = conn.prepare(&format!(
        "SELECT u.appid, {GAME_NAME},
                (SELECT o.icon_hash FROM steam_owned o WHERE o.profile_id=u.profile_id AND o.appid=u.appid),
                SUM(u.unlocked_at IS NOT NULL), COUNT(*), MAX(u.unlocked_at),
                (SELECT l.game_id FROM steam_links l JOIN profile_games pg ON pg.game_id=l.game_id AND pg.profile_id=u.profile_id WHERE l.appid=u.appid LIMIT 1)
         FROM steam_ach_user u JOIN steam_ach_schema s ON s.appid=u.appid AND s.apiname=u.apiname
         WHERE u.profile_id=?1 GROUP BY u.appid"
    ))?;
    let games = st
        .query_map([profile_id], |r| {
            let appid: i64 = r.get(0)?;
            let icon: Option<String> = r.get(2)?;
            Ok(AchGame {
                appid,
                name: r.get(1)?,
                icon_url: icon.map(|h| app_icon_url(appid, &h)),
                unlocked: r.get(3)?,
                total: r.get(4)?,
                last_unlock: r.get::<_, Option<i64>>(5)?.filter(|t| *t > 1).map(fmt_epoch),
                library_game_id: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let pending_games: i64 = conn.query_row(
        "SELECT COUNT(*) FROM steam_owned o WHERE o.profile_id=?1 AND o.has_stats=1 AND COALESCE(o.playtime_forever,0) > 0
         AND NOT EXISTS(SELECT 1 FROM steam_stats s WHERE s.profile_id=o.profile_id AND s.appid=o.appid)",
        [profile_id],
        |r| r.get(0),
    )?;
    let totals = AchTotals {
        unlocked: games.iter().map(|g| g.unlocked).sum(),
        total: games.iter().map(|g| g.total).sum(),
        games: games.iter().filter(|g| g.unlocked > 0).count() as i64,
        perfect: games.iter().filter(|g| g.total > 0 && g.unlocked == g.total).count() as i64,
        pending_games,
    };
    let recent = ach_list(conn, profile_id, "earned", "", 24)?;
    Ok(AchOverview { totals, recent, games })
}

/// Lista de conquistas de todos os jogos. `tab`: "recent" (30 dias) | "earned" | "pending" | "rare"
/// (obtidas, das mais raras para as mais comuns). `query` filtra por jogo ou conquista.
pub fn ach_list(conn: &Connection, profile_id: i64, tab: &str, query: &str, limit: i64) -> AppResult<Vec<AchEntry>> {
    let (cond, order) = match tab {
        "recent" => ("u.unlocked_at > CAST(strftime('%s','now','-30 days') AS INTEGER)", "u.unlocked_at DESC"),
        "pending" => ("u.unlocked_at IS NULL", "COALESCE(s.global_pct,-1) DESC, s.sort"),
        "rare" => ("u.unlocked_at IS NOT NULL", "COALESCE(s.global_pct,101) ASC, u.unlocked_at DESC"),
        _ => ("u.unlocked_at IS NOT NULL", "u.unlocked_at DESC, s.sort"),
    };
    let q = format!("%{}%", query.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut st = conn.prepare(&format!(
        "SELECT {GAME_NAME} AS game, {ENTRY_COLS}
         FROM steam_ach_user u JOIN steam_ach_schema s ON s.appid=u.appid AND s.apiname=u.apiname
         WHERE u.profile_id=?1 AND {cond} AND (?2 = '%%' OR game LIKE ?2 ESCAPE '\\' OR s.name LIKE ?2 ESCAPE '\\')
         ORDER BY {order} LIMIT ?3"
    ))?;
    let rows = st.query_map(params![profile_id, q, limit], entry)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------- aviso de desbloqueio (ver `watch`)

/// Conquistas já obtidas de um jogo, segundo o cache (antes de consultar a API).
pub fn unlocked_set(conn: &Connection, profile_id: i64, appid: i64) -> AppResult<HashSet<String>> {
    let mut st = conn.prepare("SELECT apiname FROM steam_ach_user WHERE profile_id=?1 AND appid=?2 AND unlocked_at IS NOT NULL")?;
    let rows = st.query_map(params![profile_id, appid], |r| r.get::<_, String>(0))?.collect::<Result<HashSet<_>, _>>()?;
    Ok(rows)
}

/// O esquema de conquistas (nomes, ícones, raridade) do jogo já está no cache?
pub fn has_schema(conn: &Connection, appid: i64) -> AppResult<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM steam_ach_schema WHERE appid=?1)", [appid], |r| r.get(0))?)
}

/// Conquistas obtidas agora no cache que NÃO estavam em `before`, desbloqueadas a partir de `since`
/// (epoch), da mais antiga para a mais nova. A janela de tempo impede que um jogo nunca buscado
/// antes "avise" conquistas antigas.
pub fn fresh_unlocks(conn: &Connection, profile_id: i64, appid: i64, before: &HashSet<String>, since: i64) -> AppResult<Vec<AchEntry>> {
    let mut st = conn.prepare(&format!(
        "SELECT {GAME_NAME} AS game, {ENTRY_COLS}
         FROM steam_ach_user u JOIN steam_ach_schema s ON s.appid=u.appid AND s.apiname=u.apiname
         WHERE u.profile_id=?1 AND u.appid=?2 AND u.unlocked_at >= ?3 ORDER BY u.unlocked_at, s.sort"
    ))?;
    let rows = st.query_map(params![profile_id, appid, since], entry)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().filter(|a| !before.contains(&a.apiname)).collect())
}

/// Esquema de conquistas em cache há menos de 7 dias?
pub fn schema_fresh(conn: &Connection, profile_id: i64, appid: i64) -> AppResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM steam_stats WHERE profile_id=?1 AND appid=?2 AND fetched_at > datetime('now','-7 days'))",
        params![profile_id, appid],
        |r| r.get(0),
    )?)
}

pub fn clear_profile(conn: &Connection, profile_id: i64) -> AppResult<()> {
    for t in ["steam_owned", "steam_ach_user", "steam_stats"] {
        conn.execute(&format!("DELETE FROM {t} WHERE profile_id=?1"), [profile_id])?;
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE profiles(id INTEGER PRIMARY KEY, username TEXT);
             CREATE TABLE games(id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, platform TEXT NOT NULL DEFAULT 'PC', executable_path TEXT NOT NULL DEFAULT '',
               launch_args TEXT NOT NULL DEFAULT '', cover TEXT, source TEXT NOT NULL DEFAULT 'manual', external_id TEXT, iso_path TEXT, rom_path TEXT);
             CREATE TABLE profile_games(profile_id INTEGER, game_id INTEGER, playtime_seconds INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(profile_id, game_id));
             CREATE TABLE game_info(game_id INTEGER PRIMARY KEY, data TEXT NOT NULL, fetched_at TEXT NOT NULL DEFAULT (datetime('now')));
             INSERT INTO profiles(id) VALUES(1),(2);
             INSERT INTO games(id, title, executable_path, cover) VALUES(1, 'Portal 2', 'C:/p2.exe', NULL), (2, 'Hades', 'C:/h.exe', 'x'), (3, 'Doom', 'C:/d.exe', 'x'), (4, 'DOOM', 'C:/d2.exe', 'x');
             INSERT INTO profile_games(profile_id, game_id, playtime_seconds) VALUES(1,1,3600),(1,2,0),(1,3,0),(1,4,0);
             INSERT INTO game_info(game_id, data) VALUES(2, '{\"externalId\":\"1145360\"}');",
        )
        .unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    fn owned() -> Vec<Value> {
        vec![
            json!({"appid": 620, "name": "Portal 2", "playtime_forever": 900, "img_icon_url": "abc", "rtime_last_played": 1700000000, "has_community_visible_stats": true}),
            json!({"appid": 1145360, "name": "Hades", "playtime_forever": 30, "playtime_2weeks": 10}),
            json!({"appid": 379720, "name": "DOOM"}),
            json!({"appid": 440, "name": "Team Fortress 2", "playtime_forever": 0}),
        ]
    }

    #[test]
    fn preview_classifies_and_import_respects_choices() {
        let c = mem();
        store_owned(&c, 1, &owned()).unwrap();
        let p = preview(&c, 1, &[620, 1145360, 379720, 440, 440, 999]).unwrap();
        let st: HashMap<i64, &str> = p.iter().map(|i| (i.appid, i.status.as_str())).collect();
        assert_eq!(st[&620], "title", "mesmo nome = só sugestão");
        assert_eq!(st[&1145360], "existing", "mesmo AppID nas informações do jogo");
        assert_eq!(st[&379720], "new", "dois 'Doom' na biblioteca = ambíguo, não vincula");
        assert_eq!(st[&440], "new");
        assert!(!st.contains_key(&999), "AppID fora da lista da conta é ignorado");
        assert!(p.iter().find(|i| i.appid == 620).unwrap().updates.contains(&"cover".to_string()));

        let r = import(&c, 1, &[620, 1145360, 440], false).unwrap();
        assert_eq!((r.added, r.linked, r.skipped), (1, 1, 1));
        // histórico local intacto
        let secs: i64 = c.query_row("SELECT playtime_seconds FROM profile_games WHERE profile_id=1 AND game_id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(secs, 3600);
        let tf2: (String, String, String) = c.query_row("SELECT source, executable_path, external_id FROM games WHERE title='Team Fortress 2'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(tf2, ("steam".into(), "".into(), "440".into()));
        // importar de novo não duplica
        let r2 = import(&c, 1, &[440], false).unwrap();
        assert_eq!((r2.added, r2.updated), (0, 1));
        // aceitar o vínculo por nome
        assert_eq!(import(&c, 1, &[620], true).unwrap().linked, 1);
        assert_eq!(appid_of(&c, 1).unwrap(), Some(620));
        let cover: Option<String> = c.query_row("SELECT cover FROM games WHERE id=1", [], |r| r.get(0)).unwrap();
        assert!(cover.unwrap().contains("/620/"));
    }

    #[test]
    fn owned_list_install_states_and_privacy_of_local() {
        let c = mem();
        let apps = vec![super::super::local::LocalApp { appid: 440, name: "Team Fortress 2".into(), state: "installed".into(), installed: true, size_on_disk: 1, last_updated: 0 }];
        store_local(&c, 1, &apps).unwrap();
        let local: HashMap<i64, _> = apps.iter().map(|a| (a.appid, a.clone())).collect();
        let l = list_owned(&c, 1, &local, true).unwrap();
        assert_eq!((l.len(), l[0].source.as_str(), l[0].install.as_str()), (1, "local", "installed"));
        assert_eq!(l[0].playtime_forever, None, "sem chave, tempo de jogo não é inventado");
        store_owned(&c, 1, &owned()).unwrap();
        store_recent(&c, 1, &[json!({"appid": 1145360, "playtime_2weeks": 12})]).unwrap();
        let l = list_owned(&c, 1, &local, true).unwrap();
        assert_eq!(l.len(), 4);
        let h = l.iter().find(|o| o.appid == 1145360).unwrap();
        assert!(h.recent && h.playtime_2weeks == Some(12) && h.install == "not_installed");
        let p = l.iter().find(|o| o.appid == 620).unwrap();
        assert!(p.icon_url.as_deref().unwrap().ends_with("/620/abc.jpg") && p.last_played.is_some());
        // outro perfil não vê
        assert!(list_owned(&c, 2, &local, true).unwrap().is_empty());
    }

    #[test]
    fn achievements_and_stats() {
        let c = mem();
        let game = json!({"availableGameStats": {
            "achievements": [
                {"name": "A1", "displayName": "Primeira", "description": "d1", "icon": "https://x/a1.jpg", "icongray": "https://x/a1g.jpg", "hidden": 0},
                {"name": "A2", "displayName": "Secreta", "description": "", "icon": "https://x/a2.jpg", "icongray": "https://x/a2g.jpg", "hidden": 1}],
            "stats": [{"name": "kills", "displayName": "Abates"}]}});
        let labels = store_schema(&c, 620, &game).unwrap();
        assert_eq!(labels, vec![("kills".to_string(), "Abates".to_string())]);
        store_global(&c, 620, &[json!({"name": "A1", "percent": "54.3"}), json!({"name": "A2", "percent": 2.1})]).unwrap();
        store_player(&c, 1, 620, &[json!({"apiname": "A1", "achieved": 1, "unlocktime": 1700000000}), json!({"apiname": "A2", "achieved": 0, "unlocktime": 0})]).unwrap();
        store_stats(&c, 1, 620, Some(&json!([{"name": "kills", "label": "Abates", "value": 7}])), None).unwrap();
        let d = game_data(&c, 1, 620, &HashMap::new(), false).unwrap();
        assert_eq!((d.unlocked, d.total, d.stats.len()), (1, 2, 1));
        assert_eq!(d.achievements[0].icon.as_deref(), Some("https://x/a1.jpg"));
        assert_eq!(d.achievements[1].icon.as_deref(), Some("https://x/a2g.jpg"), "pendente usa o ícone cinza");
        assert_eq!(d.achievements[0].global_pct, Some(54.3));
        assert!(d.achievements[1].hidden);
        // perfil privado: guarda o motivo e mantém o que havia
        store_stats(&c, 1, 620, None, Some("private")).unwrap();
        let d = game_data(&c, 1, 620, &HashMap::new(), false).unwrap();
        assert_eq!((d.error.as_deref(), d.stats.len()), (Some("private"), 1));
        clear_profile(&c, 1).unwrap();
        assert_eq!(game_data(&c, 1, 620, &HashMap::new(), false).unwrap().unlocked, 0);
    }

    #[test]
    fn steam_images_use_current_cdn_path() {
        // caso real: ícone de jogo novo (ReStory, 3812600) dava 404 no caminho antigo, em todos os domínios
        assert_eq!(
            media_url("https://steamcdn-a.akamaihd.net/steamcommunity/public/images/apps/3812600/944b944e39bc6a9157e0e95526b7f551bc48f3f8.jpg"),
            "https://shared.akamai.steamstatic.com/community_assets/images/apps/3812600/944b944e39bc6a9157e0e95526b7f551bc48f3f8.jpg"
        );
        assert_eq!(
            media_url("https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/8850/a.jpg"),
            "https://shared.akamai.steamstatic.com/community_assets/images/apps/8850/a.jpg"
        );
        // outros endereços ficam como estão
        for u in ["https://x/a1.jpg", "https://evil.example/steamcommunity/public/images/apps/1/a.jpg", "https://avatars.steamstatic.com/abc_full.jpg"] {
            assert_eq!(media_url(u), u);
        }
        assert_eq!(app_icon_url(220, "fcfb"), "https://shared.akamai.steamstatic.com/community_assets/images/apps/220/fcfb.jpg");
    }

    #[test]
    fn achievements_center_overview_and_lists() {
        let c = mem();
        store_owned(&c, 1, &owned()).unwrap(); // Portal 2 (com estatísticas e tempo), Hades, DOOM
        let game = json!({"availableGameStats": {"achievements": [
            {"name": "A1", "displayName": "Comum", "description": "d1", "icon": "https://x/a1.jpg", "icongray": "https://x/a1g.jpg"},
            {"name": "A2", "displayName": "Rara", "description": "d2", "icon": "https://x/a2.jpg", "icongray": "https://x/a2g.jpg"},
            {"name": "A3", "displayName": "Falta", "description": "", "icon": "https://x/a3.jpg", "icongray": "https://x/a3g.jpg", "hidden": 1}]}});
        store_schema(&c, 620, &game).unwrap();
        store_global(&c, 620, &[json!({"name": "A1", "percent": 60.0}), json!({"name": "A2", "percent": 3.5}), json!({"name": "A3", "percent": 1.0})]).unwrap();
        // antes de buscar: Portal 2 conta como jogo pendente (tem estatísticas e tempo de jogo)
        assert_eq!(ach_overview(&c, 1).unwrap().totals.pending_games, 1);
        let now = crate::retro::util::now_epoch();
        store_player(&c, 1, 620, &[
            json!({"apiname": "A1", "achieved": 1, "unlocktime": now - 100 * 86400}),
            json!({"apiname": "A2", "achieved": 1, "unlocktime": now - 3600}),
            json!({"apiname": "A3", "achieved": 0, "unlocktime": 0}),
        ]).unwrap();
        store_stats(&c, 1, 620, None, None).unwrap();

        let ov = ach_overview(&c, 1).unwrap();
        assert_eq!((ov.totals.unlocked, ov.totals.total, ov.totals.games, ov.totals.perfect, ov.totals.pending_games), (2, 3, 1, 0, 0));
        assert_eq!((ov.games[0].name.as_str(), ov.games[0].unlocked, ov.games[0].total), ("Portal 2", 2, 3));
        assert!(ov.games[0].icon_url.as_deref().unwrap().ends_with("/620/abc.jpg"));
        assert_eq!(ov.recent.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Rara", "Comum"], "mais recente primeiro");

        let names = |tab: &str, q: &str| ach_list(&c, 1, tab, q, 50).unwrap().into_iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(names("recent", ""), ["Rara"], "só os últimos 30 dias");
        assert_eq!(names("rare", ""), ["Rara", "Comum"], "obtidas, da mais rara para a mais comum");
        assert_eq!(names("pending", ""), ["Falta"]);
        assert_eq!(names("earned", "portal"), ["Rara", "Comum"], "busca pelo nome do jogo");
        assert_eq!(names("earned", "comum"), ["Comum"], "busca pelo nome da conquista");
        assert!(names("earned", "100%").is_empty(), "% na busca é literal");
        let pend = &ach_list(&c, 1, "pending", "", 50).unwrap()[0];
        assert!(pend.hidden && pend.unlocked_at.is_none());
        assert_eq!(pend.icon.as_deref(), Some("https://x/a3g.jpg"), "pendente usa o ícone cinza");
        // outro perfil não vê nada
        assert!(ach_overview(&c, 2).unwrap().games.is_empty());

        // aviso de desbloqueio: só o que não estava no cache E é recente
        let before = unlocked_set(&c, 1, 620).unwrap(); // {A1, A2}
        assert_eq!(before.len(), 2);
        assert!(has_schema(&c, 620).unwrap() && !has_schema(&c, 999).unwrap());
        store_player(&c, 1, 620, &[
            json!({"apiname": "A1", "achieved": 1, "unlocktime": now - 100 * 86400}),
            json!({"apiname": "A2", "achieved": 1, "unlocktime": now - 3600}),
            json!({"apiname": "A3", "achieved": 1, "unlocktime": now - 20}), // acabou de sair
        ]).unwrap();
        let fresh = fresh_unlocks(&c, 1, 620, &before, now - 900).unwrap();
        assert_eq!(fresh.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Falta"]);
        assert_eq!(fresh[0].global_pct, Some(1.0));
        assert_eq!(fresh[0].icon.as_deref(), Some("https://x/a3.jpg"), "obtida usa o ícone colorido");
        // jogo nunca buscado (cache vazio): conquistas antigas não viram aviso, só as dos últimos 15 min
        let none = HashSet::new();
        assert_eq!(fresh_unlocks(&c, 1, 620, &none, now - 900).unwrap().len(), 1);
    }

    #[test]
    fn rarity_rules() {
        use crate::shot_toast::{ra_is_rare, steam_is_rare};
        assert!(steam_is_rare(Some(3.4)) && !steam_is_rare(Some(10.0)) && !steam_is_rare(None));
        assert!(ra_is_rare(50, 60) && ra_is_rare(10, 45) && !ra_is_rare(10, 25) && !ra_is_rare(0, 0));
    }
}
