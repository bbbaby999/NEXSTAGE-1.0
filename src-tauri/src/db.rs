//! Camada de persistência SQLite: abertura da conexão e schema.
//!
//! O schema é idempotente (`IF NOT EXISTS`). Para futuras mudanças, adicione
//! migrações versionadas usando `PRAGMA user_version`.

use crate::error::AppResult;
use rusqlite::{params, Connection};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        // Schema + migrações numa única transação: se o app fechar no meio (queda de energia, kill),
        // nada fica pela metade (ex.: só parte das colunas de uma migração criada).
        let tx = conn.transaction()?;
        tx.execute_batch(SCHEMA)?;
        migrate(&tx)?;
        // RetroAchievements: tabelas próprias de cache (criadas depois das migrações de platform_accounts).
        tx.execute_batch(crate::retro::store::SCHEMA)?;
        // Steam: cache da conta, vínculos com a biblioteca e conquistas.
        tx.execute_batch(crate::steam::store::SCHEMA)?;
        tx.execute_batch(crate::steam::news::SCHEMA)?;
        // Central de fontes de dados (IGDB, HowLongToBeat, Nexus Mods).
        tx.execute_batch(crate::sources::SCHEMA)?;
        // Proteção de perfis (PIN, sequência do controle, recuperação por e-mail) foi removida do app:
        // apaga as credenciais e os dados ligados a ela que versões anteriores tenham gravado.
        // Idempotente; também roda ao restaurar backups antigos (o banco restaurado passa por `Db::open`).
        tx.execute_batch(
            "DROP TABLE IF EXISTS auth_codes;
             DROP TABLE IF EXISTS profile_auth;
             DELETE FROM app_settings WHERE key IN ('mail_config','mail_secret');",
        )?;
        // Chaves de API (SteamGridDB, RAWG) gravadas em texto puro por versões anteriores: cifra.
        crate::apikeys::migrate(&tx)?;
        tx.commit()?;
        Ok(Self(Mutex::new(conn)))
    }

    pub fn conn(&self) -> AppResult<MutexGuard<'_, Connection>> {
        // Um pânico em outro comando "envenena" o Mutex; a conexão SQLite continua válida (cada comando
        // usa transações próprias), então recupera em vez de deixar o banco indisponível até reiniciar.
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS profiles (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    username           TEXT NOT NULL UNIQUE,
    display_name       TEXT NOT NULL,
    bio                TEXT NOT NULL DEFAULT '',
    avatar             TEXT,
    banner             TEXT,
    favorite_game      TEXT,
    favorite_platform  TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Catálogo de jogos. `source` permite futuras origens: manual, steam, epic, xbox, emulator.
CREATE TABLE IF NOT EXISTS games (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    title            TEXT NOT NULL,
    platform         TEXT NOT NULL DEFAULT 'PC',
    executable_path  TEXT NOT NULL,
    launch_args      TEXT NOT NULL DEFAULT '',
    cover            TEXT,
    source           TEXT NOT NULL DEFAULT 'manual',
    external_id      TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Biblioteca individual de cada perfil.
CREATE TABLE IF NOT EXISTS profile_games (
    profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id           INTEGER NOT NULL REFERENCES games(id)    ON DELETE CASCADE,
    favorite          INTEGER NOT NULL DEFAULT 0,
    playtime_seconds  INTEGER NOT NULL DEFAULT 0,
    last_played_at    TEXT,
    added_at          TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, game_id)
);

CREATE TABLE IF NOT EXISTS achievements (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id      INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    external_id  TEXT,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    icon         TEXT
);

CREATE TABLE IF NOT EXISTS profile_achievements (
    profile_id      INTEGER NOT NULL REFERENCES profiles(id)     ON DELETE CASCADE,
    achievement_id  INTEGER NOT NULL REFERENCES achievements(id) ON DELETE CASCADE,
    unlocked_at     TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, achievement_id)
);

-- Contas de plataformas (Steam, Epic, Xbox...). Reservado para integrações futuras.
CREATE TABLE IF NOT EXISTS platform_accounts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    platform      TEXT NOT NULL,
    account_name  TEXT NOT NULL,
    external_id   TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (profile_id, platform)
);

CREATE TABLE IF NOT EXISTS play_sessions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id           INTEGER NOT NULL REFERENCES games(id)    ON DELETE CASCADE,
    started_at        TEXT NOT NULL,
    ended_at          TEXT,
    duration_seconds  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS profile_settings (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (profile_id, key)
);

CREATE TABLE IF NOT EXISTS app_settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

-- Coleções personalizadas (como as pastas da Steam), por perfil.
CREATE TABLE IF NOT EXISTS collections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    name        TEXT NOT NULL COLLATE NOCASE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (profile_id, name)
);

CREATE TABLE IF NOT EXISTS collection_games (
    collection_id  INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    game_id        INTEGER NOT NULL REFERENCES games(id)       ON DELETE CASCADE,
    added_at       TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (collection_id, game_id)
);

-- Metadados do jogo (JSON): descrição, gêneros, notas, capturas de tela, links.
CREATE TABLE IF NOT EXISTS game_info (
    game_id     INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    data        TEXT NOT NULL,
    fetched_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Capturas de tela copiadas para a pasta gerenciada pelo NEXSTAGE (por perfil).
CREATE TABLE IF NOT EXISTS gallery_items (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id     INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    game_id        INTEGER REFERENCES games(id) ON DELETE SET NULL,
    path           TEXT NOT NULL,
    hash           TEXT NOT NULL,
    original_name  TEXT NOT NULL DEFAULT '',
    favorite       INTEGER NOT NULL DEFAULT 0,
    taken_at       TEXT,
    added_at       TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (profile_id, hash)
);

-- Emuladores cadastrados (globais do aplicativo).
CREATE TABLE IF NOT EXISTS emulators (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    exe_path    TEXT NOT NULL,
    platforms   TEXT NOT NULL DEFAULT '[]',
    formats     TEXT NOT NULL DEFAULT '[]',
    args        TEXT NOT NULL DEFAULT '',
    cover       TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Pastas de jogos adicionadas à biblioteca de cada perfil (remover só desfaz o vínculo).
CREATE TABLE IF NOT EXISTS library_folders (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    path          TEXT NOT NULL,
    added_at      TEXT NOT NULL DEFAULT (datetime('now')),
    last_scan_at  TEXT,
    UNIQUE (profile_id, path)
);

CREATE INDEX IF NOT EXISTS idx_play_sessions_profile ON play_sessions(profile_id, started_at);
"#;

fn has_column(conn: &Connection, table: &str, col: &str) -> AppResult<bool> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|n| n == col))
}

/// Migrações aditivas: preservam todos os dados existentes.
fn migrate(conn: &Connection) -> AppResult<()> {
    if !has_column(conn, "games", "background")? {
        conn.execute_batch(
            "ALTER TABLE games ADD COLUMN background TEXT;
             ALTER TABLE games ADD COLUMN bg_x INTEGER NOT NULL DEFAULT 50;
             ALTER TABLE games ADD COLUMN bg_y INTEGER NOT NULL DEFAULT 50;",
        )?;
    }
    if !has_column(conn, "profile_games", "completed")? {
        conn.execute_batch(
            "ALTER TABLE profile_games ADD COLUMN completed INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE profile_games ADD COLUMN completed_at TEXT;",
        )?;
    }
    if !has_column(conn, "games", "iso_path")? {
        conn.execute_batch("ALTER TABLE games ADD COLUMN iso_path TEXT;")?;
    }
    if !has_column(conn, "games", "rom_path")? {
        conn.execute_batch(
            "ALTER TABLE games ADD COLUMN rom_path TEXT;
             ALTER TABLE games ADD COLUMN emulator_id INTEGER REFERENCES emulators(id) ON DELETE SET NULL;
             ALTER TABLE games ADD COLUMN folder_id INTEGER REFERENCES library_folders(id) ON DELETE SET NULL;",
        )?;
    }
    if !has_column(conn, "emulators", "cover")? {
        conn.execute_batch("ALTER TABLE emulators ADD COLUMN cover TEXT;")?;
    }
    // RetroAchievements: a conta usa a tabela reservada `platform_accounts` (uma por perfil), com a chave
    // cifrada (DPAPI), o último perfil sincronizado e o estado da sincronização.
    if !has_column(conn, "platform_accounts", "secret")? {
        conn.execute_batch(
            "ALTER TABLE platform_accounts ADD COLUMN secret BLOB;
             ALTER TABLE platform_accounts ADD COLUMN data TEXT;
             ALTER TABLE platform_accounts ADD COLUMN synced_at TEXT;
             ALTER TABLE platform_accounts ADD COLUMN sync_error TEXT;",
        )?;
    }
    migrate_platforms(conn)?;
    Ok(())
}

/// Padroniza plataformas já cadastradas (jogos e plataforma favorita dos perfis) uma única vez.
/// Só altera valores com equivalente seguro; o restante é preservado.
fn migrate_platforms(conn: &Connection) -> AppResult<()> {
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v >= 1 {
        return Ok(());
    }
    for (table, col) in [("games", "platform"), ("profiles", "favorite_platform")] {
        let mut st = conn.prepare(&format!("SELECT DISTINCT {col} FROM {table} WHERE {col} IS NOT NULL"))?;
        let vals = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        drop(st);
        for old in vals {
            if let Some(new) = crate::commands::games::canonical_platform(&old) {
                if new != old {
                    conn.execute(&format!("UPDATE {table} SET {col}=?1 WHERE {col}=?2"), params![new, old])?;
                }
            }
        }
    }
    conn.execute_batch("PRAGMA user_version = 1;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("nexstage-test-{}-{name}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        p
    }

    #[test]
    fn removes_old_profile_protection_data() {
        let p = tmp("authgone");
        drop(Db::open(&p).unwrap());
        {
            // banco de uma versão com proteção de perfis
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "INSERT INTO profiles(id,username,display_name) VALUES(1,'a','A');
                 CREATE TABLE profile_auth(profile_id INTEGER PRIMARY KEY, mode TEXT, pin_hash TEXT, email_enc BLOB);
                 INSERT INTO profile_auth VALUES(1,'pin','$argon2id$hash',x'00');
                 CREATE TABLE auth_codes(id INTEGER PRIMARY KEY, profile_id INTEGER, code_hash TEXT);
                 INSERT INTO auth_codes VALUES(1,1,'h');
                 INSERT INTO app_settings(key,value) VALUES('mail_config','{}'),('mail_secret','AAAA'),('auto_profile','1');",
            ).unwrap();
        }
        let db = Db::open(&p).unwrap();
        let c = db.conn().unwrap();
        let tables: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('profile_auth','auth_codes')", [], |r| r.get(0)).unwrap();
        assert_eq!(tables, 0, "PINs e códigos apagados");
        let mail: i64 = c.query_row("SELECT COUNT(*) FROM app_settings WHERE key LIKE 'mail_%'", [], |r| r.get(0)).unwrap();
        assert_eq!(mail, 0, "configuração de e-mail apagada");
        let kept: i64 = c.query_row("SELECT COUNT(*) FROM app_settings WHERE key='auto_profile'", [], |r| r.get(0)).unwrap();
        assert_eq!(kept, 1, "demais configurações preservadas");
        let profiles: i64 = c.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get(0)).unwrap();
        assert_eq!(profiles, 1, "perfis preservados");
    }

    #[test]
    fn opens_fresh_and_reopens_idempotently() {
        let p = tmp("fresh");
        drop(Db::open(&p).unwrap());
        let db = Db::open(&p).unwrap(); // migrações de novo: nada quebra nem duplica
        let conn = db.conn().unwrap();
        assert!(has_column(&conn, "emulators", "cover").unwrap());
        assert!(has_column(&conn, "games", "folder_id").unwrap());
        assert!(has_column(&conn, "platform_accounts", "secret").unwrap());
        assert!(has_column(&conn, "ra_links", "status").unwrap());
        assert!(has_column(&conn, "steam_links", "appid").unwrap());
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn migrates_old_database_keeping_data() {
        let p = tmp("old");
        {
            // Banco de uma versão antiga: emuladores sem capa e um registro existente.
            let c = Connection::open(&p).unwrap();
            c.execute_batch(
                "CREATE TABLE emulators (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, exe_path TEXT NOT NULL,
                   platforms TEXT NOT NULL DEFAULT '[]', formats TEXT NOT NULL DEFAULT '[]', args TEXT NOT NULL DEFAULT '',
                   created_at TEXT NOT NULL DEFAULT (datetime('now')));
                 INSERT INTO emulators(name, exe_path) VALUES('PCSX2', 'C:/pcsx2.exe');",
            )
            .unwrap();
        }
        let db = Db::open(&p).unwrap();
        let conn = db.conn().unwrap();
        assert!(has_column(&conn, "emulators", "cover").unwrap());
        let name: String = conn.query_row("SELECT name FROM emulators", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "PCSX2");
    }
}
