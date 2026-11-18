//! "Últimas atualizações" (Home): notas de atualização OFICIAIS dos jogos Steam da biblioteca.
//!
//! Fonte: ISteamNews/GetNewsForApp (Web API oficial, sem chave), feed `steam_community_announcements`
//! — só o que o próprio desenvolvedor publicou na página do jogo na Steam. Nada é inventado:
//!  - "oficial": anúncio marcado pelo desenvolvedor como Notas de atualização (tag `patchnotes`);
//!  - "provável": anúncio sem a marcação, mas com título de atualização ("Patch", "Hotfix", "Update
//!    1.2"...). A interface mostra a diferença;
//!  - a data é a da PUBLICAÇÃO da nota. A Steam não oferece API pública com o horário em que cada build
//!    foi liberada; atualizações silenciosas (sem nota) não aparecem. Como complemento, o manifesto
//!    local informa quando a cópia instalada neste PC foi atualizada.
//!
//! Custo: 1 requisição por jogo, em segundo plano, espaçada (~1 s) e pela mesma fila/limite do resto da
//! integração (`client::call`). Cache global por jogo (as notícias não dependem do perfil) com validade
//! de 6 h; a atualização manual só reconsulta o que tem mais de 15 min.

use super::client::{self, SteamError};
use crate::db::Db;
use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS steam_news (
    appid       INTEGER PRIMARY KEY,
    checked_at  INTEGER NOT NULL,          -- epoch da última consulta à Steam
    gid         TEXT,                      -- NULL = nenhuma nota de atualização nos anúncios recentes
    title       TEXT,
    author      TEXT,
    date        INTEGER,                   -- epoch da publicação
    contents    TEXT,                      -- BBCode da Steam (convertido com segurança na interface)
    official    INTEGER NOT NULL DEFAULT 0 -- 1 = marcado como Notas de atualização pelo desenvolvedor
);
"#;

const TTL: i64 = 6 * 3600;
const MANUAL_TTL: i64 = 15 * 60;
/// Anúncios lidos por jogo (a nota mais recente costuma estar entre os primeiros).
const PER_APP: u32 = 10;
/// Espaço extra entre jogos, além da fila da integração (consulta de baixa prioridade).
const PACE: Duration = Duration::from_millis(900);
const MAX_CONTENTS: usize = 60_000;
const MAX_PER_RUN: usize = 400;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Título com cara de atualização (para anúncios sem a marcação oficial).
pub fn title_is_update(title: &str) -> bool {
    let t = title.to_lowercase();
    const NOT: [&str; 9] = ["community update", "roadmap", "sale", "% off", "discount", "livestream", "dev diary", "giveaway", "merch"];
    if NOT.iter().any(|w| t.contains(w)) {
        return false;
    }
    const YES: [&str; 12] = ["patch", "hotfix", "hot fix", "update", "changelog", "change log", "release note", "fixes", "atualiza", "notas de", "bug fix", "bugfix"];
    if YES.iter().any(|w| t.contains(w)) {
        return true;
    }
    // "Version 1.17", "v1.2.3"
    let b = t.as_bytes();
    t.contains("version ") || (0..b.len().saturating_sub(2)).any(|i| b[i] == b'v' && b[i + 1].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) && t[i + 1..].contains('.'))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub gid: String,
    pub title: String,
    pub author: String,
    pub date: i64,
    pub contents: String,
    pub official: bool,
}

/// A nota de atualização mais recente entre os anúncios (oficial pela tag ou provável pelo título).
pub fn pick(items: &[Value]) -> Option<Picked> {
    let mut best: Option<Picked> = None;
    for i in items {
        let title = i["title"].as_str().unwrap_or("").trim();
        let tags: Vec<&str> = i["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str()).collect()).unwrap_or_default();
        let official = tags.contains(&"patchnotes");
        if !(official || title_is_update(title)) {
            continue;
        }
        let date = i["date"].as_i64().unwrap_or(0);
        let gid = match &i["gid"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => continue,
        };
        if date <= 0 || gid.is_empty() || best.as_ref().is_some_and(|b| b.date >= date) {
            continue;
        }
        let mut contents = i["contents"].as_str().unwrap_or("").to_string();
        if contents.len() > MAX_CONTENTS {
            let mut cut = MAX_CONTENTS;
            while !contents.is_char_boundary(cut) {
                cut -= 1;
            }
            contents.truncate(cut);
        }
        best = Some(Picked { gid, title: title.to_string(), author: i["author"].as_str().unwrap_or("").to_string(), date, contents, official });
    }
    best
}

fn save(conn: &Connection, appid: i64, p: Option<&Picked>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO steam_news(appid, checked_at, gid, title, author, date, contents, official) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(appid) DO UPDATE SET checked_at=excluded.checked_at, gid=excluded.gid, title=excluded.title,
           author=excluded.author, date=excluded.date, contents=excluded.contents, official=excluded.official",
        params![appid, now(), p.map(|p| &p.gid), p.map(|p| &p.title), p.map(|p| &p.author), p.map(|p| p.date), p.map(|p| &p.contents), p.map(|p| p.official).unwrap_or(false)],
    )?;
    Ok(())
}

/// Jogos Steam da biblioteca do perfil (vinculados), os mais jogados recentemente primeiro.
pub(crate) fn candidates(conn: &Connection, profile_id: i64) -> AppResult<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT sl.appid FROM steam_links sl
           JOIN profile_games pg ON pg.game_id = sl.game_id AND pg.profile_id = ?1
           LEFT JOIN steam_owned so ON so.profile_id = ?1 AND so.appid = sl.appid
          GROUP BY sl.appid
          ORDER BY MAX(COALESCE(so.last_played, 0)) DESC, MAX(COALESCE(pg.last_played_at, '')) DESC",
    )?;
    let v = st.query_map([profile_id], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(v)
}

fn connected(conn: &Connection, profile_id: i64) -> AppResult<bool> {
    Ok(crate::accounts::get(conn, profile_id, super::store::PLATFORM)?.and_then(|a| a.external_id).is_some())
}

// ---------- consulta em segundo plano ----------

static RUNNING: AtomicBool = AtomicBool::new(false);
static DONE: AtomicU32 = AtomicU32::new(0);
static TOTAL: AtomicU32 = AtomicU32::new(0);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Começa a verificação dos jogos com cache vencido (não bloqueia). `false` = já havia uma em andamento.
pub fn start(app: &AppHandle, profile_id: i64, manual: bool) -> bool {
    if RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _g = Guard;
        let ttl = if manual { MANUAL_TTL } else { TTL };
        let stale: Vec<i64> = {
            let db = app.state::<Db>();
            let Ok(conn) = db.conn() else { return };
            if !connected(&conn, profile_id).unwrap_or(false) {
                return;
            }
            let list = candidates(&conn, profile_id).unwrap_or_default();
            let limit = now() - ttl;
            list.into_iter()
                .filter(|a| {
                    let at: Option<i64> = conn.query_row("SELECT checked_at FROM steam_news WHERE appid=?1", [a], |r| r.get(0)).optional().ok().flatten();
                    at.map_or(true, |t| t < limit)
                })
                .take(MAX_PER_RUN)
                .collect()
        };
        if stale.is_empty() {
            return;
        }
        DONE.store(0, Ordering::SeqCst);
        TOTAL.store(stale.len() as u32, Ordering::SeqCst);
        if let Ok(mut e) = LAST_ERROR.lock() {
            *e = None;
        }
        let _ = app.emit("steam-news-state", json!({ "profileId": profile_id, "running": true }));
        for (n, appid) in stale.iter().enumerate() {
            if n > 0 {
                tokio::time::sleep(PACE).await;
            }
            match client::news_for_app(*appid, PER_APP).await {
                Ok(items) => {
                    let picked = pick(&items);
                    let db = app.state::<Db>();
                    let saved = db.conn().map(|conn| save(&conn, *appid, picked.as_ref()));
                    drop(saved);
                }
                // sem rede ou limite da Steam: para aqui e mantém o cache (tenta de novo mais tarde)
                Err(e @ (SteamError::Offline | SteamError::RateLimited(_))) => {
                    if let Ok(mut le) = LAST_ERROR.lock() {
                        *le = Some(e.message());
                    }
                    break;
                }
                Err(_) => {} // resposta ruim para este jogo: segue para o próximo
            }
            DONE.store(n as u32 + 1, Ordering::SeqCst);
            if (n + 1) % 5 == 0 || n + 1 == stale.len() {
                let _ = app.emit("steam-news-updated", json!({ "profileId": profile_id }));
            }
        }
        let _ = app.emit("steam-news-state", json!({ "profileId": profile_id, "running": false }));
        let _ = app.emit("steam-news-updated", json!({ "profileId": profile_id }));
    });
    true
}

// ---------- leitura (só cache; funciona offline) ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsView {
    pub appid: i64,
    pub game_id: i64,
    pub game_title: String,
    pub cover: Option<String>,
    pub gid: String,
    pub title: String,
    pub author: String,
    pub date: i64,
    pub contents: String,
    pub official: bool,
    pub url: String,
    /// Quando a cópia instalada neste PC foi atualizada pela Steam (manifesto local), se houver.
    pub local_updated: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub connected: bool,
    pub candidates: usize,
    pub checked: usize,
    pub running: bool,
    pub done: u32,
    pub total: u32,
    pub error: Option<String>,
    pub checked_at: Option<i64>,
    pub items: Vec<NewsView>,
    pub seen_gid: Option<String>,
}

pub fn news_url(appid: i64, gid: &str) -> String {
    format!("https://store.steampowered.com/news/app/{appid}/view/{gid}")
}

pub fn overview(conn: &Connection, profile_id: i64, limit: usize) -> AppResult<Overview> {
    let is_connected = connected(conn, profile_id)?;
    let list = if is_connected { candidates(conn, profile_id)? } else { Vec::new() };
    let (mut checked, mut checked_at) = (0usize, None::<i64>);
    let mut items = Vec::new();
    if !list.is_empty() {
        let local = super::local::local_apps();
        let mut st = conn.prepare(
            "SELECT n.appid, n.checked_at, n.gid, n.title, n.author, n.date, n.contents, n.official,
                    (SELECT g.id FROM steam_links sl JOIN games g ON g.id = sl.game_id
                       JOIN profile_games pg ON pg.game_id = g.id AND pg.profile_id = ?1 WHERE sl.appid = n.appid LIMIT 1),
                    (SELECT g.title FROM steam_links sl JOIN games g ON g.id = sl.game_id
                       JOIN profile_games pg ON pg.game_id = g.id AND pg.profile_id = ?1 WHERE sl.appid = n.appid LIMIT 1),
                    (SELECT g.cover FROM steam_links sl JOIN games g ON g.id = sl.game_id
                       JOIN profile_games pg ON pg.game_id = g.id AND pg.profile_id = ?1 WHERE sl.appid = n.appid LIMIT 1)
               FROM steam_news n WHERE n.appid = ?2",
        )?;
        for appid in &list {
            let row = st
                .query_row(params![profile_id, appid], |r| {
                    Ok((
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, bool>(7)?,
                        r.get::<_, Option<i64>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, Option<String>>(10)?,
                    ))
                })
                .optional()?;
            let Some((at, gid, title, author, date, contents, official, game_id, game_title, cover)) = row else { continue };
            checked += 1;
            checked_at = Some(checked_at.map_or(at, |c: i64| c.max(at)));
            let (Some(gid), Some(date), Some(game_id)) = (gid, date, game_id) else { continue };
            items.push(NewsView {
                appid: *appid,
                game_id,
                game_title: game_title.unwrap_or_default(),
                // capa salva pelo app (`nximg:`) vira caminho do arquivo, como nas outras telas; sem isso a
                // interface recebia a referência crua e o jogo aparecia sem capa
                cover: crate::images::resolve(cover),
                url: news_url(*appid, &gid),
                gid,
                title: title.unwrap_or_default(),
                author: author.unwrap_or_default(),
                date,
                contents: contents.unwrap_or_default(),
                official,
                local_updated: local.get(appid).map(|a| a.last_updated).filter(|t| *t > 0),
            });
        }
        items.sort_by(|a, b| b.date.cmp(&a.date));
        items.truncate(limit);
    }
    let seen_gid: Option<String> = conn
        .query_row("SELECT value FROM profile_settings WHERE profile_id=?1 AND key='steam_news_seen'", [profile_id], |r| r.get(0))
        .optional()?;
    Ok(Overview {
        connected: is_connected,
        candidates: list.len(),
        checked,
        running: RUNNING.load(Ordering::SeqCst),
        done: DONE.load(Ordering::SeqCst),
        total: TOTAL.load(Ordering::SeqCst),
        error: LAST_ERROR.lock().ok().and_then(|e| e.clone()),
        checked_at,
        items,
        seen_gid,
    })
}

// ---------- página do jogo (um jogo só) ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameNewsItem {
    pub gid: String,
    pub title: String,
    pub author: String,
    pub date: i64,
    pub contents: String,
    pub official: bool,
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameNews {
    pub appid: i64,
    /// Última consulta à Steam (None = nunca consultado).
    pub checked_at: Option<i64>,
    /// Nota de atualização mais recente (None = nenhuma nos anúncios recentes).
    pub item: Option<GameNewsItem>,
    /// Quando a cópia instalada neste PC foi atualizada pela Steam (manifesto local), se houver.
    pub local_updated: Option<i64>,
    /// Falha na última consulta (o que aparece é do cache).
    pub error: Option<String>,
}

/// Nota de atualização de um jogo a partir do cache (o mesmo da Home).
pub fn game_view(conn: &Connection, appid: i64, error: Option<String>) -> AppResult<GameNews> {
    let row = conn
        .query_row("SELECT checked_at, gid, title, author, date, contents, official FROM steam_news WHERE appid=?1", [appid], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, bool>(6)?,
            ))
        })
        .optional()?;
    let (checked_at, item) = match row {
        Some((at, Some(gid), title, author, Some(date), contents, official)) => (
            Some(at),
            Some(GameNewsItem {
                url: news_url(appid, &gid),
                gid,
                title: title.unwrap_or_default(),
                author: author.unwrap_or_default(),
                date,
                contents: contents.unwrap_or_default(),
                official,
            }),
        ),
        Some((at, ..)) => (Some(at), None),
        None => (None, None),
    };
    let local_updated = super::local::app(appid).map(|a| a.last_updated).filter(|t| *t > 0);
    Ok(GameNews { appid, checked_at, item, local_updated, error })
}

/// Página do jogo: usa o cache (6 h) e consulta a Steam se ele estiver vencido (`refresh`: 15 min).
/// API oficial sem chave: não depende de conta Steam conectada.
pub async fn game_news(db: &Db, appid: i64, refresh: bool) -> AppResult<GameNews> {
    let at: Option<i64> = db.conn()?.query_row("SELECT checked_at FROM steam_news WHERE appid=?1", [appid], |r| r.get(0)).optional()?;
    let ttl = if refresh { MANUAL_TTL } else { TTL };
    let mut error = None;
    if at.map_or(true, |t| now() - t > ttl) {
        match client::news_for_app(appid, PER_APP).await {
            Ok(items) => save(&*db.conn()?, appid, pick(&items).as_ref())?,
            Err(e) => error = Some(e.message()),
        }
    }
    game_view(&*db.conn()?, appid, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_view_from_cache() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        assert!(game_view(&c, 730, None).unwrap().checked_at.is_none(), "nunca consultado");
        save(&c, 570, None).unwrap();
        let v = game_view(&c, 570, None).unwrap();
        assert!(v.checked_at.is_some() && v.item.is_none(), "consultado, sem nota");
        let p = Picked { gid: String::from("9"), title: String::from("Patch 1.2"), author: String::new(), date: 50, contents: String::from("[list][*]a[/list]"), official: true };
        save(&c, 730, Some(&p)).unwrap();
        let v = game_view(&c, 730, None).unwrap();
        let it = v.item.unwrap();
        assert_eq!((it.gid.as_str(), it.official, it.url.as_str()), ("9", true, "https://store.steampowered.com/news/app/730/view/9"));
    }

    #[test]
    fn titles_that_look_like_updates() {
        for t in ["Patch Notes Version 1.17", "Hotfix #36 Now Live!", "Counter-Strike 2 Update", "v1.2.3 is out", "Release Notes for 2025/12/16", "Atualização 2.0"] {
            assert!(title_is_update(t), "{t}");
        }
        for t in ["Community Update #36 A Very Moddy Christmas", "New Elden Ring Mini Replicas Arrive", "Summer Sale: 50% off", "2026 Roadmap", "Dev Diary #4", "Survival mode"] {
            assert!(!title_is_update(t), "{t}");
        }
    }

    #[test]
    fn picks_newest_update_and_marks_official() {
        let items = vec![
            json!({"gid": "3", "title": "Merch drop", "date": 300, "tags": [], "contents": "x"}),
            json!({"gid": "2", "title": "Patch Notes Version 1.17", "date": 200, "tags": [], "contents": "fix"}),
            json!({"gid": "1", "title": "Big news", "date": 100, "tags": ["patchnotes"], "contents": "fix"}),
        ];
        let p = pick(&items).unwrap();
        assert_eq!((p.gid.as_str(), p.official), ("2", false), "a mais recente que é atualização, mesmo sem tag");
        let only_tag = vec![json!({"gid": 9, "title": "Big news", "date": 50, "tags": ["patchnotes"], "contents": ""})];
        let p = pick(&only_tag).unwrap();
        assert_eq!((p.gid.as_str(), p.official), ("9", true));
        assert!(pick(&[json!({"gid": "1", "title": "Sale", "date": 1, "tags": []})]).is_none(), "nada inventado");
    }

    #[test]
    fn cache_and_overview() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        let p = Picked { gid: "7".into(), title: "Update".into(), author: "Dev".into(), date: 123, contents: "[p]a[/p]".into(), official: true };
        save(&c, 730, Some(&p)).unwrap();
        save(&c, 570, None).unwrap();
        let (gid, off): (Option<String>, bool) = c.query_row("SELECT gid, official FROM steam_news WHERE appid=730", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((gid.as_deref(), off), (Some("7"), true));
        let none: Option<String> = c.query_row("SELECT gid FROM steam_news WHERE appid=570", [], |r| r.get(0)).unwrap();
        assert!(none.is_none());
        assert_eq!(news_url(730, "7"), "https://store.steampowered.com/news/app/730/view/7");
    }
}
