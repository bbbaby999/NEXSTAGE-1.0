//! Monitor preferido da janela principal (`app_settings.preferred_monitor` + `preferred_monitor_name`).
//!
//! - Identificação: no Windows, o caminho do dispositivo do monitor (`monitorDevicePath`, vem do EDID e
//!   distingue até dois monitores do mesmo modelo) e o nome amigável ("LG ULTRAGEAR"), via
//!   QueryDisplayConfig. O "\\.\DISPLAYn" do Windows muda quando monitores são ligados/desligados, por isso
//!   só serve para achar o monitor correspondente do Tauri naquele momento.
//! - Abertura: com o preferido conectado, a janela abre nele; sem ele (só um monitor ligado), abre onde o
//!   Windows/geometria salva mandar (`window_bounds::fit` descarta posições em monitores desligados).
//! - Religado: um vigia verifica os monitores a cada 2 s; quando o preferido volta a aparecer, a janela é
//!   levada para ele (adiado enquanto há jogo em execução, overlay aberto ou janela minimizada). Mover a
//!   janela manualmente depois disso não é desfeito — só a reconexão dispara a mudança.

use crate::db::Db;
use crate::error::AppResult;
use rusqlite::params;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

const KEY: &str = "preferred_monitor";
const KEY_NAME: &str = "preferred_monitor_name";

/// Monitor conectado: `id` estável (caminho do dispositivo do monitor).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    /// A janela do NEXSTAGE está neste monitor agora.
    pub current: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorState {
    pub monitors: Vec<MonitorInfo>,
    /// Preferido salvo (pode estar desconectado): id e nome guardados ao escolher.
    pub preferred: Option<String>,
    pub preferred_name: Option<String>,
}

/* ---------- Identificação (Windows) ---------- */

/// (gdi, id, nome amigável) de cada monitor ativo.
#[cfg(windows)]
fn display_ids() -> Vec<(String, String, String)> {
    use windows::Win32::Devices::Display::*;
    let wide = |s: &[u16]| String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]);
    let mut out = Vec::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).0 != 0 {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).0 != 0 {
            return out;
        }
        paths.truncate(np as usize);
        for p in &paths {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                continue;
            }
            let gdi = wide(&src.viewGdiDeviceName);
            let path = wide(&tgt.monitorDevicePath);
            let id = if path.is_empty() { gdi.clone() } else { path.to_lowercase() };
            // espelhamento (dois alvos na mesma origem): fica o primeiro
            if !out.iter().any(|(g, _, _): &(String, String, String)| *g == gdi) {
                out.push((gdi, id, wide(&tgt.monitorFriendlyDeviceName)));
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn display_ids() -> Vec<(String, String, String)> { Vec::new() }

fn rect(m: &Monitor) -> (i32, i32, u32, u32) { (m.position().x, m.position().y, m.size().width, m.size().height) }

/// Monitores conectados (ordem do Tauri), com o id estável quando o Windows o informa.
pub fn list(w: &WebviewWindow) -> Vec<(MonitorInfo, Monitor)> {
    let ids = display_ids();
    let cur = w.current_monitor().ok().flatten().and_then(|m| m.name().cloned());
    w.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, m)| {
            let gdi = m.name().cloned().unwrap_or_default();
            let hit = ids.iter().find(|(g, _, _)| g.eq_ignore_ascii_case(&gdi));
            let (x, y, width, height) = rect(&m);
            let friendly = hit.map(|h| h.2.trim().to_string()).filter(|s| !s.is_empty());
            let info = MonitorInfo {
                id: hit.map(|h| h.1.clone()).unwrap_or_else(|| gdi.clone()),
                name: friendly.unwrap_or_else(|| format!("Monitor {}", i + 1)),
                current: cur.as_deref() == Some(gdi.as_str()),
                width,
                height,
                primary: x == 0 && y == 0,
            };
            (info, m)
        })
        .collect()
}

/* ---------- Preferência ---------- */

fn get(app: &AppHandle, key: &str) -> Option<String> {
    let db = app.state::<Db>();
    let conn = db.conn().ok()?;
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get::<_, String>(0))
        .ok()
        .filter(|v| !v.is_empty())
}

fn put(conn: &rusqlite::Connection, key: &str, v: Option<&str>) -> rusqlite::Result<usize> {
    match v {
        Some(v) => conn.execute(
            "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, v],
        ),
        None => conn.execute("DELETE FROM app_settings WHERE key=?1", [key]),
    }
}

pub fn preferred(app: &AppHandle) -> Option<String> { get(app, KEY) }

/* ---------- Mover a janela ---------- */

/// Leva a janela para o monitor, centralizada (tamanho limitado ao do monitor), preservando tela
/// cheia/maximizada. Minimizada: não mexe (devolve false).
fn move_to(w: &WebviewWindow, m: &Monitor) -> bool {
    if w.is_minimized().unwrap_or(false) {
        return false;
    }
    let full = w.is_fullscreen().unwrap_or(false);
    let maxi = w.is_maximized().unwrap_or(false);
    if full { let _ = w.set_fullscreen(false); }
    if maxi { let _ = w.unmaximize(); }
    let (mx, my, mw, mh) = rect(m);
    let size = w.outer_size().unwrap_or(PhysicalSize::new(mw * 3 / 4, mh * 3 / 4));
    let (ww, wh) = (size.width.min(mw), size.height.min(mh));
    if (ww, wh) != (size.width, size.height) {
        let _ = w.set_size(PhysicalSize::new(ww, wh));
    }
    let _ = w.set_position(PhysicalPosition::new(mx + (mw - ww) as i32 / 2, my + (mh - wh) as i32 / 2));
    if maxi { let _ = w.maximize(); }
    if full { let _ = w.set_fullscreen(true); }
    true
}

/// Leva a janela para o preferido, se ele estiver conectado e a janela não estiver nele.
/// Devolve true se a janela já está (ou foi colocada) no preferido.
fn ensure(app: &AppHandle, w: &WebviewWindow) -> bool {
    let Some(pref) = preferred(app) else { return true };
    let all = list(w);
    let Some((info, m)) = all.iter().find(|(i, _)| i.id == pref) else { return false };
    info.current || move_to(w, m)
}

/// Na abertura (janela ainda oculta, antes da tela cheia): posiciona no preferido, se conectado.
pub fn place_on_start(app: &AppHandle, w: &WebviewWindow) {
    if preferred(app).is_some() {
        ensure(app, w);
    }
}

/* ---------- Vigia: preferido religado ---------- */

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut prev: Vec<String> = display_ids().into_iter().map(|d| d.1).collect();
        prev.sort();
        let mut pending = false;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let mut now: Vec<String> = display_ids().into_iter().map(|d| d.1).collect();
            now.sort();
            let pref = preferred(&app);
            if now != prev {
                if let Some(p) = &pref {
                    if now.contains(p) && !prev.contains(p) {
                        pending = true;
                    }
                }
                let _ = app.emit("monitors-changed", ());
                prev = now.clone();
            }
            match &pref {
                Some(p) if now.contains(p) => {}
                _ => { pending = false; continue; }
            }
            if !pending {
                continue;
            }
            // Jogo em execução ou overlay aberto: espera (não tira a janela de cima do jogo).
            let rt = app.state::<crate::overlay::Runtime>();
            if rt.active.load(Ordering::SeqCst) || rt.is_running() {
                continue;
            }
            let Some(w) = app.get_webview_window("main") else { continue };
            // Dá um instante para o Windows reposicionar as telas antes de mover.
            std::thread::sleep(Duration::from_millis(800));
            if ensure(&app, &w) {
                pending = false;
                let _ = app.emit("monitors-changed", ());
            }
        }
    });
}

/* ---------- Comandos ---------- */

fn state(app: &AppHandle, w: &WebviewWindow) -> MonitorState {
    MonitorState {
        monitors: list(w).into_iter().map(|(i, _)| i).collect(),
        preferred: preferred(app),
        preferred_name: get(app, KEY_NAME),
    }
}

#[tauri::command]
pub fn get_monitors(app: AppHandle, window: WebviewWindow) -> MonitorState {
    state(&app, &window)
}

/// `id` = None: sem preferência (abre onde o Windows mandar). Com id: salva e já leva a janela para ele.
#[tauri::command]
pub fn set_preferred_monitor(app: AppHandle, window: WebviewWindow, db: tauri::State<Db>, id: Option<String>) -> AppResult<MonitorState> {
    let all = list(&window);
    let hit = id.as_ref().and_then(|id| all.iter().find(|(i, _)| &i.id == id));
    {
        let conn = db.conn()?;
        match (&id, hit) {
            (Some(id), Some((info, _))) => {
                put(&conn, KEY, Some(id))?;
                put(&conn, KEY_NAME, Some(&info.name))?;
            }
            (Some(_), None) => return Err(crate::error::AppError::Validation("Monitor não encontrado. Ele foi desconectado?".into())),
            (None, _) => {
                put(&conn, KEY, None)?;
                put(&conn, KEY_NAME, None)?;
            }
        }
    }
    if let Some((info, m)) = hit {
        if !info.current {
            move_to(&window, m);
        }
    }
    Ok(state(&app, &window))
}
