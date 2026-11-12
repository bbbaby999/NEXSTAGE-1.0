//! Controles lidos por HID — VÁRIOS ao mesmo tempo, cada um com identidade própria:
//!  - Sony: DualSense, DualSense Edge e DualShock 4 (USB e Bluetooth);
//!  - Nintendo: Switch Pro Controller e Joy-Con (L) / (R) (Bluetooth; Pro Controller também por USB).
//!    O WebView2 normalmente não entrega esses controles à Gamepad API, então o NEXSTAGE os lê aqui.
//!
//! Identidade (chave do dispositivo, usada pela Central de Controles): `ps:<id>` / `ns:<id>`, onde
//! `<id>` é o endereço MAC do controle (o mesmo por USB e Bluetooth; dois controles do mesmo modelo
//! nunca se confundem). Sem MAC legível: número de série do HID, depois o endereço/serial e o
//! ContainerId que o Windows atribui ao aparelho físico (`devident`) e, por último, um hash do caminho
//! (único enquanto conectado). O mesmo controle aberto por dois caminhos (outra coleção HID, USB e
//! Bluetooth ao mesmo tempo) conta uma vez só.
//!
//! Controles VIRTUAIS (ex.: o DualShock 4 que o DS4Windows/ViGEm cria para um DualSense) recebem a
//! chave `ps:v<hash>` e a marca `virtual_src`: nunca assumem a identidade do controle real e a
//! interface os apresenta como virtuais, não como mais um controle físico.
//!
//! REGRA DE ISOLAMENTO (ver `input`): só lemos o estado e, a pedido da interface, mudamos LED/vibração
//! dentro do NEXSTAGE. Nos Nintendo, o modo de relatório completo (0x30) e a luz de "jogador" são os
//! mesmos que o próprio Switch/Steam usam; o Windows e os jogos continuam recebendo o controle normalmente.

use crate::input::{btn, parse_hid, parse_sony_battery, RawPad, NAV_DEADZONE};
use hidapi::{BusType, HidApi, HidDevice};
use std::ffi::CString;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    DualSense,
    DualSenseEdge,
    DualShock4,
    SwitchPro,
    JoyConL,
    JoyConR,
    /// Joy-Con (L) + Joy-Con (R) juntos como um controle só. Não é um dispositivo HID: é a soma das
    /// duas metades, cada uma aberta como `JoyConL` / `JoyConR` (ver `HidPads::joycons`).
    JoyConPair,
    /// Controle de jogo da Sony com PID que o NEXSTAGE não conhece: listado, mas não lido (o formato
    /// dos relatórios é desconhecido) — o usuário escolhe o modelo no cadastro.
    SonyUnknown,
}

impl Model {
    pub fn of(vid: u16, pid: u16) -> Option<Model> {
        Some(match (vid, pid) {
            (0x054C, 0x0CE6) => Model::DualSense,
            (0x054C, 0x0DF2) => Model::DualSenseEdge,
            (0x054C, 0x05C4 | 0x09CC) => Model::DualShock4,
            (0x057E, 0x2009) => Model::SwitchPro,
            (0x057E, 0x2006) => Model::JoyConL,
            (0x057E, 0x2007) => Model::JoyConR,
            _ => return None,
        })
    }
    /// Modelo de um dispositivo HID de controle de jogo (usage page 1, usage 4/5).
    pub fn of_gamepad(vid: u16, pid: u16) -> Option<Model> {
        Model::of(vid, pid).or((vid == 0x054C).then_some(Model::SonyUnknown))
    }
    /// Os relatórios deste modelo são lidos (botões, analógicos, bateria)?
    pub fn readable(self) -> bool {
        self != Model::SonyUnknown
    }
    pub fn sony(self) -> bool {
        matches!(self, Model::DualSense | Model::DualSenseEdge | Model::DualShock4 | Model::SonyUnknown)
    }
    pub fn ds4(self) -> bool {
        self == Model::DualShock4
    }
    pub fn name(self) -> &'static str {
        match self {
            Model::DualSense => "DualSense",
            Model::DualSenseEdge => "DualSense Edge",
            Model::DualShock4 => "DualShock 4",
            Model::SwitchPro => "Pro Controller",
            Model::JoyConL => "Joy-Con (L)",
            Model::JoyConR => "Joy-Con (R)",
            Model::JoyConPair => "Joy-Con (L+R)",
            Model::SonyUnknown => "Controle Sony não identificado",
        }
    }
    /// Identificador estável do modelo (a interface sugere imagem, marca e plataforma por ele).
    pub fn id(self) -> &'static str {
        match self {
            Model::DualSense => "dualsense",
            Model::DualSenseEdge => "dualsense-edge",
            Model::DualShock4 => "dualshock4",
            Model::SwitchPro => "switch-pro",
            Model::JoyConL => "joycon-l",
            Model::JoyConR => "joycon-r",
            Model::JoyConPair => "joycon-pair",
            Model::SonyUnknown => "sony-unknown",
        }
    }
    pub fn brand(self) -> &'static str {
        if self.sony() { "Sony" } else { "Nintendo" }
    }
    /// Família para os glifos da interface (`pad-input.device`).
    pub fn kind(self) -> &'static str {
        if self.sony() { "playstation" } else { "nintendo" }
    }
}

/// Resumo de um controle aberto (lista da Central de Controles e da barra de status).
#[derive(Clone, Debug, PartialEq)]
pub struct HidInfo {
    pub key: String,
    pub model: Model,
    pub bt: bool,
    /// Origem do identificador: "mac" | "serial" | "container" | "path" | "virtual".
    pub id_source: &'static str,
    /// ContainerId do Windows (o mesmo para todas as interfaces do aparelho físico).
    pub container: Option<String>,
    /// Quem criou o dispositivo, se ele é virtual (ViGEm, G HUB...).
    pub virtual_src: Option<&'static str>,
    pub vid: u16,
    pub pid: u16,
    pub battery_percent: Option<u8>,
    pub battery_label: Option<&'static str>,
    pub charging: bool,
    /// Luz de jogador acesa nos controles Nintendo (1–4).
    pub player: Option<u8>,
}

/// Controles abertos agora (atualizado pela thread de leitura; lido por `commands::controllers`).
pub static PADS: Mutex<Vec<HidInfo>> = Mutex::new(Vec::new());

// ---------- identidade ----------

fn hex(bytes: impl IntoIterator<Item = u8>) -> String {
    bytes.into_iter().map(|b| format!("{b:02x}")).collect()
}

/// MAC (12 dígitos hex) a partir de 6 bytes em ordem inversa (como os controles Sony informam).
fn mac_rev(b: &[u8]) -> Option<String> {
    let m: Vec<u8> = b.get(..6)?.iter().rev().copied().collect();
    (m.iter().any(|v| *v != 0) && m.iter().any(|v| *v != 0xFF)).then(|| hex(m))
}

/// Endereço MAC de um controle Sony (relatório de características de pareamento).
fn sony_mac(dev: &HidDevice, model: Model, bt: bool) -> Option<String> {
    let id = match model {
        Model::DualShock4 if bt => 0x81u8,
        Model::DualShock4 => 0x12,
        _ => 0x09,
    };
    // buffer do tamanho do maior relatório: no Windows, um buffer menor faz a leitura falhar
    let mut buf = vec![0u8; 64];
    buf[0] = id;
    let n = dev.get_feature_report(&mut buf).ok()?;
    if n < 7 {
        return None;
    }
    mac_rev(&buf[1..7])
}

/// Número de série do HID normalizado (por Bluetooth, costuma ser o MAC).
pub fn serial_id(serial: Option<&str>) -> Option<String> {
    let s: String = serial?.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase();
    (s.len() >= 12 && s.chars().any(|c| c != '0')).then_some(s)
}

/// Identificador do controle físico, do mais forte ao mais fraco. Virtual: sempre um id próprio
/// ("v…"), mesmo que ele repita o MAC do controle real que espelha.
pub fn identity(
    is_virtual: bool,
    mac: impl FnOnce() -> Option<String>,
    serial: Option<String>,
    pnp: Option<&crate::devident::PnpInfo>,
    path: &[u8],
) -> (String, &'static str) {
    if is_virtual {
        if let Some(x) = pnp.and_then(|p| p.stable_id()) {
            return x;
        }
        return (format!("v{}", &path_id(path)[1..]), "virtual");
    }
    if let Some(m) = mac() {
        return (m, "mac");
    }
    if let Some(s) = serial {
        return (s, "serial");
    }
    if let Some(x) = pnp.and_then(|p| p.stable_id()) {
        return x;
    }
    (path_id(path), "path")
}

/// Hash curto (FNV-1a) do caminho do dispositivo: último recurso, único enquanto conectado.
pub fn path_id(path: &[u8]) -> String {
    let mut h: u32 = 0x811C_9DC5;
    for &b in path {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("p{h:08x}")
}

// ---------- Nintendo: relatórios ----------

const NS_NEUTRAL_RUMBLE: [u8; 8] = [0x00, 0x01, 0x40, 0x40, 0x00, 0x01, 0x40, 0x40];

/// Relatório de saída 0x01 (subcomando), 49 bytes.
pub fn ns_subcommand(counter: u8, cmd: u8, args: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 49];
    b[0] = 0x01;
    b[1] = counter & 0x0F;
    b[2..10].copy_from_slice(&NS_NEUTRAL_RUMBLE);
    b[10] = cmd;
    for (i, a) in args.iter().enumerate().take(38) {
        b[11 + i] = *a;
    }
    b
}

/// Dois valores de 12 bits em 3 bytes (formato dos analógicos e da calibração Nintendo).
fn unpack12(b: &[u8]) -> (u16, u16) {
    (b[0] as u16 | ((b[1] as u16 & 0x0F) << 8), (b[1] as u16 >> 4) | ((b[2] as u16) << 4))
}

/// Calibração de um eixo: centro e curso até cada extremo (12 bits).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisCal {
    pub center: u16,
    pub below: u16,
    pub above: u16,
}

impl AxisCal {
    fn norm(&self, v: u16) -> f32 {
        let d = v as f32 - self.center as f32;
        let span = if d >= 0.0 { self.above } else { self.below } as f32;
        (d / span.max(1.0)).clamp(-1.0, 1.0)
    }
    fn valid(&self) -> bool {
        (1000..=3100).contains(&self.center) && (400..=2200).contains(&self.below) && (400..=2200).contains(&self.above)
    }
}

/// Calibração de um analógico. Cada Joy-Con / Pro Controller tem a sua gravada de fábrica: o centro
/// varia bastante de um controle para outro (de ~1800 a ~2300) e o curso também. Sem ela, um Joy-Con
/// pode parecer inclinado em repouso ou não alcançar o limiar de navegação.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StickCal {
    pub x: AxisCal,
    pub y: AxisCal,
}

impl Default for StickCal {
    fn default() -> Self {
        let a = AxisCal { center: 2048, below: 1400, above: 1400 };
        Self { x: a, y: a }
    }
}

impl StickCal {
    /// Analógico esquerdo (fábrica 0x603D / usuário 0x8012): curso acima, centro, curso abaixo.
    pub fn left(d: &[u8]) -> Option<StickCal> {
        let ((xa, ya), (cx, cy), (xb, yb)) = (unpack12(d.get(0..3)?), unpack12(d.get(3..6)?), unpack12(d.get(6..9)?));
        StickCal { x: AxisCal { center: cx, below: xb, above: xa }, y: AxisCal { center: cy, below: yb, above: ya } }.checked()
    }
    /// Analógico direito (fábrica 0x6046 / usuário 0x801D): centro, curso abaixo, curso acima.
    pub fn right(d: &[u8]) -> Option<StickCal> {
        let ((cx, cy), (xb, yb), (xa, ya)) = (unpack12(d.get(0..3)?), unpack12(d.get(3..6)?), unpack12(d.get(6..9)?));
        StickCal { x: AxisCal { center: cx, below: xb, above: xa }, y: AxisCal { center: cy, below: yb, above: ya } }.checked()
    }
    fn checked(self) -> Option<StickCal> {
        (self.x.valid() && self.y.valid()).then_some(self)
    }
}

/// Endereços da memória SPI com a calibração dos analógicos: fábrica (esquerdo 9 bytes + direito
/// 9 bytes) e do usuário (para cada lado: 2 bytes de marca 0xB2 0xA1 + 9 bytes; só vale com a marca).
const SPI_CAL_FACTORY: u32 = 0x603D;
const SPI_CAL_USER: u32 = 0x8010;

/// Resposta de leitura da memória SPI (relatório 0x21, subcomando 0x10): endereço e dados.
fn spi_reply(buf: &[u8], n: usize) -> Option<(u32, &[u8])> {
    if n < 20 || buf[0] != 0x21 || buf[14] != 0x10 || buf[13] & 0x80 == 0 {
        return None;
    }
    let addr = u32::from_le_bytes([buf[15], buf[16], buf[17], buf[18]]);
    let len = buf[19] as usize;
    Some((addr, buf.get(20..(20 + len).min(n))?))
}

/// Calibração lida da memória: (esquerdo, direito) de quem estiver gravado.
fn cal_from_spi(addr: u32, d: &[u8]) -> [Option<StickCal>; 2] {
    match addr {
        SPI_CAL_FACTORY if d.len() >= 18 => [StickCal::left(&d[0..9]), StickCal::right(&d[9..18])],
        SPI_CAL_USER if d.len() >= 22 => {
            let user = |o: usize| d[o] == 0xB2 && d[o + 1] == 0xA1;
            [
                if user(0) { StickCal::left(&d[2..11]) } else { None },
                if user(11) { StickCal::right(&d[13..22]) } else { None },
            ]
        }
        _ => [None, None],
    }
}

/// Relatório padrão dos controles Nintendo (0x30 completo; 0x21/0x31 trazem o mesmo cabeçalho).
/// Botões por POSIÇÃO física, como no resto do app: o de baixo (B no Switch) = `btn::A`, o da direita
/// (A no Switch) = `btn::B`, o da esquerda (Y) = `btn::X`, o de cima (X) = `btn::Y`.
/// Joy-Con sozinho: o analógico dele navega (vai para o analógico esquerdo).
/// Devolve o estado e a bateria (rótulo, carregando).
pub fn parse_switch(buf: &[u8], n: usize, model: Model, cal: &[StickCal; 2]) -> Option<(RawPad, Option<(&'static str, bool)>)> {
    if n < 12 || !matches!(buf[0], 0x30 | 0x21 | 0x31) {
        return None;
    }
    let (r, s, l) = (buf[3], buf[4], buf[5]);
    let stick = |o: usize, c: &StickCal| -> (f32, f32) {
        let (x, y) = unpack12(&buf[o..o + 3]);
        // no Switch y positivo = para cima; no app, para baixo
        (c.x.norm(x), -c.y.norm(y))
    };
    let (lsx, lsy) = stick(6, &cal[0]);
    let (rsx, rsy) = stick(9, &cal[1]);
    let mut m = 0u16;
    let mut set = |byte: u8, bit: u8, v: u16| {
        if byte & bit != 0 {
            m |= v;
        }
    };
    let mut extra = 0u8;
    match model {
        Model::JoyConL => {
            set(l, 0x01, btn::DOWN);
            set(l, 0x02, btn::UP);
            set(l, 0x04, btn::RIGHT);
            set(l, 0x08, btn::LEFT);
            set(l, 0x40, btn::LB);
            set(l, 0x80, btn::LT);
            set(s, 0x01, btn::SELECT);
            set(s, 0x08, btn::L3);
        }
        Model::JoyConR => {
            set(r, 0x04, btn::A);
            set(r, 0x08, btn::B);
            set(r, 0x01, btn::X);
            set(r, 0x02, btn::Y);
            set(r, 0x40, btn::RB);
            set(r, 0x80, btn::RT);
            set(s, 0x02, btn::START);
            set(s, 0x04, btn::R3);
            if s & 0x10 != 0 {
                extra |= 0x01;
            }
        }
        _ => {
            set(r, 0x04, btn::A);
            set(r, 0x08, btn::B);
            set(r, 0x01, btn::X);
            set(r, 0x02, btn::Y);
            set(r, 0x40, btn::RB);
            set(r, 0x80, btn::RT);
            set(l, 0x40, btn::LB);
            set(l, 0x80, btn::LT);
            set(s, 0x01, btn::SELECT);
            set(s, 0x02, btn::START);
            set(s, 0x08, btn::L3);
            set(s, 0x04, btn::R3);
            set(l, 0x01, btn::DOWN);
            set(l, 0x02, btn::UP);
            set(l, 0x04, btn::RIGHT);
            set(l, 0x08, btn::LEFT);
            if s & 0x10 != 0 {
                extra |= 0x01;
            }
        }
    }
    let (lx, ly, rx, ry) = match model {
        Model::JoyConL => (lsx, lsy, 0.0, 0.0),
        Model::JoyConR => (rsx, rsy, 0.0, 0.0),
        _ => (lsx, lsy, rsx, rsy),
    };
    let trig = |on: bool| if on { 1.0 } else { 0.0 };
    let mut raw = RawPad {
        mask: m,
        lx,
        ly,
        rx,
        ry,
        lt: trig(m & btn::LT != 0),
        rt: trig(m & btn::RT != 0),
        extra,
        ..Default::default()
    };
    raw.compute_dirs(NAV_DEADZONE);
    // bateria: nibble alto do byte 2 — nível par (8 cheia, 6 média, 4 baixa, 2 crítica, 0 vazia) + bit carregando
    let nib = buf[2] >> 4;
    let label = match nib & 0x0E {
        8 => "Cheia",
        6 => "Média",
        4 => "Baixa",
        _ => "Crítica",
    };
    Some((raw, Some((label, nib & 0x01 != 0))))
}

// ---------- um controle aberto ----------

pub struct Pad {
    pub key: String,
    pub model: Model,
    pub bt: bool,
    pub id_source: &'static str,
    pub container: Option<String>,
    pub virtual_src: Option<&'static str>,
    vid: u16,
    pid: u16,
    path: CString,
    dev: HidDevice,
    pub raw: RawPad,
    pub tracker: crate::input::Tracker,
    pub combos: crate::input::Combos,
    /// Relatórios recebidos (taxa por segundo em `hz`, para o testador).
    pub reports: u32,
    pub hz: f32,
    battery_percent: Option<u8>,
    battery_label: Option<&'static str>,
    charging: bool,
    // Sony: LED e vibração
    led_applied: Option<u32>,
    led_retry: Option<Instant>,
    seq: u8,
    led_released: bool,
    rumble_until: Option<Instant>,
    // Nintendo: modo de relatório completo e luz de jogador
    player: u8,
    ns_counter: u8,
    ns_full: bool,
    ns_tries: u8,
    ns_next: Option<Instant>,
    /// Sony por Bluetooth recebendo o relatório simples; próximo pedido do modo completo.
    sony_simple: bool,
    sony_full_next: Option<Instant>,
    /// Metade de um par de Joy-Cons: a entrada sai pelo par, não por esta metade.
    pub joined: bool,
    /// Quando foi aberto; `alive` = já mandou algum relatório; `announced` = já está na lista.
    opened: Instant,
    alive: bool,
    announced: bool,
    /// Desde quando o botão de juntar (L no Joy-Con esquerdo, R no direito) está pressionado.
    join_held: Option<Instant>,
    /// Nintendo: calibração dos analógicos (lida da memória do controle; padrão até chegar).
    pub cal: [StickCal; 2],
    /// 0 = lendo a de fábrica, 1 = lendo a do usuário, 2 = pronta.
    cal_step: u8,
    cal_tries: u8,
    cal_next: Option<Instant>,
}

impl Pad {
    fn info(&self) -> HidInfo {
        HidInfo {
            key: self.key.clone(),
            model: self.model,
            bt: self.bt,
            id_source: self.id_source,
            container: self.container.clone(),
            virtual_src: self.virtual_src,
            vid: self.vid,
            pid: self.pid,
            battery_percent: self.battery_percent,
            battery_label: self.battery_label,
            charging: self.charging,
            player: (!self.model.sony() && self.player > 0).then_some(self.player),
        }
    }

    fn ns_send(&mut self, cmd: u8, args: &[u8]) -> bool {
        let pkt = ns_subcommand(self.ns_counter, cmd, args);
        self.ns_counter = self.ns_counter.wrapping_add(1);
        self.dev.write(&pkt).is_ok()
    }

    /// Nintendo: pede o relatório completo (0x30) e acende a luz do jogador; repete até chegar 0x30.
    fn ns_init(&mut self, now: Instant) {
        if self.model.sony() || self.ns_full || self.ns_tries >= 6 || self.ns_next.is_some_and(|t| now < t) {
            return;
        }
        self.ns_tries += 1;
        self.ns_next = Some(now + Duration::from_millis(800));
        if self.ns_tries == 1 && !self.bt {
            // USB (Pro Controller): handshake, 3 Mbit/s, handshake e "só HID, sem tempo limite"
            for c in [0x02u8, 0x03, 0x02, 0x04] {
                let _ = self.dev.write(&[0x80, c]);
                std::thread::sleep(Duration::from_millis(15));
            }
        }
        let _ = self.ns_send(0x03, &[0x30]);
        std::thread::sleep(Duration::from_millis(15));
        let lights = 1u8 << (self.player.clamp(1, 4) - 1);
        let _ = self.ns_send(0x30, &[lights]);
    }

    /// Nintendo: lê a calibração dos analógicos (fábrica, depois a do usuário, que prevalece), já com
    /// o relatório completo ativo. Até 4 tentativas por etapa; sem resposta, fica a calibração padrão.
    fn ns_cal(&mut self, now: Instant) {
        if self.model.sony() || !self.ns_full || self.cal_step >= 2 || self.cal_next.is_some_and(|t| now < t) {
            return;
        }
        if self.cal_tries >= 4 {
            self.cal_step += 1;
            self.cal_tries = 0;
            return;
        }
        self.cal_tries += 1;
        self.cal_next = Some(now + Duration::from_millis(400));
        let (addr, len) = if self.cal_step == 0 { (SPI_CAL_FACTORY, 18u8) } else { (SPI_CAL_USER, 22u8) };
        let a = addr.to_le_bytes();
        let _ = self.ns_send(0x10, &[a[0], a[1], a[2], a[3], len]);
    }

    fn apply_cal(&mut self, addr: u32, d: &[u8]) {
        let step = match addr {
            SPI_CAL_FACTORY if self.cal_step == 0 => 1,
            SPI_CAL_USER if self.cal_step == 1 => 2,
            _ => return,
        };
        for (slot, c) in self.cal.iter_mut().zip(cal_from_spi(addr, d)) {
            if let Some(c) = c {
                *slot = c;
            }
        }
        self.cal_step = step;
        self.cal_tries = 0;
        self.cal_next = None;
    }

    /// Sony por Bluetooth: pede o relatório completo (lendo o relatório de características 0x05, como o
    /// driver do PS5/Linux) enquanto o controle estiver mandando o simples. No máximo a cada 2 s.
    fn sony_full(&mut self, now: Instant) {
        if !self.sony_simple || !self.model.sony() || !self.model.readable() || self.sony_full_next.is_some_and(|t| now < t) {
            return;
        }
        self.sony_full_next = Some(now + Duration::from_secs(2));
        let mut feat = [0u8; 41];
        feat[0] = 0x05;
        let _ = self.dev.get_feature_report(&mut feat);
    }

    /// Nintendo: troca a luz de jogador (1–4) com o controle já em uso.
    fn set_player(&mut self, n: u8) {
        if self.model.sony() || self.player == n {
            return;
        }
        self.player = n;
        if self.ns_full {
            let _ = self.ns_send(0x30, &[1u8 << (n.clamp(1, 4) - 1)]);
        }
    }

    /// Cor desejada (`None` = não mexer). Só para Sony.
    pub fn apply_led(&mut self, want: Option<(u8, u8, u8)>, now: Instant) {
        if !self.model.sony() || !self.model.readable() {
            return;
        }
        let Some(rgb) = want else { return };
        let code = (rgb.0 as u32) << 16 | (rgb.1 as u32) << 8 | rgb.2 as u32;
        if self.led_applied == Some(code) || self.led_retry.is_some_and(|t| now < t) {
            return;
        }
        let bt = self.bt;
        let mut ok = true;
        if self.model.ds4() {
            ok &= self.dev.write(&crate::pad_led::ds4_color(bt, rgb)).is_ok();
        } else {
            if !self.led_released {
                ok &= self.dev.write(&crate::pad_led::dualsense_release(bt, self.seq)).is_ok();
                self.seq = self.seq.wrapping_add(1);
                self.led_released = ok;
            }
            ok &= self.dev.write(&crate::pad_led::dualsense_color(bt, self.seq, rgb)).is_ok();
            self.seq = self.seq.wrapping_add(1);
        }
        if ok {
            self.led_applied = Some(code);
            self.led_retry = None;
        } else {
            self.led_retry = Some(now + Duration::from_millis(1000));
        }
    }

    fn sony_rumble(&mut self, s: u8) -> bool {
        let l = (s.min(100) as u16 * 255 / 100) as u8;
        let r = l / 2;
        if self.model.ds4() {
            self.dev.write(&crate::pad_led::ds4_rumble(self.bt, l, r)).is_ok()
        } else {
            let w = self.dev.write(&crate::pad_led::dualsense_rumble(self.bt, self.seq, l, r)).is_ok();
            self.seq = self.seq.wrapping_add(1);
            w
        }
    }

    /// Vibração pedida pela interface (só Sony; nos Nintendo o pedido é ignorado).
    pub fn rumble(&mut self, strength: u8, ms: u16, now: Instant) {
        if self.model.sony() && self.model.readable() && self.sony_rumble(strength) {
            self.rumble_until = Some(now + Duration::from_millis(ms as u64));
        }
    }

    fn rumble_tick(&mut self, now: Instant) {
        if self.rumble_until.is_some_and(|t| now >= t) {
            self.sony_rumble(0);
            self.rumble_until = None;
        }
    }

    /// Lê todos os relatórios pendentes. `false` = controle perdido (desconectado).
    fn read(&mut self) -> bool {
        let mut buf = [0u8; 128];
        loop {
            match self.dev.read(&mut buf) {
                Ok(0) => return true,
                Ok(n) => {
                    self.reports += 1;
                    self.alive = true;
                    if !self.model.readable() {
                        // formato desconhecido: só esvazia a fila (nada navega por engano)
                    } else if self.model.sony() {
                        let ds4 = self.model.ds4();
                        if self.bt {
                            // relatório simples (0x01) por Bluetooth: o controle saiu do modo completo (reconexão,
                            // outro programa) — pede de volta (bateria, gatilhos analógicos, taxa normal)
                            self.sony_simple = buf[0] == 0x01;
                        }
                        if let Some(raw) = parse_hid(&buf, n, ds4, self.bt) {
                            self.raw = raw;
                        }
                        if let Some(b) = parse_sony_battery(&buf, n, ds4, self.bt) {
                            self.battery_percent = b.percent;
                            self.battery_label = None;
                            self.charging = b.charging;
                        }
                    } else if let Some((raw, batt)) = {
                        if let Some((addr, d)) = spi_reply(&buf, n) {
                            self.apply_cal(addr, d);
                        }
                        parse_switch(&buf, n, self.model, &self.cal)
                    } {
                        if buf[0] == 0x30 {
                            self.ns_full = true;
                        }
                        self.raw = raw;
                        if let Some((label, charging)) = batt {
                            self.battery_label = Some(label);
                            self.charging = charging;
                        }
                    }
                }
                Err(_) => return false,
            }
        }
    }
}

// ---------- varredura em segundo plano ----------
//
// Enumerar os dispositivos HID e abrir um controle (identidade, relatório de características do Sony)
// leva de 100 a 200 ms — e muito mais num controle Bluetooth que acabou de desligar. Feito na thread de
// leitura, isso congelava a leitura de TODOS os controles a cada 2 s: toques curtos se perdiam e a
// repetição do direcional saía irregular. A varredura roda numa thread própria e entrega os controles já
// abertos; a thread de leitura só os recebe (\`HidPads::scan\`).

const SCAN_EVERY: Duration = Duration::from_millis(2000);
/// Sem nenhum relatório depois de aberto: interface "fantasma" (o Windows mantém a de um controle
/// Bluetooth desligado por um tempo). É descartado e só volta a ser tentado depois de RETRY_SILENT.
const SILENT_LIMIT: Duration = Duration::from_millis(3000);
const RETRY_SILENT: Duration = Duration::from_secs(15);

#[derive(Default)]
struct ScanState {
    /// Caminhos abertos agora (não reabrir).
    open: std::collections::HashSet<CString>,
    /// Outro caminho de um controle já aberto (não reabrir até algum controle ser perdido).
    skip: std::collections::HashSet<CString>,
    /// Não tentar antes de.
    backoff: std::collections::HashMap<CString, Instant>,
}

#[derive(Clone, Default)]
struct Shared(std::sync::Arc<Mutex<ScanState>>);

impl Shared {
    fn with<R>(&self, f: impl FnOnce(&mut ScanState) -> R) -> R {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

fn scanner(tx: std::sync::mpsc::Sender<Pad>, shared: Shared) {
    let Ok(mut api) = HidApi::new() else { return };
    loop {
        if api.refresh_devices().is_ok() {
            for info in api.device_list() {
                let gamepad = info.usage_page() == 0x01 && matches!(info.usage(), 0x04 | 0x05);
                let model = if gamepad { Model::of_gamepad(info.vendor_id(), info.product_id()) } else { Model::of(info.vendor_id(), info.product_id()) };
                let Some(model) = model else { continue };
                // Sony expõe várias coleções: só a de controle de jogo (Generic Desktop / Game Pad)
                if model.sony() && (info.usage_page() != 0x01 || info.usage() != 0x05) {
                    continue;
                }
                let path = info.path();
                let now = Instant::now();
                let busy = shared.with(|sh| {
                    sh.backoff.retain(|_, t| *t > now);
                    sh.open.contains(path) || sh.skip.contains(path) || sh.backoff.contains_key(path)
                });
                if busy {
                    continue;
                }
                let Some(pad) = open_pad(&api, info, model) else { continue };
                shared.with(|sh| sh.open.insert(pad.path.clone()));
                if tx.send(pad).is_err() {
                    return; // leitura encerrada
                }
            }
        }
        std::thread::sleep(SCAN_EVERY);
    }
}

/// Abre um controle e descobre a identidade dele (ver \`identity\`).
fn open_pad(api: &HidApi, info: &hidapi::DeviceInfo, model: Model) -> Option<Pad> {
    let path = info.path();
    let dev = info.open_device(api).ok()?;
    let bt = matches!(info.bus_type(), BusType::Bluetooth);
    let pnp = crate::devident::from_interface(&path.to_string_lossy());
    let virtual_src = pnp.as_ref().and_then(|p| p.virtual_src);
    let (id, id_source) = identity(
        virtual_src.is_some(),
        || if model.sony() && model.readable() { sony_mac(&dev, model, bt) } else { None },
        serial_id(info.serial_number()),
        pnp.as_ref(),
        path.to_bytes(),
    );
    let key = format!("{}:{id}", if model.sony() { "ps" } else { "ns" });
    let container = pnp.as_ref().and_then(|p| p.container.clone());
    if model.sony() && model.readable() && bt {
        // calibração: faz o controle passar ao relatório completo (com bateria)
        let mut feat = [0u8; 41];
        feat[0] = 0x05;
        let _ = dev.get_feature_report(&mut feat);
    }
    let _ = dev.set_blocking_mode(false);
    Some(Pad {
        key,
        model,
        bt,
        id_source,
        container,
        virtual_src,
        vid: info.vendor_id(),
        pid: info.product_id(),
        path: path.to_owned(),
        dev,
        raw: RawPad::default(),
        tracker: Default::default(),
        combos: Default::default(),
        reports: 0,
        hz: 0.0,
        battery_percent: None,
        battery_label: None,
        charging: false,
        led_applied: None,
        led_retry: None,
        seq: 0,
        led_released: false,
        rumble_until: None,
        player: 0,
        ns_counter: 0,
        ns_full: false,
        ns_tries: 0,
        ns_next: None,
        sony_simple: false,
        sony_full_next: None,
        joined: false,
        opened: Instant::now(),
        alive: false,
        announced: false,
        join_held: None,
        cal: [StickCal::default(); 2],
        cal_step: 0,
        cal_tries: 0,
        cal_next: None,
    })
}

// ---------- conjunto de controles ----------

pub struct HidPads {
    /// Controles abertos pela varredura em segundo plano (`scanner`).
    rx: std::sync::mpsc::Receiver<Pad>,
    shared: Shared,
    pub pads: Vec<Pad>,
    /// Pares de Joy-Cons formados agora (as metades ficam em `pads` com `joined = true`).
    pub pairs: Vec<Pair>,
    /// Pares lembrados (esquerdo, direito): os gravados pela interface + os juntados nesta sessão.
    saved: Vec<(String, String)>,
    saved_gen: u32,
    rate_since: Instant,
}

// ---------- Joy-Cons juntos ----------
//
// Como no Switch: com os dois Joy-Cons conectados, segurar L (no esquerdo) e R (no direito) ao mesmo
// tempo junta os dois num controle só ("Joy-Con (L+R)", chave `ns:<esquerdo>+<direito>`). O par fica
// gravado na interface (`app_settings.joycon_pairs`) e volta a se formar sozinho quando os dois
// reconectam. Separar: Central de Controles (a interface envia a lista sem o par).

/// Tempo segurando L + R para juntar (evita juntar sem querer durante o uso normal).
const JOIN_HOLD: Duration = Duration::from_millis(600);

/// Pares gravados, enviados pela interface (`set_joycon_pairs`).
static SAVED_PAIRS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static SAVED_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Pares de Joy-Cons gravados (esquerdo, direito). Substitui a lista inteira; pares que saírem dela
/// se separam na hora.
#[tauri::command]
pub fn set_joycon_pairs(pairs: Vec<(String, String)>) {
    if let Ok(mut g) = SAVED_PAIRS.lock() {
        *g = pairs;
    }
    SAVED_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Par recém-juntado pelo gesto (evento `joycon-joined`: a interface grava e passa a enviá-lo).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct JoinedPair {
    pub key: String,
    pub left: String,
    pub right: String,
}

#[derive(Default)]
pub struct JoyconEvents {
    /// A lista de controles mudou (par formado ou desfeito).
    pub changed: bool,
    pub joined: Vec<JoinedPair>,
}

pub struct Pair {
    pub key: String,
    left: String,
    right: String,
    pub raw: RawPad,
    pub tracker: crate::input::Tracker,
    pub combos: crate::input::Combos,
    pub hz: f32,
}

pub fn pair_key(left: &str, right: &str) -> String {
    let id = |k: &str| k.strip_prefix("ns:").unwrap_or(k).to_string();
    format!("ns:{}+{}", id(left), id(right))
}

/// Estado do par: o esquerdo dá analógico esquerdo, setas, L/ZL, − e L3; o direito dá A/B/X/Y, R/ZR,
/// +, R3, Home e o analógico direito (que o Joy-Con (R) sozinho informa como analógico esquerdo).
pub fn merge_joycons(l: &RawPad, r: &RawPad) -> RawPad {
    let mut m = RawPad {
        mask: l.mask | r.mask,
        lx: l.lx,
        ly: l.ly,
        rx: r.lx,
        ry: r.ly,
        lt: l.lt,
        rt: r.rt,
        extra: l.extra | r.extra,
        ..Default::default()
    };
    m.compute_dirs(NAV_DEADZONE);
    m
}

/// Um controle do ponto de vista da navegação: um controle HID sozinho ou um par de Joy-Cons.
pub struct Input<'a> {
    pub key: &'a str,
    pub kind: &'static str,
    pub raw: &'a mut RawPad,
    pub tracker: &'a mut crate::input::Tracker,
    pub combos: &'a mut crate::input::Combos,
    pub hz: f32,
}

impl HidPads {
    pub fn new(now: Instant) -> Self {
        let shared = Shared::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let sh = shared.clone();
        let _ = std::thread::Builder::new().name("hid-scan".into()).spawn(move || scanner(tx, sh));
        Self {
            rx,
            shared,
            pads: Vec::new(),
            pairs: Vec::new(),
            saved: Vec::new(),
            saved_gen: u32::MAX,
            rate_since: now,
        }
    }

    /// Controles que navegam (metades de um par ficam de fora; o par entra no lugar delas).
    pub fn inputs(&mut self) -> impl Iterator<Item = Input<'_>> {
        let solo = self.pads.iter_mut().filter(|p| p.announced && !p.joined).map(|p| Input {
            key: &p.key,
            kind: p.model.kind(),
            raw: &mut p.raw,
            tracker: &mut p.tracker,
            combos: &mut p.combos,
            hz: p.hz,
        });
        let pairs = self.pairs.iter_mut().map(|q| Input {
            key: &q.key,
            kind: "nintendo",
            raw: &mut q.raw,
            tracker: &mut q.tracker,
            combos: &mut q.combos,
            hz: q.hz,
        });
        solo.chain(pairs)
    }

    /// Chaves dos controles conectados como a interface os vê (par no lugar das metades).
    pub fn input_keys(&self) -> impl Iterator<Item = &str> {
        self.pads.iter().filter(|p| p.announced && !p.joined).map(|p| p.key.as_str()).chain(self.pairs.iter().map(|q| q.key.as_str()))
    }

    /// Junta / separa Joy-Cons (gesto L + R, pares gravados) e atualiza o estado de cada par.
    /// Chamar depois de `read_all`.
    pub fn joycons(&mut self, now: Instant) -> JoyconEvents {
        let mut ev = JoyconEvents::default();
        let gen = SAVED_GEN.load(std::sync::atomic::Ordering::SeqCst);
        if gen != self.saved_gen {
            self.saved_gen = gen;
            self.saved = SAVED_PAIRS.lock().map(|g| g.clone()).unwrap_or_default();
        }

        // gesto: L segurado num Joy-Con (L) livre e R num Joy-Con (R) livre, juntos por JOIN_HOLD
        for p in self.pads.iter_mut() {
            let bit = match p.model {
                Model::JoyConL => btn::LB,
                Model::JoyConR => btn::RB,
                _ => 0,
            };
            let holding = bit != 0 && !p.joined && p.virtual_src.is_none() && p.raw.mask & bit != 0;
            p.join_held = if holding { p.join_held.or(Some(now)) } else { None };
        }
        let held = |m: Model| -> Vec<(String, Instant)> {
            self.pads.iter().filter(|p| p.model == m).filter_map(|p| Some((p.key.clone(), p.join_held?))).collect()
        };
        let (ls, rs) = (held(Model::JoyConL), held(Model::JoyConR));
        if let Some((l, r)) = ls.iter().find_map(|(l, tl)| {
            rs.iter().find(|(_, tr)| now.duration_since((*tl).max(*tr)) >= JOIN_HOLD).map(|(r, _)| (l.clone(), r.clone()))
        }) {
            self.saved.retain(|(a, b)| *a != l && *b != r);
            self.saved.push((l.clone(), r.clone()));
            ev.joined.push(JoinedPair { key: pair_key(&l, &r), left: l, right: r });
        }

        // desfaz pares sem uma das metades ou que deixaram de estar gravados
        let before: Vec<String> = self.pairs.iter().map(|q| q.key.clone()).collect();
        let pads = &self.pads;
        let saved = &self.saved;
        self.pairs.retain(|q| {
            pads.iter().any(|p| p.key == q.left) && pads.iter().any(|p| p.key == q.right) && saved.iter().any(|(l, r)| *l == q.left && *r == q.right)
        });
        // forma os pares gravados cujas metades estão conectadas e livres
        for (l, r) in self.saved.iter() {
            if self.pairs.iter().any(|q| q.left == *l || q.right == *r) {
                continue;
            }
            let ok = |k: &str, m: Model| self.pads.iter().any(|p| p.key == k && p.model == m && p.announced && p.virtual_src.is_none());
            if ok(l, Model::JoyConL) && ok(r, Model::JoyConR) {
                self.pairs.push(Pair {
                    key: pair_key(l, r),
                    left: l.clone(),
                    right: r.clone(),
                    raw: RawPad::default(),
                    tracker: Default::default(),
                    combos: Default::default(),
                    hz: 0.0,
                });
            }
        }
        let changed = before.len() != self.pairs.len() || self.pairs.iter().zip(&before).any(|(q, k)| q.key != *k);

        // metades: marca, zera o estado de navegação ao entrar/sair do par
        for p in self.pads.iter_mut() {
            let joined = self.pairs.iter().any(|q| q.left == p.key || q.right == p.key);
            if joined != p.joined {
                p.joined = joined;
                p.tracker = Default::default();
                p.combos = Default::default();
                p.join_held = None;
            }
        }
        if changed {
            self.assign_players();
        }

        for q in self.pairs.iter_mut() {
            let (Some(l), Some(r)) = (self.pads.iter().find(|p| p.key == q.left), self.pads.iter().find(|p| p.key == q.right)) else { continue };
            q.raw = merge_joycons(&l.raw, &r.raw);
            q.hz = l.hz.min(r.hz);
        }
        if changed {
            ev.changed = true;
            self.publish();
        }
        ev
    }

    /// Luzes de jogador: cada controle Nintendo (par conta como um) com um número próprio; as duas
    /// metades de um par acendem o mesmo número.
    fn assign_players(&mut self) {
        let mut used: Vec<u8> = Vec::new();
        for i in 0..self.pads.len() {
            let p = &self.pads[i];
            let is_right_half = p.joined && self.pairs.iter().any(|q| q.right == p.key);
            if p.model.sony() || is_right_half {
                continue;
            }
            let n = if p.player > 0 && !used.contains(&p.player) { p.player } else { (1..=4u8).find(|n| !used.contains(n)).unwrap_or(4) };
            used.push(n);
            self.pads[i].set_player(n);
        }
        for q in &self.pairs {
            let Some(n) = self.pads.iter().find(|p| p.key == q.left).map(|p| p.player) else { continue };
            if let Some(r) = self.pads.iter_mut().find(|p| p.key == q.right) {
                r.set_player(n);
            }
        }
    }

    fn pair_info(&self, q: &Pair) -> Option<HidInfo> {
        let l = self.pads.iter().find(|p| p.key == q.left)?;
        let r = self.pads.iter().find(|p| p.key == q.right)?;
        // bateria: a da metade mais fraca
        let rank = |b: Option<&str>| match b {
            Some("Cheia") => 4,
            Some("Média") => 3,
            Some("Baixa") => 2,
            Some(_) => 1,
            None => 5,
        };
        let battery_label = if rank(l.battery_label) <= rank(r.battery_label) { l.battery_label } else { r.battery_label };
        Some(HidInfo {
            key: q.key.clone(),
            model: Model::JoyConPair,
            bt: l.bt && r.bt,
            id_source: l.id_source,
            container: None,
            virtual_src: None,
            vid: l.vid,
            pid: l.pid,
            battery_percent: None,
            battery_label,
            charging: l.charging && r.charging,
            player: (l.player > 0).then_some(l.player),
        })
    }

    /// Recebe os controles que a varredura em segundo plano abriu e anuncia os que já responderam.
    /// Barato (não bloqueia): pode ser chamado a cada volta da leitura. Devolve `true` se a lista mudou.
    pub fn scan(&mut self, _now: Instant) -> bool {
        let mut changed = false;
        while let Ok(mut p) = self.rx.try_recv() {
            // o mesmo controle por outro caminho (outra coleção, USB e Bluetooth ao mesmo tempo): mantém o primeiro
            let same_physical = |q: &Pad| q.key == p.key
                || (p.virtual_src.is_none() && q.virtual_src.is_none() && p.container.is_some() && q.container == p.container);
            if self.pads.iter().any(same_physical) {
                self.shared.with(|sh| {
                    sh.open.remove(&p.path);
                    sh.skip.insert(p.path.clone());
                });
                continue;
            }
            if !p.model.sony() {
                // menor número de jogador livre entre os Nintendo conectados
                p.player = (1..=4u8).find(|n| !self.pads.iter().any(|q| !q.model.sony() && q.player == *n)).unwrap_or(4);
            }
            self.pads.push(p);
        }
        // só aparece como conectado depois do primeiro relatório: o Windows pode manter a interface de um
        // controle Bluetooth já desligado, que abre normalmente mas nunca envia nada
        for p in self.pads.iter_mut().filter(|p| p.alive && !p.announced) {
            p.announced = true;
            changed = true;
        }
        if changed {
            self.publish();
        }
        changed
    }

    /// Lê todos os controles; remove os desconectados. Devolve as chaves perdidas.
    pub fn read_all(&mut self, now: Instant) -> Vec<String> {
        let mut lost = Vec::new();
        let mut dead: Vec<(CString, bool)> = Vec::new();
        for p in self.pads.iter_mut() {
            p.ns_init(now);
            p.ns_cal(now);
            p.sony_full(now);
            // abriu mas não mandou nada em SILENT_LIMIT: interface "fantasma" de um controle desligado
            if !p.alive && now.duration_since(p.opened) >= SILENT_LIMIT {
                dead.push((p.path.clone(), false));
                continue;
            }
            p.rumble_tick(now);
            if !p.read() {
                if p.announced {
                    lost.push(p.key.clone()); // só os que a interface já via contam como perdidos
                }
                dead.push((p.path.clone(), p.alive));
            }
        }
        if !dead.is_empty() {
            self.pads.retain(|p| !dead.iter().any(|(d, _)| *d == p.path));
            self.shared.with(|sh| {
                for (path, was_alive) in &dead {
                    sh.open.remove(path);
                    if !was_alive {
                        // nunca respondeu: não reabre a cada 2 s (o controle "piscava" como conectado)
                        sh.backoff.insert(path.clone(), now + RETRY_SILENT);
                    }
                }
                sh.skip.clear(); // o caminho duplicado de um controle perdido pode ser o único agora
            });
        }
        let el = now.duration_since(self.rate_since);
        let mut batt_changed = false;
        if el >= Duration::from_secs(1) {
            for p in self.pads.iter_mut() {
                p.hz = (p.reports as f32 / el.as_secs_f32()).round();
                p.reports = 0;
            }
            self.rate_since = now;
            batt_changed = true; // bateria/estado publicados 1x por segundo
        }
        if !lost.is_empty() || batt_changed {
            self.publish();
        }
        lost
    }

    pub fn pad_mut(&mut self, key: &str) -> Option<&mut Pad> {
        self.pads.iter_mut().find(|p| p.key == key)
    }

    fn publish(&self) {
        if let Ok(mut g) = PADS.lock() {
            *g = self.pads.iter().filter(|p| p.announced && !p.joined).map(Pad::info)
                .chain(self.pairs.iter().filter_map(|q| self.pair_info(q)))
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns_report(r: u8, s: u8, l: u8, lstick: (u16, u16), batt: u8) -> Vec<u8> {
        let mut b = vec![0u8; 49];
        b[0] = 0x30;
        b[2] = batt;
        b[3] = r;
        b[4] = s;
        b[5] = l;
        let put = |b: &mut Vec<u8>, o: usize, (x, y): (u16, u16)| {
            b[o] = (x & 0xFF) as u8;
            b[o + 1] = ((x >> 8) & 0x0F) as u8 | ((y & 0x0F) << 4) as u8;
            b[o + 2] = (y >> 4) as u8;
        };
        put(&mut b, 6, lstick);
        put(&mut b, 9, (2048, 2048));
        b
    }

    #[test]
    fn identifies_models() {
        assert_eq!(Model::of(0x057E, 0x2009), Some(Model::SwitchPro));
        assert_eq!(Model::of(0x057E, 0x2006), Some(Model::JoyConL));
        assert_eq!(Model::of(0x057E, 0x2007), Some(Model::JoyConR));
        assert_eq!(Model::of(0x054C, 0x0CE6), Some(Model::DualSense));
        assert_eq!(Model::of(0x057E, 0x200E), None); // grip de carga: não é lido
        // DualSense e DualShock 4 só pelo PID, nunca pela conexão ou pelo nome
        assert_eq!(Model::of(0x054C, 0x05C4), Some(Model::DualShock4));
        assert_eq!(Model::of(0x054C, 0x09CC), Some(Model::DualShock4));
        assert_eq!(Model::of(0x054C, 0x0DF2), Some(Model::DualSenseEdge));
        // controle Sony desconhecido: listado como não identificado, sem leitura
        assert_eq!(Model::of_gamepad(0x054C, 0x0E5F), Some(Model::SonyUnknown));
        assert!(!Model::SonyUnknown.readable() && Model::SonyUnknown.sony());
        assert_eq!(Model::of_gamepad(0x1234, 0x0001), None);
    }

    #[test]
    fn identity_prefers_physical_ids_and_isolates_virtual() {
        use crate::devident::{Bus, PnpInfo};
        let pnp = |virt: Option<&'static str>, addr: Option<&str>| PnpInfo {
            instance: "HID\\VID_054C&PID_05C4\\1".into(),
            container: Some("c0ffee00-0000-0000-0000-000000000001".into()),
            bus: Bus::Usb,
            address: addr.map(Into::into),
            virtual_src: virt,
            vid: Some(0x054C),
            pid: Some(0x05C4),
        };
        // DS4 virtual (ViGEm) que repete o MAC do DualSense real: identidade própria
        let (id, src) = identity(true, || Some("50ee3242a709".into()), None, Some(&pnp(Some("ViGEm"), None)), b"p");
        assert!(id.starts_with('v') && src == "virtual" && id != "50ee3242a709");
        assert_eq!(identity(false, || Some("aa".into()), Some("bb".into()), None, b"p"), ("aa".into(), "mac"));
        assert_eq!(identity(false, || None, Some("bb".into()), None, b"p"), ("bb".into(), "serial"));
        assert_eq!(identity(false, || None, None, Some(&pnp(None, Some("3032"))), b"p"), ("3032".into(), "serial"));
        assert_eq!(identity(false, || None, None, Some(&pnp(None, None)), b"p").1, "container");
        assert_eq!(identity(false, || None, None, None, b"p").1, "path");
    }

    #[test]
    fn distinct_ids_for_same_model() {
        assert_eq!(serial_id(Some("98:B6:E9:12:34:56")).as_deref(), Some("98b6e9123456"));
        assert_eq!(serial_id(Some("")), None);
        assert_eq!(serial_id(Some("000000000000")), None);
        assert_ne!(path_id(b"\\\\?\\HID#VID_054C&PID_0CE6#1"), path_id(b"\\\\?\\HID#VID_054C&PID_0CE6#2"));
        assert_eq!(mac_rev(&[0x56, 0x34, 0x12, 0xE9, 0xB6, 0x98]).as_deref(), Some("98b6e9123456"));
        assert_eq!(mac_rev(&[0; 6]), None);
    }

    #[test]
    fn pro_controller_buttons_by_position() {
        // A do Switch (direita) = voltar; B do Switch (baixo) = confirmar; + = menu
        let (raw, batt) = parse_switch(&ns_report(0x04 | 0x40, 0x02, 0x02, (2048, 2048), 0x91), 49, Model::SwitchPro, &[StickCal::default(); 2]).unwrap();
        assert!(raw.mask & btn::A != 0, "B do Switch (baixo) = btn::A");
        assert!(raw.mask & btn::RB != 0 && raw.mask & btn::START != 0 && raw.mask & btn::UP != 0);
        assert!(raw.dirs[2], "direcional para cima navega");
        assert_eq!(batt, Some(("Cheia", true)));
        let (raw, _) = parse_switch(&ns_report(0x08, 0, 0, (2048, 2048), 0x40), 49, Model::SwitchPro, &[StickCal::default(); 2]).unwrap();
        assert!(raw.mask & btn::B != 0, "A do Switch (direita) = btn::B");
    }

    #[test]
    fn pro_controller_stick_up_is_negative_y() {
        let (raw, _) = parse_switch(&ns_report(0, 0, 0, (2048, 3500), 0x80), 49, Model::SwitchPro, &[StickCal::default(); 2]).unwrap();
        assert!(raw.ly < -0.9 && raw.dirs[2], "alavanca para cima = navegar para cima");
        let (raw, _) = parse_switch(&ns_report(0, 0, 0, (600, 2048), 0x80), 49, Model::SwitchPro, &[StickCal::default(); 2]).unwrap();
        assert!(raw.lx < -0.9 && raw.dirs[0], "alavanca para a esquerda");
    }

    #[test]
    fn joycons_alone() {
        let (l, batt) = parse_switch(&ns_report(0, 0x01, 0x08 | 0x40, (2048, 2048), 0x40), 49, Model::JoyConL, &[StickCal::default(); 2]).unwrap();
        assert!(l.mask & btn::LEFT != 0 && l.mask & btn::LB != 0 && l.mask & btn::SELECT != 0);
        assert_eq!(batt, Some(("Baixa", false)));
        // Joy-Con (R): botões da direita; o analógico dele (bytes 9–11) vai para a navegação
        let mut b = ns_report(0x04, 0x02, 0, (2048, 2048), 0x60);
        let y: u16 = 3500; // para cima
        b[10] = 0x08 | ((y & 0x0F) << 4) as u8;
        b[11] = (y >> 4) as u8;
        let (r, _) = parse_switch(&b, 49, Model::JoyConR, &[StickCal::default(); 2]).unwrap();
        assert!(r.mask & btn::A != 0 && r.mask & btn::START != 0);
        assert!(r.dirs[2], "alavanca do Joy-Con (R) navega");
    }

    /// Controles reais: `cargo test --lib hidpads::tests::live_list -- --ignored --nocapture`
    /// Lista os controles HID abertos (chave única de cada um), e o que chega deles por 4 s.
    #[test]
    #[ignore]
    fn live_list() {
        let t0 = Instant::now();
        let mut h = HidPads::new(t0);
        let s0 = Instant::now();
        h.scan(t0);
        println!("varredura inicial: {} ms", s0.elapsed().as_millis());
        for p in &h.pads {
            println!("{} · {} · {}", p.key, p.model.name(), if p.bt { "Bluetooth" } else { "USB" });
        }
        let mut masks = std::collections::HashMap::<String, u16>::new();
        let secs = std::env::var("NX_LIVE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
        while t0.elapsed() < Duration::from_secs(secs) {
            let s0 = Instant::now();
            let ch = h.scan(Instant::now());
            if s0.elapsed() > Duration::from_millis(50) { println!("[{:>4.1}s] varredura lenta: {} ms", t0.elapsed().as_secs_f32(), s0.elapsed().as_millis()); }
            let r0 = Instant::now();
            let lost = h.read_all(Instant::now());
            if !lost.is_empty() { println!("[{:>4.1}s] perdido: {:?} (leitura {} ms)", t0.elapsed().as_secs_f32(), lost, r0.elapsed().as_millis()); }
            if ch {
                for p in &h.pads { println!("[{:>4.1}s] conectado: {} · {}", t0.elapsed().as_secs_f32(), p.key, p.model.name()); }
            }
            h.read_all(Instant::now());
            for p in &h.pads {
                *masks.entry(p.key.clone()).or_default() |= p.raw.mask;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        for p in h.pads.iter().filter(|p| !p.model.sony()) {
            println!("{} calibração (etapa {}): esq {:?} / dir {:?} · agora lx {:.2} ly {:.2}", p.key, p.cal_step, p.cal[0], p.cal[1], p.raw.lx, p.raw.ly);
        }
        for i in PADS.lock().unwrap().iter() {
            println!("{} → botões vistos {:#06x}, bateria {:?}/{:?}, jogador {:?}", i.key, masks.get(&i.key).unwrap_or(&0), i.battery_percent, i.battery_label, i.player);
        }
        println!("controles HID: {}", h.pads.len());
    }

    #[test]
    fn joycon_pair_merges_both_halves() {
        // metade esquerda: L, seta para cima e analógico; direita: A, R, + e o analógico dela (como esquerdo)
        let l = RawPad { mask: btn::LB | btn::UP | btn::SELECT, lx: -0.8, ly: 0.1, lt: 0.0, ..Default::default() };
        let r = RawPad { mask: btn::A | btn::RB | btn::START | btn::RT, lx: 0.3, ly: -0.9, rt: 1.0, extra: 0x01, ..Default::default() };
        let m = merge_joycons(&l, &r);
        assert_eq!(m.mask, btn::LB | btn::UP | btn::SELECT | btn::A | btn::RB | btn::START | btn::RT);
        assert_eq!((m.lx, m.ly, m.rx, m.ry), (-0.8, 0.1, 0.3, -0.9), "analógico do Joy-Con (R) vira o direito");
        assert_eq!((m.lt, m.rt, m.extra), (0.0, 1.0, 0x01));
        assert!(m.dirs[0], "analógico esquerdo inclinado navega para a esquerda");
    }

    #[test]
    fn joycon_pair_key_combines_both_identities() {
        assert_eq!(pair_key("ns:7820a54734f6", "ns:7820a547d9ec"), "ns:7820a54734f6+7820a547d9ec");
        assert_eq!(Model::JoyConPair.id(), "joycon-pair");
        assert_eq!(Model::JoyConPair.kind(), "nintendo");
        assert_eq!(Model::of(0x057E, 0x2006), Some(Model::JoyConL), "o par nunca vem de um dispositivo HID");
    }

    fn pack12(a: u16, b: u16) -> [u8; 3] {
        [(a & 0xFF) as u8, ((a >> 8) as u8 & 0x0F) | ((b & 0x0F) << 4) as u8, (b >> 4) as u8]
    }

    #[test]
    fn stick_calibration_from_spi() {
        // esquerdo: acima, centro, abaixo; direito: centro, abaixo, acima
        let mut f = Vec::new();
        f.extend(pack12(1300, 1250)); f.extend(pack12(1850, 2210)); f.extend(pack12(1200, 1100));
        f.extend(pack12(2100, 1950)); f.extend(pack12(1150, 1180)); f.extend(pack12(1320, 1290));
        let [l, r] = cal_from_spi(SPI_CAL_FACTORY, &f);
        let l = l.unwrap();
        assert_eq!((l.x.center, l.x.below, l.x.above), (1850, 1200, 1300));
        assert_eq!((l.y.center, l.y.below, l.y.above), (2210, 1100, 1250));
        let r = r.unwrap();
        assert_eq!((r.x.center, r.x.below, r.x.above), (2100, 1150, 1320));
        // memória apagada (0xFF) não é calibração
        assert_eq!(cal_from_spi(SPI_CAL_FACTORY, &[0xFF; 18]), [None, None]);
        // do usuário só vale com a marca B2 A1
        let mut u = vec![0xFFu8; 22];
        u[0] = 0xB2; u[1] = 0xA1; u[2..11].copy_from_slice(&f[0..9]);
        let [ul, ur] = cal_from_spi(SPI_CAL_USER, &u);
        assert_eq!(ul, Some(l));
        assert_eq!(ur, None);
    }

    #[test]
    fn calibrated_stick_rests_at_zero_and_reaches_the_ends() {
        // Joy-Con com centro deslocado (1850, 2210): sem calibração parecia inclinado
        let c = StickCal { x: AxisCal { center: 1850, below: 1200, above: 1300 }, y: AxisCal { center: 2210, below: 1100, above: 1250 } };
        let cal = [c, StickCal::default()];
        let (raw, _) = parse_switch(&ns_report(0, 0, 0, (1850, 2210), 0x80), 49, Model::JoyConL, &cal).unwrap();
        assert_eq!((raw.lx, raw.ly), (0.0, 0.0));
        let (raw, _) = parse_switch(&ns_report(0, 0, 0, (1850 + 1300, 2210), 0x80), 49, Model::JoyConL, &cal).unwrap();
        assert_eq!(raw.lx, 1.0);
        let (raw, _) = parse_switch(&ns_report(0, 0, 0, (1850 - 600, 2210 + 625), 0x80), 49, Model::JoyConL, &cal).unwrap();
        assert_eq!((raw.lx, raw.ly), (-0.5, -0.5), "metade do curso para a esquerda e para cima");
        let (default, _) = parse_switch(&ns_report(0, 0, 0, (1850, 2210), 0x80), 49, Model::JoyConL, &[StickCal::default(); 2]).unwrap();
        assert!(default.lx.abs() > 0.1 || default.ly.abs() > 0.1, "o padrão errava este controle");
    }

    #[test]
    fn spi_reply_parsing() {
        let mut b = vec![0u8; 49];
        b[0] = 0x21; b[13] = 0x90; b[14] = 0x10;
        b[15..19].copy_from_slice(&SPI_CAL_FACTORY.to_le_bytes());
        b[19] = 18;
        b[20] = 0xAB;
        let (addr, d) = spi_reply(&b, 49).unwrap();
        assert_eq!((addr, d.len(), d[0]), (SPI_CAL_FACTORY, 18, 0xAB));
        b[13] = 0x00; // NACK
        assert!(spi_reply(&b, 49).is_none());
    }

    #[test]
    fn subcommand_layout() {
        let p = ns_subcommand(3, 0x03, &[0x30]);
        assert_eq!(p.len(), 49);
        assert_eq!((p[0], p[1], p[10], p[11]), (0x01, 3, 0x03, 0x30));
        assert_eq!(&p[2..10], &NS_NEUTRAL_RUMBLE);
    }
}
