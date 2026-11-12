//! Comandos de Backup e Restauração (Configurações → Backup e restauração). A lógica fica em `crate::backup`.

use crate::backup::{self, auto, CreateOpts, CreateReport, Ctx, Inspect, Kind, Op, RestoreReport, VerifyReport};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| AppError::Other("Operação interrompida.".into()))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// Credenciais salvas (um backup sem senha não as leva).
    pub secrets: usize,
    pub captures_dir: String,
    pub busy: bool,
}

#[tauri::command]
pub fn backup_info(app: AppHandle, db: tauri::State<Db>) -> AppResult<BackupInfo> {
    let secrets = backup::secrets::count(&*db.conn()?)?;
    Ok(BackupInfo {
        secrets,
        captures_dir: crate::screenshot::captures_dir(&app).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        busy: backup::is_busy(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetStatus {
    /// "new" | "replace" (já existe um backup do NEXSTAGE ali) | "conflict" (pasta com o mesmo nome que não é backup)
    pub status: String,
    pub path: String,
}

#[tauri::command]
pub fn backup_target_status(dir: String) -> AppResult<TargetStatus> {
    let parent = PathBuf::from(dir.trim());
    if !parent.is_dir() {
        return Err(AppError::Validation("Pasta de destino indisponível.".into()));
    }
    let p = parent.join(backup::FOLDER);
    let status = if !p.exists() { "new" } else if backup::is_backup_dir(&p) { "replace" } else { "conflict" };
    Ok(TargetStatus { status: status.into(), path: p.to_string_lossy().into_owned() })
}

#[tauri::command]
pub async fn backup_create(app: AppHandle, dir: String, password: Option<String>, include_captures: bool, replace: bool) -> AppResult<CreateReport> {
    blocking(move || {
        let _busy = backup::Busy::take()?;
        let mut op = Op::new(&app, "create", "manual");
        let opts = CreateOpts {
            parent: PathBuf::from(dir.trim()),
            name: backup::FOLDER.into(),
            password: password.filter(|p| !p.is_empty()),
            include_captures,
            kind: Kind::Manual,
            replace,
        };
        backup::create(&Ctx::new(&app)?, &opts, &mut op)
    })
    .await
}

#[tauri::command]
pub fn backup_cancel() -> bool {
    backup::cancel()
}

#[tauri::command]
pub async fn backup_inspect(app: AppHandle, path: String, password: Option<String>) -> AppResult<Inspect> {
    blocking(move || {
        let root = backup::locate(Path::new(path.trim()))?;
        Ok(backup::check(&Ctx::new(&app)?, &root, password.as_deref())?.1)
    })
    .await
}

#[tauri::command]
pub async fn backup_verify(app: AppHandle, path: String) -> AppResult<VerifyReport> {
    blocking(move || {
        let _busy = backup::Busy::take()?;
        let root = backup::locate(Path::new(path.trim()))?;
        let m = backup::read_manifest(&root)?;
        let mut op = Op::new(&app, "verify", "restore");
        backup::verify(&root, &m, &mut op)
    })
    .await
}

#[tauri::command]
pub async fn backup_restore(app: AppHandle, path: String, password: Option<String>) -> AppResult<RestoreReport> {
    blocking(move || {
        let _busy = backup::Busy::take()?;
        let root = backup::locate(Path::new(path.trim()))?;
        let cx = Ctx::new(&app)?;
        let (m, info, key) = backup::check(&cx, &root, password.as_deref())?;
        if let Some(p) = info.problem {
            return Err(AppError::Validation(p));
        }
        if info.needs_password {
            return Err(AppError::Validation("Informe a senha do backup.".into()));
        }
        let mut op = Op::new(&app, "restore", "restore");
        let report = backup::restore(&cx, &root, &m, key.as_ref(), &mut op)?;
        // Banco trocado: a entrada de inicialização do Windows passa a seguir a configuração restaurada.
        crate::commands::startup::sync_autostart(&app);
        Ok(report)
    })
    .await
}

/// Reinicia o NEXSTAGE (após restaurar: todos os serviços recarregam com os dados novos).
#[tauri::command]
pub fn backup_restart(app: AppHandle) {
    auto::skip_exit_backup(); // não sobrescrever o recém-restaurado com um backup "antes de encerrar"
    app.restart();
}

#[tauri::command]
pub fn backup_auto_get(db: tauri::State<Db>) -> AppResult<auto::AutoView> {
    auto::view(&*db.conn()?)
}

#[tauri::command]
pub fn backup_auto_set(db: tauri::State<Db>, mode: String, dir: Option<String>, keep: u32, include_captures: bool) -> AppResult<auto::AutoView> {
    if !auto::MODES.contains(&mode.as_str()) {
        return Err(AppError::Validation("Opção de backup automático inválida.".into()));
    }
    if ![0, 3, 5, 10, 20].contains(&keep) {
        return Err(AppError::Validation("Opção de retenção inválida.".into()));
    }
    let conn = db.conn()?;
    let dir = dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    if let Some(d) = &dir {
        if !Path::new(d).is_dir() {
            return Err(AppError::Validation("Pasta de destino indisponível.".into()));
        }
        auto::put(&conn, auto::K_DIR, Some(d))?;
    }
    if mode != "never" && auto::get(&conn, auto::K_DIR)?.is_none() {
        return Err(AppError::Validation("Escolha a pasta dos backups automáticos.".into()));
    }
    auto::put(&conn, auto::K_MODE, Some(&mode))?;
    auto::put(&conn, auto::K_KEEP, Some(&keep.to_string()))?;
    auto::put(&conn, auto::K_CAPTURES, Some(if include_captures { "1" } else { "0" }))?;
    auto::view(&conn)
}

#[tauri::command]
pub async fn backup_auto_password(app: AppHandle, password: Option<String>) -> AppResult<auto::AutoView> {
    blocking(move || {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        auto::set_password(&conn, password.as_deref())?;
        auto::view(&conn)
    })
    .await
}

#[tauri::command]
pub async fn backup_auto_run(app: AppHandle) -> AppResult<CreateReport> {
    blocking(move || auto::run(&app, "now")).await
}

/// Abre a pasta do backup no Explorador de Arquivos.
#[tauri::command]
pub fn backup_reveal(path: String) -> AppResult<()> {
    let p = PathBuf::from(path.trim());
    if !p.exists() {
        return Err(AppError::NotFound("Pasta não encontrada.".into()));
    }
    #[cfg(windows)]
    std::process::Command::new("explorer")
        .arg(&p)
        .spawn()
        .map_err(|e| AppError::Other(format!("Não foi possível abrir o Explorador de Arquivos: {e}")))?;
    Ok(())
}
