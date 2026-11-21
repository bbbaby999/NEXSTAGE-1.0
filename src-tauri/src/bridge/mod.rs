//! NexStage Game Bridge: comunicação local, autenticada e opcional com jogos feitos para o NexStage.
//! Especificação: `docs/bridge/NEXSTAGE_GAME_BRIDGE.md`.
//!
//! - `protocol`: envelope assinado (HMAC-SHA256), campos, formatos dos eventos, horário.
//! - `session`: validação e respostas de uma sessão (sem rede, testável).
//! - `server`: transporte TCP em 127.0.0.1 (uma conexão por vez, reconexão permitida).
//! - `store`: tabelas próprias (migração aditiva).
//! - `achievements`: catálogo (do manifesto assinado) e conquistas por perfil.
//! - `statistics`: estatísticas por perfil e slot (definições do manifesto assinado).
//!
//! Nada aqui conhece regras de um jogo específico: a lista de jogos oficiais vem de `integrated`.

pub mod achievements;
pub mod protocol;
pub mod server;
pub mod session;
pub mod statistics;
pub mod store;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

/// Sessões abertas: id → sinal de parada do servidor.
#[derive(Default)]
pub struct Bridge {
    sessions: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// Sessão criada para um jogo que vai ser iniciado: as variáveis vão para o processo do jogo.
pub struct Started {
    pub session: String,
    pub env: Vec<(String, String)>,
}

/// Abre uma sessão (servidor local numa porta efêmera) para `game_id`, presa ao perfil `profile_id`.
pub fn start(app: &AppHandle, game_id: &str, profile_id: i64) -> AppResult<Started> {
    if !crate::integrated::is_official(game_id) {
        return Err(AppError::Validation("Jogo não integrado ao NexStage.".into()));
    }
    let (profile_key, profile_name) = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        let name: String = conn
            .query_row("SELECT display_name FROM profiles WHERE id=?1", [profile_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| AppError::NotFound("Perfil não encontrado.".into()))?;
        (store::profile_key(&conn, profile_id)?, name)
    };
    let session = protocol::uuid_v4();
    let key = protocol::random_key();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        store::session_open(&conn, &session, game_id, profile_id)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut m) = app.state::<Bridge>().sessions.lock() {
        m.insert(session.clone(), stop.clone());
    }
    let core = session::Core::new(session.clone(), game_id.to_string(), profile_id, profile_key.clone(), key.clone());
    tauri::async_runtime::spawn(server::run(app.clone(), listener, core, stop));
    use base64::Engine;
    let env = vec![
        ("NEXSTAGE_BRIDGE_PROTO".into(), protocol::PROTOCOL.to_string()),
        ("NEXSTAGE_BRIDGE_PORT".into(), port.to_string()),
        ("NEXSTAGE_BRIDGE_GAME".into(), game_id.to_string()),
        ("NEXSTAGE_BRIDGE_SESSION".into(), session.clone()),
        ("NEXSTAGE_BRIDGE_KEY".into(), base64::engine::general_purpose::STANDARD.encode(&key)),
        ("NEXSTAGE_BRIDGE_PROFILE".into(), profile_key),
        ("NEXSTAGE_BRIDGE_PROFILE_NAME".into(), profile_name),
    ];
    Ok(Started { session, env })
}

/// Encerra a sessão (o processo do jogo terminou ou não chegou a iniciar).
pub fn end(app: &AppHandle, session: &str) {
    if let Ok(mut m) = app.state::<Bridge>().sessions.lock() {
        if let Some(stop) = m.remove(session) {
            stop.store(true, Ordering::SeqCst);
        }
    }
    if let Ok(conn) = app.state::<Db>().conn() {
        let _ = store::session_end(&conn, session, "ended");
    }
}

/// Se o jogo da biblioteca é um jogo integrado oficial, abre a sessão do Bridge para ele.
/// Qualquer falha aqui só é registrada: o jogo abre normalmente (independência).
pub fn for_game(app: &AppHandle, profile_id: i64, game_id: i64) -> Option<Started> {
    let ext: Option<String> = {
        let db = app.state::<Db>();
        let conn = db.conn().ok()?;
        conn.query_row("SELECT external_id FROM games WHERE id=?1 AND source='nexstage'", params![game_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    };
    let ext = ext.filter(|e| crate::integrated::is_official(e))?;
    match start(app, &ext, profile_id) {
        Ok(s) => Some(s),
        Err(e) => {
            crate::diaglog::write("erro", "bridge", &format!("sessão não iniciada para {ext}: {e}"));
            None
        }
    }
}
