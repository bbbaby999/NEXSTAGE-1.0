//! Tabelas do Game Bridge (migração aditiva: só cria tabelas novas; nada existente é alterado).
//! Conquistas (`bridge_achievements`/`_unlocks`/`_progress`) e estatísticas (`bridge_stat_defs`/`bridge_stats`)
//! são preenchidas a partir das Fases 6 e 8.

use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS integrated_games (
    game_id             TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    installed_version   TEXT,
    installed_build     INTEGER,
    install_dir         TEXT,
    manifest            TEXT,
    manifest_checked_at TEXT,
    library_game_id     INTEGER REFERENCES games(id) ON DELETE SET NULL
);
CREATE TABLE IF NOT EXISTS bridge_sessions (
    session_id    TEXT PRIMARY KEY,
    game_id       TEXT NOT NULL,
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_version  TEXT,
    slot_uid      TEXT,
    started_at    TEXT NOT NULL,
    ended_at      TEXT,
    status        TEXT NOT NULL,
    last_seq      INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS bridge_achievements (
    game_id          TEXT NOT NULL,
    achievement_id   TEXT NOT NULL,
    data             TEXT NOT NULL,
    catalog_version  INTEGER NOT NULL,
    PRIMARY KEY (game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_unlocks (
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id         TEXT NOT NULL,
    achievement_id  TEXT NOT NULL,
    unlocked_at     TEXT NOT NULL,
    origin          TEXT NOT NULL,
    event_id        TEXT NOT NULL,
    PRIMARY KEY (profile_id, game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_progress (
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id         TEXT NOT NULL,
    achievement_id  TEXT NOT NULL,
    current         INTEGER NOT NULL,
    target          INTEGER NOT NULL,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (profile_id, game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_stats (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id     TEXT NOT NULL,
    slot_uid    TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (profile_id, game_id, slot_uid, key)
);
CREATE TABLE IF NOT EXISTS bridge_stat_defs (
    game_id          TEXT NOT NULL,
    stat_key         TEXT NOT NULL,
    data             TEXT NOT NULL,
    catalog_version  INTEGER NOT NULL,
    PRIMARY KEY (game_id, stat_key)
);
CREATE TABLE IF NOT EXISTS bridge_slots (
    profile_id   INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id      TEXT NOT NULL,
    slot_uid     TEXT NOT NULL,
    slot_number  INTEGER,
    first_seen   TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen    TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, game_id, slot_uid)
);
CREATE TABLE IF NOT EXISTS bridge_events_seen (
    game_id   TEXT NOT NULL,
    event_id  TEXT NOT NULL,
    seen_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (game_id, event_id)
);
"#;

/// Chave opaca do perfil para os jogos (UUID aleatório, criado na primeira vez e guardado em
/// `profile_settings`). O jogo nunca recebe o id numérico do perfil.
pub fn profile_key(conn: &Connection, profile_id: i64) -> AppResult<String> {
    let found: Option<String> = conn
        .query_row("SELECT value FROM profile_settings WHERE profile_id=?1 AND key='bridge_profile_key'", [profile_id], |r| r.get(0))
        .optional()?;
    if let Some(k) = found {
        return Ok(k);
    }
    let k = super::protocol::uuid_v4();
    conn.execute("INSERT INTO profile_settings(profile_id,key,value) VALUES(?1,'bridge_profile_key',?2)", params![profile_id, k])?;
    Ok(k)
}

pub fn session_open(conn: &Connection, session: &str, game: &str, profile_id: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO bridge_sessions(session_id,game_id,profile_id,started_at,status) VALUES(?1,?2,?3,datetime('now'),'waiting')",
        params![session, game, profile_id],
    )?;
    // identificadores vistos há mais de 90 dias não precisam mais ser guardados
    conn.execute("DELETE FROM bridge_events_seen WHERE seen_at < datetime('now','-90 days')", [])?;
    Ok(())
}

pub fn session_set(conn: &Connection, session: &str, col: &str, value: &str) -> AppResult<()> {
    // `col` vem só de chamadas internas (lista fixa)
    debug_assert!(matches!(col, "status" | "game_version" | "slot_uid"));
    conn.execute(&format!("UPDATE bridge_sessions SET {col}=?1 WHERE session_id=?2"), params![value, session])?;
    Ok(())
}

pub fn session_seq(conn: &Connection, session: &str, seq: i64) -> AppResult<()> {
    conn.execute("UPDATE bridge_sessions SET last_seq=?1 WHERE session_id=?2", params![seq, session])?;
    Ok(())
}

pub fn session_end(conn: &Connection, session: &str, status: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE bridge_sessions SET ended_at=datetime('now'), status=CASE WHEN status='closed' THEN status ELSE ?1 END WHERE session_id=?2 AND ended_at IS NULL",
        params![status, session],
    )?;
    Ok(())
}

/// Slot do jogo usado por este perfil (página especial → aba Slots). `slot_number` = número na tela do jogo.
pub fn slot_seen(conn: &Connection, profile: i64, game: &str, slot: &str, number: Option<i64>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO bridge_slots(profile_id,game_id,slot_uid,slot_number) VALUES(?1,?2,?3,?4)
         ON CONFLICT(profile_id,game_id,slot_uid) DO UPDATE SET last_seen=datetime('now'), slot_number=COALESCE(excluded.slot_number, bridge_slots.slot_number)",
        params![profile, game, slot, number],
    )?;
    Ok(())
}

/// Registra um id de evento. `false` = já visto (evento repetido).
pub fn mark_seen(conn: &Connection, game: &str, event_id: &str) -> AppResult<bool> {
    Ok(conn.execute("INSERT OR IGNORE INTO bridge_events_seen(game_id,event_id) VALUES(?1,?2)", params![game, event_id])? == 1)
}

pub fn was_seen(conn: &Connection, game: &str, event_id: &str) -> AppResult<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM bridge_events_seen WHERE game_id=?1 AND event_id=?2", params![game, event_id], |_| Ok(()))
        .optional()?
        .is_some())
}
