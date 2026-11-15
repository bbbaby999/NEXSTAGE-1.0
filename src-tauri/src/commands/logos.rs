//! Logos automáticas dos jogos da biblioteca (SteamGridDB).
//!
//! Para cada jogo SEM logo, usa a primeira logo do primeiro jogo da pesquisa pelo nome — o mesmo que
//! aparece primeiro no seletor de logos. A escolha manual continua valendo e nunca é substituída:
//! - só jogos sem logo são preenchidos;
//! - cada jogo é tentado uma vez (`logo_auto`): uma logo removida pelo usuário não volta sozinha, e
//!   jogos sem resultado não são pesquisados de novo a cada ciclo (há "Tentar de novo" em Configurações);
//! - falha de rede ou limite de requisições interrompe a rodada sem marcar nada (tenta no próximo ciclo).
//! Roda em segundo plano (jogos novos ganham logo em até 1 min) e só com a chave do SteamGridDB.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::params;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS logo_auto (
    game_id   INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    found     INTEGER NOT NULL DEFAULT 0,
    tried_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

/// Chave em `app_settings`: "0" desliga o preenchimento automático (padrão: ligado).
pub const SETTING: &str = "auto_logos";

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LogoReport {
    pub filled: usize,
    pub not_found: usize,
    /// A rodada parou por falha de rede/limite: os demais ficam para depois.
    pub interrupted: bool,
}

/// Jogos do perfil ainda sem logo e nunca tentados.
fn pending(db: &Db, profile_id: i64) -> AppResult<Vec<(i64, String)>> {
    let conn = db.conn()?;
    let mut st = conn.prepare(
        "SELECT g.id, g.title FROM profile_games pg JOIN games g ON g.id=pg.game_id
         WHERE pg.profile_id=?1 AND (g.logo IS NULL OR trim(g.logo)='')
           AND NOT EXISTS (SELECT 1 FROM logo_auto l WHERE l.game_id=g.id)
         ORDER BY pg.last_played_at DESC, g.title",
    )?;
    let rows = st.query_map([profile_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Preenche as logos que faltam no perfil. Erro só sem chave do SteamGridDB ou com outra rodada em andamento.
pub async fn fill(app: &AppHandle, profile_id: i64) -> AppResult<LogoReport> {
    let key = crate::apikeys::get(app)("sgdb_key")
        .ok_or_else(|| AppError::Validation("Configure a chave do SteamGridDB em Configurações → Pesquisa de imagens para buscar logos.".into()))?;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Validation("A busca de logos já está em andamento.".into()));
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    let db = app.state::<Db>();
    let c = super::webshots::client()?;
    let mut rep = LogoReport::default();
    for (game_id, title) in pending(&db, profile_id)? {
        match super::webshots::sgdb_first_logo(&c, &key, &title).await {
            Ok(found) => {
                let conn = db.conn()?;
                if let Some(url) = &found {
                    // a escolha do usuário feita durante a busca prevalece
                    let n = conn.execute("UPDATE games SET logo=?2 WHERE id=?1 AND (logo IS NULL OR trim(logo)='')", params![game_id, url])?;
                    if n > 0 {
                        rep.filled += 1;
                    }
                } else {
                    rep.not_found += 1;
                }
                conn.execute("INSERT OR REPLACE INTO logo_auto(game_id, found) VALUES(?1, ?2)", params![game_id, found.is_some() as i64])?;
            }
            Err(()) => {
                rep.interrupted = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await; // ritmo gentil com a API
    }
    if rep.filled > 0 {
        let _ = app.emit("library-changed", serde_json::json!({ "profileId": profile_id }));
    }
    Ok(rep)
}

/// Busca agora (Configurações). `retry` = pesquisa de novo os jogos que ficaram sem resultado.
#[tauri::command]
pub async fn auto_logos(app: AppHandle, profile_id: i64, retry: bool) -> AppResult<LogoReport> {
    if retry {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        conn.execute(
            "DELETE FROM logo_auto WHERE found=0 AND game_id IN (SELECT game_id FROM profile_games WHERE profile_id=?1)",
            [profile_id],
        )?;
    }
    fill(&app, profile_id).await
}

/// Ciclo em segundo plano: a cada minuto, logos para jogos novos do perfil ativo (sem rede quando não há
/// jogo pendente). Primeira rodada 45 s após abrir o app.
pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("auto-logos".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(45));
            loop {
                let profile = app.state::<crate::overlay::Runtime>().active_profile.load(Ordering::SeqCst);
                let on = crate::overlay::setting(&app, SETTING).as_deref() != Some("0");
                let has_key = crate::apikeys::get(&app)("sgdb_key").is_some();
                let todo = profile > 0 && on && has_key && pending(&app.state::<Db>(), profile).map(|p| !p.is_empty()).unwrap_or(false);
                if todo && !RUNNING.load(Ordering::SeqCst) {
                    let _ = tauri::async_runtime::block_on(fill(&app, profile));
                }
                std::thread::sleep(Duration::from_secs(60));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_skips_games_with_logo_or_already_tried() {
        let dir = std::env::temp_dir().join(format!("nexstage-logos-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let db = Db::open(&dir).unwrap();
        {
            let c = db.conn().unwrap();
            c.execute_batch(
                "INSERT INTO profiles(id,username,display_name) VALUES(1,'ana','Ana');
                 INSERT INTO games(id,title,executable_path,logo) VALUES(1,'Sem logo','x',NULL),(2,'Com logo','y','https://cdn2.steamgriddb.com/l.png'),
                   (3,'Já tentado','z',''),(4,'Vazio','w','  ');
                 INSERT INTO profile_games(profile_id,game_id) VALUES(1,1),(1,2),(1,3),(1,4);
                 INSERT INTO logo_auto(game_id,found) VALUES(3,0);",
            )
            .unwrap();
        }
        let ids: Vec<i64> = pending(&db, 1).unwrap().into_iter().map(|g| g.0).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1) && ids.contains(&4), "sem logo (ou só espaços) e nunca tentados: {ids:?}");
        drop(db);
        let _ = std::fs::remove_file(&dir);
    }
}
