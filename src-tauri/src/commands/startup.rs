//! Inicialização com o Windows e modo de abertura.
//!
//! Mecanismo: plugin oficial `tauri-plugin-autostart`, que grava UMA entrada com o nome do app em
//! HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run (sem administrador, sem duplicar) e a remove ao
//! desativar. O caminho gravado é o do executável em execução, por isso vale a versão instalada.
//!
//! Modos (mutuamente exclusivos, salvos em `app_settings.startup_mode`):
//! none (sem iniciar com o Windows; abre conforme "Modo de tela"), minimized, window, fullscreen.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

const MODES: [&str; 4] = ["none", "minimized", "window", "fullscreen"];

fn setting(app: &AppHandle, key: &str) -> Option<String> {
    let db = app.state::<Db>();
    let conn = db.conn().ok()?;
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get::<_, String>(0)).ok()
}

pub fn saved_mode(app: &AppHandle) -> String {
    setting(app, "startup_mode").filter(|m| MODES.contains(&m.as_str())).unwrap_or_else(|| "none".into())
}

/// Aplica o modo de abertura (a janela é criada oculta no tauri.conf.json).
///
/// Ordem importa no Windows: a tela cheia só é aplicada DEPOIS de exibir a janela. O tao precisa
/// ativá-la ao entrar em tela cheia (para cobrir a barra de tarefas) e isso não tem efeito numa janela
/// oculta — antes, ela abria "quase" em tela cheia e só corrigia alternando o modo manualmente.
pub fn apply_start_mode(app: &AppHandle) {
    let mode = saved_mode(app);
    let fullscreen_pref = setting(app, "fullscreen").map(|v| v == "1").unwrap_or(false);
    let fullscreen = match mode.as_str() {
        "fullscreen" => true,
        "window" | "minimized" => false,
        _ => fullscreen_pref,
    };
    if let Some(w) = app.get_webview_window("main") {
        if !fullscreen {
            crate::window_bounds::restore(app, &w); // último tamanho/posição em modo janela
        }
        let _ = w.show(); // sempre exibe: nunca deixa a janela invisível
        if fullscreen {
            let _ = w.set_fullscreen(true);
        }
        if mode == "minimized" {
            let _ = w.minimize(); // o usuário restaura pela barra de tarefas
        } else {
            let _ = w.set_focus(); // primeiro plano: controles e teclado já funcionam na abertura
        }
    }
    crate::window_bounds::track(app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupInfo {
    pub mode: String,
    /// Estado REAL da entrada de inicialização no Windows.
    pub autostart_enabled: bool,
    /// Em build de desenvolvimento a entrada aponta para o executável de desenvolvimento.
    pub dev_build: bool,
}

fn info(app: &AppHandle) -> StartupInfo {
    StartupInfo {
        mode: saved_mode(app),
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
        dev_build: cfg!(debug_assertions),
    }
}

#[tauri::command]
pub fn get_startup_info(app: AppHandle) -> StartupInfo {
    info(&app)
}

#[tauri::command]
pub fn set_startup_mode(app: AppHandle, db: State<Db>, mode: String) -> AppResult<StartupInfo> {
    if !MODES.contains(&mode.as_str()) {
        return Err(AppError::Validation("Opção de inicialização inválida.".into()));
    }
    let al = app.autolaunch();
    let fail = |e: &dyn std::fmt::Display| {
        AppError::Other(format!(
            "Não foi possível alterar a inicialização com o Windows: {e}. Verifique permissões da conta ou bloqueios de antivírus/política do sistema."
        ))
    };
    let enabled = al.is_enabled().map_err(|e| fail(&e))?;
    if mode == "none" {
        if enabled { al.disable().map_err(|e| fail(&e))?; }
    } else if !enabled {
        al.enable().map_err(|e| fail(&e))?;
    }
    // Confirma o resultado real antes de gravar a preferência.
    let now = al.is_enabled().map_err(|e| fail(&e))?;
    if (mode != "none") != now {
        return Err(AppError::Other("O Windows não confirmou a alteração da inicialização automática.".into()));
    }
    {
        let conn = db.conn()?;
        conn.execute(
            "INSERT INTO app_settings(key,value) VALUES('startup_mode',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![mode],
        )?;
    }
    Ok(info(&app))
}

/// Depois de restaurar um backup: a entrada de inicialização do Windows passa a seguir o `startup_mode`
/// restaurado (a entrada é deste computador e não vem no backup). Melhor esforço.
pub fn sync_autostart(app: &AppHandle) {
    let want = saved_mode(app) != "none";
    let al = app.autolaunch();
    match al.is_enabled() {
        Ok(true) if !want => { let _ = al.disable(); }
        Ok(false) if want => { let _ = al.enable(); }
        _ => {}
    }
}
