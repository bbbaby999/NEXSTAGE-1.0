//! Estatísticas gamer consolidadas por perfil — UMA fonte de verdade para Home, Perfil e página
//! Estatísticas. Tudo vem do que já está no banco local (nenhuma chamada de rede aqui):
//! - NEXSTAGE (local): `profile_games`, `play_sessions`;
//! - Steam: cache `steam_owned` / `steam_ach_*` (sincronizado por `steam::sync`);
//! - RetroAchievements: cache `ra_progress` / `ra_unlocks` / conta (sincronizado por `retro::sync`).
//!
//! Regras de precisão:
//! - nada é estimado: campo sem dado vem `None` e a interface diz como obter o dado;
//! - tempo de jogo: quando o mesmo jogo da biblioteca está vinculado a mais de uma origem (vínculo
//!   confirmado: `steam_links`, `ra_links`), usa-se o MAIOR valor, nunca a soma;
//! - conquistas: cada plataforma fica separada (identidade = plataforma + id). Steam e RetroAchievements
//!   são conjuntos distintos (uma conquista de um nunca é a mesma do outro), então o total consolidado
//!   é a soma de conjuntos disjuntos, sem duplicação dentro de cada plataforma.
//!
//! Plataformas futuras (Epic Games, Xbox...): escreva um `collect_<plataforma>(conn, perfil)` que leia o
//! cache local dela e devolva `Collected` (jogos com tempo/conquistas, desbloqueios e o vínculo confirmado
//! com a biblioteca), e acrescente-o em `PROVIDERS`. Consolidação, rankings, totais por plataforma,
//! histórico e privacidade são genéricos e não mudam (ver o teste `new_platform_plugs_in`). Na interface,
//! acrescente o rótulo e uma cor validada em `lib/gamerStats.ts`.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::retro::util::{fmt_epoch, now_epoch, parse_epoch};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use tauri::State;

// ---------------------------------------------------------------- modelo

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// "local" | "steam" | "retroachievements"
    pub id: String,
    pub connected: bool,
    pub synced_at: Option<String>,
    /// A origem fornece tempo de jogo (Steam sem chave, por exemplo, não fornece).
    pub has_playtime: bool,
    pub has_achievements: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AchProgress {
    pub source: String,
    pub unlocked: i64,
    pub total: i64,
    pub pct: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameStat {
    pub key: String,
    pub title: String,
    pub cover: Option<String>,
    /// Reserva quando a capa não carrega (Steam: ícone do jogo — jogos novos não têm capa no endereço fixo).
    pub icon: Option<String>,
    pub platform: String,
    pub library_game_id: Option<i64>,
    /// Tempo consolidado (maior valor entre as origens vinculadas).
    pub total_seconds: i64,
    pub local_seconds: Option<i64>,
    pub steam_seconds: Option<i64>,
    pub recent_seconds: i64,
    pub last_activity: Option<String>,
    pub sources: Vec<String>,
    pub achievements: Vec<AchProgress>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnlockRow {
    pub source: String,
    pub name: String,
    pub description: String,
    pub game: String,
    pub icon: Option<String>,
    pub at: Option<String>,
    /// % global de jogadores (só quando a fonte informa: Steam).
    pub rarity_pct: Option<f64>,
    pub hardcore: bool,
    pub points: Option<i64>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAch {
    pub source: String,
    pub unlocked: i64,
    pub total: i64,
    pub games: i64,
    pub games_complete: i64,
    pub pct: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RaSummary {
    pub unlocked: i64,
    pub unlocked_hc: i64,
    pub total: i64,
    pub games: i64,
    pub mastered: i64,
    pub points: Option<i64>,
    pub points_softcore: Option<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameAch {
    pub source: String,
    pub title: String,
    pub icon: Option<String>,
    pub unlocked: i64,
    pub total: i64,
    pub pct: f64,
    pub library_game_id: Option<i64>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Achievements {
    /// Totais separados por plataforma (uma linha por origem com conquistas).
    pub platforms: Vec<PlatformAch>,
    /// Extras próprios do RetroAchievements (Hardcore, pontos, maestria).
    pub ra: Option<RaSummary>,
    pub unlocked: i64,
    pub total: i64,
    pub pct: f64,
    pub completed: Vec<GameAch>,
    pub closest: Vec<GameAch>,
    pub recent: Vec<UnlockRow>,
    pub rare: Vec<UnlockRow>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MonthRow {
    /// "AAAA-MM"
    pub month: String,
    pub local_seconds: i64,
    /// Desbloqueios no mês por plataforma ("steam", "retroachievements", ...).
    pub unlocks: BTreeMap<String, i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SteamDetail {
    pub total_seconds: i64,
    pub recent_seconds: i64,
    pub owned: i64,
    pub played: i64,
    pub ranking: Vec<GameStat>,
    pub recent: Vec<GameStat>,
    pub ach_games: Vec<GameAch>,
    pub ach_unlocked: i64,
    pub ach_total: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlatformTime {
    pub platform: String,
    pub seconds: i64,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GamerStats {
    pub generated_at: String,
    pub sources: Vec<SourceInfo>,
    pub library_count: i64,
    pub total_seconds: i64,
    pub local_seconds: i64,
    pub steam_seconds: Option<i64>,
    pub recent_seconds: i64,
    pub favorite: Option<GameStat>,
    pub most_played_recent: Option<GameStat>,
    pub last_game: Option<GameStat>,
    pub recent_games: Vec<GameStat>,
    pub top_games: Vec<GameStat>,
    pub favorite_platform: Option<PlatformTime>,
    pub platforms: Vec<PlatformTime>,
    pub achievements: Achievements,
    pub history: Vec<MonthRow>,
    pub steam: Option<SteamDetail>,
}

// ---------------------------------------------------------------- coleta por origem

pub struct SrcGame {
    source: &'static str,
    key: String,
    title: String,
    cover: Option<String>,
    icon: Option<String>,
    platform: String,
    library_game_id: Option<i64>,
    seconds: Option<i64>,
    recent_seconds: i64,
    last_activity: Option<i64>,
    ach: Option<(i64, i64)>,
}

pub struct Collected {
    info: SourceInfo,
    games: Vec<SrcGame>,
    unlocks: Vec<(i64, UnlockRow)>,
    /// Dados extras da conta (ex.: pontos do RetroAchievements), sem segredos.
    extra: Option<serde_json::Value>,
}

/// Uma origem de estatísticas: lê SÓ o cache local e devolve `None` se a conta não estiver conectada.
pub type Collector = fn(&Connection, i64) -> AppResult<Option<Collected>>;

/// Registro das origens, na ordem de exibição. Epic Games, Xbox etc. entram aqui.
pub const PROVIDERS: &[Collector] = &[collect_local, collect_steam, collect_ra];

fn pct(a: i64, b: i64) -> f64 {
    if b <= 0 { 0.0 } else { ((a as f64 / b as f64) * 1000.0).round() / 10.0 }
}

fn collect_local(conn: &Connection, profile_id: i64) -> AppResult<Option<Collected>> {
    let since = fmt_epoch(now_epoch() - 14 * 86400);
    let mut st = conn.prepare(
        "SELECT g.id, g.title, g.cover, g.platform, pg.playtime_seconds, pg.last_played_at,
                (SELECT COALESCE(SUM(s.duration_seconds),0) FROM play_sessions s WHERE s.profile_id=pg.profile_id AND s.game_id=g.id AND s.started_at>=?2)
         FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1",
    )?;
    let games = st
        .query_map(params![profile_id, since], |r| {
            let id: i64 = r.get(0)?;
            Ok(SrcGame {
                source: "local",
                key: format!("lib:{id}"),
                title: r.get(1)?,
                cover: crate::images::resolve(r.get(2)?),
                icon: None,
                platform: r.get(3)?,
                library_game_id: Some(id),
                seconds: Some(r.get(4)?),
                last_activity: r.get::<_, Option<String>>(5)?.as_deref().and_then(parse_epoch),
                recent_seconds: r.get(6)?,
                ach: None,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Collected { info: SourceInfo { id: "local".into(), connected: true, synced_at: None, has_playtime: true, has_achievements: false }, games, unlocks: vec![], extra: None }))
}

fn collect_steam(conn: &Connection, profile_id: i64) -> AppResult<Option<Collected>> {
    let Some(acc) = crate::accounts::get(conn, profile_id, crate::steam::store::PLATFORM)? else { return Ok(None) };
    // vínculos confirmados AppID → jogo da biblioteca deste perfil
    let mut links: HashMap<i64, i64> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT COALESCE(sl.appid, CASE WHEN g.source='steam' THEN CAST(g.external_id AS INTEGER) END), g.id
             FROM profile_games pg JOIN games g ON g.id=pg.game_id LEFT JOIN steam_links sl ON sl.game_id=g.id
             WHERE pg.profile_id=?1",
        )?;
        for row in st.query_map([profile_id], |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?)))? {
            if let (Some(a), id) = row? {
                links.insert(a, id);
            }
        }
    }
    let mut ach: HashMap<i64, (i64, i64)> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT s.appid, COUNT(*), SUM(CASE WHEN u.unlocked_at IS NOT NULL THEN 1 ELSE 0 END)
             FROM steam_ach_schema s LEFT JOIN steam_ach_user u ON u.appid=s.appid AND u.apiname=s.apiname AND u.profile_id=?1
             WHERE s.appid IN (SELECT appid FROM steam_stats WHERE profile_id=?1 AND (error IS NULL OR error<>'private'))
             GROUP BY s.appid",
        )?;
        for row in st.query_map([profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
            let (a, total, done) = row?;
            ach.insert(a, (done, total));
        }
    }
    let mut has_time = false;
    let mut st = conn.prepare(
        "SELECT appid, name, playtime_forever, playtime_2weeks, last_played, source, icon_hash FROM steam_owned WHERE profile_id=?1",
    )?;
    let games = st
        .query_map([profile_id], |r| {
            let appid: i64 = r.get(0)?;
            let api: bool = r.get::<_, String>(5)? == "api";
            let pf: Option<i64> = r.get(2)?;
            let icon = r.get::<_, Option<String>>(6)?.map(|h| crate::steam::store::app_icon_url(appid, &h));
            Ok((appid, r.get::<_, String>(1)?, if api { pf.map(|m| m * 60) } else { None }, r.get::<_, Option<i64>>(3)?.unwrap_or(0) * 60, r.get::<_, Option<i64>>(4)?, icon))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(appid, name, secs, recent, last, icon)| {
            has_time |= secs.is_some();
            SrcGame {
                source: "steam",
                key: format!("steam:{appid}"),
                title: name,
                cover: Some(crate::steam::store::cover_url(appid)),
                icon,
                platform: "PC".into(),
                library_game_id: links.get(&appid).copied(),
                seconds: secs,
                recent_seconds: recent,
                last_activity: last,
                ach: ach.get(&appid).copied(),
            }
        })
        .collect();
    let mut st = conn.prepare(
        "SELECT u.unlocked_at, s.name, s.description, COALESCE(o.name, 'App ' || s.appid), s.icon, s.global_pct
         FROM steam_ach_user u JOIN steam_ach_schema s ON s.appid=u.appid AND s.apiname=u.apiname
         LEFT JOIN steam_owned o ON o.profile_id=u.profile_id AND o.appid=u.appid
         WHERE u.profile_id=?1 AND u.unlocked_at > 1",
    )?;
    let unlocks = st
        .query_map([profile_id], |r| {
            let at: i64 = r.get(0)?;
            Ok((at, UnlockRow {
                source: "steam".into(), name: r.get(1)?, description: r.get(2)?, game: r.get(3)?, icon: r.get(4)?,
                at: Some(fmt_epoch(at)), rarity_pct: r.get(5)?, hardcore: false, points: None,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(Collected {
        info: SourceInfo { id: "steam".into(), connected: true, synced_at: acc.synced_at, has_playtime: has_time, has_achievements: !ach.is_empty() },
        games,
        unlocks,
        extra: None,
    }))
}

fn collect_ra(conn: &Connection, profile_id: i64) -> AppResult<Option<Collected>> {
    let Some(acc) = crate::accounts::get(conn, profile_id, crate::retro::store::PLATFORM)? else { return Ok(None) };
    let mut st = conn.prepare(
        "SELECT g.ra_game_id, g.title, g.image_icon, p.awarded, COALESCE(NULLIF(p.max_possible,0), g.num_achievements), p.last_award_at,
                (SELECT l.game_id FROM ra_links l JOIN profile_games pg ON pg.game_id=l.game_id AND pg.profile_id=?1
                  WHERE l.ra_game_id=g.ra_game_id AND l.status IN ('hash','manual') LIMIT 1),
                g.console_name
         FROM ra_progress p JOIN ra_games g ON g.ra_game_id=p.ra_game_id WHERE p.profile_id=?1",
    )?;
    let games = st
        .query_map([profile_id], |r| {
            let id: i64 = r.get(0)?;
            Ok(SrcGame {
                source: "retroachievements",
                key: format!("ra:{id}"),
                title: r.get(1)?,
                cover: crate::retro::store::icon_url(&r.get(2)?),
                icon: None,
                platform: r.get::<_, String>(7)?,
                library_game_id: r.get(6)?,
                seconds: None, // o cache do RA não guarda tempo de jogo: nada é estimado
                recent_seconds: 0,
                last_activity: r.get::<_, Option<String>>(5)?.as_deref().and_then(parse_epoch),
                ach: Some((r.get(3)?, r.get(4)?)),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut st = conn.prepare(
        "SELECT COALESCE(u.unlocked_hc_at, u.unlocked_at), a.title, a.description, COALESCE(g.title,''), a.badge, u.unlocked_hc_at IS NOT NULL, a.points
         FROM ra_unlocks u JOIN ra_achievements a ON a.ra_achievement_id=u.ra_achievement_id LEFT JOIN ra_games g ON g.ra_game_id=u.ra_game_id
         WHERE u.profile_id=?1",
    )?;
    let unlocks = st
        .query_map([profile_id], |r| {
            let at: Option<String> = r.get(0)?;
            Ok((at.as_deref().and_then(parse_epoch).unwrap_or(0), UnlockRow {
                source: "retroachievements".into(), name: r.get(1)?, description: r.get(2)?, game: r.get(3)?,
                icon: crate::retro::store::badge_url(&r.get(4)?, false), at, rarity_pct: None, hardcore: r.get(5)?, points: Some(r.get(6)?),
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let data = acc.data.as_deref().and_then(|d| serde_json::from_str(d).ok());
    Ok(Some(Collected { info: SourceInfo { id: "retroachievements".into(), connected: true, synced_at: acc.synced_at, has_playtime: false, has_achievements: true }, games, unlocks, extra: data }))
}

// ---------------------------------------------------------------- consolidação

fn to_stat(group: &[&SrcGame]) -> GameStat {
    // a entrada da biblioteca (local) dá título/capa/plataforma; senão, a primeira origem
    let main = group.iter().find(|g| g.source == "local").unwrap_or(&group[0]);
    let local = group.iter().find(|g| g.source == "local").and_then(|g| g.seconds);
    let steam = group.iter().find(|g| g.source == "steam").and_then(|g| g.seconds);
    GameStat {
        key: main.key.clone(),
        title: main.title.clone(),
        cover: main.cover.clone().or_else(|| group.iter().find_map(|g| g.cover.clone())),
        icon: group.iter().find_map(|g| g.icon.clone()),
        platform: main.platform.clone(),
        library_game_id: group.iter().find_map(|g| g.library_game_id),
        // mesmo jogo em várias origens: o MAIOR tempo (as sessões podem se sobrepor), nunca a soma
        total_seconds: group.iter().filter_map(|g| g.seconds).max().unwrap_or(0),
        local_seconds: local,
        steam_seconds: steam,
        recent_seconds: group.iter().map(|g| g.recent_seconds).max().unwrap_or(0),
        last_activity: group.iter().filter_map(|g| g.last_activity).max().map(fmt_epoch),
        sources: group.iter().map(|g| g.source.to_string()).collect(),
        achievements: group.iter().filter_map(|g| g.ach.map(|(u, t)| AchProgress { source: g.source.into(), unlocked: u, total: t, pct: pct(u, t) })).filter(|a| a.total > 0).collect(),
    }
}

fn month_row(map: &mut HashMap<String, MonthRow>, m: String) -> &mut MonthRow {
    map.entry(m.clone()).or_insert(MonthRow { month: m, local_seconds: 0, unlocks: BTreeMap::new() })
}

fn month_of(epoch: i64) -> String {
    fmt_epoch(epoch)[..7].to_string()
}

pub fn build(conn: &Connection, profile_id: i64) -> AppResult<GamerStats> {
    build_with(conn, profile_id, PROVIDERS)
}

/// Consolidação genérica sobre qualquer conjunto de origens.
pub fn build_with(conn: &Connection, profile_id: i64, providers: &[Collector]) -> AppResult<GamerStats> {
    let mut cols: Vec<Collected> = Vec::new();
    for p in providers {
        if let Some(c) = p(conn, profile_id)? {
            cols.push(c);
        }
    }
    let local = cols.iter().find(|c| c.info.id == "local");
    let steam = cols.iter().find(|c| c.info.id == "steam");
    let ra = cols.iter().find(|c| c.info.id == "retroachievements");

    let mut out = GamerStats { generated_at: fmt_epoch(now_epoch()), library_count: local.map(|l| l.games.len() as i64).unwrap_or(0), ..Default::default() };
    out.sources = cols.iter().map(|c| c.info.clone()).collect();

    // agrupa: vínculo confirmado com a biblioteca = mesmo jogo; sem vínculo, cada origem fica separada
    let all: Vec<&SrcGame> = cols.iter().flat_map(|c| c.games.iter()).collect();
    let mut groups: Vec<Vec<&SrcGame>> = Vec::new();
    let mut by_lib: HashMap<i64, usize> = HashMap::new();
    for g in all {
        match g.library_game_id {
            Some(id) => match by_lib.get(&id) {
                Some(&i) => groups[i].push(g),
                None => {
                    by_lib.insert(id, groups.len());
                    groups.push(vec![g]);
                }
            },
            None => groups.push(vec![g]),
        }
    }
    let stats: Vec<GameStat> = groups.iter().map(|g| to_stat(g)).collect();

    out.local_seconds = local.map(|l| l.games.iter().filter_map(|g| g.seconds).sum()).unwrap_or(0);
    out.steam_seconds = steam.filter(|s| s.info.has_playtime).map(|s| s.games.iter().filter_map(|g| g.seconds).sum());
    out.total_seconds = stats.iter().map(|s| s.total_seconds).sum();
    out.recent_seconds = stats.iter().map(|s| s.recent_seconds).sum();

    let mut ranked: Vec<&GameStat> = stats.iter().filter(|s| s.total_seconds > 0).collect();
    ranked.sort_by(|a, b| b.total_seconds.cmp(&a.total_seconds).then(a.title.cmp(&b.title)));
    out.top_games = ranked.iter().take(10).map(|s| (*s).clone()).collect();
    out.favorite = out.top_games.first().cloned();
    out.most_played_recent = stats.iter().filter(|s| s.recent_seconds > 0).max_by_key(|s| s.recent_seconds).cloned();
    let mut by_recent: Vec<&GameStat> = stats.iter().filter(|s| s.last_activity.is_some()).collect();
    by_recent.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    out.recent_games = by_recent.iter().take(5).map(|s| (*s).clone()).collect();
    out.last_game = out.recent_games.first().cloned();

    // plataforma favorita: só com dados suficientes (≥ 1 h no total e ao menos 2 jogos com tempo)
    let mut plat: HashMap<String, i64> = HashMap::new();
    for s in stats.iter().filter(|s| s.total_seconds > 0) {
        *plat.entry(s.platform.clone()).or_default() += s.total_seconds;
    }
    let mut platforms: Vec<PlatformTime> = plat.into_iter().map(|(platform, seconds)| PlatformTime { platform, seconds }).collect();
    platforms.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.platform.cmp(&b.platform)));
    if out.total_seconds >= 3600 && ranked.len() >= 2 {
        out.favorite_platform = platforms.first().cloned();
    }
    out.platforms = platforms;

    // conquistas: uma linha por plataforma (conjuntos distintos; identidade = plataforma + id)
    let game_achs = |c: &Collected| -> Vec<GameAch> {
        c.games
            .iter()
            .filter_map(|g| {
                g.ach.filter(|(_, t)| *t > 0).map(|(u, t)| GameAch {
                    source: c.info.id.clone(), title: g.title.clone(), icon: g.cover.clone(), unlocked: u, total: t, pct: pct(u, t), library_game_id: g.library_game_id,
                })
            })
            .collect()
    };
    let mut a = Achievements::default();
    let mut every: Vec<GameAch> = Vec::new();
    for c in cols.iter().filter(|c| c.info.has_achievements) {
        let ga = game_achs(c);
        let (u, t): (i64, i64) = (ga.iter().map(|g| g.unlocked).sum(), ga.iter().map(|g| g.total).sum());
        a.platforms.push(PlatformAch {
            source: c.info.id.clone(), unlocked: u, total: t, games: ga.len() as i64,
            games_complete: ga.iter().filter(|g| g.unlocked >= g.total).count() as i64, pct: pct(u, t),
        });
        every.extend(ga);
    }
    if let Some(r) = ra {
        let p = a.platforms.iter().find(|p| p.source == "retroachievements").cloned().unwrap_or_default();
        let hc: i64 = conn.query_row("SELECT COALESCE(SUM(awarded_hc),0) FROM ra_progress WHERE profile_id=?1", [profile_id], |x| x.get(0))?;
        let mastered: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ra_progress WHERE profile_id=?1 AND (award_kind IN ('mastered','completed') OR (max_possible>0 AND awarded>=max_possible))",
            [profile_id],
            |x| x.get(0),
        )?;
        let pts = |k: &str| r.extra.as_ref().and_then(|d| d[k].as_i64());
        a.ra = Some(RaSummary {
            unlocked: p.unlocked, unlocked_hc: hc, total: p.total, games: p.games, mastered,
            points: pts("totalPoints"), points_softcore: pts("totalSoftcorePoints"),
        });
    }
    a.unlocked = every.iter().map(|g| g.unlocked).sum();
    a.total = every.iter().map(|g| g.total).sum();
    a.pct = pct(a.unlocked, a.total);
    let mut done: Vec<GameAch> = every.iter().filter(|g| g.unlocked >= g.total).cloned().collect();
    done.sort_by(|x, y| y.total.cmp(&x.total));
    a.completed = done;
    let mut close: Vec<GameAch> = every.iter().filter(|g| g.unlocked > 0 && g.unlocked < g.total).cloned().collect();
    close.sort_by(|x, y| y.pct.partial_cmp(&x.pct).unwrap_or(std::cmp::Ordering::Equal));
    a.closest = close.into_iter().take(8).collect();
    let mut unl: Vec<(i64, UnlockRow)> = cols.iter().flat_map(|c| c.unlocks.iter().cloned()).collect();
    unl.sort_by(|x, y| y.0.cmp(&x.0));
    a.recent = unl.iter().take(16).map(|u| u.1.clone()).collect();
    let mut rare: Vec<UnlockRow> = unl.iter().filter_map(|u| u.1.rarity_pct.map(|_| u.1.clone())).collect();
    rare.sort_by(|x, y| x.rarity_pct.partial_cmp(&y.rarity_pct).unwrap_or(std::cmp::Ordering::Equal));
    a.rare = rare.into_iter().take(8).collect();

    // histórico mensal (24 meses): tempo local por sessão + desbloqueios por mês e plataforma.
    // A Steam só informa o total acumulado (não por mês): não entra na série, para não inventar.
    let mut months: HashMap<String, MonthRow> = HashMap::new();
    {
        let mut st = conn.prepare(
            "SELECT strftime('%Y-%m', started_at), SUM(duration_seconds) FROM play_sessions
             WHERE profile_id=?1 AND started_at >= datetime('now','-24 months') GROUP BY 1",
        )?;
        for r in st.query_map([profile_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (m, s) = r?;
            month_row(&mut months, m).local_seconds = s;
        }
    }
    let cutoff = now_epoch() - 24 * 31 * 86400;
    for (at, u) in &unl {
        if *at > cutoff {
            *month_row(&mut months, month_of(*at)).unlocks.entry(u.source.clone()).or_default() += 1;
        }
    }
    let mut hist: Vec<MonthRow> = months.into_values().collect();
    hist.sort_by(|x, y| x.month.cmp(&y.month));
    out.history = hist;

    // detalhes da Steam (só o que a API forneceu)
    if let Some(s) = steam {
        let mut sg: Vec<GameStat> = s.games.iter().map(|g| to_stat(&[g])).collect();
        let played = sg.iter().filter(|g| g.total_seconds > 0).count() as i64;
        let mut recent: Vec<GameStat> = sg.iter().filter(|g| g.recent_seconds > 0).cloned().collect();
        recent.sort_by(|x, y| y.recent_seconds.cmp(&x.recent_seconds));
        sg.sort_by(|x, y| y.total_seconds.cmp(&x.total_seconds));
        let mut ach_games: Vec<GameAch> = game_achs(s);
        ach_games.sort_by(|x, y| y.pct.partial_cmp(&x.pct).unwrap_or(std::cmp::Ordering::Equal));
        out.steam = Some(SteamDetail {
            total_seconds: out.steam_seconds.unwrap_or(0),
            recent_seconds: s.games.iter().map(|g| g.recent_seconds).sum(),
            owned: s.games.len() as i64,
            played,
            ranking: if s.info.has_playtime { sg.into_iter().filter(|g| g.total_seconds > 0).take(10).collect() } else { vec![] },
            recent,
            ach_unlocked: ach_games.iter().map(|g| g.unlocked).sum(),
            ach_total: ach_games.iter().map(|g| g.total).sum(),
            ach_games,
        });
    }
    out.achievements = a;
    Ok(out)
}

// ---------------------------------------------------------------- privacidade

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Visibility {
    /// "private" (padrão) | "public" | "custom"
    pub mode: String,
    #[serde(default)]
    pub summary: bool,
    #[serde(default)]
    pub top_games: bool,
    #[serde(default)]
    pub achievements: bool,
    #[serde(default)]
    pub history: bool,
    #[serde(default)]
    pub steam: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self { mode: "private".into(), summary: false, top_games: false, achievements: false, history: false, steam: false }
    }
}

pub fn visibility(conn: &Connection, profile_id: i64) -> AppResult<Visibility> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM profile_settings WHERE profile_id=?1 AND key='stats_visibility'", [profile_id], |r| r.get(0))
        .optional()?;
    let v: Visibility = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
    Ok(if ["private", "public", "custom"].contains(&v.mode.as_str()) { v } else { Visibility::default() })
}

/// Versão pública: só as categorias liberadas. Nunca contém chaves, SteamID, tokens ou caminhos
/// (o `GamerStats` não carrega nada disso); o modo privado não devolve nada.
pub fn filter_public(s: GamerStats, v: &Visibility) -> Option<GamerStats> {
    let (summary, top, ach, hist, steam) = match v.mode.as_str() {
        "public" => (true, true, true, true, true),
        "custom" => (v.summary, v.top_games, v.achievements, v.history, v.steam),
        _ => return None,
    };
    let mut p = GamerStats { generated_at: s.generated_at, ..Default::default() };
    if summary {
        p.library_count = s.library_count;
        p.total_seconds = s.total_seconds;
        p.local_seconds = s.local_seconds;
        p.steam_seconds = s.steam_seconds;
        p.recent_seconds = s.recent_seconds;
        p.favorite = s.favorite;
        p.most_played_recent = s.most_played_recent;
        p.last_game = s.last_game;
        p.recent_games = s.recent_games;
        p.favorite_platform = s.favorite_platform;
        p.platforms = s.platforms;
    }
    if top {
        p.top_games = s.top_games;
    }
    if ach {
        p.achievements = s.achievements;
    }
    if hist {
        p.history = s.history;
    }
    if steam {
        p.steam = s.steam;
    }
    // sem datas de sincronização nem estado das contas na versão pública
    p.sources = s.sources.into_iter().map(|mut x| { x.synced_at = None; x }).collect();
    // capas locais (caminhos de arquivo) não saem do perfil
    let strip = |g: &mut GameStat| if g.cover.as_deref().map(|c| !c.starts_with("https://")).unwrap_or(false) { g.cover = None };
    p.top_games.iter_mut().for_each(strip);
    p.recent_games.iter_mut().for_each(strip);
    for g in [&mut p.favorite, &mut p.most_played_recent, &mut p.last_game].into_iter().flatten() {
        strip(g);
    }
    Some(p)
}

// ---------------------------------------------------------------- comandos

#[tauri::command(async)]
pub fn gamer_stats(db: State<Db>, profile_id: i64) -> AppResult<GamerStats> {
    build(&*db.conn()?, profile_id)
}

#[tauri::command]
pub fn get_stats_visibility(db: State<Db>, profile_id: i64) -> AppResult<Visibility> {
    visibility(&*db.conn()?, profile_id)
}

#[tauri::command]
pub fn set_stats_visibility(db: State<Db>, profile_id: i64, visibility: Visibility) -> AppResult<Visibility> {
    if !["private", "public", "custom"].contains(&visibility.mode.as_str()) {
        return Err(AppError::Validation("Modo de visibilidade inválido.".into()));
    }
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO profile_settings(profile_id,key,value) VALUES(?1,'stats_visibility',?2)
         ON CONFLICT(profile_id,key) DO UPDATE SET value=excluded.value",
        params![profile_id, serde_json::to_string(&visibility).unwrap_or_default()],
    )?;
    Ok(visibility)
}

/// O que OUTROS perfis podem ver das estatísticas deste perfil (respeita a visibilidade).
#[tauri::command(async)]
pub fn public_gamer_stats(db: State<Db>, profile_id: i64) -> AppResult<Option<GamerStats>> {
    let conn = db.conn()?;
    let v = visibility(&conn, profile_id)?;
    if v.mode == "private" {
        return Ok(None);
    }
    Ok(filter_public(build(&conn, profile_id)?, &v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let p = std::env::temp_dir().join(format!("nexstage-stats-{}-{}.db", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_file(&p);
        let d = crate::db::Db::open(&p).unwrap();
        drop(d);
        let c = Connection::open(&p).unwrap();
        c.execute_batch(
            "INSERT INTO profiles(id, username, display_name) VALUES(1,'a','A'),(2,'b','B');
             INSERT INTO games(id, title, platform, executable_path) VALUES(1,'Portal 2','PC','x'),(2,'Ico','Playstation 2','y'),(3,'Hades','PC','z');
             INSERT INTO profile_games(profile_id, game_id, playtime_seconds, last_played_at) VALUES
               (1,1,7200,'2026-01-10 10:00:00'),(1,2,1800,'2026-02-01 10:00:00'),(1,3,0,NULL);
             INSERT INTO play_sessions(profile_id, game_id, started_at, ended_at, duration_seconds) VALUES(1,1,'2026-01-10 08:00:00','2026-01-10 10:00:00',7200);",
        )
        .unwrap();
        c
    }
    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn local_only_has_no_invented_data() {
        let c = db();
        let s = build(&c, 1).unwrap();
        assert_eq!((s.library_count, s.total_seconds, s.local_seconds), (3, 9000, 9000));
        assert_eq!(s.steam_seconds, None, "sem Steam conectada: sem tempo da Steam");
        assert!(s.achievements.platforms.is_empty() && s.achievements.ra.is_none());
        assert_eq!(s.favorite.as_ref().unwrap().title, "Portal 2");
        assert_eq!(s.last_game.as_ref().unwrap().title, "Ico");
        assert_eq!(s.favorite_platform.as_ref().unwrap().platform, "PC");
        assert_eq!(s.history.len(), 1);
        assert!(build(&c, 2).unwrap().favorite.is_none(), "outro perfil não vê os dados");
    }

    #[test]
    fn linked_sources_never_double_count() {
        let c = db();
        crate::accounts::save(&c, 1, "steam", "Eu", Some("76561197960287930"), None, "{}").unwrap();
        c.execute_batch(
            "INSERT INTO steam_owned(profile_id, appid, name, playtime_forever, playtime_2weeks, source) VALUES
               (1, 620, 'Portal 2', 300, 30, 'api'), (1, 440, 'Team Fortress 2', 60, 0, 'api');
             INSERT INTO steam_links(game_id, appid) VALUES(1, 620);
             INSERT INTO steam_ach_schema(appid, apiname, name, global_pct) VALUES(620,'A','A',3.5),(620,'B','B',50.0);
             INSERT INTO steam_ach_user(profile_id, appid, apiname, unlocked_at) VALUES(1,620,'A',1767000000),(1,620,'B',NULL);
             INSERT INTO steam_stats(profile_id, appid) VALUES(1,620);",
        )
        .unwrap();
        let s = build(&c, 1).unwrap();
        // Portal 2: local 2 h vs Steam 5 h → 5 h (máximo), não 7 h; TF2 (fora da biblioteca) 1 h; Ico 0,5 h
        assert_eq!(s.total_seconds, 18000 + 3600 + 1800);
        let p2 = s.top_games.iter().find(|g| g.title == "Portal 2").unwrap();
        assert_eq!((p2.local_seconds, p2.steam_seconds, p2.total_seconds), (Some(7200), Some(18000), 18000));
        assert_eq!(p2.achievements.len(), 1);
        assert_eq!(s.achievements.platforms[0].unlocked, 1);
        assert_eq!(s.achievements.rare.len(), 1);
        assert_eq!(s.steam.as_ref().unwrap().ranking[0].title, "Portal 2");
        assert_eq!(s.most_played_recent.as_ref().unwrap().title, "Portal 2");
    }

    #[test]
    fn privacy_filter() {
        let c = db();
        let s = build(&c, 1).unwrap();
        assert!(filter_public(s.clone(), &Visibility::default()).is_none(), "privado não expõe nada");
        let v = Visibility { mode: "custom".into(), summary: true, top_games: false, achievements: false, history: false, steam: false };
        let p = filter_public(s.clone(), &v).unwrap();
        assert_eq!(p.total_seconds, 9000);
        assert!(p.top_games.is_empty() && p.history.is_empty());
        let all = filter_public(s, &Visibility { mode: "public".into(), ..Visibility::default() }).unwrap();
        assert!(!all.top_games.is_empty());
        assert!(all.sources.iter().all(|x| x.synced_at.is_none()));
    }
fn with_ra(c: &Connection) {
        crate::accounts::save(c, 1, "retroachievements", "Jogador", None, None, r#"{"totalPoints":120,"totalSoftcorePoints":30}"#).unwrap();
        c.execute_batch(
            "INSERT INTO ra_games(ra_game_id, title, console_id, console_name, num_achievements) VALUES(10, 'Ico (RA)', 21, 'PlayStation 2', 4);
             INSERT INTO ra_progress(profile_id, ra_game_id, awarded, awarded_hc, max_possible, last_award_at) VALUES(1, 10, 3, 2, 4, '2026-03-01 10:00:00');
             INSERT INTO ra_achievements(ra_achievement_id, ra_game_id, title, points) VALUES(1,10,'A',5),(2,10,'B',10),(3,10,'C',25),(4,10,'D',50);
             INSERT INTO ra_unlocks(profile_id, ra_achievement_id, ra_game_id, unlocked_at, unlocked_hc_at) VALUES
               (1,1,10,'2026-03-01 10:00:00','2026-03-01 10:00:00'),(1,2,10,'2026-03-01 11:00:00','2026-03-01 11:00:00'),(1,3,10,'2026-03-02 11:00:00',NULL);
             INSERT INTO ra_links(game_id, ra_game_id, status) VALUES(2, 10, 'hash');",
        )
        .unwrap();
    }

    #[test]
    fn retroachievements_only() {
        let c = db();
        with_ra(&c);
        let s = build(&c, 1).unwrap();
        assert_eq!(s.achievements.platforms.len(), 1);
        let p = &s.achievements.platforms[0];
        assert_eq!((p.source.as_str(), p.unlocked, p.total, p.pct), ("retroachievements", 3, 4, 75.0));
        let ra = s.achievements.ra.as_ref().unwrap();
        assert_eq!((ra.unlocked_hc, ra.points, ra.points_softcore), (2, Some(120), Some(30)));
        assert_eq!(s.total_seconds, 9000, "o RA não informa tempo: nada é somado");
        let ico = s.top_games.iter().find(|g| g.title == "Ico").unwrap();
        assert_eq!(ico.sources, vec!["local", "retroachievements"], "vínculo por hash une o jogo da biblioteca");
        assert_eq!(s.history.iter().map(|m| m.unlocks.get("retroachievements").copied().unwrap_or(0)).sum::<i64>(), 3);
        assert!(s.steam.is_none());
    }

    #[test]
    fn both_platforms_on_the_same_game_stay_separate() {
        let c = db();
        with_ra(&c);
        crate::accounts::save(&c, 1, "steam", "Eu", Some("76561197960287930"), None, "{}").unwrap();
        // o mesmo jogo da biblioteca (Ico) vinculado também a um AppID com 1/3 conquistas
        c.execute_batch(
            "INSERT INTO steam_owned(profile_id, appid, name, playtime_forever, source) VALUES(1, 999, 'Ico HD', 120, 'api');
             INSERT INTO steam_links(game_id, appid) VALUES(2, 999);
             INSERT INTO steam_ach_schema(appid, apiname, name) VALUES(999,'X','X'),(999,'Y','Y'),(999,'Z','Z');
             INSERT INTO steam_ach_user(profile_id, appid, apiname, unlocked_at) VALUES(1,999,'X',1767000000);
             INSERT INTO steam_stats(profile_id, appid) VALUES(1,999);",
        )
        .unwrap();
        let s = build(&c, 1).unwrap();
        let ico = s.top_games.iter().find(|g| g.title == "Ico").unwrap();
        // tempo: local 30 min vs Steam 2 h → 2 h (máximo), não 2 h 30
        assert_eq!(ico.total_seconds, 7200);
        assert_eq!(ico.achievements.len(), 2, "uma barra por plataforma no mesmo jogo");
        let by: HashMap<&str, &PlatformAch> = s.achievements.platforms.iter().map(|p| (p.source.as_str(), p)).collect();
        assert_eq!((by["steam"].unlocked, by["steam"].total, by["steam"].pct), (1, 3, 33.3));
        assert_eq!((by["retroachievements"].unlocked, by["retroachievements"].total), (3, 4));
        // consolidado = soma de conjuntos distintos; percentual exato
        assert_eq!((s.achievements.unlocked, s.achievements.total, s.achievements.pct), (4, 7, 57.1));
        assert_eq!(s.total_seconds, 7200 + 7200, "Portal 2 (2 h) + Ico (2 h); Hades sem tempo");
    }

    #[test]
    fn unavailable_api_keeps_cached_stats() {
        let c = db();
        crate::accounts::save(&c, 1, "steam", "Eu", Some("76561197960287930"), None, "{}").unwrap();
        c.execute_batch("INSERT INTO steam_owned(profile_id, appid, name, playtime_forever, source) VALUES(1, 440, 'TF2', 600, 'api');").unwrap();
        let before = build(&c, 1).unwrap();
        // falha de sincronização: só registra o erro, o cache fica
        crate::accounts::set_sync_result(&c, 1, "steam", None, Some("Steam: sem conexão.")).unwrap();
        let after = build(&c, 1).unwrap();
        assert_eq!(before.total_seconds, after.total_seconds);
        assert_eq!(after.steam_seconds, Some(36000));
    }

    #[test]
    fn refresh_reflects_new_data() {
        let c = db();
        with_ra(&c);
        assert_eq!(build(&c, 1).unwrap().achievements.unlocked, 3);
        c.execute_batch(
            "INSERT INTO ra_unlocks(profile_id, ra_achievement_id, ra_game_id, unlocked_at) VALUES(1,4,10,'2026-04-01 10:00:00');
             UPDATE ra_progress SET awarded=4 WHERE profile_id=1 AND ra_game_id=10;
             UPDATE profile_games SET playtime_seconds=playtime_seconds+600 WHERE profile_id=1 AND game_id=3;",
        )
        .unwrap();
        let s = build(&c, 1).unwrap();
        assert_eq!((s.achievements.unlocked, s.achievements.pct), (4, 100.0));
        assert_eq!(s.achievements.completed.len(), 1, "jogo com 100% entra na lista de concluídos");
        assert_eq!(s.total_seconds, 9600);
        assert_eq!(s.achievements.recent[0].name, "D");
    }

    fn fake_epic(_: &Connection, _: i64) -> AppResult<Option<Collected>> {
        Ok(Some(Collected {
            info: SourceInfo { id: "epic".into(), connected: true, synced_at: None, has_playtime: true, has_achievements: true },
            games: vec![SrcGame {
                source: "epic", key: "epic:hades".into(), title: "Hades".into(), cover: None, icon: None, platform: "PC".into(),
                library_game_id: Some(3), seconds: Some(5400), recent_seconds: 0, last_activity: None, ach: Some((2, 4)),
            }],
            unlocks: vec![(now_epoch() - 86400, UnlockRow {
                source: "epic".into(), name: "E1".into(), description: String::new(), game: "Hades".into(), icon: None,
                at: Some(fmt_epoch(now_epoch() - 86400)), rarity_pct: None, hardcore: false, points: None,
            })],
            extra: None,
        }))
    }

    #[test]
    fn new_platform_plugs_in() {
        let c = db();
        let s = build_with(&c, 1, &[collect_local, fake_epic]).unwrap();
        let hades = s.top_games.iter().find(|g| g.title == "Hades").unwrap();
        assert_eq!((hades.total_seconds, hades.sources.clone()), (5400, vec!["local".to_string(), "epic".to_string()]));
        assert_eq!(s.achievements.platforms[0].source, "epic");
        assert_eq!(s.achievements.pct, 50.0);
        assert!(s.history.iter().any(|m| m.unlocks.get("epic") == Some(&1)));
        assert!(s.sources.iter().any(|x| x.id == "epic"));
    }
}
