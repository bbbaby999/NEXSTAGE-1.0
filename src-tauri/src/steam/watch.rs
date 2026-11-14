//! Steam: aviso de conquista desbloqueada quase em tempo real (cartão por cima do jogo).
//!
//! A Web API não avisa nada sozinha, e a sincronização normal roda de hora em hora. Por isso:
//! - gatilho local: o cliente Steam regrava `appcache/stats/UserGameStats_<conta>_<appid>.bin` quando
//!   as conquistas/estatísticas do jogo mudam. Só a DATA do arquivo é observada (a cada 2 s);
//! - reforço: com um jogo da Steam aberto (RunningAppID no registro, seja qual for o jeito como foi
//!   aberto), a API é consultada a cada 45 s, caso o arquivo não seja regravado;
//! - confirmação: o aviso só sai para conquistas que a API devolve como obtidas, que não estavam no
//!   cache e foram desbloqueadas nos últimos 15 minutos. A API pode levar alguns segundos para
//!   refletir o desbloqueio, então a consulta é repetida algumas vezes depois de cada gatilho.
//! Precisa da conta Steam do perfil com a chave pessoal da Web API (a mesma das conquistas).

use super::{client, local, store, sync};
use crate::db::Db;
use crate::retro::util::now_epoch;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

/// SteamID64 → ID de conta (32 bits) usado nos nomes de arquivo do cliente.
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;
const TICK: Duration = Duration::from_secs(2);
const RUNNING_POLL: Duration = Duration::from_secs(45);
/// Só conquistas desbloqueadas nesta janela viram aviso.
const FRESH_SECS: i64 = 15 * 60;
/// Espera antes de cada consulta após um gatilho (a API às vezes demora para refletir).
const RETRY_GAPS: [u64; 4] = [3, 5, 12, 25];

struct Pending {
    before: HashSet<String>,
    attempt: usize,
    next: Instant,
}

/// Jogos a conferir depois de uma varredura. Vários arquivos mudando ao mesmo tempo é o cliente
/// regravando o cache (ao abrir, ao sincronizar a nuvem…), não conquistas: nesse caso só conta o
/// jogo em execução.
pub fn pick_targets(mut changed: Vec<i64>, running: Option<i64>) -> Vec<i64> {
    if changed.len() > 2 {
        changed.retain(|a| Some(*a) == running);
    }
    changed.sort_unstable();
    changed.dedup();
    changed
}

pub fn account_id(steamid64: &str) -> Option<u32> {
    steamid64.parse::<u64>().ok()?.checked_sub(STEAMID64_BASE).and_then(|v| u32::try_from(v).ok())
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("steam-ach-watch".into())
        .spawn(move || {
            let mut baseline: Option<(i64, HashMap<i64, SystemTime>)> = None;
            let mut pending: HashMap<i64, Pending> = HashMap::new();
            let mut last_poll: HashMap<i64, Instant> = HashMap::new();
            loop {
                std::thread::sleep(TICK);
                let profile = app.state::<crate::overlay::Runtime>().active_profile.load(Ordering::SeqCst);
                if profile <= 0 || crate::overlay::setting(&app, "steam_notify").as_deref() == Some("0") {
                    baseline = None;
                    continue;
                }
                let db = app.state::<Db>();
                let Ok(Some((steamid, Some(key)))) = sync::identity(&db, profile) else {
                    baseline = None;
                    std::thread::sleep(Duration::from_secs(20)); // sem conta/chave: confere de novo depois
                    continue;
                };
                let Some(acct) = account_id(&steamid) else { continue };
                let now = Instant::now();
                let running = local::running_appid();
                let mtimes = local::stats_mtimes(acct);

                let mut targets = match &baseline {
                    Some((p, old)) if *p == profile => {
                        pick_targets(mtimes.iter().filter(|(a, t)| old.get(a) != Some(t)).map(|(a, _)| *a).collect(), running)
                    }
                    _ => Vec::new(), // primeira varredura do perfil: só registra o estado atual
                };
                baseline = Some((profile, mtimes));
                let mut one_shot = Vec::new();
                if let Some(r) = running {
                    if last_poll.get(&r).is_none_or(|t| now.duration_since(*t) >= RUNNING_POLL) {
                        last_poll.insert(r, now);
                        if !targets.contains(&r) {
                            one_shot.push(r);
                        }
                    }
                }
                for (appid, single) in targets.drain(..).map(|a| (a, false)).chain(one_shot.into_iter().map(|a| (a, true))) {
                    if pending.contains_key(&appid) {
                        continue;
                    }
                    let Ok(before) = db.conn().and_then(|c| store::unlocked_set(&c, profile, appid)) else { continue };
                    let attempt = if single { RETRY_GAPS.len() - 1 } else { 0 };
                    let wait = if single { 0 } else { RETRY_GAPS[0] };
                    pending.insert(appid, Pending { before, attempt, next: now + Duration::from_secs(wait) });
                }

                let due: Vec<i64> = pending.iter().filter(|(_, p)| p.next <= now).map(|(a, _)| *a).collect();
                for appid in due {
                    let Some(p) = pending.get_mut(&appid) else { continue };
                    match check(&app, &db, profile, &steamid, &key, appid, &p.before) {
                        Ok(true) | Err(_) => {
                            pending.remove(&appid);
                        }
                        Ok(false) if p.attempt + 1 < RETRY_GAPS.len() => {
                            p.attempt += 1;
                            p.next = Instant::now() + Duration::from_secs(RETRY_GAPS[p.attempt]);
                        }
                        Ok(false) => {
                            pending.remove(&appid);
                        }
                    }
                }
            }
        })
        .ok();
}

/// Atualiza as conquistas do jogo pela API e avisa as novas. `Ok(true)` = houve aviso.
fn check(app: &AppHandle, db: &Db, profile: i64, steamid: &str, key: &str, appid: i64, before: &HashSet<String>) -> Result<bool, ()> {
    let lang = crate::overlay::setting(app, "language").unwrap_or_else(|| "pt-BR".into());
    let fresh = detect(db, profile, steamid, key, appid, before, &lang, now_epoch() - FRESH_SECS)?;
    if fresh.is_empty() {
        return Ok(false);
    }
    notify(app, profile, &fresh);
    Ok(true)
}

/// Busca o estado do jogador na API (e o esquema, na primeira vez), grava no cache e devolve as
/// conquistas que não estavam em `before` e foram desbloqueadas a partir de `since`.
pub fn detect(db: &Db, profile: i64, steamid: &str, key: &str, appid: i64, before: &HashSet<String>, lang: &str, since: i64) -> Result<Vec<store::AchEntry>, ()> {
    let schema_cached = db.conn().ok().and_then(|c| store::has_schema(&c, appid).ok()).unwrap_or(false);
    let res: Result<(), String> = tauri::async_runtime::block_on(async {
        if schema_cached {
            let list = client::player_achievements(Some(key), steamid, appid).await.map_err(|e| e.message())?;
            let c = db.conn().map_err(|e| e.to_string())?;
            store::store_player(&c, profile, appid, &list).map_err(|e| e.to_string())
        } else {
            // primeira vez deste jogo: nomes, ícones e raridade junto com o estado do jogador
            sync::fetch_game(db, profile, steamid, Some(key), appid, client::steam_lang(lang)).await.map_err(|e| e.to_string())
        }
    });
    if res.is_err() {
        return Err(());
    }
    let c = db.conn().map_err(|_| ())?;
    store::fresh_unlocks(&c, profile, appid, before, since).map_err(|_| ())
}

fn notify(app: &AppHandle, profile: i64, fresh: &[store::AchEntry]) {
    let toasts: Vec<crate::shot_toast::AchievementToast> = fresh
        .iter()
        .take(5)
        .map(|a| crate::shot_toast::AchievementToast {
            source: "steam".into(),
            icon: a.icon.clone(),
            title: a.name.clone(),
            game: a.game_name.clone(),
            points: 0,
            hardcore: false,
            rare: crate::shot_toast::steam_is_rare(a.global_pct),
            global_pct: a.global_pct,
            more: 0,
            grade: None,
        })
        .collect();
    crate::shot_toast::notify_achievements(app, toasts, fresh.len().saturating_sub(5));
    // telas abertas (aba Conquistas, página do jogo) recarregam do cache
    let _ = app.emit("steam-synced", serde_json::json!({ "profileId": profile }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_ignore_bulk_rewrites_except_running_game() {
        assert_eq!(pick_targets(vec![620], None), vec![620]);
        assert_eq!(pick_targets(vec![620, 220], None), vec![220, 620]);
        // o cliente regravou vários arquivos de uma vez: só o jogo aberto interessa
        assert_eq!(pick_targets(vec![3812600, 1091500, 409720], Some(409720)), vec![409720]);
        assert!(pick_targets(vec![3812600, 1091500, 409720], None).is_empty());
    }

    #[test]
    fn account_id_from_steamid64() {
        // caso real desta máquina: ActiveUser 97486066
        assert_eq!(account_id("76561198057751794"), Some(97_486_066));
        assert_eq!(account_id("abc"), None);
        assert_eq!(account_id("1"), None);
    }
}

#[cfg(all(test, windows))]
mod live {
    use super::*;

    /// Detecção real, numa CÓPIA do banco (o oficial não é alterado):
    /// `cargo test --lib steam::watch::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn detects_a_forgotten_unlock_against_the_real_api() {
        let src = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("app.nexstage.launcher").join("nexstage.db");
        let tmp = std::env::temp_dir().join(format!("nexstage-watch-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let ro = rusqlite::Connection::open_with_flags(&src, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            ro.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let db = Db::open(&tmp).unwrap();
        let profile = 1;
        let (steamid, key) = sync::identity(&db, profile).unwrap().expect("perfil 1 sem conta Steam");
        let key = key.expect("sem chave da Web API legível");
        println!("conta {steamid} → ID {:?}", account_id(&steamid));

        // conquista mais recente da conta, "esquecida" na cópia (como se tivesse acabado de sair)
        let (appid, api, at): (i64, String, i64) = db.conn().unwrap().query_row(
            "SELECT appid, apiname, unlocked_at FROM steam_ach_user WHERE profile_id=?1 AND unlocked_at > 1 ORDER BY unlocked_at DESC LIMIT 1",
            [profile], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        db.conn().unwrap().execute("UPDATE steam_ach_user SET unlocked_at=NULL WHERE profile_id=?1 AND appid=?2 AND apiname=?3", rusqlite::params![profile, appid, api]).unwrap();
        let before = store::unlocked_set(&db.conn().unwrap(), profile, appid).unwrap();
        assert!(!before.contains(&api));

        let t0 = std::time::Instant::now();
        let fresh = detect(&db, profile, &steamid, &key, appid, &before, "pt-BR", at - 1).expect("API falhou");
        println!("API + diff em {:?}: {:?}", t0.elapsed(), fresh.iter().map(|a| (&a.game_name, &a.name, a.global_pct, a.icon.as_deref())).collect::<Vec<_>>());
        assert_eq!(fresh.len(), 1, "deveria achar só a conquista esquecida");
        assert_eq!(fresh[0].apiname, api);
        assert!(fresh[0].icon.as_deref().unwrap_or("").contains("/community_assets/"), "ícone no endereço atual da CDN");
        let rare = crate::shot_toast::steam_is_rare(fresh[0].global_pct);
        println!("seria aviso {}", if rare { "RARO" } else { "COMUM" });

        // com a janela real de 15 min, uma conquista antiga NÃO vira aviso
        db.conn().unwrap().execute("UPDATE steam_ach_user SET unlocked_at=NULL WHERE profile_id=?1 AND appid=?2 AND apiname=?3", rusqlite::params![profile, appid, api]).unwrap();
        let old = detect(&db, profile, &steamid, &key, appid, &before, "pt-BR", now_epoch() - FRESH_SECS).unwrap();
        assert!(old.is_empty(), "conquista antiga não pode avisar: {:?}", old.iter().map(|a| &a.name).collect::<Vec<_>>());

        // gatilhos locais
        let m = local::stats_mtimes(account_id(&steamid).unwrap());
        println!("arquivos de estatísticas da conta: {} (este jogo: {}) · jogo da Steam aberto agora: {:?}", m.len(), m.contains_key(&appid), local::running_appid());
        assert!(!m.is_empty());
        drop(db);
        let _ = std::fs::remove_file(&tmp);
    }
}
