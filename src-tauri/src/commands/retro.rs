//! Comandos do RetroAchievements expostos à interface. A chave da API entra só em `ra_connect`
//! e nunca é devolvida: a interface recebe apenas o nome de usuário e os dados públicos do perfil.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::retro::{client, link, secret, store, sync, watch};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaAccount {
    pub username: String,
    /// Chave guardada e legível neste Windows (false = precisa reconectar).
    pub has_key: bool,
    pub profile: Option<Value>,
    pub synced_at: Option<String>,
    pub sync_error: Option<String>,
    pub syncing: bool,
    pub blocked_secs: Option<u64>,
    pub profile_url: String,
}

fn account_view(conn: &rusqlite::Connection, profile_id: i64) -> AppResult<Option<RaAccount>> {
    let Some(a) = store::account(conn, profile_id)? else { return Ok(None) };
    let has_key = a.secret.as_deref().and_then(secret::unprotect).is_some();
    Ok(Some(RaAccount {
        profile_url: format!("{}/user/{}", client::SITE, a.username),
        username: a.username,
        has_key,
        profile: a.data.and_then(|d| serde_json::from_str(&d).ok()),
        synced_at: a.synced_at,
        sync_error: a.sync_error,
        syncing: sync::is_running(),
        blocked_secs: client::blocked_for(),
    }))
}

#[tauri::command]
pub fn ra_get_account(db: State<Db>, profile_id: i64) -> AppResult<Option<RaAccount>> {
    account_view(&*db.conn()?, profile_id)
}

fn validate(username: &str, key: &str) -> AppResult<()> {
    let u = username.trim();
    if u.len() < 2 || u.len() > 32 || !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return Err(AppError::Validation("Nome de usuário do RetroAchievements inválido.".into()));
    }
    let k = key.trim();
    if k.len() < 16 || k.len() > 64 || !k.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::Validation("Chave da API inválida: copie a \"Web API Key\" em retroachievements.org → Configurações.".into()));
    }
    Ok(())
}

/// Testa as credenciais na API e, se válidas, guarda a conta (chave cifrada) no perfil.
#[tauri::command]
pub async fn ra_connect(db: State<'_, Db>, profile_id: i64, username: String, api_key: String) -> AppResult<RaAccount> {
    validate(&username, &api_key)?;
    let creds = client::Creds::new(&username, &api_key);
    let prof = client::user_profile(&creds).await?;
    // usa a grafia oficial do nome de usuário devolvida pela API
    let official = prof["User"].as_str().unwrap_or(username.trim()).to_string();
    let blob = secret::protect(api_key.trim()).ok_or_else(|| AppError::Other("Não foi possível proteger a chave da API neste computador.".into()))?;
    let conn = db.conn()?;
    store::save_account(&conn, profile_id, &official, &blob, &sync::profile_snapshot(&prof).to_string())?;
    account_view(&conn, profile_id)?.ok_or_else(|| AppError::Other("Falha ao salvar a conta.".into()))
}

/// Testa a conexão com a conta já salva (atualiza os dados do perfil).
#[tauri::command]
pub async fn ra_test(db: State<'_, Db>, profile_id: i64) -> AppResult<RaAccount> {
    let creds = {
        let conn = db.conn()?;
        store::creds(&conn, profile_id)?
    }
    .ok_or_else(|| AppError::Validation("Nenhuma conta conectada, ou a chave salva não pode ser lida neste Windows. Conecte novamente.".into()))?;
    let prof = client::user_profile(&creds).await?;
    let conn = db.conn()?;
    store::set_account_data(&conn, profile_id, &sync::profile_snapshot(&prof).to_string())?;
    account_view(&conn, profile_id)?.ok_or_else(|| AppError::Other("Conta não encontrada.".into()))
}

/// Desconecta: apaga a conta e a chave; com `clear_cache`, também o progresso guardado do perfil.
#[tauri::command]
pub fn ra_disconnect(db: State<Db>, profile_id: i64, clear_cache: bool) -> AppResult<()> {
    let conn = db.conn()?;
    store::delete_account(&conn, profile_id, clear_cache)
}

#[tauri::command]
pub async fn ra_sync(app: AppHandle, profile_id: i64) -> AppResult<sync::SyncReport> {
    sync::sync_profile(&app, profile_id).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub account: Option<RaAccount>,
    pub totals: store::Totals,
    pub recent: Vec<store::AchievementRow>,
    pub games: Vec<store::GameRow>,
}

/// Tudo o que a central de conquistas precisa, só do cache local (funciona offline).
#[tauri::command(async)]
pub fn ra_overview(db: State<Db>, profile_id: i64) -> AppResult<Overview> {
    let conn = db.conn()?;
    Ok(Overview {
        account: account_view(&conn, profile_id)?,
        totals: store::totals(&conn, profile_id)?,
        recent: store::list_achievements(&conn, profile_id, "earned", None, "", 24)?,
        games: store::list_games(&conn, profile_id)?,
    })
}

#[tauri::command(async)]
pub fn ra_list_achievements(db: State<Db>, profile_id: i64, filter: String, ra_game_id: Option<i64>, query: String) -> AppResult<Vec<store::AchievementRow>> {
    let conn = db.conn()?;
    store::list_achievements(&conn, profile_id, &filter, ra_game_id, &query, 600)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    pub game: Option<store::GameRow>,
    pub achievements: Vec<store::AchievementRow>,
    /// Aviso quando não foi possível atualizar (mostrando o cache).
    pub warning: Option<String>,
}

/// Detalhes de um jogo do RA. Atualiza pela API se pedido (ou se nunca foi buscado); senão, cache.
#[tauri::command]
pub async fn ra_game(app: AppHandle, db: State<'_, Db>, profile_id: i64, ra_game_id: i64, refresh: bool) -> AppResult<GameDetail> {
    let (creds, has) = {
        let conn = db.conn()?;
        let has: bool = conn.query_row("SELECT details_at IS NOT NULL FROM ra_games WHERE ra_game_id=?1", [ra_game_id], |r| r.get(0)).unwrap_or(false);
        (store::creds(&conn, profile_id)?, has)
    };
    let mut warning = None;
    if refresh || !has {
        match &creds {
            Some(c) => match client::game_info_user(c, ra_game_id).await {
                Ok(v) => {
                    let mut conn = db.conn()?;
                    let tx = conn.transaction()?;
                    // desbloqueios encontrados aqui entram no cache; o aviso fica com a sincronização
                    let _ = store::store_game_details(&tx, profile_id, &v)?;
                    tx.commit()?;
                }
                Err(e) => warning = Some(e.message()),
            },
            None if !has => warning = Some("Conecte sua conta do RetroAchievements para carregar as conquistas deste jogo.".into()),
            None => {}
        }
    }
    let _ = app;
    let conn = db.conn()?;
    Ok(GameDetail {
        game: store::game(&conn, profile_id, ra_game_id)?,
        achievements: store::list_achievements(&conn, profile_id, "game", Some(ra_game_id), "", 2000)?,
        warning,
    })
}

#[tauri::command]
pub async fn ra_link_status(app: AppHandle, profile_id: i64, game_id: i64) -> AppResult<link::LinkInfo> {
    link::link_status(&app, profile_id, game_id).await
}

#[tauri::command]
pub fn ra_set_link(db: State<Db>, profile_id: i64, game_id: i64, ra_game_id: Option<i64>, unsupported: bool) -> AppResult<()> {
    let conn = db.conn()?;
    link::set_link(&conn, profile_id, game_id, ra_game_id, unsupported)
}

/// Busca no catálogo do console da plataforma (baixa o catálogo se preciso e possível).
#[tauri::command]
pub async fn ra_search_games(db: State<'_, Db>, profile_id: i64, platform: String, query: String) -> AppResult<Vec<store::GameRow>> {
    let console = crate::retro::hash::console_for_platform(&platform)
        .ok_or_else(|| AppError::Validation("Esta plataforma não tem suporte no RetroAchievements.".into()))?;
    let creds = {
        let conn = db.conn()?;
        store::creds(&conn, profile_id)?
    };
    let mut warn = None;
    if let Some(c) = &creds {
        if let Err(e) = link::ensure_catalog(&db, c, console).await {
            warn = Some(e);
        }
    }
    let conn = db.conn()?;
    if !store::catalog_exists(&conn, console)? {
        return Err(match warn {
            Some(e) => e.into(),
            None => AppError::Validation("Conecte sua conta do RetroAchievements para pesquisar no catálogo.".into()),
        });
    }
    store::search_catalog(&conn, profile_id, console, &query, 40)
}

#[tauri::command]
pub async fn ra_scan_library(app: AppHandle, profile_id: i64) -> AppResult<link::ScanReport> {
    link::scan_library(&app, profile_id).await
}

#[tauri::command(async)]
pub fn ra_emulator_support(db: State<Db>) -> Vec<watch::EmuSupport> {
    watch::emulator_support(&db)
}
