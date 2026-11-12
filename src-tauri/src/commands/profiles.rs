//! CRUD de perfis. Perfis são locais e privados (sem contas públicas).
//! Excluir um perfil remove em cascata sua biblioteca, sessões e configurações.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    pub favorite_game: Option<String>,
    pub favorite_platform: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub bio: String,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    pub favorite_game: Option<String>,
    pub favorite_platform: Option<String>,
}

const COLS: &str =
    "id,username,display_name,bio,avatar,banner,favorite_game,favorite_platform,created_at";

fn map(r: &Row) -> rusqlite::Result<Profile> {
    Ok(Profile {
        id: r.get(0)?,
        username: r.get(1)?,
        display_name: r.get(2)?,
        bio: r.get(3)?,
        avatar: r.get(4)?,
        banner: r.get(5)?,
        favorite_game: r.get(6)?,
        favorite_platform: r.get(7)?,
        created_at: r.get(8)?,
    })
}

fn validate(i: &mut ProfileInput) -> AppResult<()> {
    i.username = i.username.trim().to_lowercase();
    i.display_name = i.display_name.trim().to_string();
    i.bio = i.bio.trim().to_string();
    if let Some(c) = i.favorite_platform.as_deref().and_then(crate::commands::games::canonical_platform) {
        i.favorite_platform = Some(c.into());
    }
    let ok_chars = i
        .username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if i.username.is_empty() || i.username.len() > 24 || !ok_chars {
        return Err(AppError::Validation(
            "Nome de usuário: 1 a 24 caracteres (letras, números, _ ou .).".into(),
        ));
    }
    let dn = i.display_name.chars().count();
    if dn == 0 || dn > 32 {
        return Err(AppError::Validation(
            "Nome de exibição: 1 a 32 caracteres.".into(),
        ));
    }
    if i.bio.chars().count() > 300 {
        return Err(AppError::Validation("Biografia: no máximo 300 caracteres.".into()));
    }
    Ok(())
}

fn map_unique(e: rusqlite::Error) -> AppError {
    if e.to_string().contains("UNIQUE") {
        AppError::Validation("Esse nome de usuário já está em uso.".into())
    } else {
        e.into()
    }
}

fn get(conn: &Connection, id: i64) -> AppResult<Profile> {
    conn.query_row(&format!("SELECT {COLS} FROM profiles WHERE id=?1"), [id], map)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                AppError::NotFound("Perfil não encontrado.".into())
            }
            other => other.into(),
        })
}

#[tauri::command]
pub fn list_profiles(db: State<Db>) -> AppResult<Vec<Profile>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM profiles ORDER BY id"))?;
    let rows = stmt.query_map([], map)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[tauri::command]
pub fn create_profile(db: State<Db>, mut input: ProfileInput) -> AppResult<Profile> {
    validate(&mut input)?;
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO profiles(username,display_name,bio,avatar,banner,favorite_game,favorite_platform)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            input.username,
            input.display_name,
            input.bio,
            input.avatar,
            input.banner,
            input.favorite_game,
            input.favorite_platform
        ],
    )
    .map_err(map_unique)?;
    get(&conn, conn.last_insert_rowid())
}

#[tauri::command]
pub fn update_profile(db: State<Db>, id: i64, mut input: ProfileInput) -> AppResult<Profile> {
    validate(&mut input)?;
    let conn = db.conn()?;
    let n = conn
        .execute(
            "UPDATE profiles SET username=?1,display_name=?2,bio=?3,avatar=?4,banner=?5,
             favorite_game=?6,favorite_platform=?7 WHERE id=?8",
            params![
                input.username,
                input.display_name,
                input.bio,
                input.avatar,
                input.banner,
                input.favorite_game,
                input.favorite_platform,
                id
            ],
        )
        .map_err(map_unique)?;
    if n == 0 {
        return Err(AppError::NotFound("Perfil não encontrado.".into()));
    }
    get(&conn, id)
}

#[tauri::command]
pub fn delete_profile(app: tauri::AppHandle, db: State<Db>, id: i64) -> AppResult<()> {
    let conn = db.conn()?;
    let n = conn.execute("DELETE FROM profiles WHERE id=?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound("Perfil não encontrado.".into()));
    }
    // Remove jogos que ficaram sem nenhum perfil associado.
    conn.execute(
        "DELETE FROM games WHERE id NOT IN (SELECT game_id FROM profile_games)",
        [],
    )?;
    // Remove as cópias de capturas gerenciadas deste perfil (arquivos originais não são tocados).
    use tauri::Manager;
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::remove_dir_all(dir.join("gallery").join(id.to_string()));
    }
    Ok(())
}
