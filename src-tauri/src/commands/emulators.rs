//! Cadastro de emuladores (global do aplicativo) e associação de cada jogo a um emulador.
//! O sistema anterior (executável + argumentos por jogo, com {iso}) continua valendo para jogos sem emulador associado.

use super::games::{canonical_platform, ensure_owned, PLATFORMS};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

pub const FORMATS: [&str; 8] = ["iso", "bin", "cue", "nsp", "xci", "nup", "pkg", "vpk"];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Emulator {
    pub id: i64,
    pub name: String,
    pub exe_path: String,
    pub platforms: Vec<String>,
    pub formats: Vec<String>,
    pub args: String,
    pub cover: Option<String>,
    pub exe_exists: bool,
    /// Iniciar os jogos em tela cheia (aplicado no lançamento, no método de cada emulador).
    pub fullscreen: bool,
    /// O NEXSTAGE reconhece o emulador e sabe aplicar a opção de tela cheia.
    pub fullscreen_supported: bool,
    pub games: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmulatorInput {
    pub id: Option<i64>,
    pub name: String,
    pub exe_path: String,
    pub platforms: Vec<String>,
    pub formats: Vec<String>,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub fullscreen: bool,
}

pub fn load_all(conn: &Connection) -> AppResult<Vec<Emulator>> {
    let mut st = conn.prepare(
        "SELECT e.id,e.name,e.exe_path,e.platforms,e.formats,e.args,e.cover,
                (SELECT COUNT(*) FROM games g WHERE g.emulator_id=e.id), e.fullscreen
         FROM emulators e ORDER BY e.name COLLATE NOCASE",
    )?;
    let rows = st
        .query_map([], |r| {
            let exe: String = r.get(2)?;
            let pl: String = r.get(3)?;
            let fm: String = r.get(4)?;
            let name: String = r.get(1)?;
            Ok(Emulator {
                id: r.get(0)?,
                fullscreen: r.get(8)?,
                fullscreen_supported: crate::emu_fullscreen::supported(&name, &exe),
                name,
                exe_exists: Path::new(&exe).is_file(),
                exe_path: exe,
                platforms: serde_json::from_str(&pl).unwrap_or_default(),
                formats: serde_json::from_str(&fm).unwrap_or_default(),
                args: r.get(5)?,
                cover: crate::images::resolve(r.get(6)?),
                games: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[tauri::command]
pub fn list_emulators(db: State<Db>) -> AppResult<Vec<Emulator>> {
    let conn = db.conn()?;
    load_all(&conn)
}

#[tauri::command]
pub fn save_emulator(db: State<Db>, mut input: EmulatorInput) -> AppResult<Emulator> {
    input.name = input.name.trim().to_string();
    input.exe_path = input.exe_path.trim().to_string();
    if input.name.is_empty() || input.name.chars().count() > 40 {
        return Err(AppError::Validation("Nome do emulador: 1 a 40 caracteres.".into()));
    }
    if !Path::new(&input.exe_path).is_file() {
        return Err(AppError::Validation("Selecione o executável do emulador (arquivo não encontrado).".into()));
    }
    let platforms: Vec<String> = input
        .platforms
        .iter()
        .filter_map(|p| canonical_platform(p))
        .filter(|p| PLATFORMS.contains(p))
        .map(String::from)
        .collect();
    if platforms.is_empty() {
        return Err(AppError::Validation("Escolha ao menos uma plataforma suportada.".into()));
    }
    let formats: Vec<String> = input.formats.iter().map(|f| f.to_lowercase()).filter(|f| FORMATS.contains(&f.as_str())).collect();
    if formats.is_empty() {
        return Err(AppError::Validation("Escolha ao menos um formato suportado.".into()));
    }
    // Capa personalizada: gravada como arquivo (como as capas dos jogos); vazia = arte padrão do console.
    let cover = crate::images::store(input.cover.take())?;
    // Com a opção de tela cheia, os argumentos de tela cheia/janela escritos à mão saem do texto: a caixa manda.
    if let Some(kind) = crate::emu_fullscreen::detect(&input.name, &input.exe_path) {
        input.args = crate::emu_fullscreen::strip_text(kind, &input.args);
    }
    let conn = db.conn()?;
    let (pl, fm) = (serde_json::to_string(&platforms).unwrap_or_default(), serde_json::to_string(&formats).unwrap_or_default());
    let id = match input.id {
        Some(id) => {
            let n = conn.execute(
                "UPDATE emulators SET name=?1,exe_path=?2,platforms=?3,formats=?4,args=?5,cover=?6,fullscreen=?7 WHERE id=?8",
                params![input.name, input.exe_path, pl, fm, input.args.trim(), cover, input.fullscreen, id],
            )?;
            if n == 0 {
                return Err(AppError::NotFound("Emulador não encontrado.".into()));
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO emulators(name,exe_path,platforms,formats,args,cover,fullscreen) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![input.name, input.exe_path, pl, fm, input.args.trim(), cover, input.fullscreen],
            )?;
            conn.last_insert_rowid()
        }
    };
    load_all(&conn)?.into_iter().find(|e| e.id == id).ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))
}

/// Remove o emulador do cadastro. Os jogos ficam na biblioteca, apenas sem associação (emulator_id volta a NULL).
#[tauri::command]
pub fn delete_emulator(db: State<Db>, id: i64) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("UPDATE games SET emulator_id=NULL WHERE emulator_id=?1", [id])?;
    conn.execute("DELETE FROM emulators WHERE id=?1", [id])?;
    Ok(())
}

/// O NEXSTAGE sabe aplicar "Iniciar jogos em tela cheia" a este emulador? (usado no formulário, antes de salvar)
#[tauri::command]
pub fn emulator_fullscreen_supported(name: String, exe_path: String) -> bool {
    crate::emu_fullscreen::supported(&name, &exe_path)
}

/// Associa (ou desassocia, com `None`) o jogo a um emulador cadastrado.
#[tauri::command]
pub fn set_game_emulator(db: State<Db>, profile_id: i64, game_id: i64, emulator_id: Option<i64>) -> AppResult<()> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    if let Some(e) = emulator_id {
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM emulators WHERE id=?1", [e], |r| r.get(0))?;
        if n == 0 {
            return Err(AppError::NotFound("Emulador não encontrado.".into()));
        }
    }
    conn.execute("UPDATE games SET emulator_id=?1 WHERE id=?2", params![emulator_id, game_id])?;
    Ok(())
}
