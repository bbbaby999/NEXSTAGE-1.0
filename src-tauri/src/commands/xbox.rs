//! Comandos da conta Xbox (ver `crate::xbox`).

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::xbox::{self, auth, store};
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxAccount {
    pub gamertag: String,
    pub xuid: String,
    pub gamerpic: Option<String>,
    pub gamerscore: Option<i64>,
    pub synced_at: Option<String>,
    pub sync_error: Option<String>,
    pub titles: i64,
    /// O refresh token guardado não pode ser lido neste Windows (ex.: backup sem senha): entrar de novo.
    pub needs_login: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxStatus {
    pub account: Option<XboxAccount>,
    /// Client ID informado em Configurações (o embutido na compilação não é mostrado).
    pub client_id: Option<String>,
    pub embedded: bool,
}

fn account_view(conn: &Connection, profile_id: i64) -> AppResult<Option<XboxAccount>> {
    let Some(a) = crate::accounts::get(conn, profile_id, store::PLATFORM)? else { return Ok(None) };
    let data: serde_json::Value = a.data.as_deref().and_then(|d| serde_json::from_str(d).ok()).unwrap_or_default();
    Ok(Some(XboxAccount {
        gamertag: a.username,
        xuid: a.external_id.unwrap_or_default(),
        gamerpic: data["gamerpic"].as_str().map(String::from),
        gamerscore: data["gamerscore"].as_i64(),
        synced_at: a.synced_at,
        sync_error: a.sync_error,
        titles: store::title_count(conn, profile_id)?,
        needs_login: a.secret.as_deref().and_then(xbox::unprotect_token).is_none(),
    }))
}

#[tauri::command]
pub fn xbox_status(db: State<Db>, profile_id: i64) -> AppResult<XboxStatus> {
    let conn = db.conn()?;
    let saved: Option<String> = conn
        .query_row("SELECT value FROM app_settings WHERE key=?1", [auth::CLIENT_ID_SETTING], |r| r.get(0))
        .ok()
        .filter(|s: &String| !s.trim().is_empty());
    Ok(XboxStatus { account: account_view(&conn, profile_id)?, client_id: saved, embedded: auth::embedded_client_id().is_some() })
}

/// Salva (ou remove, com `None`) o Client ID do aplicativo do Azure.
#[tauri::command]
pub fn xbox_set_client_id(db: State<Db>, client_id: Option<String>) -> AppResult<()> {
    let conn = db.conn()?;
    match client_id.map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
        Some(id) => {
            if !auth::valid_client_id(&id) {
                return Err(AppError::Validation("Client ID inválido: copie o “ID do aplicativo (cliente)” da página do app no Azure (formato 00000000-0000-0000-0000-000000000000).".into()));
            }
            conn.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", [auth::CLIENT_ID_SETTING, &id])?;
        }
        None => {
            conn.execute("DELETE FROM app_settings WHERE key=?1", [auth::CLIENT_ID_SETTING])?;
        }
    }
    Ok(())
}

/// "Entrar com a conta Microsoft": página oficial no navegador, espera o retorno (até 5 min).
#[tauri::command]
pub async fn xbox_login(app: AppHandle, db: State<'_, Db>, profile_id: i64) -> AppResult<XboxAccount> {
    let client_id = auth::client_id(&*db.conn()?)
        .ok_or_else(|| AppError::Validation("Informe o Client ID do aplicativo do Azure em Configurações → Contas e plataformas → Xbox.".into()))?;
    let pending = std::sync::Arc::new(auth::start(&client_id)?);
    app.opener()
        .open_url(pending.login_url.clone(), None::<&str>)
        .map_err(|_| AppError::Other("Não foi possível abrir o navegador para o login da Microsoft.".into()))?;
    let p2 = pending.clone();
    let code = tauri::async_runtime::spawn_blocking(move || auth::wait(&p2))
        .await
        .map_err(|_| AppError::Other("Falha ao aguardar o login da Microsoft.".into()))??;
    let ms = auth::exchange(&pending, &code).await?;
    let x = auth::xsts(&ms.access).await?;
    let (gt, pic, gs) = xbox::client::profile(&x).await.unwrap_or((None, None, None));
    let blob = xbox::protect_token(&ms.refresh).ok_or_else(|| AppError::Other("Não foi possível proteger a sessão da conta Xbox neste Windows.".into()))?;
    let name = gt.unwrap_or_else(|| x.gamertag.clone());
    let data = serde_json::json!({ "gamerpic": pic, "gamerscore": gs });
    {
        let conn = db.conn()?;
        // outra conta Xbox no mesmo perfil: o cache da anterior não vale mais
        if crate::accounts::get(&conn, profile_id, store::PLATFORM)?.and_then(|a| a.external_id).is_some_and(|old| old != x.xuid) {
            store::clear_profile(&conn, profile_id)?;
        }
        crate::accounts::save(&conn, profile_id, store::PLATFORM, &name, Some(&x.xuid), Some(&blob), &data.to_string())?;
    }
    xbox::cache_session(profile_id, x);
    account_view(&*db.conn()?, profile_id)?.ok_or_else(|| AppError::Other("Falha ao salvar a conta Xbox.".into()))
}

#[tauri::command]
pub fn xbox_cancel_login() {
    auth::cancel();
}

#[tauri::command]
pub fn xbox_disconnect(db: State<Db>, profile_id: i64, clear_cache: bool) -> AppResult<()> {
    let conn = db.conn()?;
    crate::accounts::delete(&conn, profile_id, store::PLATFORM)?;
    xbox::forget_session(profile_id);
    if clear_cache {
        store::clear_profile(&conn, profile_id)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn xbox_sync(app: AppHandle, profile_id: i64, lang: String) -> AppResult<xbox::SyncReport> {
    xbox::sync_profile(&app, profile_id, &lang).await
}

/// Dados do Xbox para a página do jogo (`None` = sem conta ou jogo não encontrado no histórico do Xbox).
/// Conquistas são buscadas de novo se o cache tiver mais de 30 min (ou `refresh`); sem internet,
/// volta o cache com o erro.
#[tauri::command]
pub async fn xbox_game(app: AppHandle, db: State<'_, Db>, profile_id: i64, game_id: i64, refresh: bool, lang: String) -> AppResult<Option<store::XboxGameData>> {
    let (title_id, stale) = {
        let conn = db.conn()?;
        if crate::accounts::get(&conn, profile_id, store::PLATFORM)?.is_none() {
            return Ok(None);
        }
        let Some(t) = store::title_for_game(&conn, profile_id, game_id)? else { return Ok(None) };
        let stale = refresh || store::achievements_stale(&conn, profile_id, &t, 30)?;
        (t, stale)
    };
    let err = if stale { xbox::refresh_title(&app, profile_id, &title_id, &lang, true).await.err() } else { None };
    let conn = db.conn()?;
    Ok(store::game_data(&conn, profile_id, &title_id)?.map(|mut d| {
        d.error = err.map(|e| e.to_string());
        d
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledItem {
    pub aumid: String,
    pub name: String,
    /// Arte do histórico do Xbox (quando o jogo já foi jogado nesta conta).
    pub image: Option<String>,
    pub in_library: bool,
}

/// Família do pacote ("Nome_Editor") de um caminho `shell:AppsFolder\<AUMID>`, em minúsculas.
fn family_key(exe: &str) -> Option<String> {
    crate::msstore::aumid(exe).and_then(|a| a.split('!').next()).map(str::to_lowercase)
}

/// Famílias de pacote já presentes na biblioteca do perfil.
pub(crate) fn library_families(conn: &Connection, profile_id: i64) -> AppResult<std::collections::HashSet<String>> {
    let mut st = conn.prepare("SELECT g.executable_path FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1")?;
    let rows = st.query_map([profile_id], |r| r.get::<_, String>(0))?;
    Ok(rows.filter_map(Result::ok).filter_map(|p| family_key(&p)).collect())
}

/// Jogos do Xbox / Microsoft Store instalados neste PC (para a importação). Não precisa de conta.
#[tauri::command(async)]
pub fn xbox_installed(db: State<Db>, profile_id: i64) -> AppResult<Vec<InstalledItem>> {
    let games = crate::msstore::installed_games();
    let conn = db.conn()?;
    let have = library_families(&conn, profile_id)?;
    let mut out = Vec::new();
    for g in games {
        let image: Option<String> = conn
            .query_row("SELECT image FROM xbox_titles WHERE profile_id=?1 AND lower(pfn)=lower(?2)", rusqlite::params![profile_id, g.family], |r| r.get(0))
            .ok()
            .flatten();
        out.push(InstalledItem { in_library: have.contains(&g.family.to_lowercase()), aumid: g.aumid, name: g.name, image });
    }
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: usize,
    pub skipped: usize,
}

/// Adiciona à biblioteca os jogos escolhidos (só os que continuam instalados e ainda não estão nela).
/// Entram como "PC" iniciados pelo Windows (`shell:AppsFolder\<AUMID>`); a capa é a arte do Xbox, se houver.
#[tauri::command(async)]
pub fn xbox_import(db: State<Db>, profile_id: i64, aumids: Vec<String>) -> AppResult<ImportReport> {
    let games = crate::msstore::installed_games();
    let mut conn = db.conn()?;
    let mut have = library_families(&conn, profile_id)?;
    let tx = conn.transaction()?;
    let mut rep = ImportReport { added: 0, skipped: 0 };
    for a in aumids {
        let Some(g) = games.iter().find(|g| g.aumid.eq_ignore_ascii_case(&a)) else {
            rep.skipped += 1;
            continue;
        };
        if !have.insert(g.family.to_lowercase()) {
            rep.skipped += 1;
            continue;
        }
        let cover: Option<String> = tx
            .query_row("SELECT image FROM xbox_titles WHERE profile_id=?1 AND lower(pfn)=lower(?2)", rusqlite::params![profile_id, g.family], |r| r.get(0))
            .ok()
            .flatten();
        tx.execute(
            "INSERT INTO games(title, platform, executable_path, launch_args, cover, source) VALUES(?1, 'PC', ?2, '', ?3, 'xbox')",
            rusqlite::params![g.name.chars().take(80).collect::<String>(), format!("{}{}", crate::msstore::PREFIX, g.aumid), cover],
        )?;
        let gid = tx.last_insert_rowid();
        tx.execute("INSERT INTO profile_games(profile_id, game_id) VALUES(?1, ?2)", rusqlite::params![profile_id, gid])?;
        rep.added += 1;
    }
    tx.commit()?;
    Ok(rep)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotInstalledList {
    pub connected: bool,
    pub items: Vec<store::NotInstalled>,
    /// Títulos do cache sem os aparelhos informados (cache antigo): a próxima sincronização completa.
    pub pending: usize,
}

/// Jogos de PC já jogados nesta conta Xbox que não estão instalados neste PC (só cache local; sem rede).
/// O Xbox não informa jogos comprados e nunca abertos: só o histórico de jogos.
#[tauri::command(async)]
pub fn xbox_not_installed(db: State<Db>, profile_id: i64) -> AppResult<NotInstalledList> {
    let conn = db.conn()?;
    if crate::accounts::get(&conn, profile_id, store::PLATFORM)?.is_none() {
        return Ok(NotInstalledList { connected: false, items: vec![], pending: 0 });
    }
    let installed = crate::msstore::installed_families();
    let (items, pending) = store::not_installed(&conn, profile_id, &installed)?;
    Ok(NotInstalledList { connected: true, items, pending })
}

/// Família de pacote válida ("Nome.Do.Pacote_<13 caracteres do editor>").
fn valid_pfn(pfn: &str) -> bool {
    let Some((name, publisher)) = pfn.rsplit_once('_') else { return false };
    !name.is_empty()
        && name.len() <= 100
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && publisher.len() == 13
        && publisher.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Abre a página do jogo na Microsoft Store (protocolo oficial `ms-windows-store://pdp/?PFN=`). A instalação,
/// a licença e o Game Pass continuam com a Microsoft; o NEXSTAGE não baixa nada.
#[tauri::command]
pub fn xbox_open_store(app: AppHandle, pfn: String) -> AppResult<()> {
    let pfn = pfn.trim();
    if !valid_pfn(pfn) {
        return Err(AppError::Validation("Pacote da Microsoft Store inválido.".into()));
    }
    app.opener()
        .open_url(format!("ms-windows-store://pdp/?PFN={pfn}"), None::<&str>)
        .map_err(|_| AppError::Other("Não foi possível abrir a Microsoft Store.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_family_validation() {
        assert!(valid_pfn("Microsoft.ForteBaseGame_8wekyb3d8bbwe"));
        assert!(valid_pfn("BethesdaSoftworks.Starfield-PC_3275kfvn8vcwc"));
        for bad in ["", "SemEditor", "Nome_curto", "Nome com espaço_8wekyb3d8bbwe", "x&y_8wekyb3d8bbwe", "_8wekyb3d8bbwe", "a/b_8wekyb3d8bbwe"] {
            assert!(!valid_pfn(bad), "{bad}");
        }
    }
}
