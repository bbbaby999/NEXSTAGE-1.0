//! Transporte do Bridge: TCP só em 127.0.0.1, uma mensagem por linha. Aceita uma conexão por vez
//! (reconexões do mesmo jogo são permitidas enquanto a sessão existir) e repassa cada linha à `Core`.

use super::protocol::{self as proto, MAX_LINE};
use super::session::{Core, Out};
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Sem nenhuma mensagem por este tempo, a conexão é dada como perdida (o jogo reconecta).
pub const IDLE_SECS: u64 = 45;

/// De onde vem a conexão com o banco (o app real usa o `Db` do Tauri; os testes, um banco em memória).
pub trait ConnSource: Send + Sync + 'static {
    fn with_conn(&self, f: &mut dyn FnMut(&Connection));
    /// Conquistas desbloqueadas pela primeira vez neste perfil (o app real mostra os cartões por cima do jogo).
    fn unlocked(&self, _game: &str, _ids: &[String]) {}
}

impl ConnSource for tauri::AppHandle {
    fn with_conn(&self, f: &mut dyn FnMut(&Connection)) {
        use tauri::Manager;
        if let Ok(c) = self.state::<crate::db::Db>().conn() {
            f(&c);
        }
    }
    fn unlocked(&self, game: &str, ids: &[String]) {
        let lang = crate::overlay::setting(self, "language").unwrap_or_default();
        let name = crate::integrated::official(game).map(|g| g.name).unwrap_or(game);
        let mut cards = Vec::new();
        self.with_conn(&mut |c| cards = super::achievements::toasts(c, game, name, ids, &lang));
        let more = cards.last().map(|c| c.more).unwrap_or(0);
        crate::shot_toast::notify_achievements(self, cards, more);
    }
}

impl ConnSource for Arc<std::sync::Mutex<Connection>> {
    fn with_conn(&self, f: &mut dyn FnMut(&Connection)) {
        let c = self.lock().unwrap_or_else(|e| e.into_inner());
        f(&c);
    }
}

pub async fn run<S: ConnSource>(src: S, listener: std::net::TcpListener, mut core: Core, stop: Arc<AtomicBool>) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
    while !stop.load(Ordering::SeqCst) {
        let accepted = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await;
        let Ok(Ok((stream, peer))) = accepted else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        core.reset_connection();
        connection(&src, stream, &mut core, &stop).await;
        src.with_conn(&mut |c| {
            let _ = super::store::session_set(c, &core.session, "status", "lost");
        });
    }
}

async fn connection<S: ConnSource>(src: &S, stream: tokio::net::TcpStream, core: &mut Core, stop: &AtomicBool) {
    let _ = stream.set_nodelay(true);
    let (r, mut w) = stream.into_split();
    let mut reader = BufReader::new(r);
    let mut buf = Vec::with_capacity(1024);
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        buf.clear();
        let mut limited = (&mut reader).take((MAX_LINE + 2) as u64);
        match tokio::time::timeout(Duration::from_secs(IDLE_SECS), limited.read_until(b'\n', &mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return, // fechou, erro ou ficou mudo
            Ok(Ok(_)) => {}
        }
        if buf.len() > MAX_LINE + 1 {
            return; // linha grande demais (E_SIZE)
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut outs = Vec::new();
        src.with_conn(&mut |c| outs = core.handle(c, line, proto::now_secs()));
        if !core.fresh_unlocks.is_empty() {
            let fresh = std::mem::take(&mut core.fresh_unlocks);
            src.unlocked(&core.game, &fresh);
        }
        for o in outs {
            match o {
                Out::Line(l) => {
                    if w.write_all(l.as_bytes()).await.is_err() {
                        return;
                    }
                }
                Out::Close => {
                    let _ = w.shutdown().await;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    fn db() -> Arc<std::sync::Mutex<Connection>> {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE profiles(id INTEGER PRIMARY KEY); INSERT INTO profiles(id) VALUES(1);
                         CREATE TABLE profile_settings(profile_id INTEGER, key TEXT, value TEXT, PRIMARY KEY(profile_id,key));
                         CREATE TABLE games(id INTEGER PRIMARY KEY);").unwrap();
        c.execute_batch(super::super::store::SCHEMA).unwrap();
        super::super::store::session_open(&c, "sess-tcp-1", "tomb-survivors", 1).unwrap();
        Arc::new(std::sync::Mutex::new(c))
    }

    fn start(db: Arc<std::sync::Mutex<Connection>>) -> (u16, Vec<u8>, Arc<AtomicBool>) {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let key = proto::random_key();
        let stop = Arc::new(AtomicBool::new(false));
        let core = Core::new("sess-tcp-1".into(), "tomb-survivors".into(), 1, "PERFIL-A".into(), key.clone());
        tokio::spawn(run(db, l, core, stop.clone()));
        (port, key, stop)
    }

    fn msg(key: &[u8], seq: i64, kind: &str, data: Value) -> String {
        let p = json!({"v":1,"id":proto::uuid_v4(),"seq":seq,"ts":proto::ts_from_secs(proto::now_secs()),"game":"tomb-survivors","session":"sess-tcp-1","type":kind,"data":data});
        proto::envelope(key, &p)
    }

    #[tokio::test]
    async fn tcp_session_handshake_reconnect_and_status() {
        let db = db();
        let (port, key, stop) = start(db.clone());
        // 1ª conexão
        let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, mut w) = s.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        w.write_all(msg(&key, 1, "hello", json!({"game_version":"0.1.0","protocol":[1]})).as_bytes()).await.unwrap();
        let l = lines.next_line().await.unwrap().unwrap();
        assert_eq!(proto::verify_envelope(&l, &key).unwrap()["type"], "launcher.welcome");
        drop(w);
        drop(lines);
        // reconexão: nova conexão, sequência continua
        tokio::time::sleep(Duration::from_millis(200)).await;
        let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, mut w) = s.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        w.write_all(msg(&key, 2, "hello", json!({"game_version":"0.1.0","protocol":[1]})).as_bytes()).await.unwrap();
        assert_eq!(proto::verify_envelope(&lines.next_line().await.unwrap().unwrap(), &key).unwrap()["type"], "launcher.welcome");
        w.write_all(msg(&key, 3, "game.closed", json!({"reason":"quit"})).as_bytes()).await.unwrap();
        assert_eq!(proto::verify_envelope(&lines.next_line().await.unwrap().unwrap(), &key).unwrap()["data"]["status"], "accepted");
        // assinatura errada derruba a conexão
        w.write_all(msg(&proto::random_key(), 4, "game.heartbeat", json!({"state":"menu"})).as_bytes()).await.unwrap();
        assert!(lines.next_line().await.unwrap().is_none(), "conexão encerrada");
        stop.store(true, Ordering::SeqCst);
        let (status, seq): (String, i64) = db.lock().unwrap().query_row("SELECT status,last_seq FROM bridge_sessions WHERE session_id='sess-tcp-1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(seq, 3);
        assert!(status == "closed" || status == "lost", "status: {status}");
    }

/// Fonte de teste que grava os avisos de conquista.
    struct Recorder {
        db: Arc<std::sync::Mutex<Connection>>,
        seen: Arc<std::sync::Mutex<Vec<String>>>,
    }
    impl ConnSource for Recorder {
        fn with_conn(&self, f: &mut dyn FnMut(&Connection)) {
            self.db.with_conn(f)
        }
        fn unlocked(&self, game: &str, ids: &[String]) {
            self.seen.lock().unwrap().extend(ids.iter().map(|i| format!("{game}:{i}")));
        }
    }

    #[tokio::test]
    async fn new_unlock_triggers_notification_once() {
        let db = db();
        super::super::achievements::save_catalog(&mut db.lock().unwrap(), "tomb-survivors", &super::super::achievements::tests::catalog()).unwrap();
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        let key = proto::random_key();
        let stop = Arc::new(AtomicBool::new(false));
        let core = Core::new("sess-tcp-1".into(), "tomb-survivors".into(), 1, "PERFIL-A".into(), key.clone());
        tokio::spawn(run(Recorder { db: db.clone(), seen: seen.clone() }, l, core, stop.clone()));
        let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (r, mut w) = s.into_split();
        let mut lines = tokio::io::BufReader::new(r).lines();
        let send = |seq: i64, kind: &str, d: Value| msg(&key, seq, kind, d);
        let steps = [
            send(1, "hello", json!({"game_version":"0.3.0","protocol":[1]})),
            send(2, "game.started", json!({"mode":"classic","slot_uid":"slot-1234","owner_profile":"PERFIL-A"})),
            send(3, "achievement.unlocked", json!({"achievement_id":"first_blood","slot_uid":"slot-1234"})),
            send(4, "achievement.unlocked", json!({"achievement_id":"first_blood","slot_uid":"slot-1234"})),
            send(5, "achievement.progress.updated", json!({"achievement_id":"relic_hunter","current":3,"target":50,"slot_uid":"slot-1234"})),
        ];
        for m in steps {
            w.write_all(m.as_bytes()).await.unwrap();
            lines.next_line().await.unwrap().unwrap();
        }
        stop.store(true, Ordering::SeqCst);
        assert_eq!(*seen.lock().unwrap(), vec!["tomb-survivors:first_blood".to_string()], "um aviso só, e progresso não avisa");
    }

    /// Teste de ponta a ponta com o jogo real (Godot). Rode com:
    ///   set NEXSTAGE_E2E_GODOT=<caminho do Godot console .exe>
    ///   set NEXSTAGE_E2E_PROJECT=<pasta do projeto Tomb Survivors>
    ///   cargo test --lib bridge::server::tests::e2e_game -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn e2e_game() {
        let godot = std::env::var("NEXSTAGE_E2E_GODOT").expect("NEXSTAGE_E2E_GODOT");
        let project = std::env::var("NEXSTAGE_E2E_PROJECT").expect("NEXSTAGE_E2E_PROJECT");
        let db = db();
        // catálogo como o do manifesto assinado (data/achievements.json do jogo)
        let cat: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(&project).join("data/achievements.json")).unwrap()).unwrap();
        super::super::achievements::save_catalog(&mut db.lock().unwrap(), "tomb-survivors", &cat).unwrap();
        let stats: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(&project).join("data/statistics.json")).unwrap()).unwrap();
        super::super::statistics::save_catalog(&mut db.lock().unwrap(), "tomb-survivors", &stats).unwrap();
        let (port, key, stop) = start(db.clone());
        use base64::Engine;
        let out = tokio::process::Command::new(godot)
            .args(["--headless", "--path", &project, "--", "--nometa", "--bridge-selftest", "--quit-at=20"])
            .env("NEXSTAGE_BRIDGE_PROTO", "1")
            .env("NEXSTAGE_BRIDGE_PORT", port.to_string())
            .env("NEXSTAGE_BRIDGE_GAME", "tomb-survivors")
            .env("NEXSTAGE_BRIDGE_SESSION", "sess-tcp-1")
            .env("NEXSTAGE_BRIDGE_KEY", base64::engine::general_purpose::STANDARD.encode(&key))
            .env("NEXSTAGE_BRIDGE_PROFILE", "PERFIL-A")
            .env("NEXSTAGE_BRIDGE_PROFILE_NAME", "Teste")
            .output()
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        println!("{text}");
        stop.store(true, Ordering::SeqCst);
        assert!(text.contains("[bridge-selftest] welcome"), "o jogo recebeu o welcome");
        assert!(text.contains("ack accepted"), "game.started/heartbeat aceitos");
        assert!(text.contains("ack rejected E_CATALOG"), "conquista fora do catálogo recusada");
        let unlocked: Vec<String> = db.lock().unwrap().prepare("SELECT achievement_id FROM bridge_unlocks WHERE profile_id=1").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(unlocked, vec!["first_blood".to_string()], "só a conquista do catálogo foi registrada");
        let p = super::super::statistics::profile(&db.lock().unwrap(), 1, "tomb-survivors").unwrap();
        assert!(p.stats.iter().any(|s| s.key == "progress_percent"), "progresso do jogo registrado");
        assert!(p.stats.iter().any(|s| s.key == "runs"), "estatísticas do jogo registradas: {:?}", p.stats);
        let (status, slot, ver): (String, Option<String>, Option<String>) = db.lock().unwrap()
            .query_row("SELECT status,slot_uid,game_version FROM bridge_sessions WHERE session_id='sess-tcp-1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(slot.as_deref(), Some("selftest-slot"));
        assert!(ver.is_some_and(|v| !v.is_empty()));
        assert!(status == "closed" || status == "lost", "status: {status}");
    }
}
