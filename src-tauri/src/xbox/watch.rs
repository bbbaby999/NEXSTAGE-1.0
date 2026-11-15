//! Xbox: aviso de conquista desbloqueada quase em tempo real (cartão por cima do jogo).
//!
//! O Xbox Live não avisa aplicativos de terceiros. Por isso:
//! - jogo aberto: a cada 5 s, os processos são comparados com as pastas dos jogos do Xbox instalados
//!   (só o caminho do executável é lido; nada é injetado nem alterado), seja qual for o jeito como o
//!   jogo foi aberto;
//! - enquanto um jogo está aberto, as conquistas dele são consultadas a cada 45 s, e uma última vez
//!   20 s depois que ele fecha;
//! - o aviso só sai para conquistas que não estavam obtidas no cache e foram desbloqueadas nos
//!   últimos 15 minutos. Na primeira consulta de um jogo (cache vazio), só o estado é registrado.
//! Precisa da conta Xbox no perfil ativo e de o jogo constar no histórico da conta (sincronização).

use super::store;
use crate::db::Db;
use crate::retro::util::now_epoch;
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_secs(45);
const TARGETS_TTL: Duration = Duration::from_secs(120);
const FINAL_DELAY: Duration = Duration::from_secs(20);
/// Só conquistas desbloqueadas nesta janela viram aviso.
const FRESH_SECS: i64 = 15 * 60;

/// Jogo instalado com título no histórico da conta.
struct Target {
    title_id: String,
    roots: Vec<String>,
}

fn targets(db: &Db, profile: i64) -> Vec<Target> {
    let games = crate::msstore::installed_games();
    let Ok(conn) = db.conn() else { return vec![] };
    let mut out = Vec::new();
    for g in games {
        let tid: Option<String> = conn
            .query_row("SELECT title_id FROM xbox_titles WHERE profile_id=?1 AND lower(pfn)=lower(?2)", rusqlite::params![profile, g.family], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        if let Some(title_id) = tid {
            out.push(Target { title_id, roots: crate::msstore::process_roots(&g.aumid) });
        }
    }
    out.retain(|t| !t.roots.is_empty());
    out
}

/// Títulos com algum processo aberto dentro da pasta do jogo.
fn running(targets: &[Target], procs: &[(u32, String)]) -> HashSet<String> {
    targets
        .iter()
        .filter(|t| procs.iter().any(|(_, p)| t.roots.iter().any(|r| p.starts_with(r.as_str()))))
        .map(|t| t.title_id.clone())
        .collect()
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("xbox-ach-watch".into())
        .spawn(move || {
            let mut cache: Option<(i64, Instant, Vec<Target>)> = None;
            let mut last_poll: HashMap<String, Instant> = HashMap::new();
            let mut prev: HashSet<String> = HashSet::new();
            let mut finals: HashMap<String, Instant> = HashMap::new();
            loop {
                std::thread::sleep(TICK);
                let profile = app.state::<crate::overlay::Runtime>().active_profile.load(Ordering::SeqCst);
                if profile <= 0 || crate::overlay::setting(&app, "xbox_notify").as_deref() == Some("0") {
                    cache = None;
                    continue;
                }
                let db = app.state::<Db>();
                let has_account = db.conn().ok().and_then(|c| crate::accounts::get(&c, profile, store::PLATFORM).ok().flatten()).is_some();
                if !has_account {
                    cache = None;
                    std::thread::sleep(Duration::from_secs(20)); // sem conta: confere de novo depois
                    continue;
                }
                let now = Instant::now();
                if !matches!(&cache, Some((p, at, _)) if *p == profile && now.duration_since(*at) < TARGETS_TTL) {
                    if cache.as_ref().is_some_and(|c| c.0 != profile) {
                        prev.clear();
                        finals.clear();
                        last_poll.clear();
                    }
                    cache = Some((profile, now, targets(&db, profile)));
                }
                let Some((_, _, list)) = &cache else { continue };
                if list.is_empty() && finals.is_empty() {
                    continue;
                }
                let open = running(list, &crate::msstore::process_paths());
                for closed in prev.difference(&open) {
                    finals.insert(closed.clone(), now + FINAL_DELAY);
                }
                for t in &open {
                    finals.remove(t);
                    if last_poll.get(t).is_none_or(|x| now.duration_since(*x) >= POLL) {
                        last_poll.insert(t.clone(), now);
                        check(&app, &db, profile, t);
                    }
                }
                let due: Vec<String> = finals.iter().filter(|(_, at)| **at <= now).map(|(t, _)| t.clone()).collect();
                for t in due {
                    finals.remove(&t);
                    last_poll.remove(&t);
                    check(&app, &db, profile, &t);
                }
                prev = open;
            }
        })
        .ok();
}

/// Consulta as conquistas do jogo e avisa as recém-desbloqueadas.
fn check(app: &AppHandle, db: &Db, profile: i64, title_id: &str) {
    let Ok((before, had_cache)) = db.conn().map_err(|_| ()).and_then(|c| {
        let before = store::unlocked_ids(&c, profile, title_id).map_err(|_| ())?;
        let had: bool = c
            .query_row(
                "SELECT ach_fetched_at IS NOT NULL FROM xbox_titles WHERE profile_id=?1 AND title_id=?2",
                rusqlite::params![profile, title_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        Ok((before, had))
    }) else {
        return;
    };
    let lang = crate::overlay::setting(app, "language").unwrap_or_else(|| "pt-BR".into());
    if tauri::async_runtime::block_on(super::refresh_title(app, profile, title_id, &lang, false)).is_err() {
        return;
    }
    if !had_cache {
        return; // primeira consulta deste jogo: só registra o estado atual
    }
    let fresh = match db.conn() {
        Ok(c) => store::fresh_unlocks(&c, profile, title_id, &before, now_epoch() - FRESH_SECS).unwrap_or_default(),
        Err(_) => return,
    };
    if fresh.is_empty() {
        return;
    }
    let toasts: Vec<crate::shot_toast::AchievementToast> = fresh
        .iter()
        .take(5)
        .map(|a| crate::shot_toast::AchievementToast {
            source: "xbox".into(),
            icon: a.icon.clone(),
            title: a.name.clone(),
            game: a.game.clone(),
            points: a.gamerscore,
            hardcore: false,
            rare: crate::shot_toast::steam_is_rare(a.rarity),
            global_pct: a.rarity,
            more: 0,
            grade: None,
        })
        .collect();
    crate::shot_toast::notify_achievements(app, toasts, fresh.len().saturating_sub(5));
    let _ = app.emit("xbox-synced", serde_json::json!({ "profileId": profile }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_matches_either_package_folder() {
        let t = vec![Target { title_id: String::from("1"), roots: vec![String::from("c:\\program files\\windowsapps\\fh6\\"), String::from("c:\\xboxgames\\fh6\\content\\")] }];
        let procs = vec![(10, String::from("c:\\xboxgames\\fh6\\content\\forzahorizon6.exe")), (11, String::from("c:\\windows\\explorer.exe"))];
        assert_eq!(running(&t, &procs), HashSet::from([String::from("1")]));
        assert!(running(&t, &[(1, String::from("c:\\xboxgames\\fh6x\\a.exe"))]).is_empty(), "pasta parecida não conta");
    }
}
