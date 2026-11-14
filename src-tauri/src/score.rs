//! Pontuação de conquistas do perfil: um valor por conquista, na escala dos troféus do PlayStation
//! (bronze 15, prata 30, ouro 90, platina 180), somado de todas as fontes.
//!
//! - Troféus (Vita3K/RPCS3): pelo tipo. Só os de jogos que estão na biblioteca do perfil (os troféus são do
//!   emulador, não de um perfil do NEXSTAGE).
//! - Steam: pela raridade global (% de jogadores com a conquista); jogo 100% vale um bônus de 180.
//! - RetroAchievements: pelos pontos da conquista, +50% se obtida em Hardcore; jogo completo/masterizado
//!   vale um bônus de 180.
//! A pontuação é sempre CALCULADA a partir dos dados guardados (nada é acumulado), então é retroativa e se
//! ajusta sozinha se a raridade de uma conquista mudar.
//! A mesma tabela existe em `src/lib/score.ts` (valor exibido nas listas e nos avisos): mude as duas juntas.

use crate::db::Db;
use crate::error::AppResult;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, State};

pub const COMPLETION_BONUS: i64 = 180;

pub fn trophy_value(grade: &str) -> i64 {
    match grade {
        "P" => 180,
        "G" => 90,
        "S" => 30,
        _ => 15,
    }
}

/// Steam: raridade desconhecida vale o mínimo.
pub fn steam_value(global_pct: Option<f64>) -> i64 {
    match global_pct {
        Some(p) if p < 3.0 => 90,
        Some(p) if p < 10.0 => 60,
        Some(p) if p < 30.0 => 30,
        _ => 15,
    }
}

pub fn ra_value(points: i64, hardcore: bool) -> i64 {
    let base = match points {
        p if p >= 25 => 90,
        p if p >= 10 => 60,
        p if p >= 5 => 30,
        _ => 15,
    };
    if hardcore { base * 3 / 2 } else { base }
}

/// Nível pela pontuação: do nível N para o N+1 são precisos 250·N pontos (250, 500, 750…).
/// Devolve (nível, pontos onde o nível atual começa, pontos do próximo nível).
pub fn level(total: i64) -> (i64, i64, i64) {
    let mut lv = 1;
    let mut floor = 0;
    loop {
        let next = floor + 250 * lv;
        if total < next {
            return (lv, floor, next);
        }
        floor = next;
        lv += 1;
    }
}

#[derive(Serialize, Default, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct SourceScore {
    pub points: i64,
    pub achievements: i64,
    /// Jogos completos (bônus) — nos troféus, platinas.
    pub completed: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileScore {
    pub total: i64,
    pub level: i64,
    pub level_floor: i64,
    pub next_level_at: i64,
    pub ra: SourceScore,
    pub steam: SourceScore,
    pub trophies: SourceScore,
}

fn ra_score(conn: &Connection, profile: i64) -> AppResult<SourceScore> {
    let mut s = SourceScore::default();
    let mut st = conn.prepare(
        "SELECT COALESCE(a.points, 0), u.unlocked_hc_at IS NOT NULL FROM ra_unlocks u
         LEFT JOIN ra_achievements a ON a.ra_achievement_id = u.ra_achievement_id
         WHERE u.profile_id = ?1 AND (u.unlocked_at IS NOT NULL OR u.unlocked_hc_at IS NOT NULL)",
    )?;
    let rows = st.query_map([profile], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)))?;
    for r in rows {
        let (pts, hc) = r?;
        s.points += ra_value(pts, hc);
        s.achievements += 1;
    }
    s.completed = conn.query_row(
        "SELECT COUNT(*) FROM ra_progress WHERE profile_id = ?1
           AND (award_kind IN ('completed', 'mastered') OR (max_possible > 0 AND awarded >= max_possible))",
        [profile],
        |r| r.get(0),
    )?;
    s.points += s.completed * COMPLETION_BONUS;
    Ok(s)
}

fn steam_score(conn: &Connection, profile: i64) -> AppResult<SourceScore> {
    let mut s = SourceScore::default();
    let mut st = conn.prepare(
        "SELECT sc.global_pct FROM steam_ach_user u
         LEFT JOIN steam_ach_schema sc ON sc.appid = u.appid AND sc.apiname = u.apiname
         WHERE u.profile_id = ?1 AND u.unlocked_at IS NOT NULL",
    )?;
    let rows = st.query_map([profile], |r| r.get::<_, Option<f64>>(0))?;
    for r in rows {
        s.points += steam_value(r?);
        s.achievements += 1;
    }
    // 100%: todas as conquistas do esquema do jogo obtidas
    s.completed = conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT sc.appid FROM steam_ach_schema sc
            LEFT JOIN steam_ach_user u ON u.profile_id = ?1 AND u.appid = sc.appid AND u.apiname = sc.apiname
            WHERE sc.appid IN (SELECT DISTINCT appid FROM steam_ach_user WHERE profile_id = ?1)
            GROUP BY sc.appid
            HAVING COUNT(*) > 0 AND SUM(u.unlocked_at IS NOT NULL) = COUNT(*))",
        [profile],
        |r| r.get(0),
    )?;
    s.points += s.completed * COMPLETION_BONUS;
    Ok(s)
}

fn trophy_score(conn: &Connection, profile: i64, lang: &str) -> AppResult<SourceScore> {
    let mut s = SourceScore::default();
    let mut st = conn.prepare("SELECT game_id FROM profile_games WHERE profile_id = ?1")?;
    let ids: Vec<i64> = st.query_map([profile], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut counted = std::collections::HashSet::new();
    for g in ids {
        let Ok(Some(crate::trophies::Resolved { root, comm_id: Some(id) })) = crate::trophies::resolve(conn, g) else { continue };
        // dois jogos da biblioteca com o mesmo conjunto contam uma vez
        if !counted.insert((root.emu, id.clone())) {
            continue;
        }
        let Some(dir) = root.find_conf(&id) else { continue };
        let Some(conf) = crate::trophies::load_conf(&dir, lang) else { continue };
        let got = root.unlocks(&dir).unwrap_or_default();
        for t in conf.trophies.iter().filter(|t| got.contains_key(&t.id)) {
            s.points += trophy_value(&t.grade);
            s.achievements += 1;
            if t.grade == "P" {
                s.completed += 1;
            }
        }
    }
    Ok(s)
}

#[tauri::command]
pub fn profile_score(app: AppHandle, db: State<Db>, profile_id: i64) -> AppResult<ProfileScore> {
    let lang = crate::overlay::setting(&app, "language").unwrap_or_else(|| "pt-BR".into());
    let conn = db.conn()?;
    compute(&conn, profile_id, &lang)
}

pub fn compute(conn: &Connection, profile_id: i64, lang: &str) -> AppResult<ProfileScore> {
    let ra = ra_score(conn, profile_id)?;
    let steam = steam_score(conn, profile_id)?;
    let trophies = trophy_score(conn, profile_id, lang)?;
    let total = ra.points + steam.points + trophies.points;
    let (level, level_floor, next_level_at) = level(total);
    Ok(ProfileScore { total, level, level_floor, next_level_at, ra, steam, trophies })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_follow_the_table() {
        assert_eq!([trophy_value("B"), trophy_value("S"), trophy_value("G"), trophy_value("P")], [15, 30, 90, 180]);
        assert_eq!([steam_value(None), steam_value(Some(55.0)), steam_value(Some(29.9)), steam_value(Some(9.0)), steam_value(Some(2.5))], [15, 15, 30, 60, 90]);
        assert_eq!([ra_value(1, false), ra_value(5, false), ra_value(10, false), ra_value(25, false), ra_value(100, false)], [15, 30, 60, 90, 90]);
        assert_eq!([ra_value(1, true), ra_value(25, true)], [22, 135]);
    }

    #[test]
    fn levels_grow_by_250_per_level() {
        assert_eq!(level(0), (1, 0, 250));
        assert_eq!(level(249), (1, 0, 250));
        assert_eq!(level(250), (2, 250, 750));
        assert_eq!(level(750), (3, 750, 1500));
        assert_eq!(level(1499), (3, 750, 1500));
    }
}

