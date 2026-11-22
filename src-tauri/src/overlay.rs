//! Overlay do NEXSTAGE sobre jogos, atalhos globais configuráveis (overlay e captura de tela)
//! e jogos em execução.
//!
//! Não é um hook gráfico dentro do jogo (isso exigiria injeção de DLL e pode acionar
//! anti-cheats). O overlay é a própria janela do NEXSTAGE, sem bordas, tela cheia, sempre no
//! topo e semitransparente. Funciona sobre jogos em modo janela ou "janela sem bordas".

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::params;
use serde::Serialize;
use std::{
    collections::HashMap,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering},
        Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+O";
pub const DEFAULT_SHOT_SHORTCUT: &str = "Ctrl+Shift+P";

#[derive(Clone, Copy, PartialEq)]
pub enum Slot {
    Overlay,
    Shot,
}
impl Slot {
    fn key(self) -> &'static str {
        match self { Slot::Overlay => "overlay_shortcut", Slot::Shot => "screenshot_shortcut" }
    }
    fn default(self) -> &'static str {
        match self { Slot::Overlay => DEFAULT_SHORTCUT, Slot::Shot => DEFAULT_SHOT_SHORTCUT }
    }
}

pub struct Runtime {
    pub active: AtomicBool,
    /// game_id -> PID do processo iniciado pelo launcher.
    pub running: Mutex<HashMap<i64, u32>>,
    pub shortcut_overlay: Mutex<Option<Shortcut>>,
    pub shortcut_shot: Mutex<Option<Shortcut>>,
    /// Combinações de botões do controle (máscara de botões, veja input.rs).
    pub pad_overlay: AtomicU32,
    pub pad_shot: AtomicU32,
    /// Combinação que traz o NEXSTAGE para frente em tela cheia (de qualquer janela).
    pub pad_summon: AtomicU32,
    /// Enquanto o usuário escolhe uma combinação nas configurações.
    pub capture: AtomicBool,
    /// Perfil ativo (para guardar capturas feitas em jogo na Galeria certa) e último jogo iniciado.
    pub active_profile: AtomicI64,
    pub last_game: AtomicI64,
    /// Cor da barra de luz dos controles PlayStation (0 = não definida; senão 1<<24 | 0xRRGGBB) e
    /// geração (incrementa a cada mudança, para a leitura dos controles reaplicar).
    pub pad_led: AtomicU32,
    pub pad_led_gen: AtomicU32,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            active: AtomicBool::new(false),
            running: Mutex::new(HashMap::new()),
            shortcut_overlay: Mutex::new(None),
            shortcut_shot: Mutex::new(None),
            pad_overlay: AtomicU32::new(0),
            pad_shot: AtomicU32::new(0),
            pad_summon: AtomicU32::new(0),
            capture: AtomicBool::new(false),
            active_profile: AtomicI64::new(0),
            last_game: AtomicI64::new(0),
            pad_led: AtomicU32::new(0),
            pad_led_gen: AtomicU32::new(0),
        }
    }
}

impl Runtime {
    pub fn add(&self, game_id: i64, pid: u32) {
        if let Ok(mut m) = self.running.lock() {
            m.insert(game_id, pid);
        }
    }
    /// Remove o jogo e informa se não restou nenhum em execução.
    pub fn remove(&self, game_id: i64) -> bool {
        match self.running.lock() {
            Ok(mut m) => {
                m.remove(&game_id);
                m.is_empty()
            }
            Err(_) => true,
        }
    }
    pub fn is_running(&self) -> bool {
        self.running.lock().map(|m| !m.is_empty()).unwrap_or(false)
    }
    pub fn is_game_running(&self, id: i64) -> bool {
        self.running.lock().map(|m| m.contains_key(&id)).unwrap_or(false)
    }
}

pub fn setting(app: &AppHandle, key: &str) -> Option<String> {
    let db = app.state::<Db>();
    let conn = db.conn().ok()?;
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get::<_, String>(0)).ok()
}

fn fullscreen_pref(app: &AppHandle) -> bool {
    setting(app, "fullscreen").map(|v| v == "1").unwrap_or(false)
}

/// Liga/desliga o modo overlay. Ao desligar com jogo em execução, minimiza o NEXSTAGE
/// para devolver o foco ao jogo.
pub fn set_active(app: &AppHandle, on: bool) {
    let rt = app.state::<Runtime>();
    rt.active.store(on, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        if on {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_decorations(false);
            let _ = w.set_always_on_top(true);
            let _ = w.set_fullscreen(true);
            let _ = w.set_focus();
            take_foreground(app);
        } else {
            let _ = w.set_always_on_top(false);
            let _ = w.set_decorations(true);
            let _ = w.set_fullscreen(fullscreen_pref(app));
            if rt.is_running() {
                let _ = w.minimize();
            }
        }
    }
    let _ = app.emit("overlay-changed", on);
}

/// Garante que o overlay vire a janela ATIVA (e não só fique por cima do jogo).
///
/// `set_focus()` usa `SetForegroundWindow`, que o Windows recusa quando o pedido não vem de uma
/// interação com o próprio app — é o caso do combo do controle (lido numa thread em segundo plano)
/// e, às vezes, do atalho global. Aí o overlay aparece, mas o jogo continua com o foco e recebendo
/// os comandos. Contorno: anexar a fila de entrada à da janela em primeiro plano (o jogo) e, se
/// ainda assim falhar, simular um toque de Alt (libera a trava de primeiro plano). Repete por ~4 s
/// porque a troca para tela cheia pode devolver a ativação ao jogo.
fn take_foreground(app: &AppHandle) {
    take_foreground_while(app, |a| a.state::<Runtime>().active.load(Ordering::SeqCst));
}

/// Mesmo contorno de `take_foreground`, repetido enquanto `keep` for verdadeiro (overlay aberto, ou a
/// janela ainda sem o foco).
fn take_foreground_while(app: &AppHandle, keep: fn(&AppHandle) -> bool) {
    #[cfg(windows)]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            // ~4 s: jogos em tela cheia costumam retomar a ativação depois da troca de modo de vídeo.
            // Só age quando o foco não está no NEXSTAGE (force_foreground não faz nada se já estiver).
            for i in 0..40 {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if !keep(&app) {
                    return;
                }
                let a = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some(w) = a.get_webview_window("main") {
                        if let Ok(h) = w.hwnd() {
                            if win::force_foreground(h.0 as *mut std::ffi::c_void) {
                                let _ = w.set_focus(); // repassa o foco do teclado ao WebView2
                            }
                        }
                    }
                });
            }
        });
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// Traz uma janela de outro programa para o primeiro plano (mesmo contorno da trava do Windows usado no overlay).
#[cfg(windows)]
pub fn force_window_foreground(h: *mut std::ffi::c_void) {
    win::force_foreground(h);
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;

    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> Hwnd;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn AttachThreadInput(attach: u32, to: u32, on: i32) -> i32;
        fn SetForegroundWindow(h: Hwnd) -> i32;
        fn BringWindowToTop(h: Hwnd) -> i32;
        fn SetActiveWindow(h: Hwnd) -> Hwnd;
        fn keybd_event(vk: u8, scan: u8, flags: u32, extra: usize);
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }

    /// INPUT (tipo mouse) do SendInput: 40 bytes no x64, 28 no x86 (o alinhamento do usize cuida do recheio).
    #[repr(C)]
    struct MouseInput { dx: i32, dy: i32, data: u32, flags: u32, time: u32, extra: usize }
    #[repr(C)]
    struct Input { kind: u32, mi: MouseInput }
    #[link(name = "user32")]
    extern "system" {
        fn SendInput(n: u32, inputs: *const Input, size: i32) -> u32;
        fn SetFocus(h: Hwnd) -> Hwnd;
        fn IsIconic(h: Hwnd) -> i32;
        fn ShowWindow(h: Hwnd, cmd: i32) -> i32;
    }

    /// Entrada sintética "vazia" (mouse sem movimento nem clique): o Windows só libera a troca de
    /// primeiro plano para o processo que recebeu a última entrada — o combo veio do controle, que o
    /// Windows não conta. Não chega ao jogo como comando (mesmo truque do PowerToys Run).
    unsafe fn unlock_foreground() {
        let i = Input { kind: 0, mi: MouseInput { dx: 0, dy: 0, data: 0, flags: 0, time: 0, extra: 0 } };
        SendInput(1, &i, std::mem::size_of::<Input>() as i32);
    }

    unsafe fn activate(h: Hwnd, fg: Hwnd) {
        let me = GetCurrentThreadId();
        let other = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
        // Fila de entrada compartilhada com a do jogo: SetForegroundWindow/SetFocus passam a valer.
        let attached = other != 0 && other != me && AttachThreadInput(me, other, 1) != 0;
        if IsIconic(h) != 0 {
            ShowWindow(h, 9); // SW_RESTORE
        }
        BringWindowToTop(h);
        SetForegroundWindow(h);
        SetActiveWindow(h);
        SetFocus(h);
        if attached {
            AttachThreadInput(me, other, 0);
        }
    }

    /// Traz `h` para o primeiro plano. Retorna `true` se precisou agir (o foco estava em outra janela).
    pub fn force_foreground(h: Hwnd) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg == h {
                return false;
            }
            unlock_foreground();
            activate(h, fg);
            if GetForegroundWindow() != h {
                // Último recurso: toque de Alt (libera a trava de primeiro plano mesmo quando a entrada
                // vazia não basta) e nova tentativa.
                const VK_MENU: u8 = 0x12;
                const KEYEVENTF_EXTENDEDKEY: u32 = 0x1;
                const KEYEVENTF_KEYUP: u32 = 0x2;
                keybd_event(VK_MENU, 0, KEYEVENTF_EXTENDEDKEY, 0);
                keybd_event(VK_MENU, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
                activate(h, GetForegroundWindow());
            }
            true
        }
    }
}

/// Traz o NEXSTAGE para frente em tela cheia (combinação do controle), vindo de qualquer janela —
/// inclusive de um jogo. Com o overlay aberto, sai do modo overlay e fica no app normal em tela cheia.
/// A tela cheia vira a preferência salva (a mesma de Configurações → Iniciar em tela cheia).
pub fn summon(app: &AppHandle) {
    let rt = app.state::<Runtime>();
    let was_overlay = rt.active.swap(false, Ordering::SeqCst);
    if let Ok(conn) = app.state::<Db>().conn() {
        let _ = conn.execute("INSERT INTO app_settings(key,value) VALUES('fullscreen','1') ON CONFLICT(key) DO UPDATE SET value='1'", []);
    }
    if let Some(w) = app.get_webview_window("main") {
        if was_overlay {
            let _ = w.set_always_on_top(false);
            let _ = w.set_decorations(true);
        }
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_fullscreen(true);
        let _ = w.set_focus();
    }
    if was_overlay {
        let _ = app.emit("overlay-changed", false);
    }
    let _ = app.emit("fullscreen-changed", true);
    hold_foreground(app);
}

/// Mantém o NEXSTAGE como janela ATIVA por ~1,5 s depois de chamado pelo controle.
///
/// O pedido vem da leitura do controle (segundo plano), então o Windows recusa `set_focus` sem ajuda; e a
/// troca para tela cheia pode devolver a ativação à janela anterior (jogo/outro app) logo depois. Por
/// isso insiste a cada 100 ms — mesmo que a janela pareça focada num instante — e, no fim, repassa o
/// foco do teclado ao WebView2 (sem isso a janela fica na frente, mas teclado/controle não navegam).
fn hold_foreground(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        const TRIES: u32 = 15;
        for i in 0..TRIES {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                let Some(w) = a.get_webview_window("main") else { return };
                #[cfg(windows)]
                let acted = w.hwnd().map(|h| win::force_foreground(h.0 as *mut std::ffi::c_void)).unwrap_or(false);
                #[cfg(not(windows))]
                let acted = false;
                if acted || i == 0 || i == TRIES - 1 || !w.is_focused().unwrap_or(false) {
                    let _ = w.set_focus();
                }
            });
        }
        let _ = app.emit("app-summoned", ());
    });
}

pub fn toggle(app: &AppHandle) {
    let on = !app.state::<Runtime>().active.load(Ordering::SeqCst);
    set_active(app, on);
}

/// Chamado quando um jogo termina.
pub fn game_finished(app: &AppHandle, game_id: i64) {
    if app.state::<Runtime>().remove(game_id) {
        set_active(app, false);
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

// ---------- atalhos globais (teclado) ----------

fn parse_checked(acc: &str) -> AppResult<Shortcut> {
    let sc: Shortcut = acc
        .trim()
        .parse()
        .map_err(|_| AppError::Validation("Combinação de teclas inválida.".into()))?;
    if sc.mods.is_empty() {
        return Err(AppError::Validation("Inclua ao menos Ctrl, Alt, Shift ou Win na combinação.".into()));
    }
    if sc.mods.contains(Modifiers::ALT) && sc.key == Code::F4 {
        return Err(AppError::Validation("Alt+F4 é reservado pelo Windows.".into()));
    }
    if sc.mods.contains(Modifiers::ALT) && sc.key == Code::Tab {
        return Err(AppError::Validation("Alt+Tab é reservado pelo Windows.".into()));
    }
    Ok(sc)
}

fn pick<'a>(rt: &'a Runtime, slot: Slot) -> (&'a Mutex<Option<Shortcut>>, &'a Mutex<Option<Shortcut>>) {
    match slot {
        Slot::Overlay => (&rt.shortcut_overlay, &rt.shortcut_shot),
        Slot::Shot => (&rt.shortcut_shot, &rt.shortcut_overlay),
    }
}

/// Troca o atalho de um slot. Falha (e restaura o anterior) se outro programa já usa a combinação
/// ou se ela for igual à do outro atalho do NEXSTAGE.
fn apply_shortcut(app: &AppHandle, acc: &str, slot: Slot) -> AppResult<()> {
    let sc = parse_checked(acc)?;
    let rt = app.state::<Runtime>();
    let (mine, other) = pick(&rt, slot);
    if other.lock().ok().and_then(|g| g.clone()).as_ref() == Some(&sc) {
        return Err(AppError::Validation("Essa combinação já é usada por outro atalho do NEXSTAGE.".into()));
    }
    let mut cur = mine.lock().map_err(|_| AppError::Other("Estado indisponível.".into()))?;
    if cur.as_ref() == Some(&sc) {
        return Ok(());
    }
    let gs = app.global_shortcut();
    let old = cur.clone();
    if let Some(o) = &old {
        let _ = gs.unregister(o.clone());
    }
    match gs.register(sc.clone()) {
        Ok(()) => {
            *cur = Some(sc);
            Ok(())
        }
        Err(e) => {
            if let Some(o) = old {
                let _ = gs.register(o);
            }
            Err(AppError::Validation(format!(
                "Não foi possível registrar esse atalho (outro programa pode estar usando): {e}"
            )))
        }
    }
}

pub fn register_saved(app: &AppHandle) {
    for slot in [Slot::Overlay, Slot::Shot] {
        let acc = setting(app, slot.key()).unwrap_or_else(|| slot.default().into());
        if apply_shortcut(app, &acc, slot).is_err() {
            let _ = apply_shortcut(app, slot.default(), slot);
        }
    }
}

/// O atalho que acabou de ser pressionado é o da captura de tela?
pub fn is_shot_shortcut(app: &AppHandle, sc: &Shortcut) -> bool {
    app.state::<Runtime>().shortcut_shot.lock().ok().and_then(|g| g.clone()).as_ref() == Some(sc)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInfo {
    pub accelerator: String,
    pub is_default: bool,
    pub active: bool,
}

fn info(app: &AppHandle, slot: Slot) -> ShortcutInfo {
    let acc = setting(app, slot.key()).unwrap_or_else(|| slot.default().into());
    let rt = app.state::<Runtime>();
    let (mine, _) = pick(&rt, slot);
    let active = mine.lock().map(|g| g.is_some()).unwrap_or(false);
    ShortcutInfo { is_default: acc == slot.default(), accelerator: acc, active }
}

fn set_slot(app: &AppHandle, db: &Db, slot: Slot, acc: &str) -> AppResult<ShortcutInfo> {
    apply_shortcut(app, acc, slot)?;
    {
        let conn = db.conn()?;
        conn.execute(
            "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![slot.key(), acc.trim()],
        )?;
    }
    Ok(info(app, slot))
}

fn reset_slot(app: &AppHandle, db: &Db, slot: Slot) -> AppResult<ShortcutInfo> {
    apply_shortcut(app, slot.default(), slot)?;
    {
        let conn = db.conn()?;
        conn.execute("DELETE FROM app_settings WHERE key=?1", params![slot.key()])?;
    }
    Ok(info(app, slot))
}

#[tauri::command]
pub fn get_overlay_shortcut(app: AppHandle) -> ShortcutInfo { info(&app, Slot::Overlay) }
#[tauri::command]
pub fn set_overlay_shortcut(app: AppHandle, db: State<Db>, accelerator: String) -> AppResult<ShortcutInfo> {
    set_slot(&app, &db, Slot::Overlay, &accelerator)
}
#[tauri::command]
pub fn reset_overlay_shortcut(app: AppHandle, db: State<Db>) -> AppResult<ShortcutInfo> {
    reset_slot(&app, &db, Slot::Overlay)
}
#[tauri::command]
pub fn get_screenshot_shortcut(app: AppHandle) -> ShortcutInfo { info(&app, Slot::Shot) }
#[tauri::command]
pub fn set_screenshot_shortcut(app: AppHandle, db: State<Db>, accelerator: String) -> AppResult<ShortcutInfo> {
    set_slot(&app, &db, Slot::Shot, &accelerator)
}
#[tauri::command]
pub fn reset_screenshot_shortcut(app: AppHandle, db: State<Db>) -> AppResult<ShortcutInfo> {
    reset_slot(&app, &db, Slot::Shot)
}

/// O frontend informa o perfil ativo (0 = nenhum) para o backend salvar capturas na Galeria certa.
#[tauri::command]
pub fn set_active_profile(rt: State<Runtime>, id: i64) {
    rt.active_profile.store(id, Ordering::SeqCst);
}

// ---------- overlay / jogos ----------

#[tauri::command]
pub fn close_overlay(app: AppHandle) {
    set_active(&app, false);
}

#[tauri::command]
pub fn running_games(rt: State<Runtime>) -> Vec<i64> {
    rt.running.lock().map(|m| m.keys().copied().collect()).unwrap_or_default()
}

/// Encerra o jogo (e seus processos filhos) iniciado pelo launcher.
#[tauri::command]
pub fn quit_game(rt: State<Runtime>, game_id: i64) -> AppResult<()> {
    let pid = {
        let m = rt.running.lock().map_err(|_| AppError::Other("Estado indisponível.".into()))?;
        m.get(&game_id).copied()
    }
    .ok_or_else(|| AppError::NotFound("Esse jogo não está em execução.".into()))?;

    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(AppError::Other("Não foi possível encerrar o jogo (ele pode exigir permissão de administrador).".into()))
    }
}
