//! Sincronização da conta do RetroAchievements de um perfil.
//!
//! Por sincronização (poucas chamadas, espaçadas pelo `client`):
//! 1. perfil (pontos, foto) → 2. conquistas obtidas desde a última sincronização (com margem) →
//! 3. progresso de todos os jogos → 4. detalhes (lista completa de conquistas) só dos jogos cujo
//! progresso mudou, dos associados à biblioteca e, aos poucos, dos demais (limite por rodada).
//!
//! Notificações: só o que não estava registrado antes E foi desbloqueado depois da sincronização
//! anterior (com margem). A primeira sincronização de um perfil apenas monta a base (sem notificar).

use super::client::{self, RaError};
use super::store::{self, NewUnlock};
use super::util::{fmt_epoch, now_epoch, parse_epoch};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

/// Margem para atrasos do servidor / relógio (desbloqueios até 6 h antes da última sincronização).
const MARGIN_SECS: i64 = 6 * 3600;
/// Na primeira sincronização, busca os desbloqueios dos últimos 90 dias (o resto vem com os detalhes).
const FIRST_WINDOW_SECS: i64 = 90 * 86400;
/// Detalhes de jogos por rodada (cada um é uma chamada à API).
const DETAILS_PER_SYNC: usize = 12;

static RUNNING: AtomicBool = AtomicBool::new(false);
struct Running;
impl Running {
    fn acquire() -> Option<Self> {
        RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Running)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}
pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub new_unlocks: usize,
    pub notified: usize,
    pub games_changed: usize,
    pub details_fetched: usize,
    pub baseline: bool,
    pub synced_at: String,
    /// Aviso não fatal (ex.: limite atingido durante os detalhes; o restante fica para a próxima).
    pub warning: Option<String>,
}

/// Instantâneo do perfil guardado em cache (sem a chave, claro).
pub fn profile_snapshot(v: &Value) -> Value {
    use super::util::{int, opt_text, text};
    json!({
        "user": text(&v["User"]),
        "userPic": opt_text(&v["UserPic"]).map(|p| if p.starts_with("http") { p } else { format!("{}{p}", client::MEDIA) }),
        "memberSince": opt_text(&v["MemberSince"]),
        "motto": opt_text(&v["Motto"]),
        "richPresence": opt_text(&v["RichPresenceMsg"]),
        "lastGameId": int(&v["LastGameID"]),
        "totalPoints": int(&v["TotalPoints"]),
        "totalSoftcorePoints": int(&v["TotalSoftcorePoints"]),
        "totalTruePoints": int(&v["TotalTruePoints"]),
    })
}

/// Fila de detalhes: jogos com progresso alterado, depois os associados à biblioteca sem detalhes,
/// depois os demais sem detalhes (mais recentes primeiro).
fn detail_queue(db: &Db, profile_id: i64, changed: &[i64]) -> AppResult<Vec<i64>> {
    let conn = db.conn()?;
    let mut q: Vec<i64> = changed.iter().copied().take(DETAILS_PER_SYNC + 8).collect();
    let linked = super::link::linked_ra_games(&conn, profile_id)?;
    let mut st = conn.prepare(
        "SELECT p.ra_game_id FROM ra_progress p JOIN ra_games g ON g.ra_game_id=p.ra_game_id
         WHERE p.profile_id=?1 AND g.details_at IS NULL ORDER BY p.last_award_at DESC LIMIT 40",
    )?;
    let missing: Vec<i64> = st.query_map([profile_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    // jogos associados que nunca tiveram detalhes (mesmo sem progresso: mostra as conquistas pendentes)
    let mut st2 = conn.prepare("SELECT ra_game_id FROM ra_games WHERE ra_game_id=?1 AND details_at IS NOT NULL")?;
    for gid in linked {
        let has: bool = st2.exists([gid])?;
        if !has && !q.contains(&gid) && q.len() < DETAILS_PER_SYNC + 4 {
            q.push(gid);
        }
    }
    for gid in missing {
        if q.len() >= DETAILS_PER_SYNC.max(changed.len().min(DETAILS_PER_SYNC + 8)) {
            break;
        }
        if !q.contains(&gid) {
            q.push(gid);
        }
    }
    Ok(q)
}

/// Executa uma sincronização completa do perfil. Erros de rede deixam o cache intacto.
pub async fn sync_profile(app: &AppHandle, profile_id: i64) -> AppResult<SyncReport> {
    let _guard = Running::acquire().ok_or_else(|| AppError::Validation("Sincronização já em andamento.".into()))?;
    let db = app.state::<Db>();
    let (creds, prev) = {
        let conn = db.conn()?;
        let acc = store::account(&conn, profile_id)?;
        (store::creds(&conn, profile_id)?, acc.and_then(|a| a.synced_at))
    };
    let creds = creds.ok_or_else(|| AppError::Validation("Nenhuma conta do RetroAchievements conectada a este perfil.".into()))?;
    let now = now_epoch();
    let prev_epoch = prev.as_deref().and_then(parse_epoch);
    let baseline = prev_epoch.is_none();
    let _ = app.emit("ra-sync-state", json!({ "profileId": profile_id, "running": true }));

    let result: Result<(Vec<NewUnlock>, usize, usize, Option<String>), RaError> = async {
        let prof = client::user_profile(&creds).await?;
        if let Ok(conn) = db.conn() {
            let _ = store::set_account_data(&conn, profile_id, &profile_snapshot(&prof).to_string());
        }
        let from = match prev_epoch {
            Some(p) => (p - MARGIN_SECS).max(now - FIRST_WINDOW_SECS),
            None => now - FIRST_WINDOW_SECS,
        };
        let earned = client::earned_between(&creds, from, now + 60).await?;
        let mut new = {
            let mut conn = db.conn().map_err(|_| RaError::BadResponse)?;
            let tx = conn.transaction().map_err(|_| RaError::BadResponse)?;
            let n = store::store_earned(&tx, profile_id, &earned).map_err(|_| RaError::BadResponse)?;
            tx.commit().map_err(|_| RaError::BadResponse)?;
            n
        };
        let progress = client::completion_progress(&creds).await?;
        let changed = {
            let mut conn = db.conn().map_err(|_| RaError::BadResponse)?;
            let tx = conn.transaction().map_err(|_| RaError::BadResponse)?;
            let c = store::store_progress(&tx, profile_id, &progress).map_err(|_| RaError::BadResponse)?;
            tx.commit().map_err(|_| RaError::BadResponse)?;
            c
        };
        let queue = detail_queue(&db, profile_id, &changed).map_err(|_| RaError::BadResponse)?;
        let mut fetched = 0;
        let mut warning = None;
        for gid in queue {
            match client::game_info_user(&creds, gid).await {
                Ok(v) => {
                    if let Ok(mut conn) = db.conn() {
                        if let Ok(tx) = conn.transaction() {
                            if let Ok(n) = store::store_game_details(&tx, profile_id, &v) {
                                if tx.commit().is_ok() {
                                    new.extend(n);
                                    fetched += 1;
                                }
                            }
                        }
                    }
                }
                Err(e @ (RaError::RateLimited(_) | RaError::Offline | RaError::InvalidCredentials)) => {
                    warning = Some(e.message());
                    break;
                }
                Err(_) => continue, // jogo específico com resposta inválida: segue com os demais
            }
        }
        Ok((new, changed.len(), fetched, warning))
    }
    .await;

    let _ = app.emit("ra-sync-state", json!({ "profileId": profile_id, "running": false }));
    match result {
        Ok((new, changed, fetched, warning)) => {
            let at = fmt_epoch(now);
            {
                let conn = db.conn()?;
                store::set_sync_result(&conn, profile_id, Some(&at), None)?;
            }
            let fresh = if baseline { Vec::new() } else { notifiable(new.clone(), prev_epoch.unwrap_or(now)) };
            let notified = fresh.len();
            if !fresh.is_empty() {
                notify(app, profile_id, &fresh);
            }
            let rep = SyncReport { new_unlocks: new.len(), notified, games_changed: changed, details_fetched: fetched, baseline, synced_at: at, warning };
            let _ = app.emit("ra-synced", json!({ "profileId": profile_id, "report": &rep }));
            Ok(rep)
        }
        Err(e) => {
            if let Ok(conn) = db.conn() {
                let _ = store::set_sync_result(&conn, profile_id, None, Some(&e.message()));
            }
            let _ = app.emit("ra-sync-failed", json!({ "profileId": profile_id, "message": e.message(), "transient": e.is_transient() }));
            Err(e.into())
        }
    }
}

/// Filtra o que merece aviso: desbloqueado depois da sincronização anterior (menos a margem);
/// uma entrada por conquista (Hardcore prevalece).
pub fn notifiable(new: Vec<NewUnlock>, prev_epoch: i64) -> Vec<NewUnlock> {
    let mut by_id: HashMap<i64, NewUnlock> = HashMap::new();
    for n in new {
        let recent = parse_epoch(&n.at).map(|e| e >= prev_epoch - MARGIN_SECS).unwrap_or(false);
        if !recent {
            continue;
        }
        by_id.entry(n.achievement_id).and_modify(|o| if n.hardcore { *o = n.clone() }).or_insert(n);
    }
    let mut v: Vec<NewUnlock> = by_id.into_values().collect();
    v.sort_by(|a, b| a.at.cmp(&b.at));
    v
}

/// Avisa a interface (recarrega as telas) e mostra o cartão de conquista por cima de qualquer janela —
/// jogo aberto pelo NEXSTAGE, pelo emulador direto ou o próprio NEXSTAGE.
fn notify(app: &AppHandle, profile_id: i64, items: &[NewUnlock]) {
    let db = app.state::<Db>();
    let rows = match db.conn() {
        Ok(conn) => store::achievements_by_ids(&conn, profile_id, &items.iter().map(|i| i.achievement_id).collect::<Vec<_>>()).unwrap_or_default(),
        Err(_) => return,
    };
    if rows.is_empty() {
        return;
    }
    let setting = |k: &str| crate::overlay::setting(app, k);
    if setting("ra_notify").as_deref() == Some("0") {
        return;
    }
    let hc: HashMap<i64, bool> = items.iter().map(|i| (i.achievement_id, i.hardcore)).collect();
    let payload: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
            v["hardcore"] = json!(hc.get(&r.id).copied().unwrap_or(false));
            v
        })
        .collect();
    let _ = app.emit("ra-unlocked", json!({ "profileId": profile_id, "items": payload }));
    // até 5 cartões em sequência; o último informa quantas outras chegaram junto
    let toasts: Vec<crate::shot_toast::AchievementToast> = rows
        .iter()
        .take(5)
        .map(|r| crate::shot_toast::AchievementToast {
            source: "ra".into(),
            icon: r.badge_url.clone(),
            title: r.title.clone(),
            game: r.game_title.clone(),
            points: r.points,
            hardcore: hc.get(&r.id).copied().unwrap_or(false),
            rare: crate::shot_toast::ra_is_rare(r.points, r.true_ratio),
            global_pct: None,
            more: 0,
            grade: None,
        })
        .collect();
    let extra = rows.len().saturating_sub(5);
    crate::shot_toast::notify_achievements(app, toasts, extra);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: i64, at: &str, hc: bool) -> NewUnlock {
        NewUnlock { achievement_id: id, hardcore: hc, at: at.into() }
    }

    #[test]
    fn only_recent_new_unlocks_are_notified() {
        let prev = parse_epoch("2024-06-10 12:00:00").unwrap();
        let v = notifiable(
            vec![
                n(1, "2024-06-10 13:00:00", false), // depois da última sincronização: avisa
                n(2, "2024-06-10 08:00:00", false), // dentro da margem (atraso do servidor): avisa
                n(3, "2023-01-01 10:00:00", false), // antigo (veio do preenchimento de detalhes): não avisa
                n(1, "2024-06-10 13:00:00", true),  // mesma conquista: uma entrada só, Hardcore prevalece
                n(4, "", false),                    // sem data confiável: não avisa
            ],
            prev,
        );
        assert_eq!(v.len(), 2);
        assert!(v.iter().any(|x| x.achievement_id == 1 && x.hardcore));
        assert!(v.iter().any(|x| x.achievement_id == 2));
    }
}
