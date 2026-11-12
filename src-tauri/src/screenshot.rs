//! Captura de tela (GDI, sem dependência pesada): fotografa o MONITOR onde está a janela em primeiro
//! plano (normalmente o jogo), salva um PNG na pasta dedicada `Imagens\NEXSTAGE\Capturas` e, se houver
//! perfil ativo, também registra uma cópia na Galeria (associada ao jogo em execução).
//!
//! Limitação do Windows: jogos em tela cheia EXCLUSIVA (alguns DirectX antigos) podem sair pretos;
//! em "janela sem bordas" a captura funciona normalmente.

use crate::commands::gallery;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::overlay::Runtime;
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
mod gdi {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }
    #[repr(C)]
    pub struct MonitorInfo { pub size: u32, pub monitor: Rect, pub work: Rect, pub flags: u32 }
    #[repr(C)]
    pub struct BitmapInfoHeader {
        pub size: u32, pub width: i32, pub height: i32, pub planes: u16, pub bit_count: u16, pub compression: u32,
        pub size_image: u32, pub xppm: i32, pub yppm: i32, pub clr_used: u32, pub clr_important: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        pub fn GetForegroundWindow() -> *mut c_void;
        pub fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        pub fn GetMonitorInfoW(h: *mut c_void, info: *mut MonitorInfo) -> i32;
        pub fn GetDC(hwnd: *mut c_void) -> *mut c_void;
        pub fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        pub fn CreateCompatibleDC(hdc: *mut c_void) -> *mut c_void;
        pub fn CreateCompatibleBitmap(hdc: *mut c_void, w: i32, h: i32) -> *mut c_void;
        pub fn SelectObject(hdc: *mut c_void, obj: *mut c_void) -> *mut c_void;
        pub fn BitBlt(dst: *mut c_void, x: i32, y: i32, w: i32, h: i32, src: *mut c_void, sx: i32, sy: i32, rop: u32) -> i32;
        pub fn GetDIBits(hdc: *mut c_void, hbm: *mut c_void, start: u32, lines: u32, bits: *mut c_void, bmi: *mut BitmapInfoHeader, usage: u32) -> i32;
        pub fn DeleteObject(o: *mut c_void) -> i32;
        pub fn DeleteDC(hdc: *mut c_void) -> i32;
    }
}

/// (largura, altura, pixels RGBA)
#[cfg(windows)]
fn grab() -> Result<(i32, i32, Vec<u8>), String> {
    use gdi::*;
    use std::ptr::null_mut;
    unsafe {
        let mon = MonitorFromWindow(GetForegroundWindow(), 2); // MONITOR_DEFAULTTONEAREST
        let mut mi = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            work: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            flags: 0,
        };
        if mon.is_null() || GetMonitorInfoW(mon, &mut mi) == 0 {
            return Err("não foi possível identificar o monitor".into());
        }
        let (x, y) = (mi.monitor.left, mi.monitor.top);
        let (w, h) = (mi.monitor.right - mi.monitor.left, mi.monitor.bottom - mi.monitor.top);
        if w <= 0 || h <= 0 {
            return Err("monitor com tamanho inválido".into());
        }
        let screen = GetDC(null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, 0x00CC_0020 | 0x4000_0000); // SRCCOPY | CAPTUREBLT
        let mut hdr = BitmapInfoHeader {
            size: 40, width: w, height: -h, planes: 1, bit_count: 32, compression: 0,
            size_image: 0, xppm: 0, yppm: 0, clr_used: 0, clr_important: 0,
        };
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let lines = GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr() as *mut _, &mut hdr, 0);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(null_mut(), screen);
        if ok == 0 || lines == 0 {
            return Err("o Windows recusou a captura".into());
        }
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2); // BGRA -> RGBA
            px[3] = 255;
        }
        Ok((w, h, buf))
    }
}
#[cfg(not(windows))]
fn grab() -> Result<(i32, i32, Vec<u8>), String> {
    Err("captura disponível apenas no Windows".into())
}

/// Pasta dedicada: Imagens\NEXSTAGE\Capturas (ou a pasta de dados do app, se "Imagens" não existir).
pub fn captures_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let base = app
        .path()
        .picture_dir()
        .or_else(|_| app.path().app_data_dir())
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(base.join("NEXSTAGE").join("Capturas"))
}

fn safe(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    t.trim_matches('_').chars().take(40).collect()
}

static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// Só em desenvolvimento (ver `dev`): captura a tela e mostra o aviso com a miniatura real, sem
/// gravar arquivo, sem copiar para a Galeria e sem emitir `screenshot-taken`.
#[cfg(debug_assertions)]
pub fn preview(app: &AppHandle) {
    match grab() {
        Ok((w, h, rgba)) => {
            if let Some(thumb) = crate::shot_toast::thumbnail(w, h, &rgba, 320) {
                crate::shot_toast::notify(app, thumb, Some(String::from("Captura de teste (não salva)")));
            }
        }
        Err(e) => eprintln!("NEXSTAGE (dev): falha na captura de teste: {e}"),
    }
}

pub fn take(app: &AppHandle) -> AppResult<PathBuf> {
    {
        let mut l = LAST.lock().map_err(|_| AppError::Other("Estado indisponível.".into()))?;
        if l.map(|t| t.elapsed() < Duration::from_millis(800)).unwrap_or(false) {
            return Err(AppError::Other("Aguarde um instante entre as capturas.".into()));
        }
        *l = Some(Instant::now());
    }
    let (w, h, rgba) = grab().map_err(|e| AppError::Other(format!("Falha na captura: {e}.")))?;
    let dir = captures_dir(app)?;
    std::fs::create_dir_all(&dir)?;

    let rt = app.state::<Runtime>();
    let profile = rt.active_profile.load(std::sync::atomic::Ordering::SeqCst);
    let game = rt.last_game.load(std::sync::atomic::Ordering::SeqCst);
    let game = if game > 0 && rt.is_game_running(game) { Some(game) } else { None };

    let (stamp, title) = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        let stamp: String = conn.query_row("SELECT strftime('%Y%m%d_%H%M%S','now','localtime')", [], |r| r.get(0))?;
        let title: Option<String> = game.and_then(|g| conn.query_row("SELECT title FROM games WHERE id=?1", [g], |r| r.get(0)).ok());
        (stamp, title)
    };
    let prefix = title.as_deref().map(safe).filter(|s| !s.is_empty()).unwrap_or_else(|| "NEXSTAGE".into());
    let mut path = dir.join(format!("{prefix}_{stamp}.png"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{prefix}_{stamp}_{n}.png"));
        n += 1;
    }

    let file = std::fs::File::create(&path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut wr = enc.write_header().map_err(|e| AppError::Other(format!("PNG: {e}")))?;
    wr.write_image_data(&rgba).map_err(|e| AppError::Other(format!("PNG: {e}")))?;
    drop(wr);

    // Cópia na Galeria do perfil ativo (melhor esforço: a captura em disco já está salva).
    let mut imported = false;
    if profile > 0 {
        if let (Ok(gdir), Ok(db)) = (gallery::gallery_dir(app), Ok::<_, AppError>(app.state::<Db>())) {
            let gdir = gdir.join(profile.to_string());
            if std::fs::create_dir_all(&gdir).is_ok() {
                if let Ok(conn) = db.conn() {
                    imported = gallery::import_one(&conn, &gdir, profile, game, &path.to_string_lossy()).is_ok();
                }
            }
        }
    }
    let _ = app.emit("screenshot-taken", serde_json::json!({ "path": path.to_string_lossy(), "imported": imported }));
    // Aviso por cima do jogo (som + miniatura no canto inferior direito), conforme as configurações.
    if let Some(thumb) = crate::shot_toast::thumbnail(w, h, &rgba, 320) {
        crate::shot_toast::notify(app, thumb, title);
    }
    Ok(path)
}

#[tauri::command]
pub async fn take_screenshot(app: AppHandle) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || take(&app).map(|p| p.to_string_lossy().to_string()))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

#[tauri::command]
pub fn captures_folder(app: AppHandle) -> AppResult<String> {
    Ok(captures_dir(&app)?.to_string_lossy().to_string())
}

/// Abre a pasta de capturas no Explorador (criando-a se ainda não existir).
#[tauri::command]
pub fn open_captures_folder(app: AppHandle) -> AppResult<String> {
    let dir = captures_dir(&app)?;
    std::fs::create_dir_all(&dir)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| AppError::Other(format!("Não foi possível abrir o Explorador de Arquivos: {e}")))?;
    }
    Ok(dir.to_string_lossy().to_string())
}
