//! Aviso de troféu desbloqueado (Vita3K/RPCS3) quase em tempo real, e vínculo jogo → conjunto.
//!
//! A cada 2 s olha só a DATA/tamanho dos `TROPUSR.DAT` dos emuladores cadastrados (barato). Quando um
//! muda, relê o progresso e compara com o anterior: os troféus novos (com data dos últimos 15 minutos,
//! para não avisar de progresso antigo copiado de outro lugar) viram o mesmo cartão de conquista da
//! Steam/RetroAchievements. A primeira varredura só guarda o estado atual (nenhum aviso ao abrir o app).
//!
//! Se o arquivo mudou enquanto um jogo aberto pelo NEXSTAGE com aquele emulador estava rodando, e o
//! conjunto desse jogo ainda não era conhecido, o vínculo é guardado (`trophy_links`).

use super::{build, learn, resolve, Emu, Root};
use crate::db::Db;
use crate::overlay::Runtime;
use crate::retro::util::now_epoch;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_secs(2);
const ROOTS_EVERY: Duration = Duration::from_secs(30);
const FRESH_SECS: i64 = 15 * 60;

struct Seen {
    stamp: (SystemTime, u64),
    unlocks: super::parse::Unlocks,
}

/// Emuladores cadastrados que gravam troféus (sem repetir a mesma pasta).
fn roots(app: &AppHandle) -> Vec<Root> {
    let db = app.state::<Db>();
    let Ok(conn) = db.conn() else { return vec![] };
    let Ok(mut st) = conn.prepare("SELECT name, exe_path FROM emulators") else { return vec![] };
    let rows: Vec<(String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|it| it.flatten().collect())
        .unwrap_or_default();
    let mut out: Vec<Root> = Vec::new();
    for (name, exe) in rows {
        if let Some(r) = Emu::of(&name, &exe).and_then(|e| Root::from_emulator(e, std::path::Path::new(&exe))) {
            if !out.contains(&r) {
                out.push(r);
            }
        }
    }
    out
}

/// Todos os progressos de um emulador: (pasta da lista, arquivo de progresso, ID do conjunto).
fn files(root: &Root) -> Vec<(PathBuf, PathBuf, String)> {
    let mut out = Vec::new();
    for d in root.conf_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let conf = e.path();
            if !conf.is_dir() {
                continue;
            }
            let Some(id) = e.file_name().to_str().map(String::from) else { continue };
            out.push((conf.clone(), root.usr_file(&conf), id));
        }
    }
    out
}

fn stamp(p: &std::path::Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("trophy-watch".into())
        .spawn(move || {
            let mut seen: HashMap<PathBuf, Seen> = HashMap::new();
            let mut list: Vec<Root> = Vec::new();
            let mut listed_at: Option<Instant> = None;
            let mut first = true;
            loop {
                if listed_at.map_or(true, |t| t.elapsed() >= ROOTS_EVERY) {
                    let fresh = roots(&app);
                    // emulador novo: a primeira passada dele só guarda o estado
                    if fresh.iter().any(|r| !list.contains(r)) && !list.is_empty() {
                        for r in fresh.iter().filter(|r| !list.contains(r)) {
                            for (_, usr, _) in files(r) {
                                if let (Some(s), Some(u)) = (stamp(&usr), r.unlocks_file(&usr)) {
                                    seen.insert(usr, Seen { stamp: s, unlocks: u });
                                }
                            }
                        }
                    }
                    list = fresh;
                    listed_at = Some(Instant::now());
                }
                for root in &list {
                    for (_conf, usr, id) in files(root) {
                        let Some(s) = stamp(&usr) else { continue };
                        if seen.get(&usr).is_some_and(|x| x.stamp == s) {
                            continue;
                        }
                        let Some(now) = root.unlocks_file(&usr) else { continue };
                        let before = seen.remove(&usr);
                        if !first {
                            on_change(&app, root, &id, before.as_ref().map(|b| &b.unlocks), &now);
                        }
                        seen.insert(usr, Seen { stamp: s, unlocks: now });
                    }
                }
                first = false;
                std::thread::sleep(TICK);
            }
        })
        .ok();
}

/// Troféus novos desde `before` (arquivo novo: todos os obtidos), só os recentes.
pub fn fresh_ids(before: Option<&super::parse::Unlocks>, now: &super::parse::Unlocks, now_ts: i64) -> Vec<u32> {
    now.iter()
        .filter(|(id, _)| before.map_or(true, |b| !b.contains_key(id)))
        .filter(|(_, ts)| ts.map_or(true, |t| t >= now_ts - FRESH_SECS))
        .map(|(id, _)| *id)
        .collect()
}

fn on_change(app: &AppHandle, root: &Root, comm_id: &str, before: Option<&super::parse::Unlocks>, now: &super::parse::Unlocks) {
    remember_link(app, root, comm_id);
    let fresh = fresh_ids(before, now, now_epoch());
    if fresh.is_empty() {
        return;
    }
    if let Some((conf, trophies, _)) = build(app, root, comm_id) {
        let game = if conf.title.is_empty() { root.emu.label().to_string() } else { conf.title.clone() };
        let mut items: Vec<_> = trophies.into_iter().filter(|t| fresh.contains(&t.id)).collect();
        // platina por último (é a conclusão das outras)
        items.sort_by_key(|t| (t.grade == "P", t.id));
        let extra = items.len().saturating_sub(5);
        let toasts = items
            .into_iter()
            .take(5)
            .map(|t| crate::shot_toast::AchievementToast {
                source: root.emu.label().to_lowercase(),
                icon: t.icon,
                title: if t.name.is_empty() { format!("#{}", t.id) } else { t.name },
                game: game.clone(),
                points: 0,
                hardcore: false,
                rare: t.grade == "P" || t.grade == "G",
                global_pct: None,
                more: 0,
                grade: Some(t.grade),
            })
            .collect();
        crate::shot_toast::notify_achievements(app, toasts, extra);
    }
    let _ = app.emit("trophies-changed", serde_json::json!({ "commId": comm_id }));
}

/// O conjunto mudou com um jogo deste emulador aberto pelo NEXSTAGE e sem conjunto conhecido: vincula.
fn remember_link(app: &AppHandle, root: &Root, comm_id: &str) {
    let running: Vec<i64> = app.state::<Runtime>().running.lock().map(|m| m.keys().copied().collect()).unwrap_or_default();
    if running.is_empty() {
        return;
    }
    let db = app.state::<Db>();
    let Ok(conn) = db.conn() else { return };
    let candidates: Vec<i64> = running
        .into_iter()
        .filter(|g| matches!(resolve(&conn, *g), Ok(Some(r)) if r.root.emu == root.emu && r.comm_id.is_none()))
        .collect();
    if let [g] = candidates[..] {
        let _ = learn(&conn, g, comm_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::parse::Unlocks;

    #[test]
    fn only_new_and_recent_unlocks() {
        let now = 1_800_000_000;
        let before: Unlocks = [(1, Some(now - 100))].into_iter().collect();
        let after: Unlocks = [(1, Some(now - 100)), (2, Some(now - 5)), (3, Some(now - 3600)), (4, None)].into_iter().collect();
        assert_eq!(fresh_ids(Some(&before), &after, now), vec![2, 4]);
        // arquivo novo: todos os recentes
        assert_eq!(fresh_ids(None, &after, now), vec![1, 2, 4]);
    }
}
