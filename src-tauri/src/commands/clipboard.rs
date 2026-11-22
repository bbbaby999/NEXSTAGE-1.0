//! Área de transferência (texto) para o menu de contexto próprio dos campos de texto (Copiar, Recortar,
//! Colar). O WebView não lê a área de transferência sem pedir permissão, então o texto passa por aqui.

use crate::error::{AppError, AppResult};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x2;

    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn GetClipboardData(fmt: u32) -> *mut c_void;
        fn SetClipboardData(fmt: u32, h: *mut c_void) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalFree(h: *mut c_void) -> *mut c_void;
        fn GlobalLock(h: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(h: *mut c_void) -> i32;
    }

    /// Abre a área de transferência (outro programa pode estar com ela aberta: tenta algumas vezes).
    fn open() -> bool {
        for _ in 0..10 {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        false
    }

    pub fn get() -> Option<String> {
        if !open() {
            return None;
        }
        let out = unsafe {
            let h = GetClipboardData(CF_UNICODETEXT);
            if h.is_null() {
                None
            } else {
                let p = GlobalLock(h) as *const u16;
                if p.is_null() {
                    None
                } else {
                    let mut n = 0;
                    while *p.add(n) != 0 {
                        n += 1;
                    }
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                    GlobalUnlock(h);
                    Some(s)
                }
            }
        };
        unsafe { CloseClipboard() };
        out
    }

    pub fn set(text: &str) -> bool {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        if !open() {
            return false;
        }
        let ok = unsafe {
            EmptyClipboard();
            let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
            if h.is_null() {
                false
            } else {
                let p = GlobalLock(h) as *mut u16;
                if p.is_null() {
                    GlobalFree(h);
                    false
                } else {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                    GlobalUnlock(h);
                    // depois de entregue, a memória passa a ser do sistema
                    if SetClipboardData(CF_UNICODETEXT, h).is_null() {
                        GlobalFree(h);
                        false
                    } else {
                        true
                    }
                }
            }
        };
        unsafe { CloseClipboard() };
        ok
    }
}

#[tauri::command]
pub fn clipboard_read_text() -> AppResult<String> {
    #[cfg(windows)]
    return Ok(win::get().unwrap_or_default());
    #[cfg(not(windows))]
    Ok(String::new())
}

#[tauri::command]
pub fn clipboard_write_text(text: String) -> AppResult<()> {
    #[cfg(windows)]
    if !win::set(&text) {
        return Err(AppError::Other("Não foi possível copiar para a área de transferência.".into()));
    }
    #[cfg(not(windows))]
    let _ = (text, AppError::Other(String::new()));
    Ok(())
}
