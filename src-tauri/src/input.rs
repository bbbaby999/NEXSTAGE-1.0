//! Leitura NATIVA dos controles (XInput para Xbox, HID para DualSense/DualShock 4).
//!
//! Dois usos:
//!  1. Navegação: eventos `pad-input`, só com a janela do NEXSTAGE em foco (apertar botões dentro de um
//!     jogo não navega pelo launcher). 1 movimento por toque, atraso de 380 ms e repetição a cada 95 ms.
//!     Analógico direito (vertical e horizontal): eventos `pad-scroll` com a inclinação já tratada pela zona morta
//!     (-1 = cima, 1 = baixo). A interface converte isso em rolagem contínua.
//!  2. Combinações de botões para abrir o overlay e capturar a tela. Estas funcionam MESMO COM O JOGO EM
//!     FOCO: são detectadas aqui (não na interface) e exigem SEGURAR a combinação, para não conflitar
//!     com os comandos do jogo.
//!
//! Configuração individual de cada controle (Central de Controles → `PadCfg`): zona morta, mapeamento
//! dos comandos da interface e vibração de resposta. REGRA DE ISOLAMENTO: tudo isso só muda como o
//! NEXSTAGE interpreta o controle para navegar. Nada é gravado no controle, no driver ou no Windows —
//! jogos e emuladores continuam recebendo o controle exatamente como o sistema o entrega. As
//! combinações de overlay/captura usam sempre os botões físicos (sem o mapeamento da interface).

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::overlay::{self, Runtime};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// Máscara unificada de botões (Xbox / PlayStation).
pub mod btn {
    pub const A: u16 = 1 << 0; // ✕
    pub const B: u16 = 1 << 1; // ○
    pub const X: u16 = 1 << 2; // □
    pub const Y: u16 = 1 << 3; // △
    pub const LB: u16 = 1 << 4; // L1
    pub const RB: u16 = 1 << 5; // R1
    pub const LT: u16 = 1 << 6; // L2
    pub const RT: u16 = 1 << 7; // R2
    pub const SELECT: u16 = 1 << 8; // View / Create
    pub const START: u16 = 1 << 9; // Menu / Options
    pub const L3: u16 = 1 << 10;
    pub const R3: u16 = 1 << 11;
    pub const UP: u16 = 1 << 12;
    pub const DOWN: u16 = 1 << 13;
    pub const LEFT: u16 = 1 << 14;
    pub const RIGHT: u16 = 1 << 15;
}

pub const DEFAULT_OVERLAY_COMBO: u16 = btn::SELECT | btn::START;
pub const DEFAULT_SHOT_COMBO: u16 = btn::SELECT | btn::RB;
/// Trazer o NEXSTAGE para frente em tela cheia (par com a captura: Select + LB).
pub const DEFAULT_SUMMON_COMBO: u16 = btn::SELECT | btn::LB;
const HOLD_OVERLAY: Duration = Duration::from_millis(700);
const HOLD_SHOT: Duration = Duration::from_millis(350);

/// Regra anti-conflito com os jogos: 2 a 3 botões e ao menos um "de menu" (Select, Start, L3 ou R3).
/// Além disso o app exige segurar a combinação (0,7 s para o overlay, 0,35 s para a captura).
pub fn validate_combo(mask: u16) -> Result<(), &'static str> {
    let n = mask.count_ones();
    if !(2..=3).contains(&n) {
        return Err("Use de 2 a 3 botões ao mesmo tempo.");
    }
    if mask & (btn::SELECT | btn::START | btn::L3 | btn::R3) == 0 {
        return Err("Inclua Select, Start, L3 ou R3 na combinação: assim ela não atrapalha os comandos do jogo.");
    }
    Ok(())
}

/// Combinações que se contêm (uma é parte da outra) disparariam juntas.
pub fn combos_conflict(a: u16, b: u16) -> bool {
    a != 0 && b != 0 && ((a & b) == a || (a & b) == b)
}

#[derive(Default, Clone)]
pub struct RawPad {
    /// esquerda, direita, cima, baixo (D-pad ou analógico)
    pub dirs: [bool; 4],
    /// Botões (máscara `btn::*`); as ações de navegação saem daqui.
    pub mask: u16,
    /// Analógico direito, eixo vertical normalizado (-1 = cima, 1 = baixo), sem zona morta.
    pub ry: f32,
    /// Analógico esquerdo (-1..1; y positivo = para baixo) e direito horizontal, sem zona morta.
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    /// Gatilhos analógicos (0..1).
    pub lt: f32,
    pub rt: f32,
    /// Botões extras só para o testador: bit 0 = PS/Home, bit 1 = clique do touchpad (Sony).
    pub extra: u8,
}

impl RawPad {
    /// Direções de navegação: D-pad OU analógico esquerdo além do limiar (metade do curso útil
    /// depois da zona morta `dz` deste controle).
    pub fn compute_dirs(&mut self, dz: f32) {
        let th = nav_threshold(dz);
        let m = self.mask;
        self.dirs = [
            m & btn::LEFT != 0 || self.lx < -th,
            m & btn::RIGHT != 0 || self.lx > th,
            m & btn::UP != 0 || self.ly < -th,
            m & btn::DOWN != 0 || self.ly > th,
        ];
    }
}

/// Zona morta do analógico direito (fração do curso). Abaixo disso nada rola.
const SCROLL_DEADZONE: f32 = 0.24;
/// Zona morta padrão do analógico esquerdo (limiar de navegação ≈ 67% do curso, como antes).
pub const NAV_DEADZONE: f32 = 0.34;
/// Intervalo mínimo entre eventos `pad-scroll` com a alavanca em movimento (~60 Hz).
const SCROLL_EMIT_EVERY: Duration = Duration::from_millis(16);

/// Limiar do analógico esquerdo para mover o foco: metade do curso que sobra depois da zona morta.
pub fn nav_threshold(dz: f32) -> f32 {
    let dz = dz.clamp(0.0, 0.9);
    dz + (1.0 - dz) * 0.5
}

/// Aplica a zona morta e reescala o restante para 0..1, sem "degrau" na saída da zona.
#[cfg(test)]
pub fn scroll_axis(v: f32) -> f32 {
    scroll_axis_dz(v, SCROLL_DEADZONE)
}

/// `scroll_axis` com a zona morta do controle (configuração individual).
pub fn scroll_axis_dz(v: f32, dz: f32) -> f32 {
    let dz = dz.clamp(0.0, 0.9);
    let a = v.abs().min(1.0);
    if a <= dz {
        return 0.0;
    }
    let r = (a - dz) / (1.0 - dz);
    // quantiza (1/50) para não inundar a interface com variações mínimas
    ((r * 50.0).round() / 50.0).copysign(v)
}

// ---------- configuração individual (Central de Controles) ----------

/// Ordem das ações da interface no mapeamento (`PadCfg::map`).
pub const ACTIONS: [&str; 9] = ["confirm", "back", "x", "y", "l1", "r1", "l2", "r2", "menu"];
/// Mapeamento padrão: mesma posição física no Xbox e no PlayStation (A/✕ confirma, B/○ volta).
pub const DEFAULT_MAP: [u16; 9] = [btn::A, btn::B, btn::X, btn::Y, btn::LB, btn::RB, btn::LT, btn::RT, btn::START];
/// Botões que podem receber uma ação da interface (o direcional fica reservado para mover o foco).
const MAPPABLE: u16 = 0x0FFF;

/// Ajustes de UM controle, válidos só dentro do NEXSTAGE (ver o topo do arquivo).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PadCfg {
    /// Zona morta do analógico esquerdo (navegação) e do direito (rolagem), fração do curso.
    pub dz_left: f32,
    pub dz_right: f32,
    /// Botão (`btn::*`) de cada ação, na ordem de `ACTIONS`.
    pub map: [u16; 9],
    /// Intensidade da vibração de resposta (0 = desligada, 100 = máxima).
    pub rumble: u8,
    /// Vibrar levemente ao confirmar/voltar na interface.
    pub rumble_nav: bool,
    /// Barra de luz (Sony): `LED_GLOBAL` (opção geral), `LED_THEME` (cor de destaque), `LED_KEEP`
    /// (não mexer) ou `1 << 24 | 0xRRGGBB` (cor fixa deste controle).
    pub led: u32,
}

pub const LED_GLOBAL: u32 = 0;
pub const LED_KEEP: u32 = 1 << 25;
pub const LED_THEME: u32 = 1 << 26;
/// Cor de destaque do perfil (`1 << 24 | rgb`, 0 = ainda não informada), para `LED_THEME`.
static THEME_LED: AtomicU32 = AtomicU32::new(0);

/// Cor que um controle Sony deve mostrar (`None` = não mexer na iluminação).
pub fn led_want(cfg_led: u32, global: u32, theme: u32) -> Option<(u8, u8, u8)> {
    let v = match cfg_led {
        LED_GLOBAL => global,
        LED_THEME => theme,
        LED_KEEP => 0,
        c => c,
    };
    (v >> 24 == 1).then(|| ((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Default for PadCfg {
    fn default() -> Self {
        Self { dz_left: NAV_DEADZONE, dz_right: SCROLL_DEADZONE, map: DEFAULT_MAP, rumble: 0, rumble_nav: false, led: LED_GLOBAL }
    }
}

impl PadCfg {
    /// Garante valores seguros: zonas mortas limitadas e mapeamento com 1 botão por ação, sem repetir.
    pub fn sanitized(mut self) -> Self {
        let ok = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        self.dz_left = ok(self.dz_left, 0.05, 0.8, NAV_DEADZONE);
        self.dz_right = ok(self.dz_right, 0.05, 0.8, SCROLL_DEADZONE);
        self.rumble = self.rumble.min(100);
        if !matches!(self.led, LED_GLOBAL | LED_KEEP | LED_THEME) && self.led >> 24 != 1 {
            self.led = LED_GLOBAL;
        }
        let mut seen = 0u16;
        let valid = self.map.iter().all(|&b| {
            let fine = b.count_ones() == 1 && b & MAPPABLE == b && seen & b == 0;
            seen |= b;
            fine
        });
        if !valid {
            self.map = DEFAULT_MAP;
        }
        self
    }
}

/// Ajustes por dispositivo ("xbox:<id>" | "ps:<id>" | "ns:<id>"; ver `devident`), enviados pela interface. Sem entrada = padrão.
static PAD_CFG: Mutex<Option<HashMap<String, PadCfg>>> = Mutex::new(None);
static PAD_CFG_GEN: AtomicU32 = AtomicU32::new(0);
/// Modo "testador": a leitura envia `pad-state` (estado bruto) e pausa navegação e combinações.
static MONITOR: AtomicBool = AtomicBool::new(false);
/// Teclado virtual aberto: segurar confirmar, X, Y, L1 ou R1 repete o comando (digitar, apagar, espaço,
/// mover o cursor), como o direcional. Fora dele os botões disparam uma vez por toque.
static BUTTON_REPEAT: AtomicBool = AtomicBool::new(false);

/// Ações que repetem ao segurar com `BUTTON_REPEAT` (índices de `ACTIONS`): confirmar, X, Y, L1, R1.
/// Voltar, L2/R2 (alternam maiúsculas/símbolos) e Start (concluir) nunca repetem.
const REPEATABLE: [usize; 5] = [0, 2, 3, 4, 5];

struct RumbleReq {
    key: String,
    strength: u8,
    ms: u16,
}
/// Pedidos de vibração da interface (aplicados pela thread de leitura, dona do controle Sony aberto).
static RUMBLE: Mutex<Vec<RumbleReq>> = Mutex::new(Vec::new());

fn push_rumble(key: &str, strength: u8, ms: u16) {
    if strength == 0 || ms == 0 {
        return;
    }
    if let Ok(mut q) = RUMBLE.lock() {
        q.push(RumbleReq { key: key.into(), strength: strength.min(100), ms: ms.min(2000) });
    }
}

fn cfg_snapshot() -> HashMap<String, PadCfg> {
    PAD_CFG.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// Estado bruto de um controle para o testador e o diagnóstico da Central de Controles.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PadState {
    key: String,
    /// `btn::*` nos 16 bits baixos; bit 16 = PS/Home e bit 17 = touchpad (mesmos índices da Gamepad API).
    mask: u32,
    lx: f32,
    ly: f32,
    rx: f32,
    ry: f32,
    lt: f32,
    rt: f32,
    /// Relatórios recebidos por segundo (Sony) ou leituras por segundo (XInput).
    hz: f32,
}

fn state_of(key: String, r: &RawPad, hz: f32) -> PadState {
    let q = |v: f32| (v * 1000.0).round() / 1000.0;
    PadState { key, mask: r.mask as u32 | (r.extra as u32) << 16, lx: q(r.lx), ly: q(r.ly), rx: q(r.rx), ry: q(r.ry), lt: q(r.lt), rt: q(r.rt), hz }
}

#[derive(Default)]
pub struct Tracker {
    held: [bool; 4],
    next: [Option<Instant>; 4],
    prev: u16,
    /// Próxima repetição de cada ação (com `BUTTON_REPEAT`).
    bnext: [Option<Instant>; 9],
    /// Confirmar segurado: quando vira "confirm_hold" (uma vez por toque) — ver `HOLD_CONFIRM`.
    hold_at: Option<Instant>,
}

/// Segurar "confirmar" por este tempo dispara `confirm_hold` (ex.: abrir a seleção de vários no Zerei!).
/// Soltar dispara `confirm_up`: a interface decide entre clique e "segurar" nos itens que aceitam os dois.
const HOLD_CONFIRM: Duration = Duration::from_millis(500);

const DIRS: [&str; 4] = ["left", "right", "up", "down"];
const REPEAT_DELAY: Duration = Duration::from_millis(380);
const REPEAT_RATE: Duration = Duration::from_millis(95);

impl Tracker {
    /// Mapeamento padrão (A/✕ confirma, B/○ volta...).
    #[cfg(test)]
    pub fn update(&mut self, raw: &RawPad, now: Instant) -> Vec<&'static str> {
        self.update_mapped(raw, &DEFAULT_MAP, now)
    }

    /// `map[i]` = botão da ação `ACTIONS[i]` (configuração individual do controle).
    pub fn update_mapped(&mut self, raw: &RawPad, map: &[u16; 9], now: Instant) -> Vec<&'static str> {
        self.update_with(raw, map, now, BUTTON_REPEAT.load(Ordering::Relaxed))
    }

    /// `repeat`: botões repetíveis (`REPEATABLE`) disparam de novo enquanto seguros, no ritmo do direcional.
    pub fn update_with(&mut self, raw: &RawPad, map: &[u16; 9], now: Instant, repeat: bool) -> Vec<&'static str> {
        let mut out = Vec::new();
        for i in 0..4 {
            if raw.dirs[i] {
                if !self.held[i] {
                    out.push(DIRS[i]);
                    self.next[i] = Some(now + REPEAT_DELAY);
                } else if let Some(t) = self.next[i] {
                    if now >= t {
                        out.push(DIRS[i]);
                        self.next[i] = Some(now + REPEAT_RATE);
                    }
                }
            } else {
                self.next[i] = None;
            }
            self.held[i] = raw.dirs[i];
        }
        for (i, (bit, action)) in map.iter().zip(ACTIONS).enumerate() {
            let down = raw.mask & bit != 0;
            // confirmar (fora do teclado virtual, que repete): segurou → "confirm_hold"; soltou → "confirm_up"
            if i == 0 && !repeat {
                if down && self.prev & bit != 0 && self.hold_at.is_some_and(|t| now >= t) {
                    out.push("confirm_hold");
                    self.hold_at = None;
                } else if !down && self.prev & bit != 0 {
                    out.push("confirm_up");
                    self.hold_at = None;
                }
            }
            if down && self.prev & bit == 0 {
                out.push(action);
                self.bnext[i] = Some(now + REPEAT_DELAY);
                if i == 0 {
                    self.hold_at = Some(now + HOLD_CONFIRM);
                }
            } else if down && repeat && REPEATABLE.contains(&i) && self.bnext[i].is_some_and(|t| now >= t) {
                out.push(action);
                self.bnext[i] = Some(now + REPEAT_RATE);
            } else if !down {
                self.bnext[i] = None;
            }
        }
        self.prev = raw.mask;
        out
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Detecta "segurar a combinação": dispara uma vez e só rearma depois de soltar.
#[derive(Default)]
pub struct Combos {
    since: [Option<Instant>; 3],
    fired: [bool; 3],
}

impl Combos {
    /// `combos`: overlay, captura, trazer para frente.
    pub fn update(&mut self, mask: u16, combos: [u16; 3], now: Instant) -> [bool; 3] {
        let holds = [HOLD_OVERLAY, HOLD_SHOT, HOLD_OVERLAY];
        let mut fire = [false; 3];
        for i in 0..3 {
            let on = combos[i] != 0 && (mask & combos[i]) == combos[i];
            if on {
                let s = *self.since[i].get_or_insert(now);
                if !self.fired[i] && now.duration_since(s) >= holds[i] {
                    self.fired[i] = true;
                    fire[i] = true;
                }
            } else {
                self.since[i] = None;
                self.fired[i] = false;
            }
        }
        fire
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Serialize)]
struct ScrollEvent {
    /// Vertical (negativo = cima): rola a página.
    value: f32,
    /// Horizontal (negativo = esquerda): rola os carrosséis.
    x: f32,
    device: &'static str,
}

#[derive(Clone, Serialize)]
struct PadEvent {
    action: &'static str,
    device: &'static str,
    /// Dispositivo que gerou o comando ("xbox:<id>" | "ps:<id>" | "ns:<id>"; ver `devident`): a interface usa para os glifos do
    /// controle em uso (padrão de confirmação e estilo de ícones individuais).
    key: String,
}

#[cfg(windows)]
fn read_xinput(i: u32) -> Option<RawPad> {
    use crate::commands::controllers::xinput::*;
    let mut st = State { packet: 0, gamepad: Gamepad { buttons: 0, lt: 0, rt: 0, lx: 0, ly: 0, rx: 0, ry: 0 } };
    if unsafe { XInputGetState(i, &mut st) } != 0 {
        return None;
    }
    let b = st.gamepad.buttons;
    let n = |v: i16| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    // XInput: positivo = para cima; aqui positivo = para baixo.
    let ry = -n(st.gamepad.ry);
    let mut m = 0u16;
    for (bit, v) in [
        (0x1000, btn::A), (0x2000, btn::B), (0x4000, btn::X), (0x8000, btn::Y), (0x0100, btn::LB), (0x0200, btn::RB),
        (0x0020, btn::SELECT), (0x0010, btn::START), (0x0040, btn::L3), (0x0080, btn::R3),
        (0x0001, btn::UP), (0x0002, btn::DOWN), (0x0004, btn::LEFT), (0x0008, btn::RIGHT),
    ] {
        if b & bit != 0 {
            m |= v;
        }
    }
    if st.gamepad.lt > 100 { m |= btn::LT; }
    if st.gamepad.rt > 100 { m |= btn::RT; }
    let mut raw = RawPad {
        mask: m,
        ry,
        lx: n(st.gamepad.lx),
        ly: -n(st.gamepad.ly),
        rx: n(st.gamepad.rx),
        lt: st.gamepad.lt as f32 / 255.0,
        rt: st.gamepad.rt as f32 / 255.0,
        ..Default::default()
    };
    raw.compute_dirs(NAV_DEADZONE);
    Some(raw)
}
#[cfg(not(windows))]
fn read_xinput(_i: u32) -> Option<RawPad> {
    None
}

/// DualSense / DualShock 4. Layouts: USB 0x01 (64 bytes), Bluetooth completo 0x31 (dados a partir
/// do byte 2), Bluetooth simples / DS4 0x01 (botões nos bytes 5 e 6) e DS4 Bluetooth completo 0x11
/// (o mesmo do 0x01 deslocado 2 bytes; o DS4 passa a enviá-lo depois de receber a cor da barra de luz).
/// `bt`: lido por Bluetooth. No Windows o relatório SIMPLES do Bluetooth (0x01, 10 bytes úteis) chega
/// preenchido com zeros até o tamanho do maior relatório (78 bytes no DualSense) — pelo tamanho ele se
/// confundia com o 0x01 COMPLETO do USB, e o direcional era lido no byte do gatilho L2 (0 em repouso =
/// "cima" preso, o foco subia sozinho).
pub(crate) fn parse_hid(buf: &[u8], n: usize, ds4: bool, bt: bool) -> Option<RawPad> {
    let (sx, sy, b0, b1) = if buf[0] == 0x31 && n >= 78 && !ds4 {
        (2, 3, 9, 10)
    } else if buf[0] == 0x11 && n >= 12 && ds4 {
        (3, 4, 7, 8)
    } else if buf[0] == 0x01 && n >= 64 && !ds4 && !bt {
        (1, 2, 8, 9)
    } else if buf[0] == 0x01 && n >= 10 {
        (1, 2, 5, 6)
    } else {
        return None;
    };
    if n <= b1 {
        return None;
    }
    let axis = |b: u8| ((b as f32 - 127.5) / 127.5).clamp(-1.0, 1.0);
    // analógico direito logo após o esquerdo (rx, ry); 0 = cima, 255 = baixo
    let ry = axis(buf[sy + 2]);
    // Gatilhos analógicos: DualSense completo logo após os analógicos; DS4 e relatório simples depois
    // dos botões (mesmos deslocamentos do hid-playstation).
    let (ti, tj) = if b0 == sx + 7 { (sx + 4, sx + 5) } else { (b1 + 2, b1 + 3) };
    let trig = |i: usize| if i < n { buf[i] as f32 / 255.0 } else { 0.0 };
    let (f, g) = (buf[b0], buf[b1]);
    let hat = f & 0x0F;
    let mut m = 0u16;
    for (bit, v) in [(0x20, btn::A), (0x40, btn::B), (0x10, btn::X), (0x80, btn::Y)] {
        if f & bit != 0 { m |= v; }
    }
    for (bit, v) in [
        (0x01, btn::LB), (0x02, btn::RB), (0x04, btn::LT), (0x08, btn::RT),
        (0x10, btn::SELECT), (0x20, btn::START), (0x40, btn::L3), (0x80, btn::R3),
    ] {
        if g & bit != 0 { m |= v; }
    }
    if matches!(hat, 7 | 0 | 1) { m |= btn::UP; }
    if matches!(hat, 3 | 4 | 5) { m |= btn::DOWN; }
    if matches!(hat, 5 | 6 | 7) { m |= btn::LEFT; }
    if matches!(hat, 1 | 2 | 3) { m |= btn::RIGHT; }
    let mut raw = RawPad {
        mask: m,
        ry,
        lx: axis(buf[sx]),
        ly: axis(buf[sy]),
        rx: axis(buf[sy + 1]),
        lt: trig(ti),
        rt: trig(tj),
        // byte seguinte aos botões: bit 0 = PS, bit 1 = clique do touchpad (DualSense e DS4)
        extra: if b1 + 1 < n { buf[b1 + 1] & 0x03 } else { 0 },
        ..Default::default()
    };
    raw.compute_dirs(NAV_DEADZONE);
    Some(raw)
}

/// Bateria de um controle Sony, lida dos mesmos relatórios usados para os botões.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SonyBattery {
    /// `None` = o controle informou erro de bateria (tensão/temperatura).
    pub percent: Option<u8>,
    pub charging: bool,
}

/// Bateria nos relatórios completos (mesmos deslocamentos do driver hid-playstation do Linux).
/// Relatórios simples do Bluetooth (10 bytes) não trazem bateria: `None`.
///  - DualSense: byte de status = nível (0–10) no nibble baixo; estado no alto (0 descarregando,
///    1 carregando, 2 carga completa, outros = erro). USB 0x01 → byte 53; Bluetooth 0x31 → byte 54.
///  - DualShock 4: nível no nibble baixo, bit 4 = cabo conectado (com cabo: 0–9 carregando, 10/11 cheia).
///    USB 0x01 → byte 30; Bluetooth 0x11 → byte 32.
/// `bt`: por Bluetooth o 0x01 é o relatório simples (sem bateria; o resto é preenchimento com zeros).
pub fn parse_sony_battery(buf: &[u8], n: usize, ds4: bool, bt: bool) -> Option<SonyBattery> {
    let pct = |level: u8| Some((level as u16 * 10 + 5).min(100) as u8);
    if ds4 {
        let i = match buf.first()? { 0x11 if n >= 33 => 32, 0x01 if n >= 31 && !bt => 30, _ => return None };
        let s = *buf.get(i)?;
        let level = s & 0x0F;
        Some(if s & 0x10 != 0 {
            match level {
                0..=9 => SonyBattery { percent: pct(level), charging: true },
                10 | 11 => SonyBattery { percent: Some(100), charging: false },
                _ => SonyBattery { percent: None, charging: false },
            }
        } else {
            SonyBattery { percent: pct(level), charging: false }
        })
    } else {
        let i = match buf.first()? { 0x31 if n >= 55 => 54, 0x01 if n >= 54 && !bt => 53, _ => return None };
        let s = *buf.get(i)?;
        let level = s & 0x0F;
        Some(match s >> 4 {
            0 => SonyBattery { percent: pct(level), charging: false },
            1 => SonyBattery { percent: pct(level), charging: true },
            2 => SonyBattery { percent: Some(100), charging: false },
            _ => SonyBattery { percent: None, charging: false },
        })
    }
}

/// O NEXSTAGE está em primeiro plano? (Só então os controles navegam pela interface.)
///
/// Não usa `WebviewWindow::is_focused()`: no Windows ele exige que a janela de nível superior tenha
/// o foco do teclado, mas o wry repassa esse foco ao WebView2 (janela filha) a cada ativação — então
/// ele fica `false` com o app na frente, e o controle "morria" até algum clique reacertar o estado.
/// A pergunta certa é ao Windows: qual é a janela em primeiro plano?
#[cfg(windows)]
fn window_focused(app: &AppHandle) -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> *mut std::ffi::c_void;
    }
    let Some(w) = app.get_webview_window("main") else { return false };
    let Ok(h) = w.hwnd() else { return false };
    let fg = unsafe { GetForegroundWindow() };
    !fg.is_null() && fg == h.0 as *mut std::ffi::c_void
}
#[cfg(not(windows))]
fn window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false)
}

/// Janela em que o mesmo comando vindo de um controle físico e de um virtual conta como um só.
const MIRROR_WINDOW: Duration = Duration::from_millis(150);

/// O comando é o eco de outro dispositivo? Só quando um dos dois é virtual (DS4Windows/ViGEm,
/// G HUB...): dois controles físicos diferentes nunca se anulam, nem o mesmo controle repetindo.
fn is_mirrored(prev: Option<&(Instant, String, bool)>, key: &str, is_virtual: bool, now: Instant) -> bool {
    prev.is_some_and(|(t, k, v)| k != key && (*v || is_virtual) && now.duration_since(*t) < MIRROR_WINDOW)
}

fn fire(app: &AppHandle, which: usize) {
    if which == 0 {
        overlay::toggle(app);
    } else if which == 2 {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || overlay::summon(&a));
    } else {
        let a = app.clone();
        std::thread::spawn(move || {
            let _ = crate::screenshot::take(&a);
        });
    }
}

fn read_mask_setting(app: &AppHandle, key: &str, default: u16) -> u16 {
    overlay::setting(app, key)
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|m| validate_combo(*m).is_ok())
        .unwrap_or(default)
}

pub fn spawn(app: AppHandle) {
    {
        let rt = app.state::<Runtime>();
        rt.pad_overlay.store(read_mask_setting(&app, "pad_combo_overlay", DEFAULT_OVERLAY_COMBO) as u32, Ordering::SeqCst);
        rt.pad_shot.store(read_mask_setting(&app, "pad_combo_shot", DEFAULT_SHOT_COMBO) as u32, Ordering::SeqCst);
        rt.pad_summon.store(read_mask_setting(&app, "pad_combo_summon", DEFAULT_SUMMON_COMBO) as u32, Ordering::SeqCst);
    }
    std::thread::spawn(move || {
        let mut xt: [Tracker; 4] = Default::default();
        let mut xc: [Combos; 4] = Default::default();
        let mut connected = [false; 4];
        // Identidade física de cada slot do XInput (chave "xbox:<id>" e se é virtual) — ver `devident`.
        // Reconsultada quando um slot conecta/desconecta, 1,5 s depois (o Windows termina de enumerar o
        // aparelho) e a cada 2 s enquanto algum slot não foi associado.
        let mut xkeys: [(String, bool); 4] = std::array::from_fn(|i| (format!("xbox:{i}"), false));
        let mut next_ident: Option<Instant> = None;
        // Ações recentes (ação → instante, dispositivo, virtual?): um controle físico e o virtual que o
        // espelha (DS4Windows, G HUB...) não podem disparar o mesmo comando duas vezes.
        let mut recent: HashMap<&'static str, (Instant, String, bool)> = HashMap::new();
        let start = Instant::now();
        let mut probe = [start; 4];

        // controles HID (Sony e Nintendo), vários ao mesmo tempo — ver `hidpads`
        let mut hid = crate::hidpads::HidPads::new(start);

        let mut focused = false;
        let mut next_focus = start;
        let mut last_mask: u16 = 0;
        let mut last_scroll: f32 = 0.0;
        let mut last_scroll_x: f32 = 0.0;
        let mut next_scroll = start;

        // configuração individual (cópia local; recarregada quando a interface envia uma nova)
        let mut cfgs: HashMap<String, PadCfg> = HashMap::new();
        let mut cfg_gen = u32::MAX;
        // vibração em andamento: até quando os motores ficam ligados
        let mut x_rumble: [Option<Instant>; 4] = [None; 4];
        // testador: estado bruto a ~30 Hz e taxas de leitura
        let mut next_state = start;
        let mut loops: u32 = 0;
        let mut loop_hz: f32 = 0.0;
        let mut rate_since = start;

        loop {
            let now = Instant::now();
            if now >= next_focus {
                focused = window_focused(&app);
                next_focus = now + Duration::from_millis(100);
            }
            let g = PAD_CFG_GEN.load(Ordering::Relaxed);
            if g != cfg_gen {
                cfgs = cfg_snapshot();
                cfg_gen = g;
            }
            let cfg_of = |key: &str| cfgs.get(key).copied().unwrap_or_default();
            let monitor = MONITOR.load(Ordering::Relaxed);
            let (combos, capture) = {
                let rt = app.state::<Runtime>();
                ([rt.pad_overlay.load(Ordering::Relaxed) as u16, rt.pad_shot.load(Ordering::Relaxed) as u16, rt.pad_summon.load(Ordering::Relaxed) as u16], rt.capture.load(Ordering::Relaxed))
            };
            // (ação, tipo, dispositivo)
            let mut out: Vec<(&'static str, &'static str, String)> = Vec::new();
            let mut states: Vec<PadState> = Vec::new();
            let mut mask_all: u16 = 0;
            let mut fires = [false; 3];
            // analógico direito: vale o controle mais inclinado
            let mut scroll: (f32, &'static str) = (0.0, "xbox");
            // horizontal (carrosséis): também o controle mais inclinado
            let mut scroll_x: (f32, &'static str) = (0.0, "xbox");
            loops += 1;
            if now.duration_since(rate_since) >= Duration::from_secs(1) {
                let secs = now.duration_since(rate_since).as_secs_f32();
                loop_hz = (loops as f32 / secs).round();
                loops = 0;
                rate_since = now;
            }

            // pedidos de vibração (botão "Testar vibração" e resposta da interface)
            let reqs: Vec<RumbleReq> = RUMBLE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
            for r in reqs {
                let slot = (0..4).find(|&i| connected[i] && xkeys[i].0 == r.key)
                    .or_else(|| r.key.strip_prefix("xbox:").and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < 4));
                if let Some(i) = slot {
                    if connected[i] && set_xinput_rumble(i as u32, r.strength) {
                        x_rumble[i] = Some(now + Duration::from_millis(r.ms as u64));
                    }
                } else if let Some(p) = hid.pad_mut(&r.key) {
                    p.rumble(r.strength, r.ms, now);
                }
            }
            for i in 0..4 {
                if x_rumble[i].is_some_and(|t| now >= t) {
                    set_xinput_rumble(i as u32, 0);
                    x_rumble[i] = None;
                }
            }

            let was_connected = connected;
            for i in 0..4usize {
                if !connected[i] && now < probe[i] {
                    continue; // slots desconectados são consultados 1x por segundo
                }
                match read_xinput(i as u32) {
                    Some(mut raw) => {
                        connected[i] = true;
                        let key = xkeys[i].0.clone();
                        let cfg = cfg_of(&key);
                        raw.compute_dirs(cfg.dz_left);
                        mask_all |= raw.mask;
                        let v = scroll_axis_dz(raw.ry, cfg.dz_right);
                        if v.abs() > scroll.0.abs() {
                            scroll = (v, "xbox");
                        }
                        let h = scroll_axis_dz(raw.rx, cfg.dz_right);
                        if h.abs() > scroll_x.0.abs() {
                            scroll_x = (h, "xbox");
                        }
                        let f = xc[i].update(raw.mask, combos, now);
                        fires[0] |= f[0];
                        fires[1] |= f[1];
                        fires[2] |= f[2];
                        for a in xt[i].update_mapped(&raw, &cfg.map, now) {
                            out.push((a, "xbox", key.clone()));
                        }
                        if monitor {
                            states.push(state_of(key, &raw, loop_hz));
                        }
                    }
                    None => {
                        if connected[i] {
                            xt[i].reset();
                            xc[i].reset();
                            connected[i] = false;
                            x_rumble[i] = None;
                        }
                        probe[i] = now + Duration::from_millis(1000);
                    }
                }
            }
            let mut devices_changed = false;
            if connected != was_connected {
                next_ident = Some(now); // agora e de novo em 1,5 s
                devices_changed = true;
            }
            if next_ident.is_some_and(|t| now >= t) {
                let slots: Vec<u32> = (0..4u32).filter(|&i| connected[i as usize]).collect();
                let ids = crate::devident::xinput_slots_refresh(&slots);
                let mut unresolved = false;
                for i in 0..4usize {
                    let next = match ids.iter().find(|x| x.slot == i as u32) {
                        Some(x) => {
                            unresolved |= x.info.is_none();
                            (x.key(), x.virtual_src().is_some())
                        }
                        None => (format!("xbox:{i}"), false),
                    };
                    if xkeys[i] != next {
                        xkeys[i] = next;
                        devices_changed = true;
                    }
                }
                let settle = connected != was_connected;
                next_ident = (settle || unresolved).then(|| now + Duration::from_millis(if settle { 1500 } else { 2000 }));
            }

            // Controles HID (Sony e Nintendo): procura novos a cada 2 s; avisa a interface quando a lista muda.
            devices_changed |= hid.scan(now);
            {
                let rt = app.state::<Runtime>();
                let global = if rt.pad_led_gen.load(Ordering::Relaxed) > 0 { rt.pad_led.load(Ordering::Relaxed) } else { 0 };
                let theme = THEME_LED.load(Ordering::Relaxed);
                for p in hid.pads.iter_mut() {
                    p.apply_led(led_want(cfg_of(&p.key).led, global, theme), now);
                }
            }
            if !hid.read_all(now).is_empty() {
                devices_changed = true;
            }
            // Joy-Cons: gesto L + R junta os dois; pares gravados se formam sozinhos
            let jc = hid.joycons(now);
            devices_changed |= jc.changed;
            for j in jc.joined {
                let _ = app.emit("joycon-joined", &j);
            }
            if devices_changed {
                let keys: Vec<&str> = (0..4).filter(|&i| connected[i]).map(|i| xkeys[i].0.as_str())
                    .chain(hid.input_keys())
                    .collect();
                let _ = app.emit("pad-devices", &keys);
            }
            for p in hid.inputs() {
                let cfg = cfg_of(p.key);
                p.raw.compute_dirs(cfg.dz_left);
                mask_all |= p.raw.mask;
                let v = scroll_axis_dz(p.raw.ry, cfg.dz_right);
                if v.abs() > scroll.0.abs() {
                    scroll = (v, p.kind);
                }
                let h = scroll_axis_dz(p.raw.rx, cfg.dz_right);
                if h.abs() > scroll_x.0.abs() {
                    scroll_x = (h, p.kind);
                }
                let f = p.combos.update(p.raw.mask, combos, now);
                fires[0] |= f[0];
                fires[1] |= f[1];
                fires[2] |= f[2];
                let raw = p.raw.clone();
                for a in p.tracker.update_mapped(&raw, &cfg.map, now) {
                    out.push((a, p.kind, p.key.to_string()));
                }
                if monitor {
                    states.push(state_of(p.key.to_string(), p.raw, p.hz));
                }
            }

            if monitor {
                // Testador aberto: só informa o estado bruto; nada navega nem dispara combinações.
                if now >= next_state {
                    let _ = app.emit("pad-state", &states);
                    next_state = now + Duration::from_millis(33);
                }
            } else if capture {
                // Escolhendo combinação nas configurações: só informa os botões pressionados.
                if mask_all != last_mask {
                    let _ = app.emit("pad-mask", mask_all);
                }
            } else {
                for (i, f) in fires.iter().enumerate() {
                    if *f {
                        fire(&app, i); // funciona com qualquer janela em foco, inclusive o jogo
                    }
                }
                if focused {
                    let is_virtual = |key: &str| {
                        (0..4).any(|i| connected[i] && xkeys[i].1 && xkeys[i].0 == key)
                            || hid.pads.iter().any(|p| p.virtual_src.is_some() && p.key == key)
                    };
                    for (action, device, key) in out {
                        let virt = is_virtual(&key);
                        if is_mirrored(recent.get(action), &key, virt, now) {
                            continue; // o mesmo comando acabou de chegar pelo controle "gêmeo"
                        }
                        recent.insert(action, (now, key.clone(), virt));
                        // vibração de resposta da interface (opcional, por controle; só com o NEXSTAGE em foco)
                        let cfg = cfg_of(&key);
                        if cfg.rumble_nav && matches!(action, "confirm" | "back") {
                            push_rumble(&key, cfg.rumble, 45);
                        }
                        let _ = app.emit("pad-input", PadEvent { action, device, key });
                    }
                }
            }
            // Rolagem: sem foco (ou escolhendo combinação / testando) conta como alavanca solta.
            let live = focused && !capture && !monitor;
            let value = if live { scroll.0 } else { 0.0 };
            let x = if live { scroll_x.0 } else { 0.0 };
            let device = if x.abs() > value.abs() { scroll_x.1 } else { scroll.1 };
            let changed = value != last_scroll || x != last_scroll_x;
            let released = (value == 0.0 && last_scroll != 0.0) || (x == 0.0 && last_scroll_x != 0.0);
            if changed && (released || now >= next_scroll) {
                let _ = app.emit("pad-scroll", ScrollEvent { value, x, device });
                last_scroll = value;
                last_scroll_x = x;
                next_scroll = now + SCROLL_EMIT_EVERY;
            }
            last_mask = mask_all;
            // Sem nenhum controle conectado não há o que ler: acorda 20x/s em vez de 125x/s (menos CPU/bateria).
            // As sondagens de conexão (1 s XInput, 2 s HID) continuam no mesmo ritmo.
            let idle = hid.pads.is_empty() && !connected.iter().any(|c| *c);
            std::thread::sleep(Duration::from_millis(if idle { 50 } else { 8 }));
        }
    });
}

/// Liga (0 = desliga) os motores de um controle Xbox. Só é chamado por pedidos da interface do NEXSTAGE.
#[cfg(windows)]
fn set_xinput_rumble(slot: u32, strength: u8) -> bool {
    use crate::commands::controllers::xinput::{Vibration, XInputSetState};
    let v = (strength.min(100) as u32 * 65535 / 100) as u16;
    let mut vib = Vibration { left: v, right: v / 2 };
    unsafe { XInputSetState(slot, &mut vib) == 0 }
}
#[cfg(not(windows))]
fn set_xinput_rumble(_slot: u32, _strength: u8) -> bool {
    false
}

// ---------- configuração individual: comandos ----------

/// Ajustes individuais de cada controle cadastrado, por dispositivo ("xbox:<id>" | "ps:<id>" | "ns:<id>"; ver `devident`).
/// Substitui o conjunto inteiro (controles sem entrada voltam ao padrão).
#[tauri::command]
pub fn set_pad_configs(configs: HashMap<String, PadCfg>) {
    let clean: HashMap<String, PadCfg> = configs.into_iter().map(|(k, v)| (k, v.sanitized())).collect();
    if let Ok(mut g) = PAD_CFG.lock() {
        *g = Some(clean);
    }
    PAD_CFG_GEN.fetch_add(1, Ordering::SeqCst);
}

/// Teclado virtual aberto/fechado: liga a repetição dos botões ao segurar (ver `BUTTON_REPEAT`).
#[tauri::command]
pub fn set_pad_button_repeat(on: bool) {
    BUTTON_REPEAT.store(on, Ordering::SeqCst);
}

/// Liga/desliga o testador de controles (estado bruto em `pad-state`; navegação pausada).
#[tauri::command]
pub fn set_pad_monitor(on: bool) {
    MONITOR.store(on, Ordering::SeqCst);
}

/// Vibração de teste de um controle (força 0–100, duração em ms, até 2 s).
#[tauri::command]
pub fn pad_rumble(key: String, strength: u8, ms: u16) {
    push_rumble(&key, strength, ms);
}

// ---------- comandos ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PadCombos {
    pub overlay: u16,
    pub screenshot: u16,
    pub summon: u16,
    pub overlay_default: u16,
    pub screenshot_default: u16,
    pub summon_default: u16,
}

fn combos_info(rt: &Runtime) -> PadCombos {
    PadCombos {
        overlay: rt.pad_overlay.load(Ordering::SeqCst) as u16,
        screenshot: rt.pad_shot.load(Ordering::SeqCst) as u16,
        summon: rt.pad_summon.load(Ordering::SeqCst) as u16,
        overlay_default: DEFAULT_OVERLAY_COMBO,
        screenshot_default: DEFAULT_SHOT_COMBO,
        summon_default: DEFAULT_SUMMON_COMBO,
    }
}

#[tauri::command]
pub fn get_pad_combos(rt: State<Runtime>) -> PadCombos {
    combos_info(&rt)
}

fn save_combo(rt: &Runtime, db: &Db, slot: &str, mask: u16, persist: bool) -> AppResult<()> {
    let (o, s, u) = (rt.pad_overlay.load(Ordering::SeqCst) as u16, rt.pad_shot.load(Ordering::SeqCst) as u16, rt.pad_summon.load(Ordering::SeqCst) as u16);
    let (mine, others, key) = match slot {
        "overlay" => (&rt.pad_overlay, [s, u], "pad_combo_overlay"),
        "screenshot" => (&rt.pad_shot, [o, u], "pad_combo_shot"),
        "summon" => (&rt.pad_summon, [o, s], "pad_combo_summon"),
        _ => return Err(AppError::Validation("Opção inválida.".into())),
    };
    validate_combo(mask).map_err(|m| AppError::Validation(m.into()))?;
    if others.iter().any(|x| combos_conflict(mask, *x)) {
        return Err(AppError::Validation("Essa combinação conflita com a do outro comando (uma contém a outra).".into()));
    }
    let conn = db.conn()?;
    if persist {
        conn.execute(
            "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, mask.to_string()],
        )?;
    } else {
        conn.execute("DELETE FROM app_settings WHERE key=?1", params![key])?;
    }
    mine.store(mask as u32, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn set_pad_combo(rt: State<Runtime>, db: State<Db>, slot: String, mask: u16) -> AppResult<PadCombos> {
    save_combo(&rt, &db, &slot, mask, true)?;
    Ok(combos_info(&rt))
}

#[tauri::command]
pub fn reset_pad_combo(rt: State<Runtime>, db: State<Db>, slot: String) -> AppResult<PadCombos> {
    let def = match slot.as_str() {
        "overlay" => DEFAULT_OVERLAY_COMBO,
        "summon" => DEFAULT_SUMMON_COMBO,
        _ => DEFAULT_SHOT_COMBO,
    };
    save_combo(&rt, &db, &slot, def, false)?;
    Ok(combos_info(&rt))
}

/// Cor da barra de luz dos controles PlayStation ("#RRGGBB"): a interface envia a cor de destaque do
/// perfil sempre que o tema é aplicado. A leitura dos controles (thread acima) aplica no controle.
/// `theme`: cor de destaque do perfil, usada pelos controles com a opção individual "Cor do tema"
/// (valem mesmo com a sincronização geral desligada).
#[tauri::command]
pub fn set_pad_led(rt: State<Runtime>, color: String, theme: Option<String>) -> AppResult<()> {
    if let Some((r, g, b)) = theme.as_deref().and_then(crate::pad_led::parse_hex) {
        THEME_LED.store(1 << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32, Ordering::SeqCst);
    }
    // "off": sincronização desligada — o NEXSTAGE para de mexer na iluminação do controle
    if color == "off" {
        rt.pad_led.store(0, Ordering::SeqCst);
        rt.pad_led_gen.fetch_add(1, Ordering::SeqCst);
        return Ok(());
    }
    let (r, g, b) = crate::pad_led::parse_hex(&color).ok_or_else(|| AppError::Validation("Cor inválida.".into()))?;
    // bit 24 marca "definida" (preto puro também é uma cor válida)
    rt.pad_led.store(1 << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32, Ordering::SeqCst);
    rt.pad_led_gen.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Liga/desliga o modo "escolher combinação": a leitura passa a enviar `pad-mask` e pausa a navegação.
#[tauri::command]
pub fn set_pad_capture(rt: State<Runtime>, on: bool) {
    rt.capture.store(on, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: u8, len: usize, at: usize, status: u8) -> Vec<u8> {
        let mut b = vec![0u8; len];
        b[0] = id;
        b[at] = status;
        b
    }

    #[test]
    fn sony_battery_offsets_and_states() {
        let b = |v: &[u8], ds4| parse_sony_battery(v, v.len(), ds4, false);
        let sb = |percent, charging| Some(SonyBattery { percent, charging });
        // DualSense USB (0x01, 64 bytes) e Bluetooth completo (0x31, 78 bytes)
        assert_eq!(b(&report(0x01, 64, 53, 0x07), false), sb(Some(75), false));
        assert_eq!(b(&report(0x31, 78, 54, 0x13), false), sb(Some(35), true));
        assert_eq!(b(&report(0x31, 78, 54, 0x2A), false), sb(Some(100), false)); // carga completa
        assert_eq!(b(&report(0x01, 64, 53, 0xA5), false), sb(None, false)); // erro de tensão
        // relatório simples do Bluetooth (10 bytes): sem bateria
        assert_eq!(b(&report(0x01, 10, 5, 0x08), false), None);
        // DualShock 4 USB (0x01, 64 bytes) e Bluetooth completo (0x11, 78 bytes)
        assert_eq!(b(&report(0x01, 64, 30, 0x06), true), sb(Some(65), false));
        assert_eq!(b(&report(0x11, 78, 32, 0x14), true), sb(Some(45), true)); // com cabo, carregando
        assert_eq!(b(&report(0x11, 78, 32, 0x1B), true), sb(Some(100), false)); // com cabo, cheia
        assert_eq!(b(&report(0x01, 10, 5, 0x08), true), None);
    }

    #[test]
    fn one_move_per_tap_then_repeat_after_delay() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let pressed = RawPad { dirs: [false, true, false, false], ..Default::default() };
        assert_eq!(t.update(&pressed, t0), vec!["right"]);
        assert!(t.update(&pressed, t0 + Duration::from_millis(200)).is_empty());
        assert_eq!(t.update(&pressed, t0 + Duration::from_millis(400)), vec!["right"]);
        assert!(t.update(&RawPad::default(), t0 + Duration::from_millis(410)).is_empty());
    }

    #[test]
    fn buttons_fire_once_per_press() {
        let mut t = Tracker::default();
        let t0 = Instant::now();
        let held = RawPad { mask: btn::X | btn::RT, ..Default::default() };
        assert_eq!(t.update(&held, t0), vec!["x", "r2"]);
        assert!(t.update(&held, t0 + Duration::from_millis(500)).is_empty());
        assert!(t.update(&RawPad::default(), t0 + Duration::from_millis(510)).is_empty());
        assert_eq!(t.update(&RawPad { mask: btn::LT, ..Default::default() }, t0 + Duration::from_millis(520)), vec!["l2"]);
    }

    #[test]
    fn combo_needs_hold_fires_once_and_rearms_after_release() {
        let mut c = Combos::default();
        let t0 = Instant::now();
        let both = btn::SELECT | btn::START;
        let combos = [both, btn::SELECT | btn::RB, btn::SELECT | btn::LB];
        assert_eq!(c.update(both, combos, t0), [false, false, false]);
        assert_eq!(c.update(both, combos, t0 + Duration::from_millis(500)), [false, false, false]);
        assert_eq!(c.update(both, combos, t0 + Duration::from_millis(750)), [true, false, false]);
        assert_eq!(c.update(both, combos, t0 + Duration::from_millis(900)), [false, false, false]); // não repete
        assert_eq!(c.update(0, combos, t0 + Duration::from_millis(950)), [false, false, false]); // soltou
        assert_eq!(c.update(both, combos, t0 + Duration::from_millis(1000)), [false, false, false]);
        assert_eq!(c.update(both, combos, t0 + Duration::from_millis(1800)), [true, false, false]);
    }

    #[test]
    fn summon_combo_fires_after_hold_and_defaults_do_not_clash() {
        let mut c = Combos::default();
        let t0 = Instant::now();
        let combos = [DEFAULT_OVERLAY_COMBO, DEFAULT_SHOT_COMBO, DEFAULT_SUMMON_COMBO];
        let summon = btn::SELECT | btn::LB;
        assert_eq!(c.update(summon, combos, t0), [false, false, false]);
        assert_eq!(c.update(summon, combos, t0 + Duration::from_millis(400)), [false, false, false], "toque curto não chama o app");
        assert_eq!(c.update(summon, combos, t0 + Duration::from_millis(750)), [false, false, true]);
        assert!(validate_combo(DEFAULT_SUMMON_COMBO).is_ok());
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            assert!(!combos_conflict(combos[a], combos[b]), "padrões {a} e {b} não podem se conter");
        }
    }

    #[test]
    fn held_buttons_repeat_only_when_enabled() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let x = RawPad { mask: btn::X, ..Default::default() };
        let a = RawPad { mask: btn::A, ..Default::default() };
        let b = RawPad { mask: btn::B, ..Default::default() };
        // com repetição (teclado virtual): toque, espera 380 ms, depois a cada 95 ms
        let mut t = Tracker::default();
        assert_eq!(t.update_with(&x, &DEFAULT_MAP, at(0), true), ["x"]);
        assert!(t.update_with(&x, &DEFAULT_MAP, at(300), true).is_empty());
        assert_eq!(t.update_with(&x, &DEFAULT_MAP, at(380), true), ["x"]);
        assert!(t.update_with(&x, &DEFAULT_MAP, at(430), true).is_empty());
        assert_eq!(t.update_with(&x, &DEFAULT_MAP, at(475), true), ["x"]);
        // soltou: para; apertar de novo recomeça com o atraso
        assert!(t.update_with(&RawPad::default(), &DEFAULT_MAP, at(500), true).is_empty());
        assert_eq!(t.update_with(&a, &DEFAULT_MAP, at(600), true), ["confirm"]);
        assert_eq!(t.update_with(&a, &DEFAULT_MAP, at(980), true), ["confirm"]);
        // voltar nunca repete
        let mut t = Tracker::default();
        assert_eq!(t.update_with(&b, &DEFAULT_MAP, at(0), true), ["back"]);
        assert!(t.update_with(&b, &DEFAULT_MAP, at(2000), true).is_empty());
        // sem repetição (resto do app): um disparo por toque
        let mut t = Tracker::default();
        assert_eq!(t.update_with(&a, &DEFAULT_MAP, at(0), false), ["confirm"]);
        assert!(t.update_with(&a, &DEFAULT_MAP, at(300), false).is_empty());
        // segurou: avisa uma vez ("confirm_hold"); soltou: "confirm_up"
        assert_eq!(t.update_with(&a, &DEFAULT_MAP, at(2000), false), ["confirm_hold"]);
        assert!(t.update_with(&a, &DEFAULT_MAP, at(2500), false).is_empty());
        assert_eq!(t.update_with(&RawPad::default(), &DEFAULT_MAP, at(2600), false), ["confirm_up"]);
        // toque curto: só "confirm" e "confirm_up"
        assert_eq!(t.update_with(&a, &DEFAULT_MAP, at(3000), false), ["confirm"]);
        assert_eq!(t.update_with(&RawPad::default(), &DEFAULT_MAP, at(3100), false), ["confirm_up"]);
    }

    #[test]
    fn dualsense_bt_simple_report_padded_by_windows() {
        // 0x01 simples do Bluetooth: analógicos 1–4, botões 5–6 (direcional solto = 8), gatilhos 8–9;
        // o Windows completa com zeros até 78 bytes
        let mut b = vec![0u8; 78];
        b[0] = 0x01;
        b[1..5].copy_from_slice(&[128, 128, 128, 128]);
        b[5] = 0x08; // direcional solto, sem botões
        let raw = parse_hid(&b, 78, false, true).unwrap();
        assert_eq!(raw.mask, 0, "nada pressionado (antes: 'cima' preso)");
        assert_eq!((raw.lt, raw.rt), (0.0, 0.0));
        b[5] = 0x08 | 0x20; // ✕
        b[6] = 0x01; // L1
        assert_eq!(parse_hid(&b, 78, false, true).unwrap().mask, btn::A | btn::LB);
        b[5] = 0x04; // direcional para baixo
        assert_eq!(parse_hid(&b, 78, false, true).unwrap().mask & (btn::UP | btn::DOWN), btn::DOWN);
        // sem bateria no simples (antes: 5%, lida do preenchimento)
        assert_eq!(parse_sony_battery(&b, 78, false, true), None);
        assert_eq!(parse_sony_battery(&b, 78, true, true), None, "DS4 também");
        // o mesmo tamanho por USB continua sendo o relatório completo
        let mut u = vec![0u8; 64];
        u[0] = 0x01;
        u[1..5].copy_from_slice(&[128, 128, 128, 128]);
        u[8] = 0x08;
        assert_eq!(parse_hid(&u, 64, false, false).unwrap().mask, 0);
    }

    /// Comandos que a navegação geraria agora, por controle (XInput e HID), com o estado bruto:
    /// `NX_LIVE_SECS=20 cargo test --lib input::tests::live_inputs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_inputs() {
        let t0 = Instant::now();
        let secs = std::env::var("NX_LIVE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
        let mut hid = crate::hidpads::HidPads::new(t0);
        let mut xt: [Tracker; 4] = Default::default();
        let mut seen_x = [false; 4];
        let show = |src: &str, a: &str, r: &RawPad| {
            println!("[{:>5.2}s] {src:<22} {a:<8} mask={:#06x} dirs={:?} L=({:+.2},{:+.2}) R=({:+.2},{:+.2}) LT={:.2} RT={:.2}",
                t0.elapsed().as_secs_f32(), r.mask, r.dirs, r.lx, r.ly, r.rx, r.ry, r.lt, r.rt);
        };
        while t0.elapsed() < Duration::from_secs(secs) {
            let now = Instant::now();
            for i in 0..4usize {
                if let Some(mut r) = read_xinput(i as u32) {
                    if !seen_x[i] {
                        seen_x[i] = true;
                        println!("[{:>5.2}s] XInput slot {i} conectado", t0.elapsed().as_secs_f32());
                    }
                    r.compute_dirs(NAV_DEADZONE);
                    for a in xt[i].update_mapped(&r, &DEFAULT_MAP, now) {
                        show(&format!("xinput:{i}"), a, &r);
                    }
                }
            }
            if hid.scan(now) {
                for k in hid.input_keys() {
                    println!("[{:>5.2}s] HID conectado: {k}", t0.elapsed().as_secs_f32());
                }
            }
            for k in hid.read_all(now) {
                println!("[{:>5.2}s] HID perdido: {k}", t0.elapsed().as_secs_f32());
            }
            hid.joycons(now);
            for p in hid.inputs() {
                p.raw.compute_dirs(NAV_DEADZONE);
                let raw = p.raw.clone();
                for a in p.tracker.update_mapped(&raw, &DEFAULT_MAP, now) {
                    show(p.key, a, &raw);
                }
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        for p in hid.inputs() { println!("{} → {} relatórios/s; parado em L=({:+.2},{:+.2}) R=({:+.2},{:+.2})", p.key, p.hz, p.raw.lx, p.raw.ly, p.raw.rx, p.raw.ry); }
        println!("fim");
    }

    /// Controles reais: `cargo test --lib input::tests::live_led -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_led() {
        let api = hidapi::HidApi::new().unwrap();
        let mut found = 0;
        for info in api.device_list() {
            let pid = info.product_id();
            if info.vendor_id() != 0x054C || !matches!(pid, 0x0CE6 | 0x0DF2 | 0x05C4 | 0x09CC) || info.usage_page() != 0x01 || info.usage() != 0x05 {
                continue;
            }
            let ds4 = matches!(pid, 0x05C4 | 0x09CC);
            let bt = matches!(info.bus_type(), hidapi::BusType::Bluetooth);
            let Ok(d) = info.open_device(&api) else { println!("{pid:04x}: não abriu"); continue };
            found += 1;
            let rgb = (0x3f, 0xa8, 0x2c); // Verde Grama
            let w = if ds4 {
                d.write(&crate::pad_led::ds4_color(bt, rgb)).map(|n| vec![n])
            } else {
                d.write(&crate::pad_led::dualsense_release(bt, 0))
                    .and_then(|a| d.write(&crate::pad_led::dualsense_color(bt, 1, rgb)).map(|b| vec![a, b]))
            };
            println!("{pid:04x} ds4={ds4} bt={bt}: escrita {w:?}");
            let mut parsed = 0;
            let mut ids = std::collections::BTreeSet::new();
            let mut buf = [0u8; 128];
            for _ in 0..40 {
                if let Ok(n) = d.read_timeout(&mut buf, 50) {
                    if n > 0 {
                        ids.insert(format!("{:#04x}/{n}", buf[0]));
                        if parse_hid(&buf, n, ds4, bt).is_some() { parsed += 1; }
                    }
                }
            }
            println!("   relatórios recebidos depois: {ids:?}; entendidos: {parsed}");
        }
        println!("controles Sony abertos: {found}");
    }

    #[test]
    fn ds4_bluetooth_full_report_after_led() {
        // 0x11: mesmo conteúdo do 0x01 deslocado 2 bytes (analógicos em 3..6, botões em 7 e 8)
        let mut b = [0u8; 78];
        b[0] = 0x11;
        b[3] = 128; b[4] = 0; // analógico esquerdo para cima
        b[7] = 0x08 | 0x20; // direcional neutro + ✕
        let raw = parse_hid(&b, 78, true, true).expect("DS4 0x11 deve ser entendido");
        assert!(raw.dirs[2], "cima");
        assert!(raw.mask & btn::A != 0, "✕ = confirmar");
        assert!(parse_hid(&b, 78, false, true).is_none(), "DualSense não usa 0x11");
    }

    #[test]
    fn scroll_deadzone_and_scale() {
        assert_eq!(scroll_axis(0.0), 0.0);
        assert_eq!(scroll_axis(0.2), 0.0);
        assert_eq!(scroll_axis(-0.24), 0.0);
        assert_eq!(scroll_axis(1.0), 1.0);
        assert_eq!(scroll_axis(-1.0), -1.0);
        let mid = scroll_axis(0.62);
        assert!(mid > 0.45 && mid < 0.55, "{mid}");
        assert!(scroll_axis(-0.62) < 0.0);
    }

    #[test]
    fn mapping_swaps_confirm_and_back_only_in_the_interface() {
        // padrão japonês: o botão da direita (B/○) confirma e o de baixo (A/✕) volta
        let mut map = DEFAULT_MAP;
        map.swap(0, 1);
        let mut t = Tracker::default();
        let t0 = Instant::now();
        assert_eq!(t.update_mapped(&RawPad { mask: btn::B, ..Default::default() }, &map, t0), vec!["confirm"]);
        assert_eq!(t.update_mapped(&RawPad { mask: btn::A, ..Default::default() }, &map, t0 + Duration::from_millis(50)), vec!["confirm_up", "back"]); // soltou B (confirmar) e apertou A (voltar)
    }

    #[test]
    fn config_is_sanitized() {
        let bad = PadCfg { dz_left: 5.0, dz_right: f32::NAN, map: [btn::A; 9], rumble: 250, rumble_nav: true, led: 0xDEAD_BEEF }.sanitized();
        assert_eq!((bad.dz_left, bad.dz_right, bad.map, bad.rumble, bad.led), (0.8, SCROLL_DEADZONE, DEFAULT_MAP, 100, LED_GLOBAL));
        let mut dpad = DEFAULT_MAP;
        dpad[0] = btn::UP; // direcional não pode virar ação
        assert_eq!(PadCfg { map: dpad, ..Default::default() }.sanitized().map, DEFAULT_MAP);
    }

    #[test]
    fn led_per_controller() {
        let global = 1 << 24 | 0x112233;
        let theme = 1 << 24 | 0x2F6BFF;
        assert_eq!(led_want(LED_GLOBAL, global, theme), Some((0x11, 0x22, 0x33)));
        assert_eq!(led_want(LED_GLOBAL, 0, theme), None, "sincronização geral desligada: não mexe");
        assert_eq!(led_want(LED_THEME, 0, theme), Some((0x2F, 0x6B, 0xFF)), "cor do tema vale mesmo com a geral desligada");
        assert_eq!(led_want(LED_KEEP, global, theme), None);
        assert_eq!(led_want(1 << 24, global, theme), Some((0, 0, 0)), "preto = apagado");
    }

    #[test]
    fn deadzone_moves_navigation_threshold() {
        assert!((nav_threshold(NAV_DEADZONE) - 0.67).abs() < 0.001);
        let mut r = RawPad { lx: 0.6, ..Default::default() };
        r.compute_dirs(NAV_DEADZONE);
        assert!(!r.dirs[1], "0.6 não passa do limiar padrão");
        r.compute_dirs(0.1);
        assert!(r.dirs[1], "com zona morta menor, passa");
        assert_eq!(scroll_axis_dz(0.3, 0.4), 0.0);
        assert!(scroll_axis_dz(0.3, 0.1) > 0.0);
    }

    #[test]
    fn combo_rules() {
        assert!(validate_combo(btn::SELECT | btn::START).is_ok());
        assert!(validate_combo(btn::A).is_err());
        assert!(validate_combo(btn::A | btn::B).is_err()); // sem botão de menu
        assert!(validate_combo(btn::A | btn::B | btn::X | btn::START).is_err()); // 4 botões
        assert!(combos_conflict(btn::SELECT | btn::START, btn::SELECT | btn::START | btn::RB));
        assert!(!combos_conflict(btn::SELECT | btn::START, btn::SELECT | btn::RB));
    }

    #[test]
    fn mirrored_virtual_pad_never_doubles_a_command() {
        let t0 = Instant::now();
        let ms = |v: u64| t0 + Duration::from_millis(v);
        // DualSense físico confirma; 20 ms depois o X360 virtual do DS4Windows repete: descartado
        let phys = (t0, "ps:50ee3242a709".to_string(), false);
        assert!(is_mirrored(Some(&phys), "xbox:vabc", true, ms(20)));
        // e na ordem inversa (o virtual chegou primeiro)
        let virt = (t0, "xbox:vabc".to_string(), true);
        assert!(is_mirrored(Some(&virt), "ps:50ee3242a709", false, ms(20)));
        // dois controles FÍSICOS diferentes (duas pessoas) não se anulam
        assert!(!is_mirrored(Some(&phys), "xbox:14cb65f29d22", false, ms(20)));
        // o mesmo controle apertando de novo, ou o espelho bem depois: valem
        assert!(!is_mirrored(Some(&phys), "ps:50ee3242a709", false, ms(20)));
        assert!(!is_mirrored(Some(&phys), "xbox:vabc", true, ms(400)));
        assert!(!is_mirrored(None, "xbox:vabc", true, t0));
    }
}
