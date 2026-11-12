//! Identidade FÍSICA dos controles no Windows (Configuration Manager / PnP).
//!
//! Por que existe: o Windows apresenta um mesmo controle por várias interfaces (HID, XInput/XUSB,
//! serviços GATT do Bluetooth LE, coleções HID...) e programas como DS4Windows/ViGEm, Logitech G HUB,
//! Steam ou o "XBOX Synthetic Gamepad" do próprio Windows criam controles VIRTUAIS que espelham um
//! controle real. Contar cada interface — ou cada slot do XInput — como um controle gerava entradas
//! duplicadas e modelos trocados (ex.: o DualShock 4 virtual do DS4Windows aparecia ao lado do DualSense).
//!
//! O que este módulo responde, para uma interface de dispositivo:
//!  - `container`: o ContainerId do Windows — o mesmo para TODAS as interfaces de um aparelho físico;
//!  - `address`: identificador estável do aparelho — endereço Bluetooth (MAC) ou número de série USB;
//!  - `virtual_src`: quem criou o dispositivo, quando ele é virtual (pelos drivers dos ancestrais);
//!  - `bus`, VID e PID.
//!
//! E, para o XInput (que só fala em "slot 0–3"), qual aparelho físico ocupa cada slot (`xinput_slots`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    Usb,
    Bluetooth,
    BluetoothLe,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PnpInfo {
    /// Device Instance ID do nó de onde a consulta partiu (interface HID/XUSB).
    pub instance: String,
    /// ContainerId (GUID, minúsculas) — `None` para o contêiner nulo do sistema.
    pub container: Option<String>,
    pub bus: Bus,
    /// MAC (12 hex) pelo Bluetooth ou número de série USB (instância sem "&").
    pub address: Option<String>,
    pub virtual_src: Option<&'static str>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

impl PnpInfo {
    /// Identificador estável do aparelho físico, do mais forte ao mais fraco, com a origem.
    pub fn stable_id(&self) -> Option<(String, &'static str)> {
        if self.virtual_src.is_some() {
            return Some((format!("v{}", short_hash(self.instance.to_ascii_lowercase().as_bytes())), "virtual"));
        }
        if let Some(a) = &self.address {
            return Some((a.clone(), if matches!(self.bus, Bus::Usb) { "serial" } else { "mac" }));
        }
        self.container.as_ref().map(|c| (format!("c{}", short_hash(c.as_bytes())), "container"))
    }
}

/// Hash curto (FNV-1a, 8 hex).
pub fn short_hash(b: &[u8]) -> String {
    let mut h: u32 = 0x811C_9DC5;
    for &x in b {
        h ^= x as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

// ---------- análise dos identificadores (sem Windows; testável) ----------

fn hex16(s: &str) -> Option<u16> {
    let h: String = s.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    if h.len() < 4 {
        return None;
    }
    u16::from_str_radix(&h[h.len() - 4..], 16).ok()
}

/// VID/PID em qualquer formato de instância: "VID_045E&PID_0B12" (USB/HID) e "VID&02045E_PID&0B13"
/// (Bluetooth: os primeiros dígitos indicam a origem do VID; valem os 4 últimos).
pub fn parse_vid_pid(id: &str) -> (Option<u16>, Option<u16>) {
    let u = id.to_ascii_uppercase();
    let grab = |tag: &str| -> Option<u16> {
        let i = u.find(tag)? + tag.len();
        hex16(&u[i..])
    };
    let vid = grab("VID_").or_else(|| grab("VID&"));
    let pid = grab("PID_").or_else(|| grab("PID&"));
    (vid, pid)
}

/// Endereço Bluetooth (12 hex, minúsculas) numa instância BTHENUM/BTHLE/BTHLEDEVICE ou num caminho HID
/// de dispositivo Bluetooth. Ignora o conteúdo entre chaves (UUIDs de serviço, que também têm 12 hex).
pub fn bt_address(id: &str) -> Option<String> {
    let mut clean = String::with_capacity(id.len());
    let mut depth = 0;
    for c in id.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ if depth == 0 => clean.push(c),
            _ => {}
        }
    }
    clean
        .split(|c: char| matches!(c, '\\' | '_' | '&' | '#'))
        .rev()
        .find(|t| t.len() == 12 && t.chars().all(|c| c.is_ascii_hexdigit()) && t.chars().any(|c| c != '0') && !t.eq_ignore_ascii_case("ffffffffffff"))
        .map(|t| t.to_ascii_lowercase())
}

/// Número de série de um dispositivo USB: o Windows usa o serial como último segmento da instância
/// quando o aparelho informa um único; sem serial, o segmento é gerado ("7&2a3b&0&2", sempre com "&").
pub fn usb_serial(instance: &str) -> Option<String> {
    let mut parts = instance.split('\\');
    let en = parts.next()?;
    if !en.eq_ignore_ascii_case("USB") {
        return None;
    }
    let hw = parts.next()?;
    if hw.to_ascii_uppercase().contains("&MI_") {
        return None; // interface de um dispositivo composto: o serial está no pai
    }
    let last = parts.next()?;
    (last.len() >= 4 && !last.contains('&') && last.chars().all(|c| c.is_ascii_alphanumeric())).then(|| last.to_ascii_lowercase())
}

pub fn bus_of(instance: &str) -> Option<Bus> {
    let u = instance.to_ascii_uppercase();
    if u.starts_with("BTHLEDEVICE\\") || u.starts_with("BTHLE\\") {
        Some(Bus::BluetoothLe)
    } else if u.starts_with("BTHENUM\\") || u.starts_with("BTH\\") {
        Some(Bus::Bluetooth)
    } else if u.starts_with("USB\\") {
        Some(Bus::Usb)
    } else {
        None
    }
}

/// Origem virtual pelo nó (instância + serviço do driver). `None` = nada indica virtual.
pub fn virtual_marker(instance: &str, service: &str) -> Option<&'static str> {
    let i = instance.to_ascii_lowercase();
    let s = service.to_ascii_lowercase();
    let has = |needle: &str| i.contains(needle) || s.contains(needle);
    if has("vigem") || s == "nssvpd" || i.starts_with("root\\usb\\") || has("nefarius") {
        Some("ViGEm")
    } else if has("scpvbus") || has("scpvirtualbus") {
        Some("SCP")
    } else if has("vjoy") {
        Some("vJoy")
    } else if has("logi_joy") || has("logi_vir") {
        Some("Logitech G HUB")
    } else if i.starts_with("swd\\syntheticgamepad") || has("syntheticgamepad") {
        Some("Windows (Synthetic Gamepad)")
    } else if has("steamstreaming") || has("steam_virtual") || has("valvevirtual") {
        Some("Steam")
    } else if has("parsecvd") || has("parsecvirtual") {
        Some("Parsec")
    } else if has("rdpbus") || has("tsusbhub") {
        Some("Área de Trabalho Remota")
    } else {
        None
    }
}

/// Nós da cadeia (do dispositivo até a raiz): instância e serviço. Resume a identidade.
pub fn summarize(chain: &[(String, String)], container: Option<String>) -> PnpInfo {
    let instance = chain.first().map(|c| c.0.clone()).unwrap_or_default();
    let virtual_src = chain.iter().find_map(|(i, s)| virtual_marker(i, s));
    let mut bus = Bus::Other;
    let mut address = None;
    let (mut vid, mut pid) = (None, None);
    for (id, _) in chain {
        if vid.is_none() {
            let (v, p) = parse_vid_pid(id);
            if v.is_some() && p.is_some() {
                vid = v;
                pid = p;
            }
        }
        if let Some(b) = bus_of(id) {
            if bus == Bus::Other {
                bus = b;
            }
            if address.is_none() {
                address = match b {
                    Bus::Usb => usb_serial(id),
                    Bus::Bluetooth | Bus::BluetoothLe => bt_address(id),
                    Bus::Other => None,
                };
            }
            // o primeiro nó Bluetooth/USB "de aparelho" basta; acima dele ficam hubs e adaptadores
            if address.is_some() || (b == Bus::Usb && !id.to_ascii_uppercase().contains("&MI_")) {
                break;
            }
        }
    }
    // Hubs/controladores USB acima de um aparelho sem serial não identificam o controle.
    let container = container.filter(|c| c != "00000000-0000-0000-ffff-ffffffffffff" && c != "00000000-0000-0000-0000-000000000000");
    PnpInfo { instance, container, bus, address, virtual_src, vid, pid }
}

// ---------- XInput: qual aparelho físico ocupa cada slot ----------

/// Candidato a ocupar um slot do XInput (um aparelho físico com interface XInput).
#[derive(Clone, Debug, PartialEq)]
pub struct XCand {
    pub info: PnpInfo,
    /// Slot informado pelo próprio driver (luz do anel), quando disponível.
    pub led_slot: Option<u32>,
}

/// Associação de um slot conectado: índice do candidato e se a escolha foi por eliminação entre
/// aparelhos idênticos (`ambiguous`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XMatch {
    pub slot: u32,
    pub cand: Option<usize>,
    pub ambiguous: bool,
}

/// Associa slots conectados (com o VID/PID que o XInput informa, quando informa) a candidatos.
/// Ordem de confiança: luz do anel → VID/PID único → único restante. Nunca associa dois slots ao mesmo
/// aparelho; sem correspondência segura o slot fica sem aparelho (identidade pelo slot, como antes).
pub fn assign_slots(slots: &[(u32, Option<(u16, u16)>)], cands: &[XCand]) -> Vec<XMatch> {
    let mut used = vec![false; cands.len()];
    let mut out: Vec<XMatch> = slots.iter().map(|(s, _)| XMatch { slot: *s, cand: None, ambiguous: false }).collect();
    let same = |c: &XCand, vp: Option<(u16, u16)>| match (vp, c.info.vid, c.info.pid) {
        (Some((v, p)), Some(cv), Some(cp)) => v == cv && p == cp,
        (None, _, _) => true,
        _ => false,
    };
    // 1) luz do anel (o driver diz o slot), conferida com o VID/PID
    for (k, (s, vp)) in slots.iter().enumerate() {
        let hits: Vec<usize> = (0..cands.len()).filter(|&i| !used[i] && cands[i].led_slot == Some(*s) && same(&cands[i], *vp)).collect();
        if hits.len() == 1 {
            used[hits[0]] = true;
            out[k].cand = Some(hits[0]);
        }
    }
    // 2) VID/PID
    for (k, (_, vp)) in slots.iter().enumerate() {
        if out[k].cand.is_some() || vp.is_none() {
            continue;
        }
        let hits: Vec<usize> = (0..cands.len()).filter(|&i| !used[i] && same(&cands[i], *vp)).collect();
        if let Some(&first) = hits.first() {
            used[first] = true;
            out[k].cand = Some(first);
            out[k].ambiguous = hits.len() > 1;
        }
    }
    // 3) um slot e um aparelho restantes
    let free_slots: Vec<usize> = (0..out.len()).filter(|&k| out[k].cand.is_none()).collect();
    let free_cands: Vec<usize> = (0..cands.len()).filter(|&i| !used[i]).collect();
    if free_slots.len() == 1 && free_cands.len() == 1 && slots[free_slots[0]].1.is_none() {
        out[free_slots[0]].cand = Some(free_cands[0]);
    }
    out
}

/// Identidade resolvida de um slot do XInput.
#[derive(Clone, Debug, PartialEq)]
pub struct XIdent {
    pub slot: u32,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub info: Option<PnpInfo>,
    pub ambiguous: bool,
}

impl XIdent {
    /// Chave do dispositivo: "xbox:<id físico>"; sem aparelho identificado, "xbox:<slot>" (formato antigo).
    pub fn key(&self) -> String {
        match self.info.as_ref().and_then(|i| i.stable_id()) {
            Some((id, _)) => format!("xbox:{id}"),
            None => format!("xbox:{}", self.slot),
        }
    }
    pub fn id_source(&self) -> &'static str {
        self.info.as_ref().and_then(|i| i.stable_id()).map(|x| x.1).unwrap_or("slot")
    }
    pub fn virtual_src(&self) -> Option<&'static str> {
        self.info.as_ref().and_then(|i| i.virtual_src)
    }
}

struct XCache {
    slots: Vec<u32>,
    at: Instant,
    out: Vec<XIdent>,
}
static XCACHE: Mutex<Option<XCache>> = Mutex::new(None);

/// Identidade de cada slot conectado. Consulta o PnP só quando o conjunto de slots muda (ou a cada
/// 10 s; 2 s enquanto algum slot ainda não foi associado — o Windows termina de enumerar depois).
pub fn xinput_slots(connected: &[u32]) -> Vec<XIdent> {
    if connected.is_empty() {
        return Vec::new();
    }
    if let Ok(g) = XCACHE.lock() {
        if let Some(c) = g.as_ref() {
            let unresolved = c.out.iter().any(|x| x.info.is_none());
            let ttl = if unresolved { Duration::from_secs(2) } else { Duration::from_secs(10) };
            if c.slots == connected && c.at.elapsed() < ttl {
                return c.out.clone();
            }
        }
    }
    xinput_slots_refresh(connected)
}

/// Como `xinput_slots`, mas sempre consulta o PnP (a leitura de entrada chama quando um slot muda).
pub fn xinput_slots_refresh(connected: &[u32]) -> Vec<XIdent> {
    let out = if connected.is_empty() { Vec::new() } else { resolve(connected) };
    if let Ok(mut g) = XCACHE.lock() {
        *g = Some(XCache { slots: connected.to_vec(), at: Instant::now(), out: out.clone() });
    }
    out
}

#[cfg(windows)]
fn resolve(connected: &[u32]) -> Vec<XIdent> {
    let caps: Vec<(u32, Option<(u16, u16)>)> = connected.iter().map(|&s| (s, win::xinput_vid_pid(s))).collect();
    let cands = win::xinput_candidates();
    let m = assign_slots(&caps, &cands);
    m.into_iter()
        .zip(caps)
        .map(|(m, (_, vp))| {
            let info = m.cand.map(|i| cands[i].info.clone());
            XIdent {
                slot: m.slot,
                vid: vp.map(|v| v.0).or(info.as_ref().and_then(|i| i.vid)),
                pid: vp.map(|v| v.1).or(info.as_ref().and_then(|i| i.pid)),
                info,
                ambiguous: m.ambiguous,
            }
        })
        .collect()
}

#[cfg(not(windows))]
fn resolve(connected: &[u32]) -> Vec<XIdent> {
    connected.iter().map(|&slot| XIdent { slot, vid: None, pid: None, info: None, ambiguous: false }).collect()
}

/// Identidade de uma interface de dispositivo (ex.: o caminho HID que o hidapi abre).
#[cfg(windows)]
pub fn from_interface(path: &str) -> Option<PnpInfo> {
    win::from_interface(path)
}
#[cfg(not(windows))]
pub fn from_interface(_path: &str) -> Option<PnpInfo> {
    None
}

#[cfg(windows)]
mod win {
    use super::{summarize, PnpInfo, XCand};
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid(u32, u16, u16, [u8; 8]);
    #[repr(C)]
    struct PropKey {
        fmtid: Guid,
        pid: u32,
    }

    #[link(name = "cfgmgr32")]
    extern "system" {
        fn CM_Locate_DevNodeW(inst: *mut u32, id: *const u16, flags: u32) -> u32;
        fn CM_Get_Parent(parent: *mut u32, inst: u32, flags: u32) -> u32;
        fn CM_Get_Device_IDW(inst: u32, buf: *mut u16, len: u32, flags: u32) -> u32;
        fn CM_Get_DevNode_PropertyW(inst: u32, key: *const PropKey, ty: *mut u32, buf: *mut u8, size: *mut u32, flags: u32) -> u32;
        fn CM_Get_Device_Interface_PropertyW(iface: *const u16, key: *const PropKey, ty: *mut u32, buf: *mut u8, size: *mut u32, flags: u32) -> u32;
        fn CM_Get_Device_Interface_List_SizeW(len: *mut u32, class: *const Guid, id: *const u16, flags: u32) -> u32;
        fn CM_Get_Device_Interface_ListW(class: *const Guid, id: *const u16, buf: *mut u16, len: u32, flags: u32) -> u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const i8) -> *mut c_void;
        fn CreateFileW(name: *const u16, access: u32, share: u32, sa: *mut c_void, disp: u32, flags: u32, template: *mut c_void) -> isize;
        fn DeviceIoControl(h: isize, code: u32, inb: *const u8, insz: u32, outb: *mut u8, outsz: u32, ret: *mut u32, ov: *mut c_void) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }

    const CR_SUCCESS: u32 = 0;
    const DEVPROP_TYPE_GUID: u32 = 0x0D;
    const DEVPROP_TYPE_STRING: u32 = 0x12;
    const DEVPKEY_INSTANCE_ID: PropKey = PropKey { fmtid: Guid(0x78C34FC8, 0x104A, 0x4ACA, [0x9E, 0xA4, 0x52, 0x4D, 0x52, 0x99, 0x6E, 0x57]), pid: 256 };
    const DEVPKEY_CONTAINER_ID: PropKey = PropKey { fmtid: Guid(0x8C7ED206, 0x3F8A, 0x4827, [0xB3, 0xAB, 0xAE, 0x9E, 0x1F, 0xAE, 0xFC, 0x6C]), pid: 2 };
    const DEVPKEY_SERVICE: PropKey = PropKey { fmtid: Guid(0xA45C254E, 0xDF1C, 0x4EFD, [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0]), pid: 6 };
    /// GUID_DEVINTERFACE_XUSB: controles do driver XInput (cabo, adaptador sem fio, virtuais X360).
    const XUSB: Guid = Guid(0xEC87F1E3, 0xC13B, 0x4100, [0xB5, 0xF7, 0x8B, 0x84, 0xD5, 0x42, 0x60, 0xCB]);
    /// GUID_DEVINTERFACE_HID: controles XInput por HID (Bluetooth) têm "IG_" no caminho.
    const HID: Guid = Guid(0x4D1E55B2, 0xF16F, 0x11CF, [0x88, 0xCB, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30]);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }
    fn from_wide(b: &[u8]) -> String {
        let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
        String::from_utf16_lossy(&w)
    }
    fn guid_str(b: &[u8]) -> Option<String> {
        if b.len() < 16 {
            return None;
        }
        let d1 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let d2 = u16::from_le_bytes([b[4], b[5]]);
        let d3 = u16::from_le_bytes([b[6], b[7]]);
        Some(format!("{d1:08x}-{d2:04x}-{d3:04x}-{:02x}{:02x}-{}", b[8], b[9], super::hex(&b[10..16])))
    }

    fn node_prop(inst: u32, key: &PropKey) -> Option<(u32, Vec<u8>)> {
        let (mut ty, mut size) = (0u32, 0u32);
        unsafe { CM_Get_DevNode_PropertyW(inst, key, &mut ty, std::ptr::null_mut(), &mut size, 0) };
        if size == 0 || size > 8192 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        (unsafe { CM_Get_DevNode_PropertyW(inst, key, &mut ty, buf.as_mut_ptr(), &mut size, 0) } == CR_SUCCESS).then_some((ty, buf))
    }

    fn node_id(inst: u32) -> Option<String> {
        let mut buf = [0u16; 512];
        (unsafe { CM_Get_Device_IDW(inst, buf.as_mut_ptr(), buf.len() as u32, 0) } == CR_SUCCESS).then(|| {
            let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..n])
        })
    }

    /// Cadeia do nó até a raiz (máx. 16 níveis): (instância, serviço).
    fn chain_of(instance: &str) -> Option<(Vec<(String, String)>, Option<String>)> {
        let mut inst = 0u32;
        if unsafe { CM_Locate_DevNodeW(&mut inst, wide(instance).as_ptr(), 0) } != CR_SUCCESS {
            return None;
        }
        let container = match node_prop(inst, &DEVPKEY_CONTAINER_ID) {
            Some((DEVPROP_TYPE_GUID, b)) => guid_str(&b),
            _ => None,
        };
        let mut chain = Vec::new();
        let mut cur = inst;
        for _ in 0..16 {
            let Some(id) = node_id(cur) else { break };
            if id.eq_ignore_ascii_case("HTREE\\ROOT\\0") {
                break;
            }
            let svc = match node_prop(cur, &DEVPKEY_SERVICE) {
                Some((DEVPROP_TYPE_STRING, b)) => from_wide(&b),
                _ => String::new(),
            };
            chain.push((id, svc));
            let mut parent = 0u32;
            if unsafe { CM_Get_Parent(&mut parent, cur, 0) } != CR_SUCCESS {
                break;
            }
            cur = parent;
        }
        Some((chain, container))
    }

    fn interface_instance(path: &str) -> Option<String> {
        let (mut ty, mut size) = (0u32, 0u32);
        let p = wide(path);
        unsafe { CM_Get_Device_Interface_PropertyW(p.as_ptr(), &DEVPKEY_INSTANCE_ID, &mut ty, std::ptr::null_mut(), &mut size, 0) };
        if size == 0 || size > 4096 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let ok = unsafe { CM_Get_Device_Interface_PropertyW(p.as_ptr(), &DEVPKEY_INSTANCE_ID, &mut ty, buf.as_mut_ptr(), &mut size, 0) } == CR_SUCCESS;
        (ok && ty == DEVPROP_TYPE_STRING).then(|| from_wide(&buf))
    }

    pub fn from_interface(path: &str) -> Option<PnpInfo> {
        let inst = interface_instance(path)?;
        let (chain, container) = chain_of(&inst)?;
        Some(summarize(&chain, container))
    }

    fn interfaces(class: &Guid) -> Vec<String> {
        let mut len = 0u32;
        // 0 = CM_GET_DEVICE_INTERFACE_LIST_PRESENT
        if unsafe { CM_Get_Device_Interface_List_SizeW(&mut len, class, std::ptr::null(), 0) } != CR_SUCCESS || len <= 1 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if unsafe { CM_Get_Device_Interface_ListW(class, std::ptr::null(), buf.as_mut_ptr(), len, 0) } != CR_SUCCESS {
            return Vec::new();
        }
        buf.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }

    /// Slot que o driver XUSB indica pela luz do anel (IOCTL_XUSB_GET_LED_STATE, só leitura).
    /// Qualquer falha ou valor fora da tabela = sem informação (a associação usa VID/PID).
    fn led_slot(path: &str) -> Option<u32> {
        const GENERIC_READ_WRITE: u32 = 0x8000_0000 | 0x4000_0000;
        const SHARE_RW: u32 = 0x1 | 0x2;
        const OPEN_EXISTING: u32 = 3;
        const IOCTL_XUSB_GET_LED_STATE: u32 = 0x8000_E008;
        let h = unsafe { CreateFileW(wide(path).as_ptr(), GENERIC_READ_WRITE, SHARE_RW, std::ptr::null_mut(), OPEN_EXISTING, 0, std::ptr::null_mut()) };
        if h == -1 || h == 0 {
            return None;
        }
        let inb = [0x01u8, 0x01, 0x00];
        let mut out = [0u8; 3];
        let mut ret = 0u32;
        let ok = unsafe { DeviceIoControl(h, IOCTL_XUSB_GET_LED_STATE, inb.as_ptr(), 3, out.as_mut_ptr(), 3, &mut ret, std::ptr::null_mut()) } != 0;
        unsafe { CloseHandle(h) };
        if !ok || ret != 3 {
            return None;
        }
        // XINPUT_LED: 2–5 = piscando no quadrante 1–4, 6–9 = aceso no quadrante 1–4
        match out[2] {
            2..=5 => Some(out[2] as u32 - 2),
            6..=9 => Some(out[2] as u32 - 6),
            _ => None,
        }
    }

    /// Aparelhos físicos com interface XInput: XUSB + HID "IG_", agrupados por ContainerId (as várias
    /// interfaces/coleções de um controle contam uma vez só).
    pub fn xinput_candidates() -> Vec<XCand> {
        let mut out: Vec<XCand> = Vec::new();
        let xusb = interfaces(&XUSB).into_iter().map(|p| (p, true));
        let hid = interfaces(&HID).into_iter().filter(|p| p.to_ascii_lowercase().contains("&ig_")).map(|p| (p, false));
        for (path, is_xusb) in xusb.chain(hid) {
            let Some(info) = from_interface(&path) else { continue };
            let led = if is_xusb { led_slot(&path) } else { None };
            let dup = out.iter_mut().find(|c| {
                (info.container.is_some() && c.info.container == info.container) || (info.address.is_some() && c.info.address == info.address)
            });
            match dup {
                Some(c) => {
                    if c.led_slot.is_none() {
                        c.led_slot = led;
                    }
                }
                None => out.push(XCand { info, led_slot: led }),
            }
        }
        out
    }

    /// VID/PID de um slot pelo XInputGetCapabilitiesEx (xinput1_4, ordinal 108 — o mesmo que o SDL usa).
    pub fn xinput_vid_pid(slot: u32) -> Option<(u16, u16)> {
        #[repr(C)]
        struct CapsEx {
            ty: u8,
            sub: u8,
            flags: u16,
            gamepad: [u8; 12],
            vibration: [u8; 4],
            vid: u16,
            pid: u16,
            ver: u16,
            unk1: u16,
            unk2: u32,
        }
        type Fn = unsafe extern "system" fn(u32, u32, u32, *mut CapsEx) -> u32;
        static PROC: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let f = *PROC.get_or_init(|| unsafe {
            let m = LoadLibraryW(wide("xinput1_4.dll").as_ptr());
            if m.is_null() { 0 } else { GetProcAddress(m, 108usize as *const i8) as usize }
        });
        if f == 0 {
            return None;
        }
        let f: Fn = unsafe { std::mem::transmute(f) };
        let mut c: CapsEx = unsafe { std::mem::zeroed() };
        (unsafe { f(1, slot, 0, &mut c) } == 0 && c.vid != 0).then_some((c.vid, c.pid))
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vid_pid_in_all_formats() {
        assert_eq!(parse_vid_pid("USB\\VID_045E&PID_0B12\\3032"), (Some(0x045E), Some(0x0B12)));
        assert_eq!(parse_vid_pid("BTHLEDEVICE\\{00001812-0000-1000-8000-00805F9B34FB}_DEV_VID&02045E_PID&0B13_REV&0524_14CB65F29D22\\9&1"), (Some(0x045E), Some(0x0B13)));
        assert_eq!(parse_vid_pid("HID\\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0CE6\\9&160FA68&6&0000"), (Some(0x054C), Some(0x0CE6)));
        assert_eq!(parse_vid_pid("ROOT\\SYSTEM\\0003"), (None, None));
    }

    #[test]
    fn bluetooth_address_ignores_service_uuids() {
        // o UUID base do Bluetooth (…-00805F9B34FB) não é o endereço do controle
        assert_eq!(bt_address("BTHENUM\\{00001124-0000-1000-8000-00805f9b34fb}_VID&0002054c_PID&0ce6\\8&17e86349&0&50EE3242A709_C00000000").as_deref(), Some("50ee3242a709"));
        assert_eq!(bt_address("BTHLEDEVICE\\{00001812-0000-1000-8000-00805F9B34FB}_DEV_VID&02045E_PID&0B13_REV&0524_14CB65F29D22\\9&1F0559B4&0&0016").as_deref(), Some("14cb65f29d22"));
        assert_eq!(bt_address("BTHLE\\DEV_14CB65F29D22\\8&CFD1D56&0&14CB65F29D22").as_deref(), Some("14cb65f29d22"));
        assert_eq!(bt_address("BTHENUM\\{00001124-0000-1000-8000-00805F9B34FB}\\7&1"), None);
    }

    #[test]
    fn usb_serial_only_when_reported() {
        assert_eq!(usb_serial("USB\\VID_045E&PID_0B12\\3032363030343637").as_deref(), Some("3032363030343637"));
        assert_eq!(usb_serial("USB\\VID_054C&PID_0CE6\\7&2A3B4C&0&2"), None, "instância gerada (sem serial)");
        assert_eq!(usb_serial("USB\\VID_054C&PID_0CE6&MI_03\\8&1"), None, "interface de composto");
        assert_eq!(usb_serial("HID\\VID_054C&PID_0CE6\\X"), None);
    }

    #[test]
    fn virtual_devices_are_flagged() {
        assert_eq!(virtual_marker("ROOT\\SYSTEM\\0003", "ViGEmBus"), Some("ViGEm"));
        assert_eq!(virtual_marker("ROOT\\USB\\0000", "nssvpd"), Some("ViGEm"));
        assert_eq!(virtual_marker("ROOT\\SYSTEM\\0002", "logi_joy_bus_enum"), Some("Logitech G HUB"));
        assert_eq!(virtual_marker("SWD\\SYNTHETICGAMEPADENUM\\SYNTHETICGAMEPAD0", "WUDFRd"), Some("Windows (Synthetic Gamepad)"));
        assert_eq!(virtual_marker("USB\\ROOT_HUB30\\5&1", "USBHUB3"), None);
        assert_eq!(virtual_marker("ROOT\\ACPI_HAL\\0000", ""), None);
    }

    fn chain(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn real_dualsense_bluetooth() {
        let i = summarize(&chain(&[
            ("HID\\{00001124-0000-1000-8000-00805F9B34FB}_VID&0002054C_PID&0CE6\\9&160FA68&6&0000", ""),
            ("BTHENUM\\{00001124-0000-1000-8000-00805f9b34fb}_VID&0002054c_PID&0ce6\\8&17e86349&0&50EE3242A709_C00000000", "HidBth"),
            ("BTH\\MS_BTHPAN\\7&1", "BthEnum"),
            ("USB\\VID_8087&PID_0029\\5&1", "BTHUSB"),
        ]), Some("aaaaaaaa-0000-1111-2222-333333333333".into()));
        assert_eq!((i.bus, i.address.as_deref(), i.virtual_src, i.vid, i.pid), (Bus::Bluetooth, Some("50ee3242a709"), None, Some(0x054C), Some(0x0CE6)));
        assert_eq!(i.stable_id(), Some(("50ee3242a709".into(), "mac")));
    }

    #[test]
    fn virtual_ds4_from_ds4windows_never_takes_the_real_identity() {
        let i = summarize(&chain(&[
            ("HID\\VID_054C&PID_05C4\\9&1", ""),
            ("USB\\VID_054C&PID_05C4\\0001", "HidUsb"),
            ("ROOT\\SYSTEM\\0003", "ViGEmBus"),
        ]), Some("bbbbbbbb-0000-1111-2222-333333333333".into()));
        assert_eq!(i.virtual_src, Some("ViGEm"));
        let (id, src) = i.stable_id().unwrap();
        assert!(id.starts_with('v') && src == "virtual");
    }

    #[test]
    fn usb_without_serial_uses_container_not_the_hub() {
        let i = summarize(&chain(&[
            ("HID\\VID_054C&PID_0CE6&MI_03\\9&1", ""),
            ("USB\\VID_054C&PID_0CE6&MI_03\\8&1", "HidUsb"),
            ("USB\\VID_054C&PID_0CE6\\7&2A3B&0&2", "usbccgp"),
            ("USB\\ROOT_HUB30\\5&1", "USBHUB3"),
        ]), Some("cccccccc-0000-1111-2222-333333333333".into()));
        assert_eq!((i.bus, i.address.clone()), (Bus::Usb, None));
        assert_eq!(i.stable_id().unwrap().1, "container");
        // o contêiner nulo do sistema não identifica nada
        let n = summarize(&chain(&[("HID\\X\\1", "")]), Some("00000000-0000-0000-ffff-ffffffffffff".into()));
        assert_eq!(n.stable_id(), None);
    }

    fn cand(vid: u16, pid: u16, mac: &str, led: Option<u32>) -> XCand {
        XCand {
            info: PnpInfo { instance: mac.into(), container: None, bus: Bus::BluetoothLe, address: Some(mac.into()), virtual_src: None, vid: Some(vid), pid: Some(pid) },
            led_slot: led,
        }
    }

    #[test]
    fn xinput_slot_follows_the_physical_device() {
        // Xbox Series por BT + X360 virtual (DS4Windows): cada slot fica com o aparelho certo pelo VID/PID,
        // mesmo com o virtual no slot 0
        let c = [cand(0x045E, 0x0B13, "14cb65f29d22", None), cand(0x045E, 0x028E, "virt", None)];
        let m = assign_slots(&[(0, Some((0x045E, 0x028E))), (1, Some((0x045E, 0x0B13)))], &c);
        assert_eq!((m[0].cand, m[1].cand), (Some(1), Some(0)));
        assert!(!m[0].ambiguous && !m[1].ambiguous);
    }

    #[test]
    fn identical_controllers_use_the_ring_light_then_mark_ambiguous() {
        let c = [cand(0x045E, 0x0B12, "a", Some(1)), cand(0x045E, 0x0B12, "b", Some(0))];
        let m = assign_slots(&[(0, Some((0x045E, 0x0B12))), (1, Some((0x045E, 0x0B12)))], &c);
        assert_eq!((m[0].cand, m[1].cand), (Some(1), Some(0)));
        let c = [cand(0x045E, 0x0B12, "a", None), cand(0x045E, 0x0B12, "b", None)];
        let m = assign_slots(&[(0, Some((0x045E, 0x0B12))), (1, Some((0x045E, 0x0B12)))], &c);
        assert!(m[0].cand.is_some() && m[1].cand.is_some() && m[0].cand != m[1].cand && m[0].ambiguous);
    }

    #[test]
    fn never_two_slots_for_one_device_and_no_guessing() {
        let c = [cand(0x045E, 0x0B13, "a", None)];
        let m = assign_slots(&[(0, Some((0x045E, 0x0B13))), (1, Some((0x045E, 0x0B13)))], &c);
        assert_eq!((m[0].cand, m[1].cand), (Some(0), None));
        // VID/PID diferente do único aparelho: não associa
        let m = assign_slots(&[(0, Some((0x2DC8, 0x3106)))], &c);
        assert_eq!(m[0].cand, None);
        // sem VID/PID do XInput, mas um slot e um aparelho: associa
        let m = assign_slots(&[(2, None)], &c);
        assert_eq!(m[0].cand, Some(0));
    }
}

#[cfg(all(test, windows))]
mod live {
    /// Interfaces reais: `cargo test --lib devident::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_identities() {
        for c in super::win::xinput_candidates() {
            println!("XINPUT: {:?} led={:?}", c.info, c.led_slot);
        }
        for s in 0..4 {
            println!("slot {s}: {:?}", super::win::xinput_vid_pid(s));
        }
        let api = hidapi::HidApi::new().unwrap();
        for d in api.device_list().filter(|d| d.usage_page() == 1 && matches!(d.usage(), 4 | 5)) {
            let p = d.path().to_string_lossy().to_string();
            println!("HID {:04x}:{:04x} {p}\n  → {:?}", d.vendor_id(), d.product_id(), super::from_interface(&p));
        }
    }
}
