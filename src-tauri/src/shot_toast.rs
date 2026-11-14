//! Notificação de captura: janelinha transparente, sempre no topo, que NÃO rouba o foco do jogo e
//! ignora o mouse (cliques passam para o jogo). Fica no canto inferior direito do monitor do jogo,
//! mostra a miniatura da captura e some sozinha. O som (discreto) é tocado pela própria janela —
//! mesmo quando só o som está ligado, a janela recebe o evento, mas continua invisível.
//!
//! Configurações (app_settings): `shot_sound` e `shot_notify` ("0" desliga; padrão ligado) e
//! `shot_notify_secs` (duração em segundos, 2–10; padrão 4).
//! Limitação: jogos em tela cheia EXCLUSIVA (DirectX antigo) não deixam janelas aparecerem por cima;
//! em "janela sem bordas" funciona normalmente.

use crate::overlay;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "shot-toast";
/// Tamanho lógico da janela (o cartão + margem para a sombra).
const W: f64 = 400.0;
const H: f64 = 128.0;
/// Distância das bordas da área de trabalho (px lógicos).
const MARGIN: f64 = 18.0;

static GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToastEvent {
    thumb: String,
    title: Option<String>,
    secs: u64,
    sound: bool,
    show: bool,
}

pub struct Prefs {
    pub sound: bool,
    pub notify: bool,
    pub secs: u64,
}

pub fn prefs(app: &AppHandle) -> Prefs {
    Prefs {
        sound: overlay::setting(app, "shot_sound").as_deref() != Some("0"),
        notify: overlay::setting(app, "shot_notify").as_deref() != Some("0"),
        secs: overlay::setting(app, "shot_notify_secs").and_then(|v| v.parse().ok()).map(|s: u64| s.clamp(2, 10)).unwrap_or(4),
    }
}

/// Cria a janela (oculta) na inicialização: assim ela aparece na hora quando a captura acontece.
pub fn create(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    // O WebView2 só cria outra janela no mesmo app se ela usar EXATAMENTE os mesmos argumentos de
    // navegador da principal (mesma pasta de dados). Sem isto a criação falhava em silêncio e nenhum
    // aviso (captura ou conquista) aparecia. Lidos do tauri.conf.json para nunca divergirem.
    let args = app.config().app.windows.iter().find(|w| w.label == "main").and_then(|w| w.additional_browser_args.clone());
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()));
    if let Some(a) = args.as_deref() {
        builder = builder.additional_browser_args(a);
    }
    let built = builder
        .title("NEXSTAGE · captura")
        .inner_size(W, H)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_ignore_cursor_events(true);
        }
        Err(e) => eprintln!("NEXSTAGE: janela de notificação não pôde ser criada: {e}"),
    }
}

/// Avisa a captura recém-salva (miniatura em data URL PNG).
pub fn notify(app: &AppHandle, thumb: String, title: Option<String>) {
    let p = prefs(app);
    if !p.sound && !p.notify {
        return;
    }
    let Some(w) = app.get_webview_window(LABEL) else { return };
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if p.notify {
        place_and_show(&w, Spot::Corner);
    }
    let _ = app.emit("shot-toast", ToastEvent { thumb, title, secs: p.secs, sound: p.sound, show: p.notify });
    if p.notify {
        let w2 = w.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(p.secs * 1000 + 450));
            if GEN.load(Ordering::SeqCst) == gen {
                hide_now(&w2); // só a mais recente esconde (capturas seguidas reiniciam o tempo)
            }
        });
    }
}

/// Conquista desbloqueada (RetroAchievements ou Steam) — mesma janela, cartão no estilo dos consoles.
#[derive(Clone, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AchievementToast {
    /// "ra" | "steam"
    pub source: String,
    pub icon: Option<String>,
    pub title: String,
    pub game: String,
    /// Pontos (RetroAchievements); 0 na Steam.
    pub points: i64,
    pub hardcore: bool,
    /// Rara: estilo dourado, som próprio e mais tempo na tela.
    pub rare: bool,
    /// % de jogadores com a conquista (Steam), quando conhecida.
    pub global_pct: Option<f64>,
    /// Quantas outras conquistas chegaram junto (mostrado no último cartão).
    pub more: usize,
    /// Troféus (Vita3K/RPCS3): "P" platina, "G" ouro, "S" prata, "B" bronze.
    pub grade: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AchievementEvent {
    item: AchievementToast,
    secs: u64,
    sound: bool,
}

/// Steam: rara = obtida por menos de 10% dos jogadores (o mesmo corte que a Steam usa para "rara").
pub fn steam_is_rare(global_pct: Option<f64>) -> bool {
    global_pct.is_some_and(|p| p < 10.0)
}

/// RetroAchievements: rara pelos "pontos reais" (TrueRatio cresce quando poucos jogadores obtêm a
/// conquista): 4x os pontos ou mais — ou conquistas de 50 pontos, as mais difíceis do sistema.
pub fn ra_is_rare(points: i64, true_ratio: i64) -> bool {
    points >= 50 || (points > 0 && true_ratio >= points * 4)
}

/// Aviso de controle: conectado, desconectado ou bateria baixa.
#[derive(Clone, Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PadToast {
    /// "connected" | "disconnected" | "battery"
    pub event: String,
    /// "xbox" | "playstation" | "generic"
    pub device: String,
    pub name: String,
    pub percent: Option<u8>,
    /// Nível em passos quando não há % (ex.: "Crítica").
    pub label: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PadEvent {
    item: PadToast,
    secs: u64,
    sound: bool,
}

/// Fila única da janela de notificação: conquistas e controles, um cartão de cada vez.
enum Queued {
    Ach(AchievementToast),
    Pad(PadToast),
}

static QUEUE: Mutex<VecDeque<Queued>> = Mutex::new(VecDeque::new());
static WORKER: AtomicBool = AtomicBool::new(false);

/// Põe os cartões na fila e os mostra um após o outro (RetroAchievements e Steam juntos, sem
/// atropelar), por cima de qualquer janela — jogo ou NEXSTAGE — sem tirar o foco.
/// Configuração (app_settings): `ach_toast` ("0" desliga; padrão ligado) — Configurações → Notificações.
pub fn notify_achievements(app: &AppHandle, mut items: Vec<AchievementToast>, extra: usize) {
    if items.is_empty() || !enabled(app, "ach_toast") {
        return;
    }
    if let Some(last) = items.last_mut() {
        last.more = extra;
    }
    if let Ok(mut q) = QUEUE.lock() {
        q.extend(items.into_iter().map(Queued::Ach));
    }
    start_worker(app);
}

/// Aviso de controle (conectado, desconectado, bateria baixa), por cima do jogo e do app.
/// Configuração: `pad_toast` ("0" desliga; padrão ligado) — Configurações → Notificações.
pub fn notify_pad(app: &AppHandle, item: PadToast) {
    if !enabled(app, "pad_toast") {
        return;
    }
    push(app, Queued::Pad(item));
}

/// Interruptor das Configurações (padrão ligado).
pub fn enabled(app: &AppHandle, key: &str) -> bool {
    overlay::setting(app, key).as_deref() != Some("0")
}

fn push(app: &AppHandle, item: Queued) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push_back(item);
    }
    start_worker(app);
}

fn start_worker(app: &AppHandle) {
    if WORKER.swap(true, Ordering::SeqCst) {
        return; // já está mostrando: a fila é consumida por ele
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(w) = app.get_webview_window(LABEL) {
            let sound = overlay::setting(&app, "ach_sound").as_deref() != Some("0");
            while let Some(item) = QUEUE.lock().ok().and_then(|mut q| q.pop_front()) {
                let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
                place_and_show(&w, Spot::BottomCenter);
                let secs = match item {
                    Queued::Ach(item) => {
                        let secs = if item.rare { 7 } else { 5 };
                        let _ = app.emit("ach-toast", AchievementEvent { item, secs, sound });
                        secs
                    }
                    Queued::Pad(item) => {
                        let secs = if item.event == "battery" { 5 } else { 3 };
                        let _ = app.emit("pad-toast", PadEvent { item, secs, sound });
                        secs
                    }
                };
                std::thread::sleep(Duration::from_millis(secs * 1000 + 700));
                if GEN.load(Ordering::SeqCst) == gen {
                    hide_now(&w);
                }
            }
        }
        WORKER.store(false, Ordering::SeqCst);
        // algo chegou entre o fim do laço e a liberação: começa de novo
        if QUEUE.lock().map(|q| !q.is_empty()).unwrap_or(false) {
            start_worker(&app);
        }
    });
}

/// Botão "Testar aviso" (Configurações): mostra um cartão de exemplo pelo mesmo caminho real
/// (fila, janela por cima de tudo e som). Textos já traduzidos pela interface.
#[tauri::command]
pub fn preview_achievement_toast(app: AppHandle, rare: bool, title: String, game: String) {
    let item = AchievementToast {
        source: "steam".into(),
        icon: None,
        title,
        game,
        points: 0,
        hardcore: false,
        rare,
        global_pct: Some(if rare { 2.7 } else { 58.0 }),
        more: 0,
        grade: None,
    };
    push(&app, Queued::Ach(item));
}

/// Botões "Testar" de controles (Configurações → Notificações): mesmo caminho real, sem o interruptor.
#[tauri::command]
pub fn preview_pad_toast(app: AppHandle, event: String, name: String) {
    let battery = event == "battery";
    push(&app, Queued::Pad(PadToast { event, device: "xbox".into(), name, percent: battery.then_some(12), label: None }));
}

/// Onde a janela aparece: capturas no canto inferior direito; conquistas embaixo, no centro.
#[derive(Clone, Copy)]
enum Spot {
    Corner,
    BottomCenter,
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }
    #[repr(C)]
    pub struct MonitorInfo { pub size: u32, pub monitor: Rect, pub work: Rect, pub flags: u32 }
    #[link(name = "user32")]
    extern "system" {
        pub fn GetForegroundWindow() -> *mut c_void;
        pub fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        pub fn GetMonitorInfoW(h: *mut c_void, info: *mut MonitorInfo) -> i32;
        pub fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
        pub fn SetWindowPos(hwnd: *mut c_void, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        pub fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
    }
}

/// Posiciona no monitor do jogo (a janela em primeiro plano) e exibe
/// SEM ativar: o jogo continua com o foco e recebendo os comandos.
#[cfg(windows)]
fn place_and_show(w: &tauri::WebviewWindow, spot: Spot) {
    use win::*;
    let Ok(h) = w.hwnd() else { return };
    let hwnd = h.0 as *mut std::ffi::c_void;
    unsafe {
        let fg = GetForegroundWindow();
        let mon = MonitorFromWindow(fg, 2); // MONITOR_DEFAULTTONEAREST
        let mut mi = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            work: Rect { left: 0, top: 0, right: 0, bottom: 0 },
            flags: 0,
        };
        if mon.is_null() || GetMonitorInfoW(mon, &mut mi) == 0 {
            let _ = w.show();
            return;
        }
        let dpi = if fg.is_null() { 96 } else { GetDpiForWindow(fg).max(96) };
        let k = dpi as f64 / 96.0;
        let (cw, ch, m) = ((W * k) as i32, (H * k) as i32, (MARGIN * k) as i32);
        let (x, y) = match spot {
            Spot::Corner => (mi.work.right - cw - m, mi.work.bottom - ch - m),
            // embaixo, no centro da área do monitor (a barra de tarefas fica de fora)
            Spot::BottomCenter => ((mi.work.left + mi.work.right - cw) / 2, mi.work.bottom - ch - m * 3),
        };
        // HWND_TOPMOST, SWP_NOACTIVATE | SWP_SHOWWINDOW
        SetWindowPos(hwnd, -1, x, y, cw, ch, 0x0010 | 0x0040);
    }
}
/// Esconde a janela pelo MESMO caminho usado para mostrá-la (Win32 direto). O `hide()` do Tauri não
/// serve aqui: como a janela é exibida por SetWindowPos (para não roubar o foco do jogo), o Tauri
/// acha que ela continua oculta e ignora o pedido — ela ficava aberta (transparente, sempre no topo)
/// para sempre depois do primeiro aviso.
#[cfg(windows)]
fn hide_now(w: &tauri::WebviewWindow) {
    if let Ok(h) = w.hwnd() {
        unsafe { win::ShowWindow(h.0 as *mut std::ffi::c_void, 0) }; // SW_HIDE
    }
}
#[cfg(not(windows))]
fn hide_now(w: &tauri::WebviewWindow) {
    let _ = w.hide();
}

#[cfg(not(windows))]
fn place_and_show(w: &tauri::WebviewWindow, _spot: Spot) {
    let _ = w.show();
}

/// Miniatura (largura `tw`) por média de blocos, em PNG → data URL. Rápido e sem dependências.
pub fn thumbnail(w: i32, h: i32, rgba: &[u8], tw: u32) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    if w <= 0 || h <= 0 {
        return None;
    }
    let (w, h) = (w as u32, h as u32);
    let tw = tw.min(w).max(1);
    let th = ((h as u64 * tw as u64) / w as u64).max(1) as u32;
    let mut out = vec![0u8; (tw * th * 3) as usize];
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            // amostra no máximo 4×4 pixels por bloco (suficiente para uma miniatura)
            let sy = ((y1 - y0) / 4).max(1);
            let sx = ((x1 - x0) / 4).max(1);
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let i = ((y * w + x) * 4) as usize;
                    r += rgba[i] as u64; g += rgba[i + 1] as u64; b += rgba[i + 2] as u64; n += 1;
                    x += sx;
                }
                y += sy;
            }
            let o = ((ty * tw + tx) * 3) as usize;
            out[o] = (r / n) as u8; out[o + 1] = (g / n) as u8; out[o + 2] = (b / n) as u8;
        }
    }
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, tw, th);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(&out).ok()?;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png_bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_keeps_aspect_and_averages() {
        // 8x4 metade vermelha, metade azul → miniatura 4x2
        let mut px = Vec::new();
        for _y in 0..4 {
            for x in 0..8 {
                px.extend_from_slice(if x < 4 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        let url = thumbnail(8, 4, &px, 4).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(thumbnail(0, 4, &px, 4).is_none());
    }
}
