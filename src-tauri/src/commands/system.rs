//! Ações de energia do aplicativo e do Windows (o frontend sempre pede confirmação antes
//! de reiniciar/desligar).

use crate::error::{AppError, AppResult};
use std::process::Command;
use tauri::{AppHandle, Manager};

/// Prefixo do erro quando o backup automático de encerramento falha ou é cancelado: a interface
/// pergunta se a ação continua mesmo assim (e chama de novo com `skipBackup`).
pub const BACKUP_FAILED: &str = "BACKUP_FAILED::";

fn shutdown(flag: &str) -> AppResult<()> {
    let mut cmd = Command::new("shutdown");
    cmd.args([flag, "/t", "0"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Other("O Windows recusou a operação.".into()))
    }
}

#[tauri::command]
pub async fn power_action(app: AppHandle, action: String, skip_backup: Option<bool>) -> AppResult<()> {
    // Backup automático "antes de encerrar": termina antes de sair, reiniciar ou desligar.
    if matches!(action.as_str(), "exit" | "restart" | "shutdown") && !skip_backup.unwrap_or(false) {
        let a = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::backup::auto::before_exit(&a))
            .await
            .map_err(|_| AppError::Other(format!("{BACKUP_FAILED}Operação interrompida.")))?
            .map_err(|e| AppError::Other(format!("{BACKUP_FAILED}{e}")))?;
    }
    if skip_backup.unwrap_or(false) && action != "minimize" {
        crate::backup::auto::skip_exit_backup();
    }
    match action.as_str() {
        "exit" => {
            app.exit(0);
            Ok(())
        }
        "minimize" => {
            let w = app
                .get_webview_window("main")
                .ok_or_else(|| AppError::Other("Janela indisponível.".into()))?;
            w.minimize().map_err(|e| AppError::Other(e.to_string()))
        }
        "restart" => shutdown("/r"),
        "shutdown" => shutdown("/s"),
        _ => Err(AppError::Validation("Ação desconhecida.".into())),
    }
}
