//! Sincronização Steam de um perfil: perfil (ISteamUser / XML público), biblioteca (IPlayerService),
//! jogados recentemente e conquistas/estatísticas (ISteamUserStats) dos jogos vinculados com atividade
//! recente. Respeita privacidade: o que a Steam não devolve fica marcado como indisponível.

use super::client::{self, SteamError};
use super::store;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::retro::util::{fmt_epoch, int, now_epoch, parse_epoch, text};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const KEY_ENTROPY: &[u8] = b"NEXSTAGE/SteamWebAPI/v1";
/// Jogos com conquistas atualizadas por sincronização (cada jogo = 2 a 4 chamadas, ~1 s com o
/// intervalo mínimo entre chamadas). Contas grandes completam a carga inicial em algumas rodadas.
const ACH_PER_SYNC: usize = 30;

/// Fila de jogos para buscar conquistas: com conquistas na Steam, nunca buscados ou com atividade desde
/// `?2` (última sincronização). Os vinculados à biblioteca vêm primeiro; os demais só com tempo de jogo.
const ACH_QUEUE_SQL: &str = "SELECT o.appid,
        EXISTS(SELECT 1 FROM steam_links l JOIN profile_games pg ON pg.game_id=l.game_id AND pg.profile_id=o.profile_id
               WHERE l.appid=o.appid) AS linked
     FROM steam_owned o
     LEFT JOIN steam_stats s ON s.profile_id=o.profile_id AND s.appid=o.appid
     WHERE o.profile_id=?1 AND o.has_stats=1 AND (s.appid IS NULL OR o.recent=1 OR COALESCE(o.last_played,0) > ?2)
       AND (linked OR COALESCE(o.playtime_forever,0) > 0)
     ORDER BY linked DESC, COALESCE(o.last_played,0) DESC LIMIT ?3";

fn ach_queue(c: &rusqlite::Connection, profile_id: i64, since: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare(ACH_QUEUE_SQL)?;
    let rows = st.query_map([profile_id, since, ACH_PER_SYNC as i64], |r| r.get(0))?.collect();
    rows
}

static RUNNING: AtomicBool = AtomicBool::new(false);
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}
struct Guard;
impl Guard {
    fn take() -> Option<Self> {
        RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Guard)
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn protect_key(k: &str) -> Option<Vec<u8>> {
    crate::vault::protect(KEY_ENTROPY, k)
}
pub fn unprotect_key(b: &[u8]) -> Option<String> {
    crate::vault::unprotect(KEY_ENTROPY, b)
}

/// (SteamID64, chave da Web API se houver e for legível neste Windows).
pub fn identity(db: &Db, profile_id: i64) -> AppResult<Option<(String, Option<String>)>> {
    let conn = db.conn()?;
    let Some(a) = crate::accounts::get(&conn, profile_id, store::PLATFORM)? else { return Ok(None) };
    let Some(id) = a.external_id else { return Ok(None) };
    Ok(Some((id, a.secret.as_deref().and_then(unprotect_key))))
}

/// Dados públicos do perfil a partir de GetPlayerSummaries.
pub fn summary_data(p: &Value) -> Value {
    let vis = match int(&p["communityvisibilitystate"]) {
        3 => "public",
        2 => "friends",
        1 => "private",
        _ => "unknown",
    };
    json!({
        "name": text(&p["personaname"]),
        "avatar": Some(text(&p["avatarfull"])).filter(|s| s.starts_with("https://")),
        "profileUrl": Some(text(&p["profileurl"])).filter(|s| s.starts_with("https://")),
        "visibility": vis,
        "configured": int(&p["profilestate"]) == 1,
        "createdAt": p.get("timecreated").map(int).filter(|t| *t > 0).map(fmt_epoch),
        "lastLogoff": p.get("lastlogoff").map(int).filter(|t| *t > 0).map(fmt_epoch),
        "source": "api",
    })
}

/// Perfil: com chave, ISteamUser; sem chave, XML público da Comunidade Steam.
pub async fn fetch_profile(steamid: &str, key: Option<&str>) -> Result<Value, SteamError> {
    if key.is_some() {
        return client::player_summary(key, steamid).await.map(|p| summary_data(&p));
    }
    let (name, avatar, privacy) = client::community_profile(steamid).await?;
    let vis = match privacy.as_str() {
        "public" => "public",
        "friendsonly" => "friends",
        "private" => "private",
        _ => "unknown",
    };
    Ok(json!({ "name": name, "avatar": avatar, "profileUrl": format!("https://steamcommunity.com/profiles/{steamid}/"), "visibility": vis, "source": "community" }))
}

/// Atualiza conquistas, porcentagens globais e estatísticas de um jogo.
pub async fn fetch_game(db: &Db, profile_id: i64, steamid: &str, key: Option<&str>, appid: i64, lang: &str) -> AppResult<()> {
    let key = key.ok_or(SteamError::NoKey)?;
    let schema = match client::schema(Some(key), appid, lang).await {
        Ok(s) => s,
        Err(SteamError::NoStats) => {
            store::store_stats(&*db.conn()?, profile_id, appid, None, Some("nostats"))?;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    let labels = store::store_schema(&*db.conn()?, appid, &schema)?;
    let has_achs = schema["availableGameStats"]["achievements"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
    if has_achs {
        if let Ok(g) = client::global_percentages(appid).await {
            store::store_global(&*db.conn()?, appid, &g)?;
        }
    }
    let mut error: Option<String> = None;
    if has_achs {
        match client::player_achievements(Some(key), steamid, appid).await {
            Ok(list) => store::store_player(&*db.conn()?, profile_id, appid, &list)?,
            Err(SteamError::Private) => error = Some("private".into()),
            Err(SteamError::NoStats) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let mut stats_json: Option<Value> = None;
    if !labels.is_empty() && error.is_none() {
        match client::user_stats(Some(key), steamid, appid).await {
            Ok(st) => {
                let rows: Vec<Value> = st
                    .iter()
                    .map(|s| {
                        let name = text(&s["name"]);
                        let label = labels.iter().find(|(n, _)| *n == name).map(|(_, l)| l.clone()).filter(|l| !l.is_empty());
                        json!({ "name": name, "label": label, "value": s["value"] })
                    })
                    .collect();
                stats_json = Some(Value::Array(rows));
            }
            Err(SteamError::Private) => error = Some("private".into()),
            Err(SteamError::NoStats) => {}
            Err(e) => return Err(e.into()),
        }
    }
    if !has_achs && labels.is_empty() {
        error = Some("nostats".into());
    }
    store::store_stats(&*db.conn()?, profile_id, appid, stats_json.as_ref(), error.as_deref())?;
    Ok(())
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub owned: Option<usize>,
    pub recent: Option<usize>,
    pub library_private: bool,
    pub achievements_updated: usize,
    pub used_key: bool,
    pub warning: Option<String>,
    pub synced_at: String,
}

pub async fn sync_profile(app: &AppHandle, profile_id: i64, lang: &str) -> AppResult<SyncReport> {
    let _g = Guard::take().ok_or_else(|| AppError::Validation("Sincronização da Steam já em andamento.".into()))?;
    let db = app.state::<Db>();
    let (steamid, key) = identity(&db, profile_id)?.ok_or_else(|| AppError::Validation("Nenhuma conta Steam conectada a este perfil.".into()))?;
    let _ = app.emit("steam-sync-state", json!({ "profileId": profile_id, "running": true }));
    let mut rep = SyncReport { used_key: key.is_some(), ..Default::default() };
    let res: Result<(), SteamError> = async {
        let prof = fetch_profile(&steamid, key.as_deref()).await?;
        let name = prof["name"].as_str().map(str::to_string);
        if let Ok(c) = db.conn() {
            let _ = crate::accounts::set_data(&c, profile_id, store::PLATFORM, name.as_deref(), &prof.to_string());
        }
        if key.is_none() {
            // sem chave: só o que é possível com segurança (perfil público + jogos instalados no PC)
            let apps: Vec<_> = super::local::local_apps().into_values().collect();
            if let Ok(c) = db.conn() {
                let _ = store::store_local(&c, profile_id, &apps);
            }
            rep.warning = Some(SteamError::NoKey.message());
            return Ok(());
        }
        match client::owned_games(key.as_deref(), &steamid).await {
            Ok(games) => {
                let c = db.conn().map_err(|_| SteamError::BadResponse)?;
                rep.owned = store::store_owned(&c, profile_id, &games).ok();
            }
            Err(SteamError::Private) => rep.library_private = true, // mantém o cache anterior
            Err(e) => return Err(e),
        }
        match client::recently_played(key.as_deref(), &steamid).await {
            Ok(r) => {
                let c = db.conn().map_err(|_| SteamError::BadResponse)?;
                let _ = store::store_recent(&c, profile_id, &r);
                rep.recent = Some(r.len());
            }
            Err(SteamError::Private) => {}
            Err(e) => return Err(e),
        }
        Ok(())
    }
    .await;

    // conquistas (aba Conquistas e página de cada jogo): jogos da conta com conquistas, nunca buscados ou
    // com atividade desde a última sincronização. Os vinculados à biblioteca vêm primeiro; os demais só
    // com tempo de jogo (sem tempo de jogo não há conquista obtida — evita centenas de chamadas inúteis).
    if res.is_ok() && key.is_some() && !rep.library_private {
        let prev = {
            let c = db.conn()?;
            crate::accounts::get(&c, profile_id, store::PLATFORM)?.and_then(|a| a.synced_at).and_then(|s| parse_epoch(&s)).unwrap_or(0)
        };
        let queue = ach_queue(&*db.conn()?, profile_id, prev)?;
        for appid in queue {
            match fetch_game(&db, profile_id, &steamid, key.as_deref(), appid, lang).await {
                Ok(()) => rep.achievements_updated += 1,
                Err(e) => {
                    rep.warning = Some(e.to_string());
                    break;
                }
            }
        }
    }

    let _ = app.emit("steam-sync-state", json!({ "profileId": profile_id, "running": false }));
    let c = db.conn()?;
    match res {
        Ok(()) => {
            rep.synced_at = fmt_epoch(now_epoch());
            crate::accounts::set_sync_result(&c, profile_id, store::PLATFORM, Some(&rep.synced_at), None)?;
            let _ = app.emit("steam-synced", json!({ "profileId": profile_id }));
            Ok(rep)
        }
        Err(e) => {
            crate::accounts::set_sync_result(&c, profile_id, store::PLATFORM, None, Some(&e.message()))?;
            let _ = app.emit("steam-synced", json!({ "profileId": profile_id }));
            Err(e.into())
        }
    }
}

/// Sincronização automática em segundo plano (padrão: a cada 60 min; nunca na abertura do app).
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("steam-sync".into())
        .spawn(move || {
            let start = std::time::Instant::now();
            let mut last: (i64, i64) = (0, 0); // (perfil, epoch da última tentativa)
            let mut failures = 0u32;
            loop {
                std::thread::sleep(Duration::from_secs(30));
                if start.elapsed() < Duration::from_secs(90) {
                    continue;
                }
                let profile = app.state::<crate::overlay::Runtime>().active_profile.load(Ordering::SeqCst);
                if profile <= 0 {
                    continue;
                }
                let minutes = crate::overlay::setting(&app, "steam_sync_minutes").and_then(|v| v.parse::<i64>().ok()).unwrap_or(60);
                if minutes <= 0 || is_running() || client::blocked_for().is_some() {
                    continue;
                }
                let db = app.state::<Db>();
                let synced = db
                    .conn()
                    .ok()
                    .and_then(|c| crate::accounts::get(&c, profile, store::PLATFORM).ok().flatten())
                    .map(|a| a.synced_at.and_then(|s| parse_epoch(&s)).unwrap_or(0));
                let Some(synced) = synced else { continue }; // sem conta Steam neste perfil
                if last.0 != profile {
                    last = (profile, synced);
                    failures = 0;
                }
                let backoff = [0, 10, 30, 60][failures.min(3) as usize] * 60;
                let now = now_epoch();
                if now - last.1.max(synced) < (minutes * 60).max(backoff) {
                    continue;
                }
                last.1 = now;
                let lang = crate::overlay::setting(&app, "language").unwrap_or_else(|| "pt-BR".into());
                match tauri::async_runtime::block_on(sync_profile(&app, profile, client::steam_lang(&lang))) {
                    Ok(_) => failures = 0,
                    Err(_) => failures += 1,
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn achievement_queue_includes_unlinked_played_games_linked_first() {
        let c = store::tests::mem();
        let t = 1_700_000_000;
        store::store_owned(&c, 1, &[
            json!({"appid": 10, "name": "Vinculado sem tempo", "playtime_forever": 0, "rtime_last_played": t - 500, "has_community_visible_stats": true}),
            json!({"appid": 20, "name": "Só na Steam, jogado", "playtime_forever": 120, "rtime_last_played": t, "has_community_visible_stats": true}),
            json!({"appid": 30, "name": "Só na Steam, nunca jogado", "playtime_forever": 0, "has_community_visible_stats": true}),
            json!({"appid": 40, "name": "Sem conquistas", "playtime_forever": 300, "rtime_last_played": t}),
            json!({"appid": 50, "name": "Já buscado, sem atividade", "playtime_forever": 60, "rtime_last_played": t - 9000, "has_community_visible_stats": true}),
        ]).unwrap();
        c.execute("INSERT INTO steam_links(game_id, appid) VALUES(1, 10)", []).unwrap(); // jogo 1 está no perfil 1
        store::store_stats(&c, 1, 50, None, None).unwrap();
        // vinculado primeiro (mesmo sem tempo de jogo); depois os jogados; nunca jogados, sem conquistas
        // e já buscados sem atividade nova ficam de fora
        assert_eq!(ach_queue(&c, 1, t - 1000).unwrap(), vec![10, 20]);
        // atividade depois da última sincronização: volta para a fila
        assert_eq!(ach_queue(&c, 1, t - 10_000).unwrap(), vec![10, 20, 50]);
    }
}
