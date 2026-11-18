//! Comandos da integração Steam. Nenhum deles recebe ou devolve senha, cookie ou token da Steam;
//! a chave pessoal da Web API só entra em `steam_set_key` e nunca volta para a interface.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::steam::{client, local, openid, store, sync};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamAccount {
    pub steam_id: String,
    pub name: String,
    pub profile: Option<Value>,
    pub profile_url: String,
    pub has_key: bool,
    /// Há uma chave salva, mas ela não abre neste Windows (outra conta/PC): é preciso salvar de novo.
    pub key_unreadable: bool,
    pub synced_at: Option<String>,
    pub sync_error: Option<String>,
    pub syncing: bool,
    pub owned_count: i64,
    pub owned_source: Option<String>,
}

fn account_view(conn: &rusqlite::Connection, profile_id: i64) -> AppResult<Option<SteamAccount>> {
    let Some(a) = crate::accounts::get(conn, profile_id, store::PLATFORM)? else { return Ok(None) };
    let Some(id) = a.external_id.clone() else { return Ok(None) };
    let readable = a.secret.as_deref().and_then(sync::unprotect_key).is_some();
    let (count, source): (i64, Option<String>) = conn.query_row(
        "SELECT COUNT(*), MAX(source) FROM steam_owned WHERE profile_id=?1",
        [profile_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let profile: Option<Value> = a.data.and_then(|d| serde_json::from_str(&d).ok());
    Ok(Some(SteamAccount {
        profile_url: profile.as_ref().and_then(|p| p["profileUrl"].as_str().map(str::to_string)).unwrap_or_else(|| format!("https://steamcommunity.com/profiles/{id}/")),
        steam_id: id,
        name: a.username,
        profile,
        has_key: readable,
        key_unreadable: a.secret.is_some() && !readable,
        synced_at: a.synced_at,
        sync_error: a.sync_error,
        syncing: sync::is_running(),
        owned_count: count,
        owned_source: source,
    }))
}

#[tauri::command]
pub fn steam_get_account(db: State<Db>, profile_id: i64) -> AppResult<Option<SteamAccount>> {
    account_view(&*db.conn()?, profile_id)
}

/// "Entrar com a Steam": abre a página oficial no navegador e espera o retorno (até 5 min).
#[tauri::command]
pub async fn steam_login(app: AppHandle, db: State<'_, Db>, profile_id: i64) -> AppResult<SteamAccount> {
    let s = openid::start()?;
    app.opener().open_url(s.login_url.clone(), None::<&str>).map_err(|_| AppError::Other("Não foi possível abrir o navegador para o login da Steam.".into()))?;
    let listener = s.listener;
    let params = tauri::async_runtime::spawn_blocking(move || openid::wait_callback(listener))
        .await
        .map_err(|_| AppError::Other("Falha ao aguardar o login da Steam.".into()))??;
    let steamid = openid::check_assertion(&params, &s.return_to, &s.state)?;
    openid::verify_with_steam(&params).await?;
    // perfil: com a chave já salva (se houver) ou pelo perfil público
    let key = {
        let conn = db.conn()?;
        crate::accounts::get(&conn, profile_id, store::PLATFORM)?.and_then(|a| a.secret).and_then(|b| sync::unprotect_key(&b))
    };
    let prof = sync::fetch_profile(&steamid, key.as_deref()).await.unwrap_or_else(|_| serde_json::json!({ "visibility": "unknown" }));
    let name = prof["name"].as_str().filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| steamid.clone());
    let conn = db.conn()?;
    crate::accounts::save(&conn, profile_id, store::PLATFORM, &name, Some(&steamid), None, &prof.to_string())?;
    account_view(&conn, profile_id)?.ok_or_else(|| AppError::Other("Falha ao salvar a conta Steam.".into()))
}

#[tauri::command]
pub fn steam_cancel_login() {
    openid::cancel();
}

/// Salva (testando na API) ou remove a chave pessoal da Web API.
#[tauri::command]
pub async fn steam_set_key(db: State<'_, Db>, profile_id: i64, key: Option<String>) -> AppResult<SteamAccount> {
    let steamid = {
        let conn = db.conn()?;
        crate::accounts::get(&conn, profile_id, store::PLATFORM)?.and_then(|a| a.external_id)
    }
    .ok_or_else(|| AppError::Validation("Conecte a conta Steam antes de informar a chave.".into()))?;
    let blob = match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(k) => {
            if k.len() != 32 || !k.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(AppError::Validation("Chave da Web API inválida: são 32 caracteres (0-9, A-F), gerados em steamcommunity.com/dev/apikey.".into()));
            }
            let p = client::player_summary(Some(&k), &steamid).await?; // testa a chave
            let conn = db.conn()?;
            crate::accounts::set_data(&conn, profile_id, store::PLATFORM, p["personaname"].as_str(), &sync::summary_data(&p).to_string())?;
            Some(sync::protect_key(&k).ok_or_else(|| AppError::Other("Não foi possível proteger a chave neste computador.".into()))?)
        }
        None => None,
    };
    let conn = db.conn()?;
    crate::accounts::set_secret(&conn, profile_id, store::PLATFORM, blob.as_deref())?;
    account_view(&conn, profile_id)?.ok_or_else(|| AppError::Other("Conta não encontrada.".into()))
}

/// Desconecta: apaga a conta, o SteamID e a chave. `clear_cache` também apaga os dados da Steam do perfil.
/// Jogos já importados continuam na biblioteca.
#[tauri::command]
pub fn steam_disconnect(db: State<Db>, profile_id: i64, clear_cache: bool) -> AppResult<()> {
    let conn = db.conn()?;
    crate::accounts::delete(&conn, profile_id, store::PLATFORM)?;
    if clear_cache {
        store::clear_profile(&conn, profile_id)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn steam_sync(app: AppHandle, profile_id: i64, lang: String) -> AppResult<sync::SyncReport> {
    sync::sync_profile(&app, profile_id, client::steam_lang(&lang)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedList {
    pub games: Vec<store::OwnedRow>,
    pub client_found: bool,
}

/// Lista para a tela de importação (cache local; atualize com `steam_sync`).
#[tauri::command(async)]
pub fn steam_owned(db: State<Db>, profile_id: i64) -> AppResult<OwnedList> {
    let apps = local::local_apps();
    let found = local::steam_root().is_some();
    let conn = db.conn()?;
    // sem chave e sem lista: mostra pelo menos os instalados neste PC
    let has_rows: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM steam_owned WHERE profile_id=?1)", [profile_id], |r| r.get(0))?;
    if !has_rows && crate::accounts::get(&conn, profile_id, store::PLATFORM)?.is_some() {
        store::store_local(&conn, profile_id, &apps.values().cloned().collect::<Vec<_>>())?;
    }
    Ok(OwnedList { games: store::list_owned(&conn, profile_id, &apps, found)?, client_found: found })
}

#[tauri::command]
pub fn steam_preview(db: State<Db>, profile_id: i64, appids: Vec<i64>) -> AppResult<Vec<store::PreviewItem>> {
    store::preview(&*db.conn()?, profile_id, &appids)
}

#[tauri::command]
pub fn steam_import(db: State<Db>, profile_id: i64, appids: Vec<i64>, link_title_matches: bool) -> AppResult<store::ImportReport> {
    if appids.is_empty() {
        return Err(AppError::Validation("Nenhum jogo selecionado.".into()));
    }
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let r = store::import(&tx, profile_id, &appids, link_title_matches)?;
    tx.commit()?;
    Ok(r)
}

#[tauri::command]
pub fn steam_set_link(db: State<Db>, profile_id: i64, game_id: i64, appid: Option<i64>) -> AppResult<()> {
    store::set_link(&*db.conn()?, profile_id, game_id, appid)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub appid: i64,
    pub client_found: bool,
    pub protocol: bool,
    /// "installed" | "updating" | "installing" | "not_installed" | "unknown"
    pub install: String,
    pub size_on_disk: i64,
}

#[tauri::command(async)]
pub fn steam_app_status(appid: i64) -> AppStatus {
    let found = local::steam_root().is_some();
    let app = local::app(appid);
    AppStatus {
        appid,
        client_found: found,
        protocol: local::protocol_available(),
        install: app.as_ref().map(|a| a.state.clone()).unwrap_or_else(|| if found { "not_installed".into() } else { "unknown".into() }),
        size_on_disk: app.map(|a| a.size_on_disk).unwrap_or(0),
    }
}

/// Pede a instalação ao cliente Steam (`steam://install/<appid>`); sem o protocolo, abre a loja.
/// O NEXSTAGE não baixa nada.
#[tauri::command]
pub fn steam_install(app: AppHandle, appid: i64) -> AppResult<String> {
    if appid <= 0 {
        return Err(AppError::Validation("AppID da Steam desconhecido.".into()));
    }
    let (url, how) = if local::protocol_available() {
        (format!("steam://install/{appid}"), "client")
    } else {
        (store::store_url(appid), "store")
    };
    app.opener().open_url(url, None::<&str>).map_err(|_| AppError::Other("Não foi possível abrir a Steam.".into()))?;
    Ok(how.into())
}

/// Abre a página do jogo no cliente Steam (ou na loja, sem o protocolo).
#[tauri::command]
pub fn steam_open_page(app: AppHandle, appid: i64) -> AppResult<()> {
    let url = if local::protocol_available() { format!("steam://nav/games/details/{appid}") } else { store::store_url(appid) };
    app.opener().open_url(url, None::<&str>).map_err(|_| AppError::Other("Não foi possível abrir a Steam.".into()))
}

/// Dados Steam de um jogo (cache). `refresh` busca conquistas/estatísticas na API.
#[tauri::command]
pub async fn steam_game(db: State<'_, Db>, profile_id: i64, appid: i64, refresh: bool, lang: String) -> AppResult<(store::GameData, Option<String>)> {
    let mut warning = None;
    let fresh = store::schema_fresh(&*db.conn()?, profile_id, appid)?;
    if refresh || !fresh {
        match sync::identity(&db, profile_id)? {
            Some((sid, Some(key))) => {
                if let Err(e) = sync::fetch_game(&db, profile_id, &sid, Some(&key), appid, client::steam_lang(&lang)).await {
                    warning = Some(e.to_string());
                }
            }
            Some((_, None)) if refresh => warning = Some(client::SteamError::NoKey.message()),
            None if refresh => warning = Some("Conecte sua conta Steam em Configurações → Contas e plataformas.".into()),
            _ => {}
        }
    }
    let apps = local::local_apps();
    let found = local::steam_root().is_some();
    Ok((store::game_data(&*db.conn()?, profile_id, appid, &apps, found)?, warning))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamAchOverview {
    pub account: Option<SteamAccount>,
    #[serde(flatten)]
    pub overview: store::AchOverview,
}

/// Aba Conquistas → Steam: conta + visão geral de todos os jogos (cache local).
#[tauri::command(async)]
pub fn steam_ach_overview(db: State<Db>, profile_id: i64) -> AppResult<SteamAchOverview> {
    let conn = db.conn()?;
    Ok(SteamAchOverview { account: account_view(&conn, profile_id)?, overview: store::ach_overview(&conn, profile_id)? })
}

/// Aba Conquistas → Steam: "recent" | "earned" | "pending" | "rare", filtrada por jogo/conquista.
#[tauri::command(async)]
pub fn steam_ach_list(db: State<Db>, profile_id: i64, tab: String, query: String) -> AppResult<Vec<store::AchEntry>> {
    store::ach_list(&*db.conn()?, profile_id, &tab, &query, 500)
}

#[tauri::command(async)]
pub fn steam_client_status() -> local::ClientStatus {
    local::client_status()
}

// ---------- Últimas atualizações (Home) ----------

/// Notas de atualização oficiais mais recentes dos jogos Steam da biblioteca (só cache; offline ok).
#[tauri::command(async)]
pub fn steam_news_overview(db: State<Db>, profile_id: i64) -> AppResult<crate::steam::news::Overview> {
    crate::steam::news::overview(&*db.conn()?, profile_id, 6)
}

/// Verifica em segundo plano os jogos com cache vencido (6 h; manual: 15 min). Não bloqueia.
#[tauri::command]
pub fn steam_news_refresh(app: AppHandle, profile_id: i64, manual: bool) -> bool {
    crate::steam::news::start(&app, profile_id, manual)
}

/// Nota de atualização oficial mais recente de um jogo (página do jogo; cache de 6 h, `refresh`: 15 min).
#[tauri::command]
pub async fn steam_game_news(db: State<'_, Db>, appid: i64, refresh: bool) -> AppResult<crate::steam::news::GameNews> {
    if appid <= 0 {
        return Err(AppError::Validation("AppID da Steam desconhecido.".into()));
    }
    crate::steam::news::game_news(&db, appid, refresh).await
}

// ---------- DLCs ----------

/// DLCs de um jogo (aba "DLCs"): cache de 24 h; `refresh` consulta de novo a loja (mín. 5 min).
#[tauri::command]
pub async fn steam_dlc(db: State<'_, Db>, appid: i64, refresh: bool, lang: String) -> AppResult<crate::steam::dlc::DlcView> {
    if appid <= 0 {
        return Err(AppError::Validation("AppID da Steam desconhecido.".into()));
    }
    crate::steam::dlc::game_dlc(&db, appid, refresh, client::steam_lang(&lang)).await
}

/// Verifica em segundo plano se os jogos Steam do perfil ganharam DLC nova (cache de 24 h). Não bloqueia.
#[tauri::command]
pub fn steam_dlc_refresh(app: AppHandle, profile_id: i64, lang: String) -> bool {
    crate::steam::dlc::start(&app, profile_id, client::steam_lang(&lang))
}

/// DLCs novas dos jogos Steam da biblioteca (Home; só cache).
#[tauri::command(async)]
pub fn steam_dlc_new(db: State<Db>, profile_id: i64) -> AppResult<Vec<crate::steam::dlc::NewDlc>> {
    crate::steam::dlc::recent_new(&*db.conn()?, profile_id, 12)
}

/// Abre a página de um item (jogo ou DLC) na loja do cliente Steam (ou no navegador, sem o protocolo).
#[tauri::command]
pub fn steam_open_store(app: AppHandle, appid: i64) -> AppResult<()> {
    if appid <= 0 {
        return Err(AppError::Validation("AppID da Steam desconhecido.".into()));
    }
    let url = if local::protocol_available() { format!("steam://store/{appid}") } else { store::store_url(appid) };
    app.opener().open_url(url, None::<&str>).map_err(|_| AppError::Other("Não foi possível abrir a Steam.".into()))
}

/// Guarda a última atualização já vista (indicação de "novo" na Home).
#[tauri::command]
pub fn steam_news_seen(db: State<Db>, profile_id: i64, gid: String) -> AppResult<()> {
    db.conn()?.execute(
        "INSERT INTO profile_settings(profile_id,key,value) VALUES(?1,'steam_news_seen',?2)
         ON CONFLICT(profile_id,key) DO UPDATE SET value=excluded.value",
        rusqlite::params![profile_id, gid],
    )?;
    Ok(())
}
