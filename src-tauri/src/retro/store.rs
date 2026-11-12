//! Cache local (SQLite) do RetroAchievements: conta por perfil, catálogo, conquistas, progresso,
//! desbloqueios e associação com os jogos da biblioteca. Permite consultar tudo offline.
//!
//! Novos desbloqueios são identificados comparando com `ra_unlocks`: só é "novo" o que ainda não
//! estava registrado (ou o modo Hardcore que ainda não estava registrado).

use super::client::{Creds, MEDIA};
use super::util::{int, norm_date, opt_text, text};
use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};

use serde::Serialize;
use serde_json::Value;

pub const PLATFORM: &str = "retroachievements";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ra_games (
    ra_game_id        INTEGER PRIMARY KEY,
    title             TEXT NOT NULL,
    console_id        INTEGER NOT NULL DEFAULT 0,
    console_name      TEXT NOT NULL DEFAULT '',
    image_icon        TEXT,
    num_achievements  INTEGER NOT NULL DEFAULT 0,
    points_total      INTEGER NOT NULL DEFAULT 0,
    details_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_ra_games_console ON ra_games(console_id);
CREATE TABLE IF NOT EXISTS ra_achievements (
    ra_achievement_id  INTEGER PRIMARY KEY,
    ra_game_id         INTEGER NOT NULL,
    title              TEXT NOT NULL,
    description        TEXT NOT NULL DEFAULT '',
    points             INTEGER NOT NULL DEFAULT 0,
    true_ratio         INTEGER NOT NULL DEFAULT 0,
    badge              TEXT,
    display_order      INTEGER NOT NULL DEFAULT 0,
    kind               TEXT
);
CREATE INDEX IF NOT EXISTS idx_ra_ach_game ON ra_achievements(ra_game_id);
CREATE TABLE IF NOT EXISTS ra_progress (
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    ra_game_id      INTEGER NOT NULL,
    awarded         INTEGER NOT NULL DEFAULT 0,
    awarded_hc      INTEGER NOT NULL DEFAULT 0,
    max_possible    INTEGER NOT NULL DEFAULT 0,
    last_award_at   TEXT,
    award_kind      TEXT,
    details_sig     TEXT,
    PRIMARY KEY (profile_id, ra_game_id)
);
CREATE TABLE IF NOT EXISTS ra_unlocks (
    profile_id         INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    ra_achievement_id  INTEGER NOT NULL,
    ra_game_id         INTEGER NOT NULL,
    unlocked_at        TEXT,
    unlocked_hc_at     TEXT,
    seen_at            TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, ra_achievement_id)
);
CREATE INDEX IF NOT EXISTS idx_ra_unlocks_game ON ra_unlocks(profile_id, ra_game_id);
-- Associação jogo da biblioteca ↔ jogo do RetroAchievements.
-- status: 'hash' (hash oficial idêntico) | 'manual' (confirmado pelo usuário) | 'unsupported' (marcado sem suporte)
CREATE TABLE IF NOT EXISTS ra_links (
    game_id     INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    ra_game_id  INTEGER,
    status      TEXT NOT NULL,
    hash        TEXT,
    file_sig    TEXT,
    checked_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
-- Hash calculado de cada arquivo (evita reler o disco) mesmo quando ainda não há associação.
CREATE TABLE IF NOT EXISTS ra_file_hashes (
    game_id   INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    file_sig  TEXT NOT NULL,
    hash      TEXT,
    error     TEXT
);
CREATE TABLE IF NOT EXISTS ra_hashes (
    md5         TEXT PRIMARY KEY,
    ra_game_id  INTEGER NOT NULL,
    console_id  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ra_catalog_meta (
    console_id  INTEGER PRIMARY KEY,
    fetched_at  TEXT NOT NULL
);
"#;

pub fn badge_url(badge: &Option<String>, locked: bool) -> Option<String> {
    badge.as_ref().filter(|b| !b.is_empty()).map(|b| format!("{MEDIA}/Badge/{b}{}.png", if locked { "_lock" } else { "" }))
}
pub fn icon_url(icon: &Option<String>) -> Option<String> {
    icon.as_ref().filter(|i| !i.is_empty()).map(|i| if i.starts_with("http") { i.clone() } else { format!("{MEDIA}{i}") })
}

// ---------------------------------------------------------------- conta (tabela compartilhada: crate::accounts)

pub use crate::accounts::AccountRow;

pub fn account(conn: &Connection, profile_id: i64) -> AppResult<Option<AccountRow>> {
    crate::accounts::get(conn, profile_id, PLATFORM)
}

pub fn creds(conn: &Connection, profile_id: i64) -> AppResult<Option<Creds>> {
    let Some(a) = account(conn, profile_id)? else { return Ok(None) };
    let key = a.secret.as_deref().and_then(super::secret::unprotect);
    Ok(key.map(|k| Creds::new(&a.username, &k)))
}

pub fn save_account(conn: &Connection, profile_id: i64, username: &str, secret: &[u8], data: &str) -> AppResult<()> {
    crate::accounts::save(conn, profile_id, PLATFORM, username, None, Some(secret), data)
}

pub fn set_account_data(conn: &Connection, profile_id: i64, data: &str) -> AppResult<()> {
    crate::accounts::set_data(conn, profile_id, PLATFORM, None, data)
}

pub fn set_sync_result(conn: &Connection, profile_id: i64, ok_at: Option<&str>, error: Option<&str>) -> AppResult<()> {
    crate::accounts::set_sync_result(conn, profile_id, PLATFORM, ok_at, error)
}

/// Remove a conta (e a chave cifrada). `clear_cache` também apaga progresso e desbloqueios do perfil.
pub fn delete_account(conn: &Connection, profile_id: i64, clear_cache: bool) -> AppResult<()> {
    crate::accounts::delete(conn, profile_id, PLATFORM)?;
    if clear_cache {
        conn.execute("DELETE FROM ra_progress WHERE profile_id=?1", [profile_id])?;
        conn.execute("DELETE FROM ra_unlocks WHERE profile_id=?1", [profile_id])?;
    }
    Ok(())
}

// ---------------------------------------------------------------- catálogo e conquistas

/// Grava/atualiza um jogo sem apagar campos que a resposta não trouxe.
pub fn upsert_game(conn: &Connection, id: i64, title: &str, console_id: i64, console_name: &str, icon: Option<String>, num: Option<i64>, points: Option<i64>) -> AppResult<()> {
    if id <= 0 || title.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO ra_games(ra_game_id, title, console_id, console_name, image_icon, num_achievements, points_total)
         VALUES(?1, ?2, ?3, ?4, ?5, COALESCE(?6, 0), COALESCE(?7, 0))
         ON CONFLICT(ra_game_id) DO UPDATE SET title=excluded.title,
           console_id=CASE WHEN excluded.console_id>0 THEN excluded.console_id ELSE ra_games.console_id END,
           console_name=CASE WHEN excluded.console_name<>'' THEN excluded.console_name ELSE ra_games.console_name END,
           image_icon=COALESCE(excluded.image_icon, ra_games.image_icon),
           num_achievements=COALESCE(?6, ra_games.num_achievements),
           points_total=COALESCE(?7, ra_games.points_total)",
        params![id, title, console_id, console_name, icon, num, points],
    )?;
    Ok(())
}

pub fn upsert_achievement(conn: &Connection, game_id: i64, a: &Value) -> AppResult<i64> {
    let id = int(&a["ID"]).max(int(&a["AchievementID"]));
    let title = text(&a["Title"]);
    if id <= 0 || title.is_empty() {
        return Ok(0);
    }
    conn.execute(
        "INSERT INTO ra_achievements(ra_achievement_id, ra_game_id, title, description, points, true_ratio, badge, display_order, kind)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(ra_achievement_id) DO UPDATE SET ra_game_id=excluded.ra_game_id, title=excluded.title,
           description=excluded.description, points=excluded.points, true_ratio=excluded.true_ratio,
           badge=COALESCE(excluded.badge, ra_achievements.badge),
           display_order=CASE WHEN excluded.display_order<>0 THEN excluded.display_order ELSE ra_achievements.display_order END,
           kind=COALESCE(excluded.kind, ra_achievements.kind)",
        params![
            id, game_id, title, text(&a["Description"]), int(&a["Points"]), int(&a["TrueRatio"]),
            opt_text(&a["BadgeName"]), int(&a["DisplayOrder"]), opt_text(&a["type"]).or_else(|| opt_text(&a["Type"]))
        ],
    )?;
    Ok(id)
}

/// Novo desbloqueio detectado nesta gravação.
#[derive(Clone, Debug)]
pub struct NewUnlock {
    pub achievement_id: i64,
    pub hardcore: bool,
    pub at: String,
}

/// Registra um desbloqueio. Devolve `Some` só se ele (ou o modo Hardcore dele) ainda não estava registrado.
/// O desbloqueio já está no cache (softcore ou Hardcore)? Usado pela checagem rápida com emulador aberto.
pub fn is_known_unlock(conn: &Connection, profile_id: i64, ach_id: i64) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM ra_unlocks WHERE profile_id=?1 AND ra_achievement_id=?2 AND (unlocked_at IS NOT NULL OR unlocked_hc_at IS NOT NULL))",
        params![profile_id, ach_id],
        |r| r.get(0),
    )
    .unwrap_or(false)
}

pub fn record_unlock(conn: &Connection, profile_id: i64, ach_id: i64, game_id: i64, soft_at: Option<String>, hc_at: Option<String>) -> AppResult<Option<NewUnlock>> {
    if ach_id <= 0 || (soft_at.is_none() && hc_at.is_none()) {
        return Ok(None);
    }
    // Hardcore também conta como desbloqueado (a data de softcore pode não vir separada).
    let soft_at = soft_at.or_else(|| hc_at.clone());
    let prev: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT unlocked_at, unlocked_hc_at FROM ra_unlocks WHERE profile_id=?1 AND ra_achievement_id=?2",
            params![profile_id, ach_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match prev {
        None => {
            conn.execute(
                "INSERT INTO ra_unlocks(profile_id, ra_achievement_id, ra_game_id, unlocked_at, unlocked_hc_at) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![profile_id, ach_id, game_id, soft_at, hc_at],
            )?;
            let at = hc_at.clone().or(soft_at).unwrap_or_default();
            Ok(Some(NewUnlock { achievement_id: ach_id, hardcore: hc_at.is_some(), at }))
        }
        Some((old_soft, old_hc)) => {
            let became_hc = old_hc.is_none() && hc_at.is_some();
            if old_soft.is_none() || became_hc {
                conn.execute(
                    "UPDATE ra_unlocks SET unlocked_at=COALESCE(unlocked_at, ?3), unlocked_hc_at=COALESCE(unlocked_hc_at, ?4), ra_game_id=?5
                     WHERE profile_id=?1 AND ra_achievement_id=?2",
                    params![profile_id, ach_id, soft_at, hc_at, game_id],
                )?;
            }
            Ok(became_hc.then(|| NewUnlock { achievement_id: ach_id, hardcore: true, at: hc_at.unwrap_or_default() }))
        }
    }
}

/// Grava a resposta de `API_GetGameInfoAndUserProgress`. Devolve os desbloqueios novos.
pub fn store_game_details(conn: &Connection, profile_id: i64, v: &Value) -> AppResult<Vec<NewUnlock>> {
    let gid = int(&v["ID"]);
    let achs: Vec<&Value> = match &v["Achievements"] {
        Value::Object(m) => m.values().collect(),
        Value::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    };
    let points: i64 = achs.iter().map(|a| int(&a["Points"])).sum();
    upsert_game(conn, gid, &text(&v["Title"]), int(&v["ConsoleID"]), &text(&v["ConsoleName"]), opt_text(&v["ImageIcon"]), Some(int(&v["NumAchievements"]).max(achs.len() as i64)), Some(points))?;
    let mut new = Vec::new();
    let mut ids = Vec::new();
    for a in achs {
        let id = upsert_achievement(conn, gid, a)?;
        if id == 0 {
            continue;
        }
        ids.push(id);
        if let Some(n) = record_unlock(conn, profile_id, id, gid, norm_date(&a["DateEarned"]), norm_date(&a["DateEarnedHardcore"]))? {
            new.push(n);
        }
    }
    // Conquistas removidas pelo RA do conjunto oficial deixam de aparecer como pendentes.
    if !ids.is_empty() {
        let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        conn.execute(&format!("DELETE FROM ra_achievements WHERE ra_game_id=?1 AND ra_achievement_id NOT IN ({list})"), [gid])?;
    }
    let awarded = int(&v["NumAwardedToUser"]);
    let awarded_hc = int(&v["NumAwardedToUserHardcore"]);
    conn.execute(
        "INSERT INTO ra_progress(profile_id, ra_game_id, awarded, awarded_hc, max_possible, award_kind, details_sig)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(profile_id, ra_game_id) DO UPDATE SET awarded=excluded.awarded, awarded_hc=excluded.awarded_hc,
           max_possible=excluded.max_possible, award_kind=COALESCE(excluded.award_kind, ra_progress.award_kind), details_sig=excluded.details_sig",
        params![profile_id, gid, awarded, awarded_hc, int(&v["NumAchievements"]), opt_text(&v["HighestAwardKind"]), format!("{awarded}/{awarded_hc}")],
    )?;
    conn.execute("UPDATE ra_games SET details_at=datetime('now') WHERE ra_game_id=?1", [gid])?;
    Ok(new)
}

/// Grava `API_GetUserCompletionProgress`. Devolve os jogos cujo progresso mudou desde o último detalhe.
pub fn store_progress(conn: &Connection, profile_id: i64, rows: &[Value]) -> AppResult<Vec<i64>> {
    let mut changed = Vec::new();
    for r in rows {
        let gid = int(&r["GameID"]);
        if gid <= 0 {
            continue;
        }
        upsert_game(conn, gid, &text(&r["Title"]), int(&r["ConsoleID"]), &text(&r["ConsoleName"]), opt_text(&r["ImageIcon"]), Some(int(&r["MaxPossible"])), None)?;
        let (aw, hc, max) = (int(&r["NumAwarded"]), int(&r["NumAwardedHardcore"]), int(&r["MaxPossible"]));
        let sig = format!("{aw}/{hc}");
        let old: Option<Option<String>> = conn
            .query_row("SELECT details_sig FROM ra_progress WHERE profile_id=?1 AND ra_game_id=?2", params![profile_id, gid], |x| x.get(0))
            .optional()?;
        conn.execute(
            "INSERT INTO ra_progress(profile_id, ra_game_id, awarded, awarded_hc, max_possible, last_award_at, award_kind)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(profile_id, ra_game_id) DO UPDATE SET awarded=excluded.awarded, awarded_hc=excluded.awarded_hc,
               max_possible=excluded.max_possible, last_award_at=COALESCE(excluded.last_award_at, ra_progress.last_award_at),
               award_kind=excluded.award_kind",
            params![profile_id, gid, aw, hc, max, norm_date(&r["MostRecentAwardedDate"]), opt_text(&r["HighestAwardKind"])],
        )?;
        if old.flatten().as_deref() != Some(sig.as_str()) {
            changed.push(gid);
        }
    }
    Ok(changed)
}

/// Grava a lista de `API_GetAchievementsEarnedBetween`. Devolve os desbloqueios novos.
pub fn store_earned(conn: &Connection, profile_id: i64, rows: &[Value]) -> AppResult<Vec<NewUnlock>> {
    let mut new = Vec::new();
    for r in rows {
        let gid = int(&r["GameID"]);
        upsert_game(conn, gid, &text(&r["GameTitle"]), int(&r["ConsoleID"]), &text(&r["ConsoleName"]), opt_text(&r["GameIcon"]), None, None)?;
        let id = upsert_achievement(conn, gid, r)?;
        let date = norm_date(&r["Date"]);
        let hc = int(&r["HardcoreMode"]) == 1;
        let (soft, hcd) = if hc { (None, date) } else { (date, None) };
        if let Some(n) = record_unlock(conn, profile_id, id, gid, soft, hcd)? {
            new.push(n);
        }
    }
    Ok(new)
}

/// Catálogo de um console (com hashes). Substitui os hashes antigos do console.
pub fn store_catalog(conn: &Connection, console_id: i64, rows: &[Value]) -> AppResult<usize> {
    conn.execute("DELETE FROM ra_hashes WHERE console_id=?1", [console_id])?;
    let mut n = 0;
    for r in rows {
        let gid = int(&r["ID"]);
        if gid <= 0 {
            continue;
        }
        upsert_game(conn, gid, &text(&r["Title"]), console_id, &text(&r["ConsoleName"]), opt_text(&r["ImageIcon"]), Some(int(&r["NumAchievements"])), Some(int(&r["Points"])))?;
        if let Some(hs) = r["Hashes"].as_array() {
            for h in hs {
                let md5 = text(h).to_lowercase();
                if md5.len() == 32 && md5.chars().all(|c| c.is_ascii_hexdigit()) {
                    conn.execute(
                        "INSERT INTO ra_hashes(md5, ra_game_id, console_id) VALUES(?1, ?2, ?3)
                         ON CONFLICT(md5) DO UPDATE SET ra_game_id=excluded.ra_game_id, console_id=excluded.console_id",
                        params![md5, gid, console_id],
                    )?;
                }
            }
        }
        n += 1;
    }
    conn.execute(
        "INSERT INTO ra_catalog_meta(console_id, fetched_at) VALUES(?1, datetime('now'))
         ON CONFLICT(console_id) DO UPDATE SET fetched_at=excluded.fetched_at",
        [console_id],
    )?;
    Ok(n)
}

/// Catálogo do console baixado há menos de `max_days` dias?
pub fn catalog_fresh(conn: &Connection, console_id: i64, max_days: i64) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM ra_catalog_meta WHERE console_id=?1 AND fetched_at > datetime('now', ?2)",
        params![console_id, format!("-{max_days} days")],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}
pub fn catalog_exists(conn: &Connection, console_id: i64) -> AppResult<bool> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM ra_catalog_meta WHERE console_id=?1", [console_id], |r| r.get(0))?;
    Ok(n > 0)
}

// ---------------------------------------------------------------- consultas para a interface

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AchievementRow {
    pub id: i64,
    pub game_id: i64,
    pub game_title: String,
    pub console_name: String,
    pub title: String,
    pub description: String,
    pub points: i64,
    pub true_ratio: i64,
    pub badge_url: Option<String>,
    pub kind: Option<String>,
    pub unlocked_at: Option<String>,
    pub unlocked_hc_at: Option<String>,
    pub url: String,
}

const ACH_SELECT: &str = "SELECT a.ra_achievement_id, a.ra_game_id, COALESCE(g.title,''), COALESCE(g.console_name,''), a.title, a.description,
    a.points, a.true_ratio, a.badge, a.kind, u.unlocked_at, u.unlocked_hc_at
    FROM ra_achievements a LEFT JOIN ra_games g ON g.ra_game_id=a.ra_game_id
    LEFT JOIN ra_unlocks u ON u.ra_achievement_id=a.ra_achievement_id AND u.profile_id=?1";

fn map_ach(r: &rusqlite::Row) -> rusqlite::Result<AchievementRow> {
    let id: i64 = r.get(0)?;
    let badge: Option<String> = r.get(8)?;
    let soft: Option<String> = r.get(10)?;
    let hc: Option<String> = r.get(11)?;
    Ok(AchievementRow {
        id,
        game_id: r.get(1)?,
        game_title: r.get(2)?,
        console_name: r.get(3)?,
        title: r.get(4)?,
        description: r.get(5)?,
        points: r.get(6)?,
        true_ratio: r.get(7)?,
        badge_url: badge_url(&badge, soft.is_none() && hc.is_none()),
        kind: r.get(9)?,
        unlocked_at: soft,
        unlocked_hc_at: hc,
        url: format!("{}/achievement/{id}", super::client::SITE),
    })
}

/// filter: "recent" (30 dias) | "earned" | "pending" | "hardcore". `query` filtra pelo nome do jogo.
pub fn list_achievements(conn: &Connection, profile_id: i64, filter: &str, game_id: Option<i64>, query: &str, limit: i64) -> AppResult<Vec<AchievementRow>> {
    let (cond, order) = match filter {
        "pending" => ("u.unlocked_at IS NULL AND u.unlocked_hc_at IS NULL AND a.ra_game_id IN (SELECT ra_game_id FROM ra_progress WHERE profile_id=?1)", "g.title COLLATE NOCASE, a.display_order, a.ra_achievement_id"),
        "hardcore" => ("u.unlocked_hc_at IS NOT NULL", "u.unlocked_hc_at DESC"),
        "recent" => ("COALESCE(u.unlocked_hc_at, u.unlocked_at) >= datetime('now','-30 days')", "COALESCE(u.unlocked_hc_at, u.unlocked_at) DESC"),
        "game" => ("1=1", "(u.unlocked_at IS NULL AND u.unlocked_hc_at IS NULL), a.display_order, a.ra_achievement_id"),
        _ => ("(u.unlocked_at IS NOT NULL OR u.unlocked_hc_at IS NOT NULL)", "COALESCE(u.unlocked_hc_at, u.unlocked_at) DESC"),
    };
    let sql = format!(
        "{ACH_SELECT} WHERE {cond} AND (?2 IS NULL OR a.ra_game_id=?2) AND (?3='' OR g.title LIKE '%'||?3||'%' OR a.title LIKE '%'||?3||'%')
         ORDER BY {order} LIMIT ?4"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map(params![profile_id, game_id, query.trim(), limit.clamp(1, 2000)], map_ach)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn achievements_by_ids(conn: &Connection, profile_id: i64, ids: &[i64]) -> AppResult<Vec<AchievementRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    let mut st = conn.prepare(&format!("{ACH_SELECT} WHERE a.ra_achievement_id IN ({list})"))?;
    let rows = st.query_map([profile_id], map_ach)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameRow {
    pub id: i64,
    pub title: String,
    pub console_id: i64,
    pub console_name: String,
    pub icon_url: Option<String>,
    pub num_achievements: i64,
    pub points_total: i64,
    pub awarded: i64,
    pub awarded_hc: i64,
    pub points_earned: i64,
    pub last_award_at: Option<String>,
    pub award_kind: Option<String>,
    pub has_details: bool,
    /// Jogos da biblioteca associados a este jogo do RA.
    pub library_game_id: Option<i64>,
    pub url: String,
}

const GAME_SELECT: &str = "SELECT g.ra_game_id, g.title, g.console_id, g.console_name, g.image_icon, g.num_achievements, g.points_total,
    COALESCE(p.awarded,0), COALESCE(p.awarded_hc,0), p.last_award_at, p.award_kind, g.details_at IS NOT NULL,
    (SELECT COALESCE(SUM(a.points),0) FROM ra_unlocks u JOIN ra_achievements a ON a.ra_achievement_id=u.ra_achievement_id
       WHERE u.profile_id=?1 AND u.ra_game_id=g.ra_game_id),
    (SELECT l.game_id FROM ra_links l JOIN profile_games pg ON pg.game_id=l.game_id AND pg.profile_id=?1
       WHERE l.ra_game_id=g.ra_game_id AND l.status IN ('hash','manual') LIMIT 1),
    COALESCE((SELECT MAX(COALESCE(u.unlocked_hc_at, u.unlocked_at)) FROM ra_unlocks u WHERE u.profile_id=?1 AND u.ra_game_id=g.ra_game_id), p.last_award_at)
    FROM ra_games g LEFT JOIN ra_progress p ON p.ra_game_id=g.ra_game_id AND p.profile_id=?1";

fn map_game(r: &rusqlite::Row) -> rusqlite::Result<GameRow> {
    let id: i64 = r.get(0)?;
    Ok(GameRow {
        id,
        title: r.get(1)?,
        console_id: r.get(2)?,
        console_name: r.get(3)?,
        icon_url: icon_url(&r.get(4)?),
        num_achievements: r.get(5)?,
        points_total: r.get(6)?,
        awarded: r.get(7)?,
        awarded_hc: r.get(8)?,
        last_award_at: r.get::<_, Option<String>>(14)?.or(r.get(9)?),
        award_kind: r.get(10)?,
        has_details: r.get(11)?,
        points_earned: r.get(12)?,
        library_game_id: r.get(13)?,
        url: format!("{}/game/{id}", super::client::SITE),
    })
}

/// Jogos com progresso do perfil (os que aparecem na central).
pub fn list_games(conn: &Connection, profile_id: i64) -> AppResult<Vec<GameRow>> {
    let mut st = conn.prepare(&format!("{GAME_SELECT} WHERE p.profile_id IS NOT NULL ORDER BY p.last_award_at DESC"))?;
    let rows = st.query_map([profile_id], map_game)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn game(conn: &Connection, profile_id: i64, ra_game_id: i64) -> AppResult<Option<GameRow>> {
    Ok(conn.query_row(&format!("{GAME_SELECT} WHERE g.ra_game_id=?2"), params![profile_id, ra_game_id], map_game).optional()?)
}

/// Busca no catálogo local de um console (para associação manual).
pub fn search_catalog(conn: &Connection, profile_id: i64, console_id: i64, query: &str, limit: i64) -> AppResult<Vec<GameRow>> {
    let q = query.trim();
    let mut st = conn.prepare(&format!(
        "{GAME_SELECT} WHERE g.console_id=?2 AND (?3='' OR g.title LIKE '%'||?3||'%') ORDER BY
         CASE WHEN lower(g.title)=lower(?3) THEN 0 WHEN g.title LIKE ?3||'%' THEN 1 ELSE 2 END, g.title COLLATE NOCASE LIMIT ?4"
    ))?;
    let rows = st.query_map(params![profile_id, console_id, q, limit.clamp(1, 200)], map_game)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Totais do perfil a partir do cache.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub unlocked: i64,
    pub unlocked_hc: i64,
    pub games: i64,
    pub completed: i64,
    pub mastered: i64,
    pub beaten: i64,
    pub games_without_details: i64,
}

pub fn totals(conn: &Connection, profile_id: i64) -> AppResult<Totals> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(awarded),0), COALESCE(SUM(awarded_hc),0), COUNT(*),
           SUM(CASE WHEN award_kind='completed' THEN 1 ELSE 0 END), SUM(CASE WHEN award_kind='mastered' THEN 1 ELSE 0 END),
           SUM(CASE WHEN award_kind LIKE 'beaten%' THEN 1 ELSE 0 END),
           SUM(CASE WHEN (SELECT details_at FROM ra_games g WHERE g.ra_game_id=p.ra_game_id) IS NULL THEN 1 ELSE 0 END)
         FROM ra_progress p WHERE profile_id=?1",
        [profile_id],
        |r| Ok(Totals {
            unlocked: r.get(0)?, unlocked_hc: r.get(1)?, games: r.get(2)?, completed: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            mastered: r.get::<_, Option<i64>>(4)?.unwrap_or(0), beaten: r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            games_without_details: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
        }),
    )?)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        c.execute_batch(
            "CREATE TABLE profiles(id INTEGER PRIMARY KEY, username TEXT);
             CREATE TABLE games(id INTEGER PRIMARY KEY, title TEXT, platform TEXT, iso_path TEXT, rom_path TEXT);
             CREATE TABLE profile_games(profile_id INTEGER, game_id INTEGER, PRIMARY KEY(profile_id, game_id));
             CREATE TABLE platform_accounts(id INTEGER PRIMARY KEY AUTOINCREMENT, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
               platform TEXT NOT NULL, account_name TEXT NOT NULL, external_id TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')),
               secret BLOB, data TEXT, synced_at TEXT, sync_error TEXT, UNIQUE(profile_id, platform));
             INSERT INTO profiles(id, username) VALUES(1, 'p1'), (2, 'p2');",
        )
        .unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    fn game_json(earned: &[(i64, Option<&str>, Option<&str>)]) -> Value {
        let mut achs = serde_json::Map::new();
        for (id, soft, hc) in earned {
            achs.insert(id.to_string(), json!({
                "ID": id, "Title": format!("Conquista {id}"), "Description": "d", "Points": 10, "TrueRatio": 12,
                "BadgeName": format!("{id}00"), "DisplayOrder": id, "DateEarned": soft, "DateEarnedHardcore": hc
            }));
        }
        json!({"ID": 100, "Title": "Jogo", "ConsoleID": 21, "ConsoleName": "PlayStation 2", "ImageIcon": "/Images/1.png",
               "NumAchievements": earned.len(), "NumAwardedToUser": earned.iter().filter(|e| e.1.is_some() || e.2.is_some()).count(),
               "NumAwardedToUserHardcore": earned.iter().filter(|e| e.2.is_some()).count(), "Achievements": achs})
    }

    #[test]
    fn detects_only_really_new_unlocks() {
        let c = mem();
        let v1 = game_json(&[(1, Some("2024-01-01 10:00:00"), None), (2, None, None)]);
        let n1 = store_game_details(&c, 1, &v1).unwrap();
        assert_eq!(n1.len(), 1); // a primeira gravação registra (a sincronização decide não notificar a carga inicial)
        // mesma resposta de novo: nada é novo
        assert!(store_game_details(&c, 1, &v1).unwrap().is_empty());
        // conquista 2 desbloqueada em Hardcore, conquista 1 passa a ter Hardcore
        let v2 = game_json(&[(1, Some("2024-01-01 10:00:00"), Some("2024-02-01 10:00:00")), (2, Some("2024-02-02 10:00:00"), Some("2024-02-02 10:00:00"))]);
        let n2 = store_game_details(&c, 1, &v2).unwrap();
        assert_eq!(n2.len(), 2);
        assert!(n2.iter().all(|n| n.hardcore));
        // outro perfil é independente
        assert_eq!(store_game_details(&c, 2, &v2).unwrap().len(), 2);
        let pend = list_achievements(&c, 1, "pending", None, "", 100).unwrap();
        assert!(pend.is_empty());
        let earned = list_achievements(&c, 1, "earned", None, "", 100).unwrap();
        assert_eq!(earned.len(), 2);
        assert!(earned[0].badge_url.as_deref().unwrap().ends_with("00.png"));
    }

    #[test]
    fn earned_between_and_progress_diff() {
        let c = mem();
        let rows = vec![json!({"Date": "2024-05-01 12:00:00", "HardcoreMode": 1, "AchievementID": 55, "Title": "Primeira", "Description": "x",
            "BadgeName": "123", "Points": 5, "TrueRatio": 7, "GameTitle": "Jogo B", "GameIcon": "/Images/2.png", "GameID": 200, "ConsoleName": "PlayStation"})];
        let n = store_earned(&c, 1, &rows).unwrap();
        assert_eq!(n.len(), 1);
        assert!(store_earned(&c, 1, &rows).unwrap().is_empty());
        let prog = vec![json!({"GameID": 200, "Title": "Jogo B", "ConsoleID": 12, "ConsoleName": "PlayStation", "MaxPossible": 10, "NumAwarded": 1,
            "NumAwardedHardcore": 1, "MostRecentAwardedDate": "2024-05-01T12:00:00+00:00", "HighestAwardKind": null})];
        assert_eq!(store_progress(&c, 1, &prog).unwrap(), vec![200]);
        let games = list_games(&c, 1).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].points_earned, 5);
        assert_eq!(games[0].console_id, 12);
        let t = totals(&c, 1).unwrap();
        assert_eq!((t.unlocked, t.unlocked_hc, t.games, t.games_without_details), (1, 1, 1, 1));
    }

    #[test]
    fn catalog_hashes_and_account_lifecycle() {
        let c = mem();
        let rows = vec![json!({"ID": 300, "Title": "Jogo C", "ConsoleName": "PlayStation 2", "NumAchievements": 20, "Points": 200,
            "Hashes": ["ABCDEF0123456789ABCDEF0123456789", "curto"]})];
        assert_eq!(store_catalog(&c, 21, &rows).unwrap(), 1);
        let gid: i64 = c.query_row("SELECT ra_game_id FROM ra_hashes WHERE md5='abcdef0123456789abcdef0123456789'", [], |r| r.get(0)).unwrap();
        assert_eq!(gid, 300);
        assert!(catalog_fresh(&c, 21, 7).unwrap());
        assert_eq!(search_catalog(&c, 1, 21, "jogo", 10).unwrap().len(), 1);

        let blob = super::super::secret::protect("CHAVE123").unwrap();
        save_account(&c, 1, "Jogador", &blob, "{}").unwrap();
        assert_eq!(creds(&c, 1).unwrap().unwrap().user, "Jogador");
        store_earned(&c, 1, &[json!({"Date": "2024-05-01 12:00:00", "HardcoreMode": 0, "AchievementID": 9, "Title": "T", "GameID": 300, "GameTitle": "Jogo C"})]).unwrap();
        delete_account(&c, 1, true).unwrap();
        assert!(account(&c, 1).unwrap().is_none());
        let n: i64 = c.query_row("SELECT COUNT(*) FROM ra_unlocks WHERE profile_id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
