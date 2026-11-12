//! Controles conectados e bateria.
//!
//! - Xbox por Bluetooth: % do serviço de bateria do Bluetooth LE, a mesma que o Windows mostra em
//!   Configurações → Bluetooth (ver `btle`). O Windows.Gaming.Input NÃO serve aqui: por Bluetooth ele
//!   devolve um valor fixo (100 de 1000 mWh = "10%") que não acompanha a bateria real.
//! - Xbox pelo adaptador sem fio: % do Windows.Gaming.Input; o XInput completa com "com fio" e o nível
//!   em 4 passos (crítica/baixa/média/cheia) quando não há %. Com cabo USB não se exibe % nem
//!   carregamento (o controle não informa isso de forma confiável).
//! - DualSense / DualSense Edge / DualShock 4: bateria dos relatórios HID que a leitura de entrada já
//!   recebe (`input::SONY`); % em passos de 10 e se está carregando.
//! - Outros controles aparecem (pela Gamepad API, no frontend) sem informação de bateria.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Controller {
    /// Identificador do controle FÍSICO (persistente entre reconexões):
    ///  - "xbox:<id>" (XInput): `<id>` = MAC Bluetooth, serial USB ou ContainerId do aparelho que ocupa
    ///    o slot (`devident`); "xbox:<slot>" (0–3) só quando o aparelho não pôde ser identificado;
    ///  - "ps:<id>" / "ns:<id>" (HID): MAC do controle (o mesmo por USB e Bluetooth);
    ///  - virtuais: "<prefixo>:v<hash>" — nunca a identidade do controle real que espelham.
    pub id: String,
    pub name: String,
    pub kind: String,
    pub battery_percent: Option<u8>,
    pub battery_label: Option<String>,
    pub charging: bool,
    pub wired: bool,
    /// "usb" | "bluetooth" | "wireless" (adaptador/sem fio sem detalhe) — `None` quando não se sabe.
    pub connection: Option<String>,
    /// Modelo identificado pelo VID/PID: "xbox-series" | "xbox-one" | "xbox-elite" | "xbox-360" |
    /// "xbox-adaptive" | "xbox" (Microsoft, modelo desconhecido) | "xinput" (outro fabricante) |
    /// "dualsense" | "dualsense-edge" | "dualshock4" | "sony-unknown" | "switch-pro" | "joycon-l" |
    /// "joycon-r" | "virtual". `None` = sem informação.
    pub model: Option<String>,
    /// Fabricante (pelo VID), quando conhecido.
    pub brand: Option<String>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    /// ContainerId do Windows: o mesmo para todas as interfaces de um aparelho físico.
    pub physical_id: Option<String>,
    /// Origem do identificador: "mac" | "serial" | "container" | "path" | "slot" | "virtual".
    pub id_source: Option<String>,
    /// Programa/driver que criou o dispositivo, quando ele é virtual (ViGEm, Logitech G HUB...).
    pub virtual_source: Option<String>,
    /// Chave usada por versões anteriores ("xbox:<slot>"), para migrar cadastros antigos.
    pub legacy_key: Option<String>,
}

/// Fabricante pelo VID (controles comuns no PC).
pub fn brand_of(vid: u16) -> Option<&'static str> {
    Some(match vid {
        0x045E => "Microsoft",
        0x054C => "Sony",
        0x057E => "Nintendo",
        0x2DC8 => "8BitDo",
        0x0E6F => "PDP",
        0x24C6 | 0x20D6 => "PowerA",
        0x0F0D => "HORI",
        0x1532 => "Razer",
        0x046D => "Logitech",
        0x0738 => "Mad Catz",
        0x146B | 0x3285 => "Nacon",
        0x28DE => "Valve",
        0x3537 => "GameSir",
        0x2563 => "ShanWan",
        0x11C0 | 0x11C1 => "Betop",
        0x0079 => "DragonRise",
        _ => return None,
    })
}

/// Modelo e nome de um controle XInput pelo VID/PID.
pub fn xinput_model(vid: Option<u16>, pid: Option<u16>) -> (&'static str, &'static str) {
    match (vid, pid) {
        (Some(0x045E), Some(p)) => match p {
            0x0B12 | 0x0B13 => ("xbox-series", "Controle Xbox Series"),
            0x02E3 | 0x0B00 | 0x0B05 | 0x0B22 => ("xbox-elite", "Controle Xbox Elite"),
            0x0B0A | 0x0B21 => ("xbox-adaptive", "Controle Adaptável Xbox"),
            0x02D1 | 0x02DD | 0x02EA | 0x02E0 | 0x02FD | 0x0B20 => ("xbox-one", "Controle Xbox One"),
            0x028E | 0x028F | 0x0291 | 0x02A1 | 0x02A9 | 0x0719 => ("xbox-360", "Controle Xbox 360"),
            _ => ("xbox", "Controle Xbox"),
        },
        (Some(_), _) => ("xinput", "Controle compatível com XInput"),
        _ => ("xbox", "Controle Xbox"),
    }
}

#[cfg(windows)]
pub(crate) mod xinput {
    #[repr(C)]
    pub struct Gamepad {
        pub buttons: u16,
        pub lt: u8,
        pub rt: u8,
        pub lx: i16,
        pub ly: i16,
        pub rx: i16,
        pub ry: i16,
    }
    #[repr(C)]
    pub struct State {
        pub packet: u32,
        pub gamepad: Gamepad,
    }
    #[repr(C)]
    pub struct Battery {
        pub battery_type: u8,
        pub battery_level: u8,
    }
    #[link(name = "xinput")]
    extern "system" {
        pub fn XInputGetState(user: u32, state: *mut State) -> u32;
        pub fn XInputGetBatteryInformation(user: u32, dev_type: u8, info: *mut Battery) -> u32;
        pub fn XInputSetState(user: u32, vibration: *mut Vibration) -> u32;
    }
    /// Motores de vibração (0–65535): esquerdo = baixa frequência, direito = alta.
    #[repr(C)]
    pub struct Vibration {
        pub left: u16,
        pub right: u16,
    }
}

/// Windows.Gaming.Input: relatório de bateria dos controles Xbox (mWh restantes/totais e status).
/// É a única fonte com bateria por Bluetooth — o XInput só informa pelo adaptador sem fio/cabo.
/// A lista de controles é preenchida de forma assíncrona após o primeiro acesso: `warm_up()` roda na
/// inicialização do app para que as consultas seguintes já a encontrem completa.
#[cfg(windows)]
pub(crate) mod wgi {
    use windows::Gaming::Input::Gamepad;
    use windows::System::Power::BatteryStatus;

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Report {
        pub percent: Option<u8>,
        pub status: Status,
    }
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Status { NotPresent, Discharging, Idle, Charging, Unknown }

    pub fn warm_up() {
        let _ = Gamepad::Gamepads();
    }

    pub fn reports() -> Vec<Report> {
        let Ok(list) = Gamepad::Gamepads() else { return Vec::new() };
        let n = list.Size().unwrap_or(0);
        (0..n)
            .filter_map(|i| list.GetAt(i).ok())
            .map(|g| match g.TryGetBatteryReport() {
                Ok(r) => {
                    let rem = r.RemainingCapacityInMilliwattHours().ok().and_then(|v| v.Value().ok());
                    let full = r.FullChargeCapacityInMilliwattHours().ok().and_then(|v| v.Value().ok());
                    let status = match r.Status() {
                        Ok(s) if s == BatteryStatus::NotPresent => Status::NotPresent,
                        Ok(s) if s == BatteryStatus::Discharging => Status::Discharging,
                        Ok(s) if s == BatteryStatus::Idle => Status::Idle,
                        Ok(s) if s == BatteryStatus::Charging => Status::Charging,
                        _ => Status::Unknown,
                    };
                    Report { percent: super::percent(rem, full), status }
                }
                Err(_) => Report { percent: None, status: Status::Unknown },
            })
            .collect()
    }
}

/// Bateria dos controles Xbox conectados por Bluetooth LE, como o Windows a lê do serviço de bateria
/// (DEVPKEY_Bluetooth_Battery). Só dispositivos BTHLE com "Xbox" no nome e CONECTADOS agora: um
/// controle pareado mas desligado continua "presente" no Windows, com a última bateria guardada.
#[cfg(windows)]
pub(crate) mod btle {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid(u32, u16, u16, [u8; 8]);
    #[repr(C)]
    struct PropKey { fmtid: Guid, pid: u32 }
    #[repr(C)]
    struct DevInfoData { cb_size: u32, class_guid: Guid, dev_inst: u32, reserved: usize }

    #[link(name = "setupapi")]
    extern "system" {
        fn SetupDiGetClassDevsW(class: *const Guid, enumerator: *const u16, parent: *mut c_void, flags: u32) -> isize;
        fn SetupDiEnumDeviceInfo(set: isize, index: u32, data: *mut DevInfoData) -> i32;
        fn SetupDiGetDevicePropertyW(set: isize, data: *const DevInfoData, key: *const PropKey, ty: *mut u32,
            buf: *mut u8, size: u32, required: *mut u32, flags: u32) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: isize) -> i32;
    }

    const DIGCF_PRESENT: u32 = 0x2;
    const DIGCF_ALLCLASSES: u32 = 0x4;
    const DEVPROP_TYPE_BYTE: u32 = 0x03;
    const DEVPROP_TYPE_BOOLEAN: u32 = 0x11;
    const DEVPROP_TYPE_STRING: u32 = 0x12;
    /// DEVPKEY_Bluetooth_Battery (0–100).
    const BATTERY: PropKey = PropKey { fmtid: Guid(0x104EA319, 0x6EE2, 0x4701, [0xBD, 0x47, 0x8D, 0xDB, 0xF4, 0x25, 0xBB, 0xE5]), pid: 2 };
    /// Conectado agora (verdadeiro com o controle ligado e conectado; falso se só pareado).
    const CONNECTED: PropKey = PropKey { fmtid: Guid(0x83DA6326, 0x97A6, 0x4088, [0x94, 0x53, 0xA1, 0x92, 0x3F, 0x57, 0x3B, 0x29]), pid: 15 };
    /// DEVPKEY_Device_FriendlyName.
    const NAME: PropKey = PropKey { fmtid: Guid(0xA45C254E, 0xDF1C, 0x4EFD, [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0]), pid: 14 };

    fn prop(set: isize, d: &DevInfoData, key: &PropKey) -> Option<(u32, Vec<u8>)> {
        let (mut ty, mut need) = (0u32, 0u32);
        unsafe { SetupDiGetDevicePropertyW(set, d, key, &mut ty, std::ptr::null_mut(), 0, &mut need, 0) };
        if need == 0 || need > 4096 {
            return None;
        }
        let mut buf = vec![0u8; need as usize];
        let ok = unsafe { SetupDiGetDevicePropertyW(set, d, key, &mut ty, buf.as_mut_ptr(), need, &mut need, 0) } != 0;
        ok.then_some((ty, buf))
    }

    /// % de cada controle Xbox conectado por Bluetooth LE (ordem de enumeração do Windows).
    pub fn xbox_batteries() -> Vec<u8> {
        let enumerator: Vec<u16> = "BTHLE".encode_utf16().chain([0]).collect();
        let set = unsafe { SetupDiGetClassDevsW(std::ptr::null(), enumerator.as_ptr(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_ALLCLASSES) };
        if set == -1 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0.. {
            let mut d = DevInfoData { cb_size: std::mem::size_of::<DevInfoData>() as u32, class_guid: Guid(0, 0, 0, [0; 8]), dev_inst: 0, reserved: 0 };
            if unsafe { SetupDiEnumDeviceInfo(set, i, &mut d) } == 0 {
                break;
            }
            let name = match prop(set, &d, &NAME) {
                Some((DEVPROP_TYPE_STRING, b)) => {
                    let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
                    String::from_utf16_lossy(&w).to_lowercase()
                }
                _ => continue,
            };
            if !name.contains("xbox") || name.contains("headset") {
                continue;
            }
            let connected = matches!(prop(set, &d, &CONNECTED), Some((DEVPROP_TYPE_BOOLEAN, b)) if b.first().is_some_and(|v| *v != 0));
            if !connected {
                continue;
            }
            if let Some((DEVPROP_TYPE_BYTE, b)) = prop(set, &d, &BATTERY) {
                if let Some(&v) = b.first().filter(|v| **v <= 100) {
                    out.push(v);
                }
            }
        }
        unsafe { SetupDiDestroyDeviceInfoList(set) };
        out
    }
}

/// % a partir da capacidade (mWh). `None` se o controle não informar valores válidos.
pub fn percent(remaining: Option<i32>, full: Option<i32>) -> Option<u8> {
    match (remaining, full) {
        (Some(r), Some(f)) if f > 0 && r >= 0 => Some(((r as f64 / f as f64) * 100.0).round().clamp(0.0, 100.0) as u8),
        _ => None,
    }
}

/// Dados do XInput de um slot conectado.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XSlot {
    pub index: u32,
    /// `BATTERY_TYPE_*`: 0 desconectado, 1 com fio, 2 alcalina (pilhas), 3 NiMH (recarregável), 0xFF desconhecido.
    pub battery_type: Option<u8>,
    /// `BATTERY_LEVEL_*`: 0 vazia … 3 cheia.
    pub level: u8,
}

/// Resumo do relatório do Windows.Gaming.Input usado na combinação (testável sem Windows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReportLite {
    pub percent: Option<u8>,
    pub charging: bool,
    pub discharging: bool,
}

/// Monta o estado exibido de cada controle Xbox, combinando XInput (com fio? nível em 4 passos),
/// bateria do Bluetooth LE (`bt`, a % que o Windows mostra) e Windows.Gaming.Input (% pelo adaptador
/// sem fio). As fontes não compartilham um identificador: só são associadas quando a correspondência
/// é inequívoca (mesma quantidade de controles, na ordem de conexão, ou um único controle de cada lado).
/// Com Bluetooth, a % do Windows.Gaming.Input é ignorada (valor fixo, ver o topo do arquivo).
/// Com cabo USB: só `wired` (ícone de raio), sem % nem "carregando" — nada é inventado.
pub fn merge_xbox(slots: &[XSlot], reports: &[ReportLite], bt: &[u8]) -> Vec<Controller> {
    let paired = |i: usize| -> Option<ReportLite> {
        if reports.len() == slots.len() {
            reports.get(i).copied()
        } else if slots.len() == 1 && reports.len() == 1 {
            Some(reports[0])
        } else {
            None
        }
    };
    slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let r = paired(i);
            // um controle e uma bateria Bluetooth: são o mesmo. Com vários, a ordem do XInput e a do
            // Windows não batem de forma garantida — melhor sem % do que trocar as baterias.
            let bt_pct = (slots.len() == 1 && bt.len() == 1).then(|| bt[0]);
            // O XInput às vezes diz "com fio" para controles Bluetooth; se a bateria está descarregando, não há cabo.
            let wired = s.battery_type == Some(1) && !matches!(r, Some(ReportLite { discharging: true, .. }));
            let (percent, label, charging) = if wired {
                (None, None, false)
            } else if let Some(p) = bt_pct {
                (Some(p), None, r.map(|r| r.charging).unwrap_or(false))
            } else if !bt.is_empty() {
                // há controle por Bluetooth mas sem associação segura: a % do Windows.Gaming.Input
                // seria o valor fixo do Bluetooth, então não se mostra nada
                (None, None, false)
            } else if let Some(p) = r.and_then(|r| r.percent) {
                (Some(p), None, r.map(|r| r.charging).unwrap_or(false))
            } else if matches!(s.battery_type, Some(2) | Some(3)) {
                let l = match s.level { 0 => "Crítica", 1 => "Baixa", 2 => "Média", _ => "Cheia" };
                (None, Some(l.to_string()), false)
            } else {
                (None, None, false)
            };
            Controller {
                id: format!("xbox:{}", s.index),
                name: format!("Controle Xbox {}", s.index + 1),
                kind: "xbox".into(),
                battery_percent: percent,
                battery_label: label,
                charging,
                wired,
                connection: Some(if wired { "usb" } else if bt_pct.is_some() { "bluetooth" } else { "wireless" }.into()),
                id_source: Some("slot".into()),
                legacy_key: Some(format!("xbox:{}", s.index)),
                ..Default::default()
            }
        })
        .collect()
}

#[cfg(windows)]
fn xbox(with_battery: bool) -> Vec<Controller> {
    use xinput::*;
    let mut slots = Vec::new();
    for i in 0..4u32 {
        let mut st = State {
            packet: 0,
            gamepad: Gamepad { buttons: 0, lt: 0, rt: 0, lx: 0, ly: 0, rx: 0, ry: 0 },
        };
        if unsafe { XInputGetState(i, &mut st) } != 0 {
            continue;
        }
        let mut b = Battery { battery_type: 0, battery_level: 0 };
        let ok = unsafe { XInputGetBatteryInformation(i, 0, &mut b) } == 0;
        slots.push(XSlot { index: i, battery_type: ok.then_some(b.battery_type), level: b.battery_level });
    }
    if slots.is_empty() {
        return Vec::new();
    }
    let idents = crate::devident::xinput_slots(&slots.iter().map(|s| s.index).collect::<Vec<_>>());
    let ident_of = |slot: u32| idents.iter().find(|x| x.slot == slot);
    // Virtuais (ex.: o X360 que o DS4Windows cria) ficam fora da associação de bateria: só controles
    // físicos entram na contagem "um controle ↔ uma bateria".
    let (virt, phys): (Vec<XSlot>, Vec<XSlot>) = slots.iter().partition(|s| ident_of(s.index).is_some_and(|x| x.virtual_src().is_some()));
    let mut out = if !with_battery {
        // só conexão (consulta rápida): sem Windows.Gaming.Input nem Bluetooth
        merge_xbox(&phys, &[], &[]).into_iter().map(|c| Controller { battery_percent: None, battery_label: None, charging: false, ..c }).collect()
    } else {
        let reports: Vec<ReportLite> = wgi::reports()
            .into_iter()
            .map(|r| ReportLite {
                percent: if matches!(r.status, wgi::Status::NotPresent) { None } else { r.percent },
                charging: matches!(r.status, wgi::Status::Charging),
                discharging: matches!(r.status, wgi::Status::Discharging),
            })
            .collect();
        // o Windows.Gaming.Input também lista os virtuais: com eles presentes a contagem não fecha
        // e nenhuma % é emprestada (ver `merge_xbox`)
        merge_xbox(&phys, &reports, &btle::xbox_batteries())
    };
    out.extend(merge_xbox(&virt, &[], &[]).into_iter().map(|c| Controller { battery_percent: None, battery_label: None, charging: false, wired: false, connection: None, ..c }));
    out.sort_by_key(|c| c.legacy_key.clone());
    out.into_iter()
        .map(|c| {
            let slot = c.legacy_key.as_deref().and_then(|k| k.strip_prefix("xbox:")).and_then(|s| s.parse().ok()).unwrap_or(0);
            match ident_of(slot) {
                Some(x) => identify_xbox(c, x),
                None => c,
            }
        })
        .collect()
}

/// Completa um controle XInput com a identidade física do aparelho que ocupa o slot.
pub fn identify_xbox(c: Controller, x: &crate::devident::XIdent) -> Controller {
    use crate::devident::Bus;
    let (model, name) = xinput_model(x.vid, x.pid);
    let info = x.info.as_ref();
    let connection = match info.map(|i| i.bus) {
        _ if c.wired => Some("usb".to_string()),
        Some(Bus::Bluetooth | Bus::BluetoothLe) => Some("bluetooth".into()),
        _ => c.connection.clone(),
    };
    let virt = x.virtual_src();
    Controller {
        id: x.key(),
        name: match virt {
            Some(v) => format!("Controle virtual ({v})"),
            None => name.into(),
        },
        model: Some(if virt.is_some() { "virtual" } else { model }.into()),
        brand: if virt.is_some() { None } else { x.vid.and_then(brand_of).map(Into::into) },
        vid: x.vid,
        pid: x.pid,
        physical_id: info.and_then(|i| i.container.clone()),
        id_source: Some(x.id_source().into()),
        virtual_source: virt.map(Into::into),
        connection,
        ..c
    }
}

#[cfg(not(windows))]
fn xbox(_with_battery: bool) -> Vec<Controller> {
    Vec::new()
}

/// Controles HID lidos pela entrada nativa (`hidpads::PADS`: Sony e Nintendo, vários ao mesmo tempo).
/// A bateria chega a cada relatório do controle, então a consulta é instantânea e não abre uma segunda
/// conexão HID com o dispositivo.
fn hid() -> Vec<Controller> {
    let pads = crate::hidpads::PADS.lock().map(|g| g.clone()).unwrap_or_default();
    pads.into_iter()
        .map(|p| {
            let virt = p.virtual_src;
            Controller {
                id: p.key,
                name: match (virt, p.player) {
                    // ex.: "DualShock 4 virtual (ViGEm)" — o DS4 que o DS4Windows cria para outro controle
                    (Some(v), _) => format!("{} virtual ({v})", p.model.name()),
                    (None, Some(n)) => format!("{} · {}", p.model.name(), n),
                    (None, None) => p.model.name().into(),
                },
                kind: p.model.kind().into(),
                battery_percent: p.battery_percent,
                battery_label: p.battery_label.map(Into::into),
                charging: p.charging,
                wired: false,
                connection: if virt.is_some() { None } else { Some(if p.bt { "bluetooth" } else { "usb" }.into()) },
                model: Some(if virt.is_some() { "virtual" } else { p.model.id() }.into()),
                brand: if virt.is_some() { None } else { Some(p.model.brand().into()) },
                vid: Some(p.vid),
                pid: Some(p.pid),
                physical_id: p.container,
                id_source: Some(p.id_source.into()),
                virtual_source: virt.map(Into::into),
                legacy_key: None,
            }
        })
        .collect()
}

/// Controles conectados agora. `with_battery = false`: só conexão (rápido, para consultas frequentes).
pub fn snapshot(with_battery: bool) -> Vec<Controller> {
    let mut v = xbox(with_battery);
    v.extend(hid());
    v
}

/// Executa em thread separada: a leitura HID pode levar centenas de milissegundos.
#[tauri::command]
pub async fn get_controllers() -> Vec<Controller> {
    tauri::async_runtime::spawn_blocking(|| snapshot(true))
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRED: XSlot = XSlot { index: 0, battery_type: Some(1), level: 0 };
    const AA: XSlot = XSlot { index: 0, battery_type: Some(2), level: 1 };
    const BT_UNKNOWN: XSlot = XSlot { index: 0, battery_type: Some(0xFF), level: 0 };

    #[test]
    fn xbox_models_by_vid_pid() {
        assert_eq!(xinput_model(Some(0x045E), Some(0x0B13)).0, "xbox-series");
        assert_eq!(xinput_model(Some(0x045E), Some(0x0B12)).0, "xbox-series");
        assert_eq!(xinput_model(Some(0x045E), Some(0x028E)).0, "xbox-360");
        assert_eq!(xinput_model(Some(0x045E), Some(0x02E0)).0, "xbox-one");
        assert_eq!(xinput_model(Some(0x2DC8), Some(0x3106)).0, "xinput");
        assert_eq!(brand_of(0x2DC8), Some("8BitDo"));
        assert_eq!(xinput_model(None, None).0, "xbox");
    }

    #[test]
    fn xbox_identity_replaces_the_slot_key() {
        use crate::devident::{Bus, PnpInfo, XIdent};
        let base = merge_xbox(&[XSlot { index: 2, ..BT_UNKNOWN }], &[], &[]).remove(0);
        assert_eq!((base.id.as_str(), base.legacy_key.as_deref()), ("xbox:2", Some("xbox:2")));
        let info = PnpInfo { instance: "x".into(), container: Some("c1".into()), bus: Bus::BluetoothLe, address: Some("14cb65f29d22".into()), virtual_src: None, vid: Some(0x045E), pid: Some(0x0B13) };
        let c = identify_xbox(base.clone(), &XIdent { slot: 2, vid: Some(0x045E), pid: Some(0x0B13), info: Some(info.clone()), ambiguous: false });
        // mesmo controle em qualquer slot = mesma chave; o slot antigo fica só para migração
        assert_eq!(c.id, "xbox:14cb65f29d22");
        assert_eq!((c.model.as_deref(), c.brand.as_deref(), c.connection.as_deref()), (Some("xbox-series"), Some("Microsoft"), Some("bluetooth")));
        assert_eq!(c.legacy_key.as_deref(), Some("xbox:2"));
        // X360 virtual do DS4Windows: marcado, sem marca nem identidade do controle real
        let v = PnpInfo { address: None, virtual_src: Some("ViGEm"), pid: Some(0x028E), ..info };
        let c = identify_xbox(base, &XIdent { slot: 2, vid: Some(0x045E), pid: Some(0x028E), info: Some(v), ambiguous: false });
        assert!(c.id.starts_with("xbox:v") && c.virtual_source.as_deref() == Some("ViGEm") && c.model.as_deref() == Some("virtual"));
    }

    #[test]
    fn percent_from_capacity() {
        assert_eq!(percent(Some(2550), Some(3000)), Some(85));
        assert_eq!(percent(Some(10), Some(0)), None);
        assert_eq!(percent(None, Some(100)), None);
    }

    #[test]
    fn usb_shows_only_wired() {
        let c = &merge_xbox(&[WIRED], &[ReportLite { percent: Some(60), charging: true, discharging: false }], &[])[0];
        assert!(c.wired);
        assert_eq!((c.battery_percent, c.battery_label.as_deref(), c.charging), (None, None, false));
    }

    #[test]
    fn wireless_adapter_uses_gaming_input_percent() {
        let c = &merge_xbox(&[BT_UNKNOWN], &[ReportLite { percent: Some(85), charging: false, discharging: true }], &[])[0];
        assert!(!c.wired);
        assert_eq!(c.battery_percent, Some(85));
    }

    #[test]
    fn bluetooth_prefers_windows_battery_over_fixed_gaming_input_value() {
        // caso real: o Windows.Gaming.Input dizia 100/1000 mWh (10%) e o Windows mostrava 71%
        let c = &merge_xbox(&[BT_UNKNOWN], &[ReportLite { percent: Some(10), charging: false, discharging: true }], &[71])[0];
        assert_eq!(c.battery_percent, Some(71));
        // dois controles por Bluetooth: sem associação segura, nenhuma % (nem o valor fixo do WGI)
        let two = merge_xbox(&[BT_UNKNOWN, XSlot { index: 1, ..BT_UNKNOWN }],
            &[ReportLite { percent: Some(10), charging: false, discharging: true }; 2], &[71, 40]);
        assert!(two.iter().all(|c| c.battery_percent.is_none()));
    }

    #[test]
    fn xinput_wired_but_discharging_is_wireless() {
        let c = &merge_xbox(&[WIRED], &[ReportLite { percent: Some(40), charging: false, discharging: true }], &[])[0];
        assert!(!c.wired);
        assert_eq!(c.battery_percent, Some(40));
    }

    #[test]
    fn falls_back_to_xinput_level_and_never_guesses() {
        let c = &merge_xbox(&[AA], &[], &[])[0];
        assert_eq!((c.battery_percent, c.battery_label.as_deref()), (None, Some("Baixa")));
        // dois controles e um relatório: associação ambígua, nenhuma % é emprestada
        let two = merge_xbox(&[BT_UNKNOWN, XSlot { index: 1, ..BT_UNKNOWN }], &[ReportLite { percent: Some(70), charging: false, discharging: true }], &[]);
        assert!(two.iter().all(|c| c.battery_percent.is_none()));
    }
}

#[cfg(all(test, windows))]
mod live {
    /// Controles reais: `cargo test --lib controllers::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_connected() {
        super::wgi::warm_up();
        std::thread::sleep(std::time::Duration::from_millis(1500));
        println!("WGI: {:?}", super::wgi::reports());
        // valores brutos (mWh) e repetição: mostra se o controle informa % real ou poucos níveis
        use windows::Gaming::Input::Gamepad;
        for round in 0..5 {
            if let Ok(list) = Gamepad::Gamepads() {
                for i in 0..list.Size().unwrap_or(0) {
                    let Ok(g) = list.GetAt(i) else { continue };
                    let Ok(r) = g.TryGetBatteryReport() else { println!("WGI raw #{i}: sem relatório"); continue };
                    let v = |x: windows::core::Result<windows::Foundation::IReference<i32>>| x.ok().and_then(|v| v.Value().ok());
                    println!("WGI raw r{round} #{i}: rem={:?} full={:?} design={:?} rate={:?} status={:?}",
                        v(r.RemainingCapacityInMilliwattHours()), v(r.FullChargeCapacityInMilliwattHours()),
                        v(r.DesignCapacityInMilliwattHours()), v(r.ChargeRateInMilliwatts()), r.Status().map(|s| s.0));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        for i in 0..4u32 {
            let mut b = super::xinput::Battery { battery_type: 0, battery_level: 0 };
            let r = unsafe { super::xinput::XInputGetBatteryInformation(i, 0, &mut b) };
            if r == 0 && b.battery_type != 0 { println!("XINPUT slot {i}: type={:#x} level={}", b.battery_type, b.battery_level); }
        }
        for c in super::xbox(true) {
            println!("XBOX: {} wired={} pct={:?} label={:?} charging={}", c.name, c.wired, c.battery_percent, c.battery_label, c.charging);
        }
        println!("BTLE (Windows): {:?}", super::btle::xbox_batteries());
    }
}
