//! Configurações globais (`app_settings`) e por perfil (`profile_settings`).

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, OptionalExtension};
use tauri::{State, WebviewWindow};

#[tauri::command]
pub fn get_app_setting(db: State<Db>, key: String) -> AppResult<Option<String>> {
    let conn = db.conn()?;
    let v: Option<String> = conn
        .query_row("SELECT value FROM app_settings WHERE key=?1", [&key], |r| r.get(0))
        .optional()?;
    // Chaves de API nunca saem do backend: a interface só fica sabendo se há uma chave salva ("1").
    if crate::apikeys::is_secret(&key) {
        return Ok(secret_status(&conn, &key));
    }
    Ok(v)
}

/// "1" se a chave de API está salva e legível neste computador; `None` se não.
fn secret_status(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    crate::apikeys::read(conn, key).map(|_| "1".to_string())
}

#[cfg(test)]
mod secret_tests {
    #[test]
    fn api_keys_are_never_returned_to_the_ui() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
        assert_eq!(super::secret_status(&c, "rawg_key"), None);
        crate::apikeys::write(&c, "rawg_key", "MINHA-CHAVE-SECRETA").unwrap();
        assert_eq!(super::secret_status(&c, "rawg_key").as_deref(), Some("1"));
    }
}

#[tauri::command]
pub fn set_app_setting(db: State<Db>, key: String, value: String) -> AppResult<()> {
    let conn = db.conn()?;
    if crate::apikeys::is_secret(&key) {
        return crate::apikeys::write(&conn, &key, &value);
    }
    conn.execute(
        "INSERT INTO app_settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[tauri::command]
pub fn get_profile_setting(db: State<Db>, profile_id: i64, key: String) -> AppResult<Option<String>> {
    let conn = db.conn()?;
    Ok(conn
        .query_row(
            "SELECT value FROM profile_settings WHERE profile_id=?1 AND key=?2",
            params![profile_id, key],
            |r| r.get(0),
        )
        .optional()?)
}

#[tauri::command]
pub fn set_profile_setting(db: State<Db>, profile_id: i64, key: String, value: String) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO profile_settings(profile_id,key,value) VALUES(?1,?2,?3)
         ON CONFLICT(profile_id,key) DO UPDATE SET value=excluded.value",
        params![profile_id, key, value],
    )?;
    Ok(())
}

/// Fundos da Home (por perfil), referências `nximg:` em `profile_settings` — protegidas da limpeza
/// de `images::migrate_and_clean`:
/// - `home_bg_image`: imagem própria (modo "Imagem própria");
/// - `home_bg_capture`: cópia da captura escolhida (modo "Escolher da Galeria").
pub const HOME_BG_IMAGE: &str = "home_bg_image";
pub const HOME_BG_CAPTURE: &str = "home_bg_capture";
pub const HOME_BG_KEYS: [&str; 2] = [HOME_BG_IMAGE, HOME_BG_CAPTURE];

const fn home_bg_key(gallery: bool) -> &'static str {
    if gallery { HOME_BG_CAPTURE } else { HOME_BG_IMAGE }
}

#[derive(serde::Serialize)]
pub struct HomeBackgrounds {
    image: Option<String>,
    capture: Option<String>,
}

#[tauri::command]
pub fn get_home_background(db: State<Db>, profile_id: i64) -> AppResult<HomeBackgrounds> {
    let conn = db.conn()?;
    let get = |key: &str| -> AppResult<Option<String>> {
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM profile_settings WHERE profile_id=?1 AND key=?2",
                params![profile_id, key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(crate::images::resolve(v).filter(|p| std::path::Path::new(p).is_file()))
    };
    Ok(HomeBackgrounds { image: get(HOME_BG_IMAGE)?, capture: get(HOME_BG_CAPTURE)? })
}

/// Grava a imagem escolhida (data URL) como arquivo e devolve o caminho; `None` remove.
/// `gallery: true` age sobre o fundo do modo "Escolher da Galeria" (só para remover).
#[tauri::command]
pub fn set_home_background(db: State<Db>, profile_id: i64, image: Option<String>, gallery: Option<bool>) -> AppResult<Option<String>> {
    let stored = crate::images::store(image)?;
    save_home_background(&db, profile_id, home_bg_key(gallery.unwrap_or(false)), stored)
}

/// Usa uma captura da Galeria do perfil como fundo da Home. O caminho vem do banco (nunca da
/// interface) e o arquivo é COPIADO: excluir a captura depois não apaga o fundo.
#[tauri::command(async)] // fora da thread da interface (leitura de arquivo grande)
pub fn set_home_background_from_capture(db: State<Db>, profile_id: i64, capture_id: i64) -> AppResult<Option<String>> {
    let path: String = db
        .conn()?
        .query_row(
            "SELECT path FROM gallery_items WHERE id=?1 AND profile_id=?2",
            params![capture_id, profile_id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound("Captura não encontrada.".into()))?;
    let p = std::path::Path::new(&path);
    if !p.is_file() {
        return Err(AppError::NotFound("O arquivo desta captura não existe mais no disco.".into()));
    }
    let stored = crate::images::store_file(p)?;
    save_home_background(&db, profile_id, HOME_BG_CAPTURE, Some(stored))
}

fn save_home_background(db: &Db, profile_id: i64, key: &str, stored: Option<String>) -> AppResult<Option<String>> {
    let conn = db.conn()?;
    match &stored {
        Some(v) => conn.execute(
            "INSERT INTO profile_settings(profile_id,key,value) VALUES(?1,?2,?3)
             ON CONFLICT(profile_id,key) DO UPDATE SET value=excluded.value",
            params![profile_id, key, v],
        )?,
        None => conn.execute(
            "DELETE FROM profile_settings WHERE profile_id=?1 AND key=?2",
            params![profile_id, key],
        )?,
    };
    Ok(crate::images::resolve(stored))
}

#[tauri::command]
pub fn set_fullscreen(window: WebviewWindow, enabled: bool) -> AppResult<()> {
    window
        .set_fullscreen(enabled)
        .map_err(|e| AppError::Other(format!("Não foi possível alterar o modo de tela: {e}")))
}

/// Idioma escolhido no instalador (código LCID do Windows, ex.: "1046" = português do Brasil).
/// O instalador grava `installer-lang.txt` na pasta do programa (hook em `windows/hooks.nsh`);
/// `None` em versões de desenvolvimento ou instalações antigas.
#[tauri::command]
pub fn installer_language() -> Option<String> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let raw = std::fs::read_to_string(dir.join("installer-lang.txt")).ok()?;
    let v = raw.trim().trim_start_matches('\u{feff}').to_string();
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_digit())).then_some(v)
}
