//! Backup automático: "nunca", "antes de encerrar" (sair do app pelo menu, fechar a janela, reiniciar
//! ou desligar o Windows pelo NEXSTAGE) ou "uma vez por semana".
//!
//! - Cada backup ganha uma pasta própria com data e hora (`NEXSTAGE_Backup_2026-11-09_19-30`): nunca
//!   sobrescreve outro.
//! - Retenção opcional ("manter os N mais recentes"): só apaga quando o usuário escolheu esse limite, e só
//!   pastas de backups AUTOMÁTICOS do NEXSTAGE (nome no padrão + manifesto `kind = auto`).
//! - Senha opcional, guardada pela DPAPI neste computador (os backups automáticos não podem perguntar).
//! - Desligar o Windows por fora do NEXSTAGE não espera o backup (o sistema não dá tempo garantido).

use super::{create, Busy, CreateOpts, CreateReport, Ctx, Kind, Op, FOLDER};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

pub const K_MODE: &str = "backup_auto_mode";
pub const K_DIR: &str = "backup_auto_dir";
pub const K_LAST: &str = "backup_auto_last";
pub const K_LAST_PATH: &str = "backup_auto_last_path";
pub const K_ERROR: &str = "backup_auto_error";
pub const K_FAIL_AT: &str = "backup_auto_fail_at";
pub const K_KEEP: &str = "backup_auto_keep";
pub const K_SECRET: &str = "backup_auto_secret";
pub const K_CAPTURES: &str = "backup_auto_captures";
const SECRET_ENTROPY: &[u8] = b"NEXSTAGE/BackupAuto/v1";
pub const MODES: [&str; 3] = ["never", "exit", "weekly"];

/// Já fez o backup de encerramento nesta sessão (evita repetir se o fechamento for pedido de novo).
static EXIT_DONE: AtomicBool = AtomicBool::new(false);

pub fn get(conn: &Connection, k: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT value FROM app_settings WHERE key=?1", [k], |r| r.get(0)).optional()?)
}
pub fn put(conn: &Connection, k: &str, v: Option<&str>) -> AppResult<()> {
    match v {
        Some(v) => conn.execute("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![k, v])?,
        None => conn.execute("DELETE FROM app_settings WHERE key=?1", [k])?,
    };
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoView {
    pub mode: String,
    pub dir: Option<String>,
    pub last: Option<String>,
    pub last_path: Option<String>,
    pub last_error: Option<String>,
    pub keep: u32,
    pub has_password: bool,
    pub include_captures: bool,
}

fn password(conn: &Connection) -> AppResult<Option<String>> {
    Ok(get(conn, K_SECRET)?.and_then(|b| STANDARD.decode(b).ok()).and_then(|b| crate::vault::unprotect(SECRET_ENTROPY, &b)))
}

pub fn mode(conn: &Connection) -> String {
    get(conn, K_MODE).ok().flatten().filter(|m| MODES.contains(&m.as_str())).unwrap_or_else(|| "never".into())
}

pub fn view(conn: &Connection) -> AppResult<AutoView> {
    Ok(AutoView {
        mode: mode(conn),
        dir: get(conn, K_DIR)?,
        last: get(conn, K_LAST)?,
        last_path: get(conn, K_LAST_PATH)?,
        last_error: get(conn, K_ERROR)?,
        keep: get(conn, K_KEEP)?.and_then(|v| v.parse().ok()).unwrap_or(0),
        has_password: password(conn)?.is_some(),
        include_captures: get(conn, K_CAPTURES)?.map(|v| v == "1").unwrap_or(false),
    })
}

pub fn set_password(conn: &Connection, p: Option<&str>) -> AppResult<()> {
    match p.filter(|p| !p.is_empty()) {
        Some(p) => {
            super::crypto::validate_password(p)?;
            let blob = crate::vault::protect(SECRET_ENTROPY, p).ok_or_else(|| AppError::Other("Não foi possível proteger a senha neste computador.".into()))?;
            put(conn, K_SECRET, Some(&STANDARD.encode(blob)))
        }
        None => put(conn, K_SECRET, None),
    }
}

/// Nome de pasta com data e hora, sem colidir com um backup existente.
fn next_name(conn: &Connection, dir: &Path) -> String {
    let stamp = super::local_now(conn, "%Y-%m-%d_%H-%M");
    let base = format!("{FOLDER}_{stamp}");
    let mut name = base.clone();
    let mut n = 2;
    while dir.join(&name).exists() {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

/// `NEXSTAGE_Backup_AAAA-MM-DD_HH-MM` (com sufixo `_N` opcional).
fn is_auto_name(n: &str) -> bool {
    let Some(rest) = n.strip_prefix(&format!("{FOLDER}_")) else { return false };
    let (stamp, suffix) = match rest.get(16..) {
        Some(s) => (&rest[..16], s),
        None => return false,
    };
    let pattern_ok = stamp.chars().enumerate().all(|(i, c)| match i {
        4 | 7 | 13 => c == '-',
        10 => c == '_',
        _ => c.is_ascii_digit(),
    });
    pattern_ok && (suffix.is_empty() || suffix.strip_prefix('_').map(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())).unwrap_or(false))
}

/// Retenção escolhida pelo usuário: mantém os `keep` backups automáticos mais recentes.
fn prune(dir: &Path, keep: u32) -> usize {
    if keep == 0 {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut v: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter(|e| is_auto_name(&e.file_name().to_string_lossy()))
        .filter_map(|e| {
            let m = super::read_manifest(&e.path()).ok()?;
            (m.kind == "auto").then(|| (m.created_at, e.path()))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = 0;
    for (_, p) in v.into_iter().skip(keep as usize) {
        if std::fs::remove_dir_all(&p).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Executa um backup automático agora (`trigger`: "exit" | "weekly" | "now").
pub fn run(app: &AppHandle, trigger: &str) -> AppResult<CreateReport> {
    let _busy = Busy::take()?;
    let (dir, pass, captures, keep) = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        let v = view(&conn)?;
        let dir = v.dir.ok_or_else(|| AppError::Validation("Escolha a pasta dos backups automáticos nas configurações.".into()))?;
        (PathBuf::from(dir), password(&conn)?, v.include_captures, v.keep)
    };
    if !dir.is_dir() {
        let msg = format!("A pasta dos backups automáticos não está disponível ({}). Verifique se a unidade está conectada.", dir.display());
        record(app, Err(&msg), None);
        return Err(AppError::Validation(msg));
    }
    let name = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        next_name(&conn, &dir)
    };
    let mut op = Op::new(app, "create", trigger);
    let opts = CreateOpts { parent: dir.clone(), name, password: pass, include_captures: captures, kind: Kind::Auto, replace: false };
    let res = create(&Ctx::new(app)?, &opts, &mut op);
    op.phase("end", 0); // a interface fecha o aviso de progresso (backup de encerramento)
    match &res {
        Ok(r) => {
            record(app, Ok(()), Some(&r.path));
            prune(&dir, keep);
        }
        Err(e) if super::is_canceled(e) => {}
        Err(e) => record(app, Err(&e.to_string()), None),
    }
    res
}

fn record(app: &AppHandle, res: Result<(), &str>, path: Option<&str>) {
    let db = app.state::<Db>();
    let Ok(conn) = db.conn() else { return };
    match res {
        Ok(()) => {
            let now = super::local_now(&conn, "%Y-%m-%d %H:%M:%S");
            let _ = put(&conn, K_LAST, Some(&now));
            let _ = put(&conn, K_LAST_PATH, path);
            let _ = put(&conn, K_ERROR, None);
            let _ = put(&conn, K_FAIL_AT, None);
        }
        Err(e) => {
            let now = super::local_now(&conn, "%Y-%m-%d %H:%M:%S");
            let _ = put(&conn, K_ERROR, Some(e));
            let _ = put(&conn, K_FAIL_AT, Some(&now));
        }
    }
}

/// Backup semanal pendente? (7 dias desde o último; depois de uma falha, tenta de novo em 6 horas).
fn weekly_due(conn: &Connection) -> bool {
    if mode(conn) != "weekly" || get(conn, K_DIR).ok().flatten().is_none() {
        return false;
    }
    let q = |k: &str, days: f64| -> bool {
        match get(conn, k).ok().flatten() {
            None => true,
            Some(v) => conn
                .query_row("SELECT julianday('now','localtime') - julianday(?1) >= ?2", params![v, days], |r| r.get::<_, bool>(0))
                .unwrap_or(true),
        }
    };
    q(K_LAST, 7.0) && q(K_FAIL_AT, 0.25)
}

/// Verificação periódica do backup semanal (primeira 2 minutos após abrir, depois a cada 30 minutos).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            let due = {
                let db = app.state::<Db>();
                db.conn().map(|c| weekly_due(&c)).unwrap_or(false)
            };
            if due && !super::is_busy() {
                match run(&app, "weekly") {
                    Ok(r) => { let _ = app.emit("backup-auto-done", serde_json::json!({ "path": r.path, "trigger": "weekly" })); }
                    Err(e) if super::is_canceled(&e) => {}
                    Err(e) => { let _ = app.emit("backup-auto-failed", serde_json::json!({ "error": e.to_string(), "trigger": "weekly" })); }
                }
            }
            std::thread::sleep(Duration::from_secs(30 * 60));
        }
    });
}

fn exit_enabled(app: &AppHandle) -> bool {
    let db = app.state::<Db>();
    db.conn().map(|c| mode(&c) == "exit" && get(&c, K_DIR).ok().flatten().is_some()).unwrap_or(false)
}

/// O usuário escolheu encerrar sem o backup: não tenta de novo ao fechar a janela.
pub fn skip_exit_backup() {
    EXIT_DONE.store(true, Ordering::SeqCst);
}

/// Antes de sair/reiniciar/desligar pelo NEXSTAGE: faz o backup e espera terminar.
/// Cancelado pelo usuário ou com falha = erro (a interface pergunta se continua mesmo assim).
pub fn before_exit(app: &AppHandle) -> AppResult<()> {
    if EXIT_DONE.load(Ordering::SeqCst) || !exit_enabled(app) {
        return Ok(());
    }
    run(app, "exit")?;
    EXIT_DONE.store(true, Ordering::SeqCst);
    Ok(())
}

/// Fechar a janela principal (X, Alt+F4). `true` = o fechamento foi adiado para fazer o backup;
/// o app encerra sozinho ao terminar (ou ao cancelar).
pub fn on_close_requested(app: &AppHandle) -> bool {
    if EXIT_DONE.load(Ordering::SeqCst) {
        return false;
    }
    if super::is_busy() {
        // backup/restauração em andamento (inclusive o próprio backup de encerramento): espera terminar
        let _ = app.emit("backup-close-blocked", ());
        return true;
    }
    if !exit_enabled(app) {
        return false;
    }
    let a = app.clone();
    std::thread::spawn(move || {
        match run(&a, "exit") {
            Ok(_) => {}
            Err(e) if super::is_canceled(&e) => {}
            Err(e) => {
                let _ = a.emit("backup-auto-failed", serde_json::json!({ "error": e.to_string(), "trigger": "exit" }));
                std::thread::sleep(Duration::from_secs(4)); // tempo para ler o aviso
            }
        }
        EXIT_DONE.store(true, Ordering::SeqCst);
        a.exit(0);
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_names() {
        assert!(is_auto_name("NEXSTAGE_Backup_2026-11-09_19-30"));
        assert!(is_auto_name("NEXSTAGE_Backup_2026-11-09_19-30_2"));
        assert!(!is_auto_name("NEXSTAGE_Backup"));
        assert!(!is_auto_name("NEXSTAGE_Backup_2026-11-09_19-30_"));
        assert!(!is_auto_name("NEXSTAGE_Backup_2026-11-09_19-30x"));
        assert!(!is_auto_name("Minhas fotos"));
    }
}
