//! Cadastro de emuladores (global do aplicativo) e associação de cada jogo a um emulador.
//! O sistema anterior (executável + argumentos por jogo, com {iso}) continua valendo para jogos sem emulador associado.

use super::games::{canonical_platform, ensure_owned, PLATFORMS};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

/// Formatos de jogo aceitos (cadastro de emuladores, "Adicionar jogo" e varredura de pastas).
pub const FORMATS: &[&str] = &[
    "iso", "bin", "cue", "nsp", "xci", "nup", "pkg", "vpk",
    // GameCube / Wii
    "gcm", "rvz", "wbfs", "wia", "gcz", "wad",
    // Wii U
    "wud", "wux", "wua", "rpx",
    // DS / 3DS / GBA / Game Boy / Game Boy Color
    "nds", "3ds", "cci", "cxi", "3dsx", "gba", "gb", "gbc",
    // Nintendo 64 (big-endian, byte-swapped e little-endian)
    "z64", "n64", "v64",
    // Mega Drive (o .bin de cartucho é reconhecido pelo cabeçalho "SEGA")
    "md", "gen", "smd",
    // Dreamcast (e imagens CHD de vários consoles)
    "gdi", "cdi", "chd",
    // SNES / NES
    "sfc", "smc", "nes",
];

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
    /// Fonte de atualização: `None` = automática (emulador reconhecido), "-" = desligada, senão a escolhida.
    pub update_source: Option<String>,
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
    #[serde(default)]
    pub update_source: Option<String>,
}

pub fn load_all(conn: &Connection) -> AppResult<Vec<Emulator>> {
    let mut st = conn.prepare(
        "SELECT e.id,e.name,e.exe_path,e.platforms,e.formats,e.args,e.cover,
                (SELECT COUNT(*) FROM games g WHERE g.emulator_id=e.id), e.fullscreen, e.update_source
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
                update_source: r.get(9)?,
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
pub fn save_emulator(db: State<Db>, input: EmulatorInput) -> AppResult<Emulator> {
    let conn = db.conn()?;
    save(&conn, input)
}

/// Valida e grava o emulador (novo ou existente). Usado também pela instalação pelo catálogo.
pub fn save(conn: &Connection, mut input: EmulatorInput) -> AppResult<Emulator> {
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
    let update_source = crate::emu_update::normalize_source(input.update_source.as_deref())?;
    let (pl, fm) = (serde_json::to_string(&platforms).unwrap_or_default(), serde_json::to_string(&formats).unwrap_or_default());
    let id = match input.id {
        Some(id) => {
            let n = conn.execute(
                // trocar o executável (ou a fonte) invalida a versão instalada que o NEXSTAGE registrou
                "UPDATE emulators SET name=?1,exe_path=?2,platforms=?3,formats=?4,args=?5,cover=?6,fullscreen=?7,update_source=?9,
                        update_version=CASE WHEN exe_path=?2 AND update_source IS ?9 THEN update_version END,
                        update_key=CASE WHEN exe_path=?2 AND update_source IS ?9 THEN update_key END
                 WHERE id=?8",
                params![input.name, input.exe_path, pl, fm, input.args.trim(), cover, input.fullscreen, id, update_source],
            )?;
            if n == 0 {
                return Err(AppError::NotFound("Emulador não encontrado.".into()));
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO emulators(name,exe_path,platforms,formats,args,cover,fullscreen,update_source) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![input.name, input.exe_path, pl, fm, input.args.trim(), cover, input.fullscreen, update_source],
            )?;
            conn.last_insert_rowid()
        }
    };
    load_all(conn)?.into_iter().find(|e| e.id == id).ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))
}

/// Remove o emulador do cadastro. Os jogos ficam na biblioteca, apenas sem associação (emulator_id volta a NULL).
#[tauri::command]
pub fn delete_emulator(db: State<Db>, id: i64) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("UPDATE games SET emulator_id=NULL WHERE emulator_id=?1", [id])?;
    conn.execute("DELETE FROM emulators WHERE id=?1", [id])?;
    Ok(())
}

/// Abre o próprio emulador (sem jogo), na pasta dele — para configurar, instalar firmware/pacotes etc.
/// Se ele já estiver aberto, traz a janela para a frente em vez de abrir outra cópia.
/// A janela abre no mesmo monitor do NEXSTAGE (ver `game_window`).
#[tauri::command]
pub fn launch_emulator(app: tauri::AppHandle, db: State<Db>, id: i64) -> AppResult<()> {
    let exe: String = {
        let conn = db.conn()?;
        conn.query_row("SELECT exe_path FROM emulators WHERE id=?1", [id], |r| r.get(0))
            .map_err(|_| AppError::NotFound("Emulador não encontrado.".into()))?
    };
    let path = Path::new(&exe);
    if !path.is_file() {
        return Err(AppError::NotFound(format!("O executável do emulador não foi encontrado: {exe}. Atualize o caminho em Editar.")));
    }
    // já aberto: só traz para a frente (não abre outra cópia)
    if crate::game_window::bring_exe_to_front(path) {
        return Ok(());
    }
    let mut cmd = std::process::Command::new(path);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    let child = cmd.spawn().map_err(|e| AppError::Other(format!("Falha ao abrir o emulador: {e}")))?;
    crate::game_window::follow(&app, crate::game_window::Track::Tree(child.id()));
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
