//! Avisos de controle: conectado, desconectado e bateria baixa (≤ 15%), com o mesmo cartão das
//! conquistas — por cima do jogo e do próprio NEXSTAGE (janela de notificação, ver `shot_toast`).
//!
//! - Conexão: consulta rápida a cada 1,5 s (XInput + controle Sony aberto pela leitura de entrada).
//!   "Desconectado" só depois de 2 consultas seguidas sem o controle (evita falsos avisos).
//!   Os controles já conectados quando o app abre não geram aviso de conexão.
//! - Bateria: a cada 20 s (leitura completa). Avisa uma vez quando cai a 15% ou menos (ou nível
//!   "Crítica", sem %); volta a avisar só depois de passar de 20% ou de reconectar. Carregando ou
//!   por cabo, não avisa.
//! Interruptor: Configurações → Notificações → Controles (`pad_toast`).

use crate::commands::controllers::{snapshot, Controller};
use crate::shot_toast::{self, PadToast};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::AppHandle;

const POLL: Duration = Duration::from_millis(1500);

/// Nome personalizado de cada controle cadastrado (chave do dispositivo → nome), enviado pela interface:
/// os avisos dizem "Controle da sala conectado", não o nome técnico do aparelho.
static NAMES: std::sync::Mutex<Option<HashMap<String, String>>> = std::sync::Mutex::new(None);

#[tauri::command]
pub fn set_pad_names(names: HashMap<String, String>) {
    if let Ok(mut g) = NAMES.lock() {
        *g = Some(names.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect());
    }
}

fn display_name(c: &Controller) -> String {
    NAMES.lock().ok().and_then(|g| g.as_ref()?.get(&c.id).cloned()).unwrap_or_else(|| c.name.clone())
}
const BATTERY_EVERY: Duration = Duration::from_secs(20);
pub const LOW: u8 = 15;
/// Acima disto o aviso de bateria baixa é "rearmado".
const REARM: u8 = 20;
/// Consultas seguidas sem o controle até considerar desconectado.
const MISSING_POLLS: u8 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum PadEvent {
    Connected(Controller),
    Disconnected(Controller),
    LowBattery(Controller),
}

struct Known {
    pad: Controller,
    missing: u8,
    low_notified: bool,
}

#[derive(Default)]
pub struct PadWatch {
    known: HashMap<String, Known>,
    started: bool,
}

/// Bateria baixa de verdade: % ≤ 15 (ou "Crítica"), sem cabo e sem carregar.
pub fn is_low(c: &Controller) -> Option<bool> {
    if c.wired || c.charging {
        return Some(false);
    }
    match (c.battery_percent, c.battery_label.as_deref()) {
        (Some(p), _) => Some(p <= LOW),
        (None, Some("Crítica")) => Some(true),
        (None, Some(_)) => Some(false),
        (None, None) => None, // sem informação: não muda o estado
    }
}

impl PadWatch {
    /// Recebe os controles conectados agora. `battery`: a lista traz bateria (leitura completa).
    pub fn update(&mut self, now: Vec<Controller>, battery: bool) -> Vec<PadEvent> {
        let mut out = Vec::new();
        let first = !self.started;
        self.started = true;
        // presentes
        for c in now.iter() {
            match self.known.get_mut(&c.id) {
                Some(k) => {
                    k.missing = 0;
                    if battery {
                        k.pad = c.clone();
                    } else {
                        k.pad.name = c.name.clone(); // a consulta rápida não traz bateria: mantém a última
                    }
                }
                None => {
                    // metade de um par que acabou de ser separado: não é uma conexão nova
                    let half = c.id.strip_prefix("ns:").unwrap_or(&c.id);
                    let was_paired = self.known.keys().any(|k| halves(k).is_some_and(|(l, r)| l == half || r == half));
                    if !first && !was_paired {
                        out.push(PadEvent::Connected(c.clone()));
                    }
                    self.known.insert(c.id.clone(), Known { pad: c.clone(), missing: 0, low_notified: false });
                }
            }
        }
        // ausentes
        let mut gone = Vec::new();
        for (id, k) in self.known.iter_mut() {
            if !now.iter().any(|c| &c.id == id) {
                k.missing += 1;
                if k.missing >= MISSING_POLLS {
                    gone.push(id.clone());
                }
            }
        }
        // Joy-Con que entrou num par não desconectou: só passou a contar como o par. E o par separado
        // com as duas metades ainda conectadas também não.
        let present = |half: &str| now.iter().any(|c| c.id.strip_prefix("ns:") == Some(half));
        let in_pair = |id: &str| {
            let half = id.strip_prefix("ns:").unwrap_or(id);
            now.iter().any(|c| halves(&c.id).is_some_and(|(l, r)| l == half || r == half))
        };
        let split = |id: &str| halves(id).is_some_and(|(l, r)| present(l) && present(r));
        for id in gone {
            if let Some(k) = self.known.remove(&id) {
                if !in_pair(&id) && !split(&id) {
                    out.push(PadEvent::Disconnected(k.pad));
                }
            }
        }
        // bateria
        if battery {
            for k in self.known.values_mut() {
                match is_low(&k.pad) {
                    Some(true) if !k.low_notified => {
                        k.low_notified = true;
                        out.push(PadEvent::LowBattery(k.pad.clone()));
                    }
                    Some(false) if k.pad.charging || k.pad.wired || k.pad.battery_percent.is_some_and(|p| p > REARM) => k.low_notified = false,
                    _ => {}
                }
            }
        }
        out
    }
}

/// Metades de um par de Joy-Cons a partir da chave `ns:<esquerdo>+<direito>`.
fn halves(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix("ns:")?.split_once('+')
}

fn device_of(c: &Controller) -> &'static str {
    match c.kind.as_str() {
        "xbox" => "xbox",
        "playstation" => "playstation",
        _ => "generic",
    }
}

pub fn to_toast(e: &PadEvent) -> PadToast {
    let (event, c) = match e {
        PadEvent::Connected(c) => ("connected", c),
        PadEvent::Disconnected(c) => ("disconnected", c),
        PadEvent::LowBattery(c) => ("battery", c),
    };
    PadToast {
        event: event.into(),
        device: device_of(c).into(),
        name: display_name(c),
        percent: c.battery_percent,
        label: c.battery_label.clone(),
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("pad-notify".into())
        .spawn(move || {
            let mut watch = PadWatch::default();
            let mut next_battery = Instant::now();
            loop {
                let now = Instant::now();
                let battery = now >= next_battery;
                if battery {
                    next_battery = now + BATTERY_EVERY;
                }
                // controles virtuais (ViGEm, G HUB...) espelham um controle físico: não geram avisos próprios
                let pads = snapshot(battery).into_iter().filter(|c| c.virtual_source.is_none()).collect();
                for e in watch.update(pads, battery) {
                    shot_toast::notify_pad(&app, to_toast(&e));
                }
                std::thread::sleep(POLL);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(id: &str, pct: Option<u8>) -> Controller {
        Controller { id: id.into(), name: format!("Controle {id}"), kind: "xbox".into(), battery_percent: pct, battery_label: None, charging: false, wired: false, connection: None, ..Default::default() }
    }
    fn kinds(v: &[PadEvent]) -> Vec<String> {
        v.iter().map(|e| match e { PadEvent::Connected(c) => format!("+{}", c.id), PadEvent::Disconnected(c) => format!("-{}", c.id), PadEvent::LowBattery(c) => format!("!{}", c.id) }).collect()
    }

    #[test]
    fn joining_and_splitting_joycons_is_silent() {
        let mut w = PadWatch::default();
        assert!(w.update(vec![], false).is_empty());
        assert_eq!(kinds(&w.update(vec![pad("ns:aa", None), pad("ns:bb", None)], false)), ["+ns:aa", "+ns:bb"]);
        // juntou: o par aparece (aviso de conexão do par), as metades somem sem aviso de desconexão
        assert_eq!(kinds(&w.update(vec![pad("ns:aa+bb", None)], false)), ["+ns:aa+bb"]);
        for _ in 0..4 {
            assert!(w.update(vec![pad("ns:aa+bb", None)], false).is_empty());
        }
        // separou: as metades voltam e o par some, tudo sem aviso
        for _ in 0..4 {
            assert!(w.update(vec![pad("ns:aa", None), pad("ns:bb", None)], false).is_empty());
        }
        // um Joy-Con desligado de verdade avisa
        let mut ev = Vec::new();
        for _ in 0..4 {
            ev.extend(w.update(vec![pad("ns:aa", None)], false));
        }
        assert_eq!(kinds(&ev), ["-ns:bb"]);
    }

    #[test]
    fn startup_is_silent_then_connect_and_debounced_disconnect() {
        let mut w = PadWatch::default();
        assert!(w.update(vec![pad("xbox:0", Some(70))], true).is_empty(), "já conectado ao abrir: sem aviso");
        assert_eq!(kinds(&w.update(vec![pad("xbox:0", None), pad("ps", None)], false)), ["+ps"]);
        // uma falha momentânea não conta como desconexão
        assert!(w.update(vec![pad("xbox:0", None)], false).is_empty());
        assert!(w.update(vec![pad("xbox:0", None), pad("ps", None)], false).is_empty());
        // duas consultas seguidas sem o controle: desconectado
        assert!(w.update(vec![pad("xbox:0", None)], false).is_empty());
        assert_eq!(kinds(&w.update(vec![pad("xbox:0", None)], false)), ["-ps"]);
        // o aviso de desconexão traz a última bateria conhecida (a consulta rápida não apaga)
        assert!(w.update(vec![], false).is_empty());
        match &w.update(vec![], false)[..] { [PadEvent::Disconnected(c)] => assert_eq!(c.battery_percent, Some(70)), o => panic!("{o:?}") }
    }

    #[test]
    fn toast_uses_the_custom_name() {
        let c = pad("ps:aa", Some(50));
        assert_eq!(to_toast(&PadEvent::Connected(c.clone())).name, "Controle ps:aa");
        set_pad_names(HashMap::from([("ps:aa".to_string(), String::from("Controle da sala")), ("xbox:1".into(), "  ".into())]));
        assert_eq!(to_toast(&PadEvent::Connected(c)).name, "Controle da sala");
        assert_eq!(to_toast(&PadEvent::Connected(pad("xbox:1", None))).name, "Controle xbox:1", "nome vazio não vale");
        set_pad_names(HashMap::new());
    }

    #[test]
    fn low_battery_once_with_hysteresis_and_never_when_charging() {
        let mut w = PadWatch::default();
        assert!(w.update(vec![pad("xbox:0", Some(40))], true).is_empty());
        assert!(w.update(vec![pad("xbox:0", Some(16))], true).is_empty());
        assert_eq!(kinds(&w.update(vec![pad("xbox:0", Some(15))], true)), ["!xbox:0"], "15% avisa");
        assert!(w.update(vec![pad("xbox:0", Some(10))], true).is_empty(), "só uma vez por queda");
        assert!(w.update(vec![pad("xbox:0", Some(18))], true).is_empty(), "18% ainda não rearma");
        assert!(w.update(vec![pad("xbox:0", Some(12))], true).is_empty());
        assert!(w.update(vec![pad("xbox:0", Some(25))], true).is_empty(), "rearmou");
        assert_eq!(kinds(&w.update(vec![pad("xbox:0", Some(14))], true)), ["!xbox:0"]);
        // carregando ou por cabo: nunca avisa
        let mut w2 = PadWatch::default();
        let charging = Controller { charging: true, ..pad("ps", Some(5)) };
        assert!(w2.update(vec![charging], true).is_empty());
        let wired = Controller { wired: true, ..pad("xbox:1", None) };
        assert_eq!(kinds(&w2.update(vec![wired], true)), ["+xbox:1"], "conectou, sem aviso de bateria");
        // nível "Crítica" (sem %) também é bateria baixa; já baixo ao abrir o app também avisa
        let mut w3 = PadWatch::default();
        let crit = Controller { battery_label: Some("Crítica".into()), ..pad("xbox:2", None) };
        assert_eq!(kinds(&w3.update(vec![crit], true)), ["!xbox:2"]);
        // consulta rápida (sem bateria) não dispara nem rearma
        assert!(w3.update(vec![pad("xbox:2", None)], false).is_empty());
    }

    #[test]
    fn toast_payload() {
        let t = to_toast(&PadEvent::LowBattery(pad("xbox:0", Some(12))));
        assert_eq!((t.event.as_str(), t.device.as_str(), t.percent), ("battery", "xbox", Some(12)));
        let ps = Controller { kind: "playstation".into(), ..pad("ps", None) };
        assert_eq!(to_toast(&PadEvent::Connected(ps)).device, "playstation");
    }
}
