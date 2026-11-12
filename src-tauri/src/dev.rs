//! Ferramentas de DESENVOLVIMENTO. Este módulo só existe em compilações de desenvolvimento
//! (`#[cfg(debug_assertions)]` em lib.rs): a versão distribuída não o contém.
//! Não tem interface, tradução nem menção em telas/notas de versão, e não grava nada.
//!
//! Atalhos globais (funcionam com o jogo em foco):
//! - Ctrl+Alt+Shift+F9  → simula uma conquista COMUM desbloqueada
//! - Ctrl+Alt+Shift+F10 → simula uma conquista RARA desbloqueada
//! - Ctrl+Alt+Shift+F8  → testa a captura de tela: aviso com a miniatura real da tela, SEM salvar arquivo
//!   nem copiar para a Galeria (o F12 é reservado pelo Windows para depuradores)
//! - Ctrl+Alt+Shift+F11 → simula um aviso de controle, em sequência: conectado → desconectado → bateria fraca
//!   (pelo caminho real `shot_toast::notify_pad`, que respeita o interruptor das Configurações)
//! A conquista é sorteada do cache local da Steam do perfil ativo (ícone, nome, jogo e raridade reais)
//! e passa pela mesma fila/janela/som do aviso real (`shot_toast::notify_achievements`). Só leitura.

use crate::db::Db;
use crate::shot_toast::{self, AchievementToast};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

fn combo(code: Code) -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT), code)
}

/// Registra os atalhos de desenvolvimento (falha em silêncio se outro programa já usa a combinação).
pub fn register(app: &AppHandle) {
    let gs = app.global_shortcut();
    for code in [Code::F8, Code::F9, Code::F10, Code::F11] {
        let _ = gs.register(combo(code));
    }
}

/// Trata um atalho global; `true` se era um atalho de desenvolvimento.
pub fn handle(app: &AppHandle, sc: &Shortcut) -> bool {
    if *sc == combo(Code::F8) {
        let a = app.clone();
        std::thread::spawn(move || crate::screenshot::preview(&a)); // a captura leva alguns ms: fora do atalho
        return true;
    }
    if *sc == combo(Code::F11) {
        static STEP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let step = STEP.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % 3;
        let event = ["connected", "disconnected", "battery"][step];
        shot_toast::notify_pad(app, shot_toast::PadToast {
            event: String::from(event),
            device: String::from("xbox"),
            name: String::from("Controle Xbox 1"),
            percent: Some(if step == 2 { 9 } else { 64 }),
            label: None,
        });
        return true;
    }
    let rare = if *sc == combo(Code::F9) {
        false
    } else if *sc == combo(Code::F10) {
        true
    } else {
        return false;
    };
    let item = sample(app, rare).unwrap_or_else(|| AchievementToast {
        source: "steam".into(),
        icon: None,
        // String::from (não .into()): o script de traduções ignora — nada daqui entra nos idiomas
        title: String::from(if rare { "Conquista rara (simulação)" } else { "Conquista (simulação)" }),
        game: String::from("NEXSTAGE · desenvolvimento"),
        points: 0,
        hardcore: false,
        rare,
        global_pct: Some(if rare { 2.5 } else { 55.0 }),
        more: 0,
    });
    shot_toast::notify_achievements(app, vec![item], 0);
    true
}

/// Conquista real sorteada do cache (comum: ≥ 10% dos jogadores; rara: < 10%). Só leitura.
fn sample(app: &AppHandle, rare: bool) -> Option<AchievementToast> {
    let profile = app.state::<crate::overlay::Runtime>().active_profile.load(std::sync::atomic::Ordering::SeqCst);
    let db = app.state::<Db>();
    let conn = db.conn().ok()?;
    let cond = if rare { "s.global_pct < 10" } else { "s.global_pct >= 10" };
    conn.query_row(
        &format!(
            "SELECT COALESCE(o.name, 'App ' || s.appid), s.name, s.icon, s.global_pct
             FROM steam_ach_schema s LEFT JOIN steam_owned o ON o.appid=s.appid AND o.profile_id=?1
             WHERE s.icon IS NOT NULL AND {cond} ORDER BY RANDOM() LIMIT 1"
        ),
        [profile],
        |r| {
            let pct: Option<f64> = r.get(3)?;
            Ok(AchievementToast {
                source: "steam".into(),
                icon: r.get::<_, Option<String>>(2)?.map(|u| crate::steam::store::media_url(&u)),
                title: r.get(1)?,
                game: r.get(0)?,
                points: 0,
                hardcore: false,
                rare: shot_toast::steam_is_rare(pct),
                global_pct: pct,
                more: 0,
            })
        },
    )
    .ok()
}
