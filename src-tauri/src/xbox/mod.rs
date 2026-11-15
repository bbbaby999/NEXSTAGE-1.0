//! Integração com o Xbox (conta Microsoft): conquistas, gamerscore e tempo de jogo registrado pelo Xbox.
//!
//! Princípios (como na Steam):
//! - login só na página oficial da Microsoft (OAuth + PKCE): o NEXSTAGE nunca vê a senha;
//! - sem segredo embutido: o app do Azure é público; só o refresh token do usuário é guardado,
//!   cifrado pela DPAPI, e pode ser apagado em "Desconectar";
//! - somente leitura: nada é publicado na conta;
//! - jogos são iniciados pelo Windows (`msstore`); nenhum acesso a memória ou arquivos dos jogos.
//!
//! - `auth`: login e tokens · `client`: serviços do Xbox Live · `store`: cache e ligação com a biblioteca
//! - `watch`: aviso de conquista quase em tempo real · `spawn`: sincronização automática

pub mod auth;
pub mod client;
pub mod store;
pub mod watch;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use auth::Xsts;
use crate::retro::util::{now_epoch, parse_epoch};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Uma sincronização por vez (manual ou automática).
static SYNCING: AtomicBool = AtomicBool::new(false);

const KEY_ENTROPY: &[u8] = b"NEXSTAGE/XboxRefresh/v1";

pub fn protect_token(t: &str) -> Option<Vec<u8>> {
    crate::vault::protect(KEY_ENTROPY, t)
}
pub fn unprotect_token(b: &[u8]) -> Option<String> {
    crate::vault::unprotect(KEY_ENTROPY, b)
}

/// Tokens XSTS na memória, por perfil (nunca gravados).
static SESSIONS: Mutex<Option<HashMap<i64, Xsts>>> = Mutex::new(None);

pub fn cache_session(profile_id: i64, x: Xsts) {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(profile_id, x);
}
pub fn forget_session(profile_id: i64) {
    if let Some(m) = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        m.remove(&profile_id);
    }
}
fn cached(profile_id: i64) -> Option<Xsts> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&profile_id)).filter(|x| x.valid()).cloned()
}

/// Sessão do Xbox Live do perfil: da memória ou renovada pelo refresh token guardado (que é trocado
/// pelo novo devolvido pela Microsoft).
pub async fn session(app: &AppHandle, profile_id: i64) -> AppResult<Xsts> {
    if let Some(x) = cached(profile_id) {
        return Ok(x);
    }
    let (client_id, token) = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        let acc = crate::accounts::get(&conn, profile_id, store::PLATFORM)?
            .ok_or_else(|| AppError::Validation("Conecte a conta Xbox em Configurações → Contas e plataformas.".into()))?;
        let token = acc.secret.as_deref().and_then(unprotect_token).ok_or_else(|| {
            AppError::Validation("A sessão da conta Xbox expirou ou foi revogada. Entre novamente em Configurações → Contas e plataformas.".into())
        })?;
        let cid = auth::client_id(&conn).ok_or_else(|| AppError::Validation("Informe o Client ID do aplicativo do Azure em Configurações → Contas e plataformas → Xbox.".into()))?;
        (cid, token)
    };
    let ms = auth::refresh(&client_id, &token).await?;
    {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        if let Some(b) = protect_token(&ms.refresh) {
            crate::accounts::set_secret(&conn, profile_id, store::PLATFORM, Some(&b))?;
        }
    }
    let x = auth::xsts(&ms.access).await?;
    cache_session(profile_id, x.clone());
    Ok(x)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub titles: usize,
}

/// Atualiza perfil (gamerscore, imagem), histórico de jogos e minutos jogados. Erros ficam gravados na conta.
pub async fn sync_profile(app: &AppHandle, profile_id: i64, lang: &str) -> AppResult<SyncReport> {
    if SYNCING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Validation("Uma sincronização do Xbox já está em andamento.".into()));
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            SYNCING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    let r = sync_inner(app, profile_id, lang).await;
    {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        match &r {
            Ok(_) => crate::accounts::set_sync_result(&conn, profile_id, store::PLATFORM, Some(&now_text(&conn)), None)?,
            Err(e) => crate::accounts::set_sync_result(&conn, profile_id, store::PLATFORM, None, Some(&e.to_string()))?,
        }
    }
    // telas abertas (Configurações, página do jogo) recarregam do cache
    let _ = app.emit("xbox-synced", serde_json::json!({ "profileId": profile_id }));
    r
}

fn now_text(conn: &rusqlite::Connection) -> String {
    conn.query_row("SELECT datetime('now')", [], |r| r.get(0)).unwrap_or_default()
}

async fn sync_inner(app: &AppHandle, profile_id: i64, lang: &str) -> AppResult<SyncReport> {
    let x = session(app, profile_id).await?;
    let lang = client::xbox_lang(lang);
    let (gt, pic, gs) = client::profile(&x).await.unwrap_or((None, None, None));
    let titles = client::title_history(&x, lang).await?;
    let ids: Vec<String> = titles.iter().filter_map(|t| t["titleId"].as_str().map(String::from)).collect();
    let minutes = client::minutes_played(&x, &ids).await.unwrap_or_default();
    let db = app.state::<Db>();
    let conn = db.conn()?;
    let n = store::store_titles(&conn, profile_id, &titles, &minutes)?;
    let data = serde_json::json!({ "gamerpic": pic, "gamerscore": gs });
    let name = gt.unwrap_or_else(|| x.gamertag.clone());
    crate::accounts::set_data(&conn, profile_id, store::PLATFORM, Some(&name).filter(|n| !n.is_empty()).map(String::as_str), &data.to_string())?;
    Ok(SyncReport { titles: n })
}

/// Busca de novo as conquistas de um jogo (e, com `with_minutes`, os minutos jogados no Xbox).
pub async fn refresh_title(app: &AppHandle, profile_id: i64, title_id: &str, lang: &str, with_minutes: bool) -> AppResult<()> {
    let x = session(app, profile_id).await?;
    let list = client::achievements(&x, title_id, client::xbox_lang(lang)).await?;
    let minutes = if with_minutes { client::minutes_played(&x, &[title_id.to_string()]).await.unwrap_or_default() } else { HashMap::new() };
    let db = app.state::<Db>();
    let conn = db.conn()?;
    store::store_achievements(&conn, profile_id, title_id, &list)?;
    if let Some(m) = minutes.get(title_id) {
        store::set_minutes(&conn, profile_id, title_id, *m)?;
    }
    Ok(())
}

/// Sincronização automática em segundo plano (padrão: a cada 60 min; nunca na abertura do app).
/// Só com conta Xbox no perfil ativo; falhas seguidas espaçam as tentativas.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("xbox-sync".into())
        .spawn(move || {
            let start = std::time::Instant::now();
            let mut last: (i64, i64) = (0, 0); // (perfil, epoch da última tentativa)
            let mut failures = 0u32;
            loop {
                std::thread::sleep(Duration::from_secs(30));
                if start.elapsed() < Duration::from_secs(120) {
                    continue;
                }
                let profile = app.state::<crate::overlay::Runtime>().active_profile.load(Ordering::SeqCst);
                if profile <= 0 || SYNCING.load(Ordering::SeqCst) {
                    continue;
                }
                let minutes = crate::overlay::setting(&app, "xbox_sync_minutes").and_then(|v| v.parse::<i64>().ok()).unwrap_or(60);
                if minutes <= 0 {
                    continue;
                }
                let synced = {
                    let db = app.state::<Db>();
                    let Ok(conn) = db.conn() else { continue };
                    crate::accounts::get(&conn, profile, store::PLATFORM).ok().flatten().map(|a| a.synced_at.and_then(|s| parse_epoch(&s)).unwrap_or(0))
                };
                let Some(synced) = synced else { continue }; // sem conta Xbox neste perfil
                if last.0 != profile {
                    last = (profile, synced);
                    failures = 0;
                }
                let backoff = [0, 10, 30, 60][failures.min(3) as usize] * 60;
                let now = now_epoch();
                if now - last.1.max(synced) < (minutes * 60).max(backoff) {
                    continue;
                }
                last.1 = now;
                let lang = crate::overlay::setting(&app, "language").unwrap_or_else(|| "pt-BR".into());
                match tauri::async_runtime::block_on(sync_profile(&app, profile, &lang)) {
                    Ok(_) => failures = 0,
                    Err(_) => failures += 1,
                }
            }
        })
        .ok();
}
