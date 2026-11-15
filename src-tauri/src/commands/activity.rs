//! Atividade recente do perfil (dados reais de `play_sessions` e de jogos concluídos).

use crate::db::Db;
use crate::error::AppResult;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub game_id: i64,
    pub title: String,
    pub started_at: String,
    pub duration_seconds: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletedRow {
    pub game_id: i64,
    pub title: String,
    pub completed_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthSummary {
    pub sessions: i64,
    pub seconds: i64,
    pub games: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub sessions: Vec<SessionRow>,
    pub completed: Vec<CompletedRow>,
    pub month: MonthSummary,
}

#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn profile_activity(db: State<Db>, profile_id: i64) -> AppResult<Activity> {
    let conn = db.conn()?;
    let sessions = {
        let mut st = conn.prepare(
            "SELECT s.game_id,g.title,s.started_at,s.duration_seconds
             FROM play_sessions s JOIN games g ON g.id=s.game_id
             WHERE s.profile_id=?1 ORDER BY s.started_at DESC, s.id DESC LIMIT 12",
        )?;
        let rows = st
            .query_map([profile_id], |r| {
                Ok(SessionRow { game_id: r.get(0)?, title: r.get(1)?, started_at: r.get(2)?, duration_seconds: r.get(3)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let completed = {
        let mut st = conn.prepare(
            // jogos da biblioteca zerados (registro do Zerei! ligado ao jogo)
            "SELECT z.game_id,g.title,MAX(z.completed_at)
             FROM zerei_entries z JOIN games g ON g.id=z.game_id
             WHERE z.profile_id=?1 AND z.status='completed' AND z.completed_at IS NOT NULL
             GROUP BY z.game_id ORDER BY MAX(z.completed_at) DESC LIMIT 5",
        )?;
        let rows = st
            .query_map([profile_id], |r| {
                Ok(CompletedRow { game_id: r.get(0)?, title: r.get(1)?, completed_at: r.get(2)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let month = conn.query_row(
        "SELECT COUNT(*),COALESCE(SUM(duration_seconds),0),COUNT(DISTINCT game_id)
         FROM play_sessions
         WHERE profile_id=?1 AND strftime('%Y-%m',started_at,'localtime')=strftime('%Y-%m','now','localtime')",
        [profile_id],
        |r| Ok(MonthSummary { sessions: r.get(0)?, seconds: r.get(1)?, games: r.get(2)? }),
    )?;
    Ok(Activity { sessions, completed, month })
}
