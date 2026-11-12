//! Chaves de API guardadas em `app_settings` (pesquisa de imagens: SteamGridDB e RAWG).
//!
//! Ficam cifradas pela DPAPI (`vault`), como as demais credenciais. O valor gravado é
//! `vault1:<base64 do blob>`; valores antigos em texto puro continuam sendo lidos e são cifrados
//! na abertura do banco (`migrate`). Vazio = integração desativada (gravado como "").

use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};

/// Chaves de `app_settings` que guardam segredos.
pub const KEYS: [&str; 2] = ["sgdb_key", "rawg_key"];
const PREFIX: &str = "vault1:";

pub fn is_secret(key: &str) -> bool {
    KEYS.contains(&key)
}

fn entropy(key: &str) -> Vec<u8> {
    format!("nexstage-setting:{key}").into_bytes()
}

/// Valor a gravar no banco. `None` = não foi possível cifrar neste computador.
pub fn seal(key: &str, value: &str) -> Option<String> {
    if value.is_empty() {
        return Some(String::new());
    }
    crate::vault::protect(&entropy(key), value).map(|b| format!("{PREFIX}{}", STANDARD.encode(b)))
}

/// Valor legível a partir do gravado. `None` = cifrado em outro usuário/computador.
pub fn open(key: &str, stored: &str) -> Option<String> {
    match stored.strip_prefix(PREFIX) {
        Some(b64) => STANDARD.decode(b64).ok().and_then(|b| crate::vault::unprotect(&entropy(key), &b)),
        None => Some(stored.to_string()), // legado em texto puro
    }
}

/// Chave configurada (sem espaços nas pontas), ou `None` se vazia/ausente/ilegível.
pub fn read(conn: &Connection, key: &str) -> Option<String> {
    let stored: String = conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get(0)).optional().ok()??;
    open(key, &stored).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Grava a chave cifrada.
pub fn write(conn: &Connection, key: &str, value: &str) -> crate::error::AppResult<()> {
    let sealed = seal(key, value).ok_or_else(|| crate::error::AppError::Other("Não foi possível proteger a credencial neste computador.".into()))?;
    conn.execute(
        "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, sealed],
    )?;
    Ok(())
}

/// Cifra chaves que versões anteriores gravaram em texto puro. Idempotente.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for key in KEYS {
        let stored: Option<String> = conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get(0)).optional()?;
        if let Some(v) = stored.filter(|v| !v.is_empty() && !v.starts_with(PREFIX)) {
            if let Some(sealed) = seal(key, &v) {
                conn.execute("UPDATE app_settings SET value=?2 WHERE key=?1", params![key, sealed])?;
            }
        }
    }
    Ok(())
}

/// Leitura pelo `AppHandle` (comandos de pesquisa).
pub fn get(app: &tauri::AppHandle) -> impl Fn(&str) -> Option<String> + '_ {
    use tauri::Manager;
    move |key| {
        let db = app.state::<crate::db::Db>();
        let conn = db.conn().ok()?;
        read(&conn, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_sealed_and_legacy_plaintext_is_migrated() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
        c.execute_batch("INSERT INTO app_settings(key,value) VALUES('sgdb_key','  LEGADO123  '),('rawg_key',''),('tema','escuro');").unwrap();
        assert_eq!(read(&c, "sgdb_key").as_deref(), Some("LEGADO123"), "texto puro antigo continua valendo");
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        let raw: String = c.query_row("SELECT value FROM app_settings WHERE key='sgdb_key'", [], |r| r.get(0)).unwrap();
        assert!(raw.starts_with(PREFIX) && !raw.contains("LEGADO123"), "{raw}");
        assert_eq!(read(&c, "sgdb_key").as_deref(), Some("LEGADO123"));
        assert_eq!(read(&c, "rawg_key"), None, "vazio = desativado");
        write(&c, "rawg_key", "RAWG-XYZ").unwrap();
        assert_eq!(read(&c, "rawg_key").as_deref(), Some("RAWG-XYZ"));
        write(&c, "rawg_key", "").unwrap();
        assert_eq!(read(&c, "rawg_key"), None);
        let tema: String = c.query_row("SELECT value FROM app_settings WHERE key='tema'", [], |r| r.get(0)).unwrap();
        assert_eq!(tema, "escuro");
        // a chave de uma integração não abre com a entropia da outra
        #[cfg(windows)]
        assert_eq!(open("rawg_key", &raw), None);
    }
}
