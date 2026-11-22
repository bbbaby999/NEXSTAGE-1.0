//! Abre o jogo no MESMO monitor em que o NEXSTAGE está (normalmente o monitor preferido, ver
//! `monitor_pref`).
//!
//! O Windows abre janelas novas no monitor principal, e não há como pedir outro monitor a um processo
//! qualquer. Por isso, ao iniciar um jogo, o monitor do NEXSTAGE é guardado (antes de minimizar) e um
//! vigia procura as janelas do jogo por alguns minutos: cada janela nova que aparecer em outro monitor é
//! levada para o do NEXSTAGE — centralizada, ou ocupando a tela inteira se cobria o monitor de origem
//! (janela sem borda / "tela cheia em janela"). Emuladores e jogos que entram em tela cheia no monitor
//! em que a janela está passam a usar esse monitor.
//!
//! - Cada janela só é corrigida nos primeiros segundos depois de aparecer (e no máximo algumas vezes):
//!   se o jogador arrastá-la para outro monitor depois, ela fica onde ele deixou.
//! - Com um só monitor, ou se o NEXSTAGE já está no monitor da janela, nada é feito.
//! - Jogos em tela cheia exclusiva que escolhem o monitor por conta própria (configuração do jogo)
//!   podem ignorar a mudança.

#[cfg(windows)]
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// Quais processos pertencem ao jogo.
pub enum Track {
    /// Processo iniciado pelo NEXSTAGE e todos os que ele abrir (lançadores que abrem o jogo e fecham).
    Tree(u32),
    /// Processos cujo executável está numa destas pastas (minúsculas, com `\` final): Steam / Microsoft Store.
    Roots(Vec<String>),
}

/// Pastas para `Track::Roots` a partir de uma pasta de instalação.
pub fn roots_of(dir: &std::path::Path) -> Vec<String> {
    let mut v = vec![dir.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(dir) {
        v.push(real);
    }
    let mut out: Vec<String> = v
        .iter()
        .map(|p| {
            let s = p.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
            if s.ends_with('\\') { s } else { format!("{s}\\") }
        })
        .collect();
    out.dedup();
    out
}

/// Monitor em que a janela principal do NEXSTAGE está agora. Chamar ANTES de minimizá-la (minimizada,
/// o Windows ainda informa o monitor da posição restaurada, mas fica mais claro assim).
#[cfg(windows)]
fn app_monitor(app: &AppHandle) -> Option<win::Hmon> {
    use tauri::Manager;
    let w = app.get_webview_window("main")?;
    let h = w.hwnd().ok()?;
    win::monitor_of(h.0 as win::Hwnd)
}

/// Começa a vigiar as janelas do jogo e levá-las ao monitor do NEXSTAGE.
#[cfg(windows)]
pub fn follow(app: &AppHandle, track: Track) {
    if win::monitor_count() < 2 {
        return;
    }
    let Some(target) = app_monitor(app) else { return };
    let target = target as usize; // HMONITOR é só um identificador: pode cruzar threads
    std::thread::spawn(move || run(target as win::Hmon, track));
}

#[cfg(not(windows))]
pub fn follow(_app: &AppHandle, _track: Track) {}

/// Programa já aberto a partir deste executável? Traz a janela principal dele para a frente (restaurando
/// se estiver minimizada) e devolve true. Sem processo/janela, false (aí quem chamou abre o programa).
#[cfg(windows)]
pub fn bring_exe_to_front(exe: &std::path::Path) -> bool {
    let norm = |p: &std::path::Path| p.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    let want = norm(exe);
    let real = std::fs::canonicalize(exe).map(|p| norm(&p)).unwrap_or_default();
    let pids: HashSet<u32> = crate::msstore::process_paths()
        .into_iter()
        .filter(|(_, p)| *p == want || (!real.is_empty() && *p == real))
        .map(|(pid, _)| pid)
        .collect();
    if pids.is_empty() {
        return false;
    }
    match win::app_windows(&pids).first() {
        Some(&h) => {
            crate::overlay::force_window_foreground(h);
            true
        }
        // processo aberto mas sem janela (ainda carregando ou só na bandeja): não abre outra cópia
        None => true,
    }
}
#[cfg(not(windows))]
pub fn bring_exe_to_front(_exe: &std::path::Path) -> bool { false }

#[cfg(windows)]
fn run(target: win::Hmon, track: Track) {
    /// Tempo total de vigia (lançadores podem demorar a abrir o jogo de verdade).
    const WATCH: Duration = Duration::from_secs(180);
    /// Janela de correção depois que uma janela aparece (o jogo pode recentralizar no principal logo depois).
    const SETTLE: Duration = Duration::from_secs(6);
    const MAX_MOVES: u8 = 3;

    let start = Instant::now();
    let mut tree: HashSet<u32> = HashSet::new();
    if let Track::Tree(pid) = &track {
        tree.insert(*pid);
    }
    // janela → (quando apareceu, quantas vezes foi movida)
    let mut seen: HashMap<usize, (Instant, u8)> = HashMap::new();
    let mut empty_since: Option<Instant> = None;

    while start.elapsed() < WATCH {
        std::thread::sleep(Duration::from_millis(250));
        let pids: HashSet<u32> = match &track {
            Track::Tree(_) => {
                win::extend_tree(&mut tree);
                tree.clone()
            }
            Track::Roots(r) => crate::msstore::pids_under(r).into_iter().collect(),
        };
        // processo do jogo encerrado (e nenhum filho): para de vigiar
        if let Track::Tree(_) = &track {
            if !pids.iter().any(|&p| win::alive(p)) {
                let t = *empty_since.get_or_insert_with(Instant::now);
                if t.elapsed() > Duration::from_secs(5) {
                    return;
                }
                continue;
            }
            empty_since = None;
        }
        if pids.is_empty() {
            continue;
        }
        for h in win::top_windows(&pids) {
            let (since, moves) = seen.entry(h as usize).or_insert((Instant::now(), 0));
            if *moves >= MAX_MOVES || since.elapsed() > SETTLE {
                continue;
            }
            if win::move_to_monitor(h, target) {
                *moves += 1;
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use std::collections::HashSet;
    use std::ffi::c_void;
    pub type Hwnd = *mut c_void;
    pub type Hmon = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect { left: i32, top: i32, right: i32, bottom: i32 }
    #[repr(C)]
    struct MonitorInfo { size: u32, monitor: Rect, work: Rect, flags: u32 }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(Hwnd, isize) -> i32, l: isize) -> i32;
        fn GetWindowThreadProcessId(h: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(h: Hwnd) -> i32;
        fn IsIconic(h: Hwnd) -> i32;
        fn IsZoomed(h: Hwnd) -> i32;
        fn GetWindow(h: Hwnd, cmd: u32) -> Hwnd;
        fn GetWindowLongW(h: Hwnd, idx: i32) -> i32;
        fn GetWindowRect(h: Hwnd, r: *mut Rect) -> i32;
        fn MonitorFromWindow(h: Hwnd, flags: u32) -> Hmon;
        fn GetMonitorInfoW(m: Hmon, mi: *mut MonitorInfo) -> i32;
        fn SetWindowPos(h: Hwnd, after: isize, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn ShowWindowAsync(h: Hwnd, cmd: i32) -> i32;
        fn GetSystemMetrics(i: i32) -> i32;
    }

    #[repr(C)]
    struct Entry { size: u32, usage: u32, pid: u32, heap: usize, module: u32, threads: u32, parent: u32, pri: i32, flags: u32, exe: [u16; 260] }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snap: isize, e: *mut Entry) -> i32;
        fn Process32NextW(snap: isize, e: *mut Entry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetExitCodeProcess(h: isize, code: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }

    pub fn monitor_count() -> i32 {
        unsafe { GetSystemMetrics(80) } // SM_CMONITORS
    }

    pub fn monitor_of(h: Hwnd) -> Option<Hmon> {
        let m = unsafe { MonitorFromWindow(h, 2) }; // MONITOR_DEFAULTTONEAREST
        (!m.is_null()).then_some(m)
    }

    fn info(m: Hmon) -> Option<MonitorInfo> {
        let mut mi = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, monitor: Rect::default(), work: Rect::default(), flags: 0 };
        (unsafe { GetMonitorInfoW(m, &mut mi) } != 0).then_some(mi)
    }

    /// Acrescenta ao conjunto todos os descendentes (filhos, netos...) dos processos já conhecidos.
    pub fn extend_tree(set: &mut HashSet<u32>) {
        let snap = unsafe { CreateToolhelp32Snapshot(0x2, 0) };
        if snap == -1 {
            return;
        }
        let mut pairs = Vec::new();
        let mut e = Entry { size: std::mem::size_of::<Entry>() as u32, usage: 0, pid: 0, heap: 0, module: 0, threads: 0, parent: 0, pri: 0, flags: 0, exe: [0; 260] };
        let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
        while ok {
            pairs.push((e.pid, e.parent));
            ok = unsafe { Process32NextW(snap, &mut e) } != 0;
        }
        unsafe { CloseHandle(snap) };
        loop {
            let before = set.len();
            for &(pid, parent) in &pairs {
                if pid > 4 && set.contains(&parent) {
                    set.insert(pid);
                }
            }
            if set.len() == before {
                break;
            }
        }
    }

    pub fn alive(pid: u32) -> bool {
        unsafe {
            let h = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
            if h == 0 {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0 && code == 259; // STILL_ACTIVE
            CloseHandle(h);
            ok
        }
    }

    struct Ctx<'a> { pids: &'a HashSet<u32>, out: Vec<Hwnd>, minimized: bool }

    unsafe extern "system" fn collect(h: Hwnd, l: isize) -> i32 {
        let ctx = &mut *(l as *mut Ctx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if !ctx.pids.contains(&pid) || IsWindowVisible(h) == 0 || (!ctx.minimized && IsIconic(h) != 0) || !GetWindow(h, 4).is_null() {
            return 1; // GW_OWNER: diálogos/janelas auxiliares seguem a principal
        }
        const WS_EX_TOOLWINDOW: i32 = 0x80;
        if GetWindowLongW(h, -20) & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let mut r = Rect::default();
        if GetWindowRect(h, &mut r) == 0 || r.right - r.left < 120 || r.bottom - r.top < 80 {
            return 1;
        }
        ctx.out.push(h);
        1
    }

    /// Janelas principais visíveis dos processos.
    pub fn top_windows(pids: &HashSet<u32>) -> Vec<Hwnd> {
        let mut ctx = Ctx { pids, out: Vec::new(), minimized: false };
        unsafe { EnumWindows(collect, &mut ctx as *mut Ctx as isize) };
        ctx.out
    }

    /// Janelas principais dos processos, inclusive minimizadas.
    pub fn app_windows(pids: &HashSet<u32>) -> Vec<Hwnd> {
        let mut ctx = Ctx { pids, out: Vec::new(), minimized: true };
        unsafe { EnumWindows(collect, &mut ctx as *mut Ctx as isize) };
        ctx.out
    }

    /// Leva a janela para o monitor `target`, se estiver em outro. Devolve true se moveu.
    pub fn move_to_monitor(h: Hwnd, target: Hmon) -> bool {
        let Some(src) = monitor_of(h) else { return false };
        if src == target {
            return false;
        }
        let (Some(s), Some(t)) = (info(src), info(target)) else { return false };
        let mut r = Rect::default();
        if unsafe { GetWindowRect(h, &mut r) } == 0 {
            return false;
        }
        let maximized = unsafe { IsZoomed(h) } != 0;
        let covers = r.left <= s.monitor.left && r.top <= s.monitor.top && r.right >= s.monitor.right && r.bottom >= s.monitor.bottom;
        let (x, y, cx, cy) = if covers && !maximized {
            // sem borda / tela cheia em janela: ocupa o monitor de destino inteiro
            let m = t.monitor;
            (m.left, m.top, m.right - m.left, m.bottom - m.top)
        } else {
            let w = t.work;
            let (ww, wh) = (w.right - w.left, w.bottom - w.top);
            let (cx, cy) = ((r.right - r.left).min(ww), (r.bottom - r.top).min(wh));
            (w.left + (ww - cx) / 2, w.top + (wh - cy) / 2, cx, cy)
        };
        const SWP_NOZORDER: u32 = 0x4;
        const SWP_NOACTIVATE: u32 = 0x10;
        const SWP_NOOWNERZORDER: u32 = 0x200;
        const SWP_ASYNCWINDOWPOS: u32 = 0x4000; // não trava se o jogo estiver ocupado
        unsafe {
            if maximized {
                ShowWindowAsync(h, 9); // SW_RESTORE
            }
            SetWindowPos(h, 0, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS);
            if maximized {
                ShowWindowAsync(h, 3); // SW_MAXIMIZE
            }
        }
        true
    }
}
