//! Coleções personalizadas por perfil (ex.: "Terror", "Retrô").

use super::games::ensure_owned;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub game_ids: Vec<i64>,
}

fn clean_name(name: &str) -> AppResult<String> {
    let n = name.trim().to_string();
    let len = n.chars().count();
    if len == 0 || len > 40 {
        return Err(AppError::Validation("Nome da coleção: 1 a 40 caracteres.".into()));
    }
    Ok(n)
}

fn map_unique(e: rusqlite::Error) -> AppError {
    if e.to_string().contains("UNIQUE") {
        AppError::Validation("Você já tem uma coleção com esse nome.".into())
    } else {
        e.into()
    }
}

fn owned_collection(conn: &Connection, profile_id: i64, id: i64) -> AppResult<()> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM collections WHERE id=?1 AND profile_id=?2",
        params![id, profile_id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(AppError::NotFound("Coleção não encontrada.".into()));
    }
    Ok(())
}

#[tauri::command]
pub fn list_collections(db: State<Db>, profile_id: i64) -> AppResult<Vec<Collection>> {
    let conn = db.conn()?;
    let mut members: HashMap<i64, Vec<i64>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT cg.collection_id, cg.game_id FROM collection_games cg
             JOIN collections c ON c.id = cg.collection_id WHERE c.profile_id=?1",
        )?;
        let rows = stmt.query_map([profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        for row in rows {
            let (c, g) = row?;
            members.entry(c).or_default().push(g);
        }
    }
    let mut stmt =
        conn.prepare("SELECT id, name FROM collections WHERE profile_id=?1 ORDER BY name COLLATE NOCASE")?;
    let list = stmt
        .query_map([profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(id, name)| Collection { id, name, game_ids: members.remove(&id).unwrap_or_default() })
        .collect();
    Ok(list)
}

#[tauri::command]
pub fn create_collection(db: State<Db>, profile_id: i64, name: String) -> AppResult<Collection> {
    let name = clean_name(&name)?;
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO collections(profile_id,name) VALUES(?1,?2)",
        params![profile_id, name],
    )
    .map_err(map_unique)?;
    Ok(Collection { id: conn.last_insert_rowid(), name, game_ids: vec![] })
}

#[tauri::command]
pub fn rename_collection(db: State<Db>, profile_id: i64, id: i64, name: String) -> AppResult<()> {
    let name = clean_name(&name)?;
    let conn = db.conn()?;
    owned_collection(&conn, profile_id, id)?;
    conn.execute("UPDATE collections SET name=?1 WHERE id=?2", params![name, id])
        .map_err(map_unique)?;
    Ok(())
}

#[tauri::command]
pub fn delete_collection(db: State<Db>, profile_id: i64, id: i64) -> AppResult<()> {
    let conn = db.conn()?;
    owned_collection(&conn, profile_id, id)?;
    conn.execute("DELETE FROM collections WHERE id=?1", [id])?;
    Ok(())
}

#[tauri::command]
pub fn set_in_collection(
    db: State<Db>,
    profile_id: i64,
    collection_id: i64,
    game_id: i64,
    member: bool,
) -> AppResult<()> {
    let conn = db.conn()?;
    owned_collection(&conn, profile_id, collection_id)?;
    ensure_owned(&conn, profile_id, game_id)?;
    if member {
        conn.execute(
            "INSERT OR IGNORE INTO collection_games(collection_id,game_id) VALUES(?1,?2)",
            params![collection_id, game_id],
        )?;
    } else {
        conn.execute(
            "DELETE FROM collection_games WHERE collection_id=?1 AND game_id=?2",
            params![collection_id, game_id],
        )?;
    }
    Ok(())
}
