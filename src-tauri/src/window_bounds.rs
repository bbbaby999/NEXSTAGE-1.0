//! Tamanho e posição da janela principal (modo janela), salvos em `app_settings.window_bounds`.
//!
//! Substitui o tauri-plugin-window-state, que gravava a geometria MESMO em tela cheia: ao fechar o
//! app em tela cheia, ele salvava "janela do tamanho do monitor em 0,0" e a restaurava na abertura
//! seguinte — a janela parecia tela cheia sem estar, e era preciso alternar o modo para corrigir.
//! Aqui só se grava em modo janela (nem tela cheia, nem minimizada, nem maximizada) e a restauração
//! valida o retângulo contra os monitores conectados.

use crate::db::Db;
use rusqlite::params;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

const KEY: &str = "window_bounds";
const MIN_W: u32 = 1024;
const MIN_H: u32 = 600;
static LAST_SAVE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }

pub fn parse(s: &str) -> Option<Bounds> {
    let v: Vec<i64> = s.split(',').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    match v[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some(Bounds { x: x as i32, y: y as i32, w: w as u32, h: h as u32 }),
        _ => None,
    }
}

/// Retângulo utilizável: tamanho mínimo, não maior que o monitor e com boa parte visível nele.
/// `monitors`: (x, y, largura, altura) de cada monitor.
pub fn fit(b: Bounds, monitors: &[(i32, i32, u32, u32)]) -> Option<Bounds> {
    if b.w < MIN_W || b.h < MIN_H {
        return None;
    }
    monitors.iter().find_map(|&(mx, my, mw, mh)| {
        let ix = (b.x + b.w as i32).min(mx + mw as i32) - b.x.max(mx);
        let iy = (b.y + b.h as i32).min(my + mh as i32) - b.y.max(my);
        // pelo menos 1/3 da janela dentro do monitor; e não pode ser do tamanho dele (era tela cheia)
        let visible = ix > 0 && iy > 0 && (ix as i64 * iy as i64) * 3 >= b.w as i64 * b.h as i64;
        (visible && (b.w < mw || b.h < mh)).then(|| Bounds { w: b.w.min(mw), h: b.h.min(mh), ..b })
    })
}

fn read(app: &AppHandle) -> Option<Bounds> {
    let db = app.state::<Db>();
    let conn = db.conn().ok()?;
    let v: Option<String> = conn.query_row("SELECT value FROM app_settings WHERE key=?1", [KEY], |r| r.get(0)).ok();
    v.as_deref().and_then(parse).or_else(|| legacy(app))
}

/// Migração: tamanho/posição que o plugin antigo deixou em `.window-state.json` (lido uma vez; se era
/// a geometria de tela cheia, `fit` descarta).
fn legacy(app: &AppHandle) -> Option<Bounds> {
    let path = app.path().app_config_dir().ok()?.join(".window-state.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let m = &v["main"];
    Some(Bounds { x: m["x"].as_i64()? as i32, y: m["y"].as_i64()? as i32, w: m["width"].as_u64()? as u32, h: m["height"].as_u64()? as u32 })
}

/// Grava a geometria atual — só em modo janela.
pub fn save(app: &AppHandle, w: &WebviewWindow) {
    let windowed = !w.is_fullscreen().unwrap_or(true) && !w.is_minimized().unwrap_or(true) && !w.is_maximized().unwrap_or(true);
    if !windowed {
        return;
    }
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size()) else { return };
    if size.width < MIN_W || size.height < MIN_H {
        return;
    }
    let db = app.state::<Db>();
    let Ok(conn) = db.conn() else { return };
    let _ = conn.execute(
        "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![KEY, format!("{},{},{},{}", pos.x, pos.y, size.width, size.height)],
    );
}

/// Aplica a geometria salva (antes de exibir a janela, em modo janela).
pub fn restore(app: &AppHandle, w: &WebviewWindow) {
    let Some(b) = read(app) else { return };
    let monitors: Vec<(i32, i32, u32, u32)> = w
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    if let Some(b) = fit(b, &monitors) {
        let _ = w.set_size(PhysicalSize::new(b.w, b.h));
        let _ = w.set_position(PhysicalPosition::new(b.x, b.y));
    }
}

/// Acompanha movimentos/redimensionamentos (no máximo 1 gravação a cada 500 ms) e grava ao fechar.
pub fn track(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let (a, w2) = (app.clone(), w.clone());
    w.on_window_event(move |e| match e {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            if now.saturating_sub(LAST_SAVE.load(Ordering::Relaxed)) >= 500 {
                LAST_SAVE.store(now, Ordering::Relaxed);
                save(&a, &w2);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            save(&a, &w2);
            // Backup automático "antes de encerrar": adia o fechamento até o backup terminar.
            if crate::backup::auto::on_close_requested(&a) {
                api.prevent_close();
            }
        }
        // Fechar a janela principal encerra o app. Sem isto, a janela de notificação (oculta, mas
        // aberta) mantinha o processo vivo e invisível — com atalhos globais e vigias ainda ativos.
        WindowEvent::Destroyed => a.exit(0),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const MON: (i32, i32, u32, u32) = (0, 0, 1920, 1080);

    #[test]
    fn parses_and_rejects_garbage() {
        assert_eq!(parse("10,20,1600,900"), Some(Bounds { x: 10, y: 20, w: 1600, h: 900 }));
        assert_eq!(parse("10,20,0,900"), None);
        assert_eq!(parse("abc"), None);
    }

    #[test]
    fn fullscreen_sized_or_offscreen_bounds_are_ignored() {
        assert_eq!(fit(Bounds { x: 0, y: 0, w: 1920, h: 1080 }, &[MON]), None); // era tela cheia
        assert_eq!(fit(Bounds { x: 5000, y: 0, w: 1600, h: 900 }, &[MON]), None); // monitor desligado
        assert_eq!(fit(Bounds { x: 0, y: 0, w: 800, h: 500 }, &[MON]), None); // menor que o mínimo
        let ok = Bounds { x: 100, y: 80, w: 1600, h: 900 };
        assert_eq!(fit(ok, &[MON]), Some(ok));
        // segundo monitor à direita
        let right = Bounds { x: 2000, y: 100, w: 1280, h: 720 };
        assert_eq!(fit(right, &[MON, (1920, 0, 2560, 1440)]), Some(right));
    }
}
