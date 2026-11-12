//! Sincronização automática e integração com emuladores.
//!
//! Fonte oficial dos dados: SEMPRE a API do RetroAchievements. Os emuladores entram só como gatilho:
//! - PCSX2 e DuckStation (rcheevos) escrevem no próprio log "Achievement … unlocked" no instante do
//!   desbloqueio (PCSX2 grava `emulog.txt` por padrão; DuckStation só com "Log to file"). Quando essa
//!   linha aparece, o NEXSTAGE sincroniza alguns segundos depois e confirma pela API → aviso quase
//!   em tempo real, sem nunca inventar uma conquista a partir do log.
//! - RetroArch: idem, se "Log to file" estiver ligado (linha "awarding cheevo").
//! - PPSSPP e demais: sem fonte local confiável → enquanto o jogo roda, sincroniza a cada 2 min
//!   (o aviso pode atrasar). Fora de jogo: intervalo configurado (padrão 15 min).
//! - Checagem rápida (o gatilho principal): com QUALQUER emulador com RetroAchievements aberto —
//!   iniciado pelo NEXSTAGE ou não —, a cada 30 s uma única chamada leve pergunta à API os
//!   desbloqueios dos últimos minutos; se houver um que o cache não conhece, sincroniza na hora (e o
//!   cartão aparece). Necessária porque o PCSX2 2.x deixou de escrever "unlocked" no log.
//! Nada é chamado na inicialização do app: a primeira verificação espera 60 s.

use super::store;
use super::util::{now_epoch, parse_epoch};
use crate::db::Db;
use crate::overlay::Runtime;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const TICK: Duration = Duration::from_secs(3);
const STARTUP_GRACE: Duration = Duration::from_secs(60);
const SESSION_INTERVAL_MIN: i64 = 2;
/// Checagem rápida com emulador aberto (segundos) e janela de busca na API.
const QUICK_EVERY_SECS: i64 = 30;
const QUICK_WINDOW_SECS: i64 = 15 * 60;

/// Processos de emuladores com RetroAchievements (nome do executável, minúsculas, contém).
const RA_EMULATORS: [&str; 8] = ["pcsx2", "duckstation", "retroarch", "ppsspp", "dolphin", "emuhawk", "flycast", "melonds"];

pub fn is_ra_emulator(exe_name: &str) -> bool {
    let n = exe_name.to_lowercase();
    RA_EMULATORS.iter().any(|k| n.contains(k))
}

/// Algum emulador com RetroAchievements aberto agora (iniciado de qualquer jeito)?
#[cfg(windows)]
pub fn ra_emulator_running() -> bool {
    #[repr(C)]
    struct Entry { size: u32, usage: u32, pid: u32, heap: usize, module: u32, threads: u32, parent: u32, pri: i32, flags: u32, exe: [u16; 260] }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snap: isize, e: *mut Entry) -> i32;
        fn Process32NextW(snap: isize, e: *mut Entry) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    let snap = unsafe { CreateToolhelp32Snapshot(0x2, 0) }; // TH32CS_SNAPPROCESS
    if snap == -1 {
        return false;
    }
    let mut e = Entry { size: std::mem::size_of::<Entry>() as u32, usage: 0, pid: 0, heap: 0, module: 0, threads: 0, parent: 0, pri: 0, flags: 0, exe: [0; 260] };
    let mut found = false;
    let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
    while ok {
        let end = e.exe.iter().position(|c| *c == 0).unwrap_or(e.exe.len());
        if is_ra_emulator(&String::from_utf16_lossy(&e.exe[..end])) {
            found = true;
            break;
        }
        ok = unsafe { Process32NextW(snap, &mut e) } != 0;
    }
    unsafe { CloseHandle(snap) };
    found
}
#[cfg(not(windows))]
pub fn ra_emulator_running() -> bool {
    false
}

/// IDs devolvidos pela API (desbloqueios recentes) que o cache ainda não conhece.
pub fn unseen_ids(rows: &[serde_json::Value], known: impl Fn(i64) -> bool) -> Vec<i64> {
    rows.iter().filter_map(|r| r["AchievementID"].as_i64()).filter(|id| *id > 0 && !known(*id)).collect()
}
const DEFAULT_INTERVAL_MIN: i64 = 15;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EmuKind {
    Pcsx2,
    DuckStation,
    Ppsspp,
    RetroArch,
    NoSupport,
    Unknown,
}

pub fn classify(name: &str, exe: &str) -> EmuKind {
    let file = Path::new(exe).file_name().and_then(|f| f.to_str()).unwrap_or("").to_lowercase();
    let n = format!("{} {file}", name.to_lowercase());
    if n.contains("pcsx2") {
        EmuKind::Pcsx2
    } else if n.contains("duckstation") {
        EmuKind::DuckStation
    } else if n.contains("ppsspp") {
        EmuKind::Ppsspp
    } else if n.contains("retroarch") {
        EmuKind::RetroArch
    } else if ["rpcs3", "vita3k", "xenia", "xemu", "ryujinx", "yuzu", "suyu", "sudachi", "citron", "lichii", "shadps4", "cemu"].iter().any(|k| n.contains(k)) {
        EmuKind::NoSupport
    } else {
        EmuKind::Unknown
    }
}

fn documents() -> Vec<PathBuf> {
    let Ok(home) = std::env::var("USERPROFILE") else { return Vec::new() };
    let h = PathBuf::from(home);
    vec![h.join("Documents"), h.join("OneDrive").join("Documents"), h.join("OneDrive").join("Documentos")]
}

/// Pasta de dados do emulador (modo portátil ao lado do .exe ou em Documentos).
fn data_dirs(kind: EmuKind, exe: &str) -> Vec<PathBuf> {
    let dir = Path::new(exe).parent().map(Path::to_path_buf).unwrap_or_default();
    let docs = documents();
    match kind {
        EmuKind::Pcsx2 => {
            let mut v = vec![dir.clone()];
            v.extend(docs.iter().map(|d| d.join("PCSX2")));
            v
        }
        EmuKind::DuckStation => {
            let mut v = vec![dir.clone()];
            v.extend(docs.iter().map(|d| d.join("DuckStation")));
            if let Ok(l) = std::env::var("LOCALAPPDATA") {
                v.push(PathBuf::from(l).join("DuckStation"));
            }
            v
        }
        EmuKind::Ppsspp => {
            let mut v = vec![dir.join("memstick")];
            v.extend(docs.iter().map(|d| d.join("PPSSPP")));
            v
        }
        EmuKind::RetroArch => vec![dir.clone()],
        _ => Vec::new(),
    }
}

fn read_ini(path: &Path, section: &str, key: &str) -> Option<String> {
    let txt = std::fs::read_to_string(path).ok()?;
    let mut in_sec = false;
    for line in txt.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_sec = l.trim_matches(|c| c == '[' || c == ']').eq_ignore_ascii_case(section);
            continue;
        }
        if in_sec || section.is_empty() {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}
fn truthy(v: Option<String>) -> Option<bool> {
    v.map(|s| matches!(s.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
}

/// Situação do RetroAchievements em um emulador cadastrado (somente leitura dos arquivos dele).
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmuSupport {
    pub id: i64,
    pub name: String,
    pub platforms: Vec<String>,
    /// O emulador tem suporte oficial ao RetroAchievements.
    pub ra_capable: bool,
    /// Ligado nas configurações do emulador (None = não deu para verificar).
    pub ra_enabled: Option<bool>,
    /// Usuário configurado no emulador (para avisar se for diferente do conectado).
    pub ra_user: Option<String>,
    /// Arquivo de log usado como gatilho de desbloqueio.
    pub event_log: Option<String>,
    /// "realtime" (gatilho pelo log + confirmação pela API) | "polling" (só sincronização) | "none"
    pub mode: String,
    pub note: String,
}

struct Probe {
    enabled: Option<bool>,
    user: Option<String>,
    log: Option<PathBuf>,
    log_enabled: Option<bool>,
}

fn probe(kind: EmuKind, exe: &str) -> Probe {
    let mut p = Probe { enabled: None, user: None, log: None, log_enabled: None };
    for d in data_dirs(kind, exe) {
        match kind {
            EmuKind::Pcsx2 => {
                let ini = d.join("inis").join("PCSX2.ini");
                if ini.is_file() {
                    p.enabled = truthy(read_ini(&ini, "Achievements", "Enabled"));
                    p.user = read_ini(&ini, "Achievements", "Username").filter(|u| !u.is_empty());
                    p.log_enabled = truthy(read_ini(&ini, "Logging", "EnableFileLogging")).or(Some(true));
                    p.log = Some(d.join("logs").join("emulog.txt"));
                    break;
                }
            }
            EmuKind::DuckStation => {
                let ini = d.join("settings.ini");
                if ini.is_file() {
                    p.enabled = truthy(read_ini(&ini, "Cheevos", "Enabled"));
                    p.user = read_ini(&ini, "Cheevos", "Username").filter(|u| !u.is_empty());
                    p.log_enabled = truthy(read_ini(&ini, "Logging", "LogToFile")).or(Some(false));
                    p.log = Some(d.join("duckstation.log"));
                    break;
                }
            }
            EmuKind::Ppsspp => {
                let ini = d.join("PSP").join("SYSTEM").join("ppsspp.ini");
                if ini.is_file() {
                    p.enabled = truthy(read_ini(&ini, "Achievements", "AchievementsEnable"));
                    p.user = read_ini(&ini, "Achievements", "AchievementsUserName").filter(|u| !u.is_empty());
                    break;
                }
            }
            EmuKind::RetroArch => {
                let cfg = d.join("retroarch.cfg");
                if cfg.is_file() {
                    p.enabled = truthy(read_ini(&cfg, "", "cheevos_enable"));
                    p.user = read_ini(&cfg, "", "cheevos_username").filter(|u| !u.is_empty());
                    p.log_enabled = truthy(read_ini(&cfg, "", "log_to_file"));
                    p.log = Some(d.join("logs").join("retroarch.log"));
                    break;
                }
            }
            _ => {}
        }
    }
    p
}

pub fn emulator_support(db: &Db) -> Vec<EmuSupport> {
    let emus = db.conn().ok().and_then(|c| crate::commands::emulators::load_all(&c).ok()).unwrap_or_default();
    emus.into_iter()
        .map(|e| {
            let kind = classify(&e.name, &e.exe_path);
            let mut s = EmuSupport { id: e.id, name: e.name.clone(), platforms: e.platforms.clone(), ..Default::default() };
            match kind {
                EmuKind::NoSupport | EmuKind::Unknown => {
                    s.mode = "none".into();
                    s.note = if kind == EmuKind::NoSupport {
                        "Este emulador não tem suporte ao RetroAchievements.".into()
                    } else {
                        "Suporte ao RetroAchievements não identificado para este emulador. Se ele tiver, o progresso aparece pela sincronização da conta.".into()
                    };
                }
                _ => {
                    s.ra_capable = true;
                    let p = probe(kind, &e.exe_path);
                    s.ra_enabled = p.enabled;
                    s.ra_user = p.user;
                    let log_ok = p.log_enabled == Some(true) && kind != EmuKind::Ppsspp;
                    if log_ok {
                        s.event_log = p.log.as_ref().map(|l| l.to_string_lossy().into_owned());
                        s.mode = "realtime".into();
                        s.note = "Desbloqueios detectados pelo log do emulador e confirmados pela API em poucos segundos.".into();
                    } else {
                        s.mode = "polling".into();
                        s.note = match kind {
                            EmuKind::DuckStation => "Ative \"Log to File\" no DuckStation (Configurações → Avançado) para avisos quase em tempo real. Sem isso, as notificações dependem da sincronização (pode haver atraso).".into(),
                            EmuKind::RetroArch => "Ative \"Registrar em arquivo\" no RetroArch para avisos quase em tempo real. Sem isso, as notificações dependem da sincronização (pode haver atraso).".into(),
                            EmuKind::Pcsx2 => "Log em arquivo do PCSX2 desligado: as notificações dependem da sincronização (pode haver atraso).".into(),
                            _ => "O emulador não oferece uma fonte local confiável de eventos: as notificações dependem da sincronização com a conta (pode haver atraso de até 2 minutos durante o jogo).".into(),
                        };
                    }
                    if s.ra_enabled == Some(false) {
                        s.note = format!("RetroAchievements está desligado neste emulador: conquistas não serão registradas. {}", s.note);
                    }
                }
            }
            s
        })
        .collect()
}

/// Linha de log que indica desbloqueio (PCSX2/DuckStation: "Achievement X (id) for game N unlocked";
/// RetroArch: "awarding cheevo").
pub fn is_unlock_line(line: &str) -> bool {
    let l = line.to_lowercase();
    (l.contains("achievement") && l.contains("for game") && l.contains("unlocked")) || l.contains("awarding cheevo")
}

#[derive(Default)]
struct WatchState {
    /// posição já lida de cada log (só o que for escrito durante a sessão conta)
    offsets: HashMap<PathBuf, u64>,
}

/// Lê o que foi acrescentado ao log desde a última vez. Devolve true se houve desbloqueio.
fn scan_log(st: &mut WatchState, path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let off = match st.offsets.get(path) {
        None => {
            st.offsets.insert(path.to_path_buf(), len);
            return false; // primeira vez nesta sessão: ignora o conteúdo antigo
        }
        Some(&o) if len < o => 0, // o emulador recriou o log (novo jogo)
        Some(&o) => o,
    };
    if len == off {
        return false;
    }
    let start = off.max(len.saturating_sub(1024 * 1024));
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_err() || f.take(len - start).read_to_end(&mut buf).is_err() {
        return false;
    }
    st.offsets.insert(path.to_path_buf(), len);
    String::from_utf8_lossy(&buf).lines().any(is_unlock_line)
}

fn interval_min(app: &AppHandle) -> i64 {
    crate::overlay::setting(app, "ra_sync_minutes").and_then(|v| v.parse::<i64>().ok()).map(|v| v.clamp(0, 240)).unwrap_or(DEFAULT_INTERVAL_MIN)
}

/// Emuladores com RetroAchievements em uso agora (jogos iniciados pelo NEXSTAGE).
fn active_sessions(app: &AppHandle) -> Vec<(EmuKind, String)> {
    let ids: Vec<i64> = app.state::<Runtime>().running.lock().map(|m| m.keys().copied().collect()).unwrap_or_default();
    if ids.is_empty() {
        return Vec::new();
    }
    let db = app.state::<Db>();
    let Ok(conn) = db.conn() else { return Vec::new() };
    let mut out = Vec::new();
    for id in ids {
        let row: Option<(String, Option<String>, Option<String>, String)> = conn
            .query_row(
                "SELECT g.platform, e.name, e.exe_path, g.executable_path FROM games g LEFT JOIN emulators e ON e.id=g.emulator_id WHERE g.id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();
        if let Some((platform, name, emu_exe, exe)) = row {
            if super::hash::console_for_platform(&platform).is_none() {
                continue;
            }
            let exe = emu_exe.unwrap_or(exe);
            let kind = classify(name.as_deref().unwrap_or(""), &exe);
            if !matches!(kind, EmuKind::NoSupport) {
                out.push((kind, exe));
            }
        }
    }
    out
}

/// Laço em segundo plano: sincronização automática + gatilho pelos logs dos emuladores.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("ra-scheduler".into())
        .spawn(move || {
            let started = Instant::now();
            let mut watch = WatchState::default();
            let mut profile = 0i64;
            let mut last_attempt = 0i64; // epoch
            let mut failures = 0u32;
            let mut trigger_at: Option<Instant> = None;
            let mut retry_after_trigger = false;
            let mut last_quick = 0i64; // epoch da última checagem rápida
            loop {
                std::thread::sleep(TICK);
                if started.elapsed() < STARTUP_GRACE {
                    continue;
                }
                let active = app.state::<Runtime>().active_profile.load(Ordering::SeqCst);
                if active != profile {
                    profile = active;
                    failures = 0;
                    trigger_at = None;
                    // continua de onde a última sincronização deste perfil parou
                    last_attempt = app
                        .state::<Db>()
                        .conn()
                        .ok()
                        .and_then(|c| store::account(&c, profile).ok().flatten())
                        .and_then(|a| a.synced_at)
                        .and_then(|s| parse_epoch(&s))
                        .unwrap_or(0);
                }
                if profile <= 0 {
                    continue;
                }
                // gatilho pelos logs (só durante jogos com emulador compatível)
                let sessions = active_sessions(&app);
                if sessions.is_empty() {
                    watch.offsets.clear();
                } else {
                    for (kind, exe) in &sessions {
                        let p = probe(*kind, exe);
                        if p.log_enabled == Some(true) {
                            if let Some(log) = p.log {
                                if scan_log(&mut watch, &log) {
                                    // o emulador envia o desbloqueio ao servidor logo depois: espera um pouco
                                    trigger_at = Some(Instant::now() + Duration::from_secs(8));
                                    retry_after_trigger = true;
                                }
                            }
                        }
                    }
                }
                let interval = interval_min(&app);
                let now = now_epoch();
                // emulador aberto (pelo NEXSTAGE ou não): checagem rápida na API a cada 30 s
                let emu_open = !sessions.is_empty() || ra_emulator_running();
                if emu_open && trigger_at.is_none() && now - last_quick >= QUICK_EVERY_SECS
                    && super::client::blocked_for().is_none() && !super::sync::is_running()
                {
                    last_quick = now;
                    let creds = app.state::<Db>().conn().ok().and_then(|c| store::creds(&c, profile).ok().flatten());
                    if let Some(creds) = creds {
                        if let Ok(rows) = tauri::async_runtime::block_on(super::client::earned_between(&creds, now - QUICK_WINDOW_SECS, now)) {
                            let fresh = app.state::<Db>().conn().ok().map(|c| unseen_ids(&rows, |id| store::is_known_unlock(&c, profile, id))).unwrap_or_default();
                            if !fresh.is_empty() {
                                trigger_at = Some(Instant::now()); // sincroniza já: registra e mostra o cartão
                                retry_after_trigger = false;
                            }
                        }
                    }
                }
                let triggered = trigger_at.map(|t| Instant::now() >= t).unwrap_or(false);
                let base = if emu_open && interval > 0 { SESSION_INTERVAL_MIN } else { interval };
                // falhas seguidas (sem internet, API fora): espera 5, 10, 20, 30 min
                let backoff = match failures { 0 => 0, 1 => 5, 2 => 10, 3 => 20, _ => 30 } * 60;
                let periodic = base > 0 && now - last_attempt >= (base * 60).max(backoff);
                if !(triggered || periodic) || super::client::blocked_for().is_some() || super::sync::is_running() {
                    continue;
                }
                let has_account = app.state::<Db>().conn().ok().and_then(|c| store::account(&c, profile).ok().flatten()).map(|a| a.secret.is_some()).unwrap_or(false);
                if !has_account {
                    last_attempt = now;
                    continue;
                }
                last_attempt = now;
                let was_trigger = triggered;
                trigger_at = None;
                match tauri::async_runtime::block_on(super::sync::sync_profile(&app, profile)) {
                    Ok(rep) => {
                        failures = 0;
                        // o servidor ainda não tinha o desbloqueio: tenta uma vez mais em 30 s
                        if was_trigger && rep.new_unlocks == 0 && retry_after_trigger {
                            retry_after_trigger = false;
                            trigger_at = Some(Instant::now() + Duration::from_secs(30));
                        }
                    }
                    Err(_) => failures = failures.saturating_add(1),
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_emulators() {
        assert_eq!(classify("PCSX2", r"C:\PCSX2\pcsx2-qt.exe"), EmuKind::Pcsx2);
        assert_eq!(classify("Meu PS1", r"C:\ds\duckstation-qt-x64-ReleaseLTCG.exe"), EmuKind::DuckStation);
        assert_eq!(classify("PPSSPP", r"C:\PPSSPP\PPSSPPWindows64.exe"), EmuKind::Ppsspp);
        assert_eq!(classify("RPCS3", r"C:\RPCS3\rpcs3.exe"), EmuKind::NoSupport);
        assert_eq!(classify("Outro", r"C:\x\emu.exe"), EmuKind::Unknown);
    }

    #[test]
    fn recognizes_ra_emulator_processes_and_unseen_unlocks() {
        for n in ["pcsx2-qt.exe", "PCSX2.exe", "duckstation-qt-x64-ReleaseLTCG.exe", "retroarch.exe", "PPSSPPWindows64.exe", "Dolphin.exe", "EmuHawk.exe"] {
            assert!(is_ra_emulator(n), "{n}");
        }
        for n in ["rpcs3.exe", "nexstage.exe", "steam.exe", "chrome.exe"] {
            assert!(!is_ra_emulator(n), "{n}");
        }
        let rows = vec![serde_json::json!({"AchievementID": 345410}), serde_json::json!({"AchievementID": 7}), serde_json::json!({"Title": "sem id"})];
        assert_eq!(unseen_ids(&rows, |id| id == 7), vec![345410], "só o que o cache não conhece");
        assert!(unseen_ids(&rows, |_| true).is_empty());
    }

    #[test]
    fn detects_unlock_lines() {
        assert!(is_unlock_line("[  812.3301] Achievement Gotta Go Fast (12345) for game 2689 unlocked"));
        assert!(is_unlock_line("[INFO] [RCHEEVOS]: awarding cheevo 1234: Title (desc)"));
        assert!(!is_unlock_line("[    0.1268] PCSX2 v2.4.0"));
        assert!(!is_unlock_line("Achievements: 12 of 40 unlocked"));
    }

    #[test]
    fn log_scan_reads_only_new_content() {
        let p = std::env::temp_dir().join(format!("nexstage-ra-log-{}.txt", std::process::id()));
        std::fs::write(&p, "Achievement Antiga (1) for game 2 unlocked\n").unwrap();
        let mut st = WatchState::default();
        assert!(!scan_log(&mut st, &p), "conteúdo anterior à sessão é ignorado");
        std::fs::write(&p, "Achievement Antiga (1) for game 2 unlocked\n[1.0] carregando\n").unwrap();
        assert!(!scan_log(&mut st, &p));
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        use std::io::Write;
        writeln!(f, "[2.0] Achievement Nova (3) for game 2 unlocked").unwrap();
        assert!(scan_log(&mut st, &p));
        assert!(!scan_log(&mut st, &p), "a mesma linha não dispara duas vezes");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn reads_ini_values() {
        let p = std::env::temp_dir().join(format!("nexstage-ra-ini-{}.ini", std::process::id()));
        std::fs::write(&p, "[Logging]\nEnableFileLogging = true\n[Achievements]\nEnabled = false\nUsername = Jogador\n").unwrap();
        assert_eq!(truthy(read_ini(&p, "Achievements", "Enabled")), Some(false));
        assert_eq!(read_ini(&p, "Achievements", "Username").as_deref(), Some("Jogador"));
        assert_eq!(truthy(read_ini(&p, "Logging", "EnableFileLogging")), Some(true));
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(all(test, windows))]
mod live {
    /// Diagnóstico (só leitura, cópia do banco): `cargo test --lib retro::watch::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ra_account_and_recent_unlocks() {
        let src = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("app.nexstage.launcher").join("nexstage.db");
        let tmp = std::env::temp_dir().join(format!("nexstage-ra-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        rusqlite::Connection::open_with_flags(&src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap().execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        let c = rusqlite::Connection::open(&tmp).unwrap();
        let acc = super::store::account(&c, 1).unwrap();
        println!("conta RA no app: {:?}", acc.as_ref().map(|a| (&a.username, a.secret.is_some(), &a.synced_at, &a.sync_error)));
        for k in ["ra_notify", "ra_sync_minutes", "ach_toast"] {
            let v: Option<String> = c.query_row("SELECT value FROM app_settings WHERE key=?1", [k], |r| r.get(0)).ok();
            println!("config {k} = {v:?}");
        }
        let creds = super::store::creds(&c, 1).unwrap().expect("sem credenciais legíveis");
        let now = super::now_epoch();
        let earned = tauri::async_runtime::block_on(super::super::client::earned_between(&creds, now - 6 * 3600, now)).unwrap();
        println!("desbloqueios na conta (últimas 6 h, API oficial): {}", earned.len());
        for e in &earned {
            let id = e["AchievementID"].as_i64().unwrap_or(0);
            let in_cache: Option<(Option<String>, Option<String>)> = c.query_row(
                "SELECT unlocked_at, unlocked_hc_at FROM ra_user_achievements WHERE profile_id=1 AND ra_achievement_id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok();
            println!("  {} | {} | {} | {} pts | em {} | no app: {:?}", id, e["Title"], e["GameTitle"], e["Points"], e["Date"], in_cache);
        }
        // checagem rápida contra a API real: o desbloqueio mais recente, "esquecido" na cópia, deve ser detectado
        let month = tauri::async_runtime::block_on(super::super::client::earned_between(&creds, now - 60 * 86400, now)).unwrap();
        println!("desbloqueios nos últimos 60 dias: {}", month.len());
        if let Some(last) = month.iter().max_by_key(|e| e["Date"].as_str().unwrap_or("").to_string()) {
            let id = last["AchievementID"].as_i64().unwrap();
            println!("mais recente: {} | {} | {} | {}", id, last["Title"], last["GameTitle"], last["Date"]);
            assert!(super::store::is_known_unlock(&c, 1, id), "o app já deveria conhecer esse desbloqueio");
            assert!(super::unseen_ids(&month, |x| super::store::is_known_unlock(&c, 1, x)).is_empty(), "nada novo com o cache completo");
            c.execute("DELETE FROM ra_unlocks WHERE profile_id=1 AND ra_achievement_id=?1", [id]).unwrap();
            let unseen = super::unseen_ids(&month, |x| super::store::is_known_unlock(&c, 1, x));
            println!("depois de esquecer {id} na cópia → checagem rápida acha: {unseen:?}");
            assert_eq!(unseen, vec![id]);
        }
        println!("emulador com RetroAchievements aberto agora: {}", super::ra_emulator_running());
        drop(c);
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(all(test, windows))]
mod live_process {
    /// `cargo test --lib retro::watch::live_process -- --ignored --nocapture` (com um emulador aberto)
    #[test]
    #[ignore]
    fn print_emulator_detection() {
        println!("emulador com RetroAchievements aberto: {}", super::ra_emulator_running());
    }
}
