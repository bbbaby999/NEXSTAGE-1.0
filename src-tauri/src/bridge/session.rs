//! Lógica de uma sessão do Bridge, sem rede (testável): recebe uma linha, valida na ordem da especificação
//! (§6) e devolve as respostas assinadas. O servidor (`server.rs`) só cuida do transporte.

use super::achievements::{self, Applied};
use super::statistics;
use super::protocol::{self as proto, Code, Msg};
use super::store;
use rusqlite::Connection;
use serde_json::{json, Value};

pub enum Out {
    Line(String),
    Close,
}

pub struct Core {
    pub session: String,
    pub game: String,
    pub profile_id: i64,
    profile_key: String,
    key: Vec<u8>,
    last_seq: i64,
    out_seq: i64,
    /// `hello` aceito NESTA conexão (cada reconexão começa com um novo `hello`)
    greeted: bool,
    slot_uid: Option<String>,
    slot_ok: bool,
    rate_window: i64,
    rate_count: u32,
    /// Conquistas desbloqueadas pela primeira vez nesta sessão (o launcher avisa — Fase 7).
    pub fresh_unlocks: Vec<String>,
}

const RATE_PER_SEC: u32 = 50;

impl Core {
    pub fn new(session: String, game: String, profile_id: i64, profile_key: String, key: Vec<u8>) -> Self {
        Self { session, game, profile_id, profile_key, key, last_seq: 0, out_seq: 0, greeted: false, slot_uid: None, slot_ok: false, rate_window: 0, rate_count: 0, fresh_unlocks: Vec::new() }
    }

    /// Nova conexão (reconexão do jogo): exige `hello` de novo; a sequência continua a mesma.
    pub fn reset_connection(&mut self) {
        self.greeted = false;
        self.slot_uid = None;
        self.slot_ok = false;
    }

    fn reply(&mut self, kind: &str, data: Value) -> Out {
        self.out_seq += 1;
        let p = json!({
            "v": proto::PROTOCOL, "id": proto::uuid_v4(), "seq": self.out_seq, "ts": proto::ts_from_secs(proto::now_secs()),
            "game": self.game, "session": self.session, "type": kind, "data": data,
        });
        Out::Line(proto::envelope(&self.key, &p))
    }

    fn ack(&mut self, id: &str, status: &str, code: Option<Code>) -> Out {
        let mut d = json!({ "ack_id": id, "status": status });
        if let Some(c) = code {
            d["code"] = json!(c.as_str());
        }
        self.reply("launcher.ack", d)
    }

    fn fail(&mut self, code: Code, id: Option<&str>) -> Vec<Out> {
        crate::diaglog::write("aviso", "bridge", &format!("{} jogo={} sessão={}", code.as_str(), self.game, self.session));
        if code.fatal() {
            return vec![Out::Close];
        }
        match id {
            Some(i) => vec![self.ack(i, "rejected", Some(code))],
            None => vec![],
        }
    }

    pub fn handle(&mut self, conn: &Connection, line: &str, now: i64) -> Vec<Out> {
        // 1-2. tamanho, JSON e assinatura
        let p = match proto::verify_envelope(line, &self.key) {
            Ok(p) => p,
            Err(c) => return self.fail(c, None),
        };
        // 3. versão e campos do envelope
        let m = match proto::parse_msg(&p) {
            Ok(m) => m,
            Err(Code::ProtoVersion) => {
                return vec![self.reply("launcher.reject", json!({"code": Code::ProtoVersion.as_str(), "message": "versão de protocolo não suportada"})), Out::Close]
            }
            Err(c) => {
                let id = p.get("id").and_then(Value::as_str).map(str::to_string);
                return self.fail(c, id.as_deref());
            }
        };
        // 4. jogo e sessão
        if m.game != self.game || m.session != self.session {
            return self.fail(Code::Session, None);
        }
        // 5. sequência estritamente crescente (protege contra repetição)
        if m.seq <= self.last_seq {
            return self.fail(Code::Replay, Some(&m.id));
        }
        self.last_seq = m.seq;
        let _ = store::session_seq(conn, &self.session, m.seq);
        // 10. ritmo
        if now != self.rate_window {
            self.rate_window = now;
            self.rate_count = 0;
        }
        self.rate_count += 1;
        if self.rate_count > RATE_PER_SEC {
            return self.fail(Code::Rate, Some(&m.id));
        }
        // 7. horário dentro da janela (mensagens ao vivo)
        match proto::parse_ts(&m.ts) {
            Some(t) if (t - now).abs() <= proto::TS_WINDOW_SECS => {}
            _ => return self.fail(Code::Replay, Some(&m.id)),
        }
        // antes do hello, nada é aceito
        if !self.greeted && m.kind != "hello" {
            return self.fail(Code::Session, None);
        }
        // 8. formato do conteúdo
        match proto::check_data(&m.kind, &m.data) {
            None => return vec![self.ack(&m.id, "unsupported", None)],
            Some(Err(c)) => return self.fail(c, Some(&m.id)),
            Some(Ok(())) => {}
        }
        self.dispatch(conn, &m)
    }

    fn dispatch(&mut self, conn: &Connection, m: &Msg) -> Vec<Out> {
        match m.kind.as_str() {
            "hello" => {
                let ok = m.data["protocol"].as_array().is_some_and(|a| a.iter().any(|v| v.as_i64() == Some(proto::PROTOCOL)));
                if !ok {
                    return vec![self.reply("launcher.reject", json!({"code": Code::ProtoVersion.as_str(), "message": "versão de protocolo não suportada"})), Out::Close];
                }
                self.greeted = true;
                if let Some(v) = m.data["game_version"].as_str() {
                    let _ = store::session_set(conn, &self.session, "game_version", v);
                }
                let _ = store::session_set(conn, &self.session, "status", "connected");
                let bound = m.data["slot"]["owner_profile"].as_str() == Some(self.profile_key.as_str());
                vec![self.reply("launcher.welcome", json!({ "protocol": proto::PROTOCOL, "catalog_version": achievements::catalog_version(conn, &self.game), "profile_bound": bound, "server_ts": proto::ts_from_secs(proto::now_secs()) }))]
            }
            "game.started" => {
                let slot = m.data["slot_uid"].as_str().unwrap_or_default().to_string();
                let owner = m.data["owner_profile"].as_str().unwrap_or_default();
                if !owner.is_empty() && owner != self.profile_key {
                    // slot de outro perfil: o jogo pode ser jogado, mas nada é sincronizado
                    self.slot_uid = Some(slot);
                    self.slot_ok = false;
                    return self.fail(Code::ProfileMismatch, Some(&m.id));
                }
                let _ = store::session_set(conn, &self.session, "slot_uid", &slot);
                let number = m.data["slot_number"].as_i64().filter(|n| (1..=99).contains(n));
                let _ = store::slot_seen(conn, self.profile_id, &self.game, &slot, number);
                self.slot_uid = Some(slot);
                self.slot_ok = true;
                vec![self.ack(&m.id, "accepted", None)]
            }
            "game.heartbeat" => vec![self.ack(&m.id, "accepted", None)],
            "game.closed" => {
                let _ = store::session_set(conn, &self.session, "status", "closed");
                vec![self.ack(&m.id, "accepted", None)]
            }
            "game.version.changed" => {
                if let Some(v) = m.data["to"].as_str() {
                    let _ = store::session_set(conn, &self.session, "game_version", v);
                }
                crate::diaglog::write("info", "bridge", &format!("{} atualizado: {} → {}", self.game, m.data["from"], m.data["to"]));
                vec![self.ack(&m.id, "accepted", None)]
            }
            "outbox.flush" => self.flush(conn, m),
            "achievement.unlocked" | "achievement.progress.updated" | "statistics.updated" | "game.progress.updated" => {
                let origin = if m.origin == "offline" { "offline" } else { "live" };
                vec![self.slot_event(conn, &m.kind, &m.id, &m.data, origin)]
            }
            // reconhecidos, mas aplicados em fases futuras: o jogo os mantém guardados
            _ => vec![self.ack(&m.id, "unsupported", None)],
        }
    }

    /// Evento de um slot (conquista, estatística, progresso), ao vivo ou pendente: só para o slot liberado nesta
    /// conexão e conferido contra o catálogo do manifesto assinado. Sem catálogo ainda, responde `unsupported`
    /// e o jogo guarda o evento para depois.
    fn slot_event(&mut self, conn: &Connection, kind: &str, id: &str, data: &Value, origin: &str) -> Out {
        let slot = data.get("slot_uid").and_then(Value::as_str).map(str::to_string).or_else(|| self.slot_uid.clone());
        if !self.slot_ok || slot != self.slot_uid {
            return self.ack(id, "rejected", Some(Code::ProfileMismatch));
        }
        let has_catalog = match kind {
            "statistics.updated" => statistics::catalog_version(conn, &self.game) > 0,
            "game.progress.updated" => true,
            _ => achievements::catalog_version(conn, &self.game) > 0,
        };
        if !has_catalog {
            return self.ack(id, "unsupported", None);
        }
        let slot = slot.unwrap_or_default();
        if store::was_seen(conn, &self.game, id).unwrap_or(false) {
            return self.ack(id, "duplicate", None);
        }
        let r = match kind {
            "achievement.unlocked" => achievements::apply_unlock(conn, self.profile_id, &self.game, data, origin, id, proto::now_secs()),
            "achievement.progress.updated" => achievements::apply_progress(conn, self.profile_id, &self.game, data),
            "statistics.updated" => statistics::apply(conn, self.profile_id, &self.game, &slot, &data["values"]).map(|_| Applied::Already),
            _ => statistics::apply_progress(conn, self.profile_id, &self.game, &slot, data["percent"].as_f64().unwrap_or(0.0)).map(|_| Applied::Already),
        };
        let _ = store::mark_seen(conn, &self.game, id);
        match r {
            Ok(a) => {
                if a == Applied::New && kind == "achievement.unlocked" {
                    let ach = data["achievement_id"].as_str().unwrap_or_default().to_string();
                    crate::diaglog::write("info", "bridge", &format!("conquista {ach} desbloqueada ({origin}) jogo={}", self.game));
                    self.fresh_unlocks.push(ach);
                }
                self.ack(id, "accepted", None)
            }
            Err(c) => self.fail(c, Some(id)).pop().unwrap_or(Out::Close),
        }
    }

    /// Eventos pendentes da caixa de saída: cada um recebe a própria confirmação.
    fn flush(&mut self, conn: &Connection, m: &Msg) -> Vec<Out> {
        let events = m.data["events"].as_array().cloned().unwrap_or_default();
        let mut out = vec![self.ack(&m.id, "accepted", None)];
        for ev in events {
            let Some(id) = ev.get("id").and_then(Value::as_str).filter(|i| proto::valid_id(i)).map(str::to_string) else { continue };
            let kind = ev.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
            let data = ev.get("data").cloned().unwrap_or(Value::Null);
            if ev.get("game").and_then(Value::as_str) != Some(self.game.as_str()) || !data.is_object() {
                out.push(self.ack(&id, "rejected", Some(Code::Schema)));
                continue;
            }
            // pendentes só valem para o slot liberado nesta conexão
            let ev_slot = ev.get("slot_uid").and_then(Value::as_str);
            if !self.slot_ok || ev_slot != self.slot_uid.as_deref() {
                out.push(self.ack(&id, "rejected", Some(Code::ProfileMismatch)));
                continue;
            }
            if store::was_seen(conn, &self.game, &id).unwrap_or(false) {
                out.push(self.ack(&id, "duplicate", None));
                continue;
            }
            match proto::check_data(&kind, &data) {
                None => out.push(self.ack(&id, "unsupported", None)),
                Some(Err(c)) => out.push(self.ack(&id, "rejected", Some(c))),
                Some(Ok(())) if !proto::applied(&kind) || kind == "outbox.flush" => out.push(self.ack(&id, "unsupported", None)),
                Some(Ok(())) if matches!(kind.as_str(), "achievement.unlocked" | "achievement.progress.updated" | "statistics.updated" | "game.progress.updated") => {
                    let o = self.slot_event(conn, &kind, &id, &data, "offline");
                    out.push(o);
                }
                Some(Ok(())) => {
                    let _ = store::mark_seen(conn, &self.game, &id);
                    out.push(self.ack(&id, "accepted", None));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE profiles(id INTEGER PRIMARY KEY); INSERT INTO profiles(id) VALUES(1);
                         CREATE TABLE profile_settings(profile_id INTEGER, key TEXT, value TEXT, PRIMARY KEY(profile_id,key));
                         CREATE TABLE games(id INTEGER PRIMARY KEY);").unwrap();
        c.execute_batch(store::SCHEMA).unwrap();
        store::session_open(&c, "sess-0001", "tomb-survivors", 1).unwrap();
        c
    }

    struct Client {
        key: Vec<u8>,
        seq: i64,
    }
    impl Client {
        fn line(&mut self, kind: &str, data: Value) -> (String, String) {
            self.seq += 1;
            let id = proto::uuid_v4();
            let p = json!({"v":1,"id":id,"seq":self.seq,"ts":proto::ts_from_secs(proto::now_secs()),"game":"tomb-survivors","session":"sess-0001","type":kind,"data":data});
            (proto::envelope(&self.key, &p).trim_end().to_string(), id)
        }
    }

    fn replies(key: &[u8], out: &[Out]) -> Vec<Value> {
        out.iter()
            .filter_map(|o| match o {
                Out::Line(l) => Some(proto::verify_envelope(l.trim_end(), key).unwrap()),
                Out::Close => None,
            })
            .collect()
    }

    fn setup() -> (Connection, Core, Client) {
        let key = proto::random_key();
        let core = Core::new("sess-0001".into(), "tomb-survivors".into(), 1, "PERFIL-A".into(), key.clone());
        (db(), core, Client { key, seq: 0 })
    }

    fn hello(c: &mut Client) -> String {
        c.line("hello", json!({"game_version":"0.1.0","protocol":[1]})).0
    }

    #[test]
    fn handshake_and_heartbeat() {
        let (db, mut core, mut c) = setup();
        let r = replies(&core.key.clone(), &core.handle(&db, &hello(&mut c), proto::now_secs()));
        assert_eq!(r[0]["type"], "launcher.welcome");
        let (l, id) = c.line("game.heartbeat", json!({"state":"menu"}));
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!((r[0]["data"]["ack_id"].as_str(), r[0]["data"]["status"].as_str()), (Some(id.as_str()), Some("accepted")));
    }

    #[test]
    fn rejects_before_hello_bad_signature_and_replay() {
        let (db, mut core, mut c) = setup();
        let (l, _) = c.line("game.heartbeat", json!({"state":"menu"}));
        assert!(matches!(core.handle(&db, &l, proto::now_secs())[..], [Out::Close]), "nada antes do hello");
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let mut forged = Client { key: proto::random_key(), seq: 10 };
        let (l, _) = forged.line("game.heartbeat", json!({"state":"menu"}));
        assert!(matches!(core.handle(&db, &l, proto::now_secs())[..], [Out::Close]), "assinatura falsa encerra");
        // repetição: mesma linha duas vezes
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let (l, _) = c.line("game.heartbeat", json!({"state":"menu"}));
        core.handle(&db, &l, proto::now_secs());
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!((r[0]["data"]["status"].as_str(), r[0]["data"]["code"].as_str()), (Some("rejected"), Some("E_REPLAY")));
    }

    #[test]
    fn stale_timestamp_and_unknown_and_unsupported() {
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let (l, _) = c.line("game.heartbeat", json!({"state":"menu"}));
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs() + 3600));
        assert_eq!(r[0]["data"]["code"], "E_REPLAY", "horário fora da janela");
        let (l, _) = c.line("tipo.novo", json!({}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "unsupported");
        let (l, _) = c.line("achievement.unlocked", json!({"achievement_id":"first_blood","slot_uid":"slot-1234"}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_PROFILE_MISMATCH", "conquista sem slot liberado");
        let (l, _) = c.line("game.heartbeat", json!({}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_SCHEMA");
    }

    #[test]
    fn slot_of_other_profile_is_not_synced() {
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let (l, _) = c.line("game.started", json!({"mode":"classic","slot_uid":"slot-1234","owner_profile":"PERFIL-B"}));
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!(r[0]["data"]["code"], "E_PROFILE_MISMATCH");
        let ev = json!({"v":1,"id":proto::uuid_v4(),"game":"tomb-survivors","type":"game.heartbeat","data":{"state":"menu"},"origin":"offline","slot_uid":"slot-1234"});
        let (l, _) = c.line("outbox.flush", json!({"events":[ev]}));
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!(r[1]["data"]["code"], "E_PROFILE_MISMATCH", "pendentes do slot alheio recusados");
    }

    #[test]
    fn outbox_flush_acks_each_event_and_dedupes() {
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let (l, _) = c.line("game.started", json!({"mode":"classic","slot_uid":"slot-1234","owner_profile":""}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "accepted");
        let a = proto::uuid_v4();
        let b = proto::uuid_v4();
        let evs = json!({"events":[
            {"v":1,"id":a,"game":"tomb-survivors","type":"game.version.changed","data":{"from":"0.1.0","to":"0.2.0"},"origin":"offline","slot_uid":"slot-1234"},
            {"v":1,"id":b,"game":"tomb-survivors","type":"achievement.unlocked","data":{"achievement_id":"first_blood","slot_uid":"slot-1234"},"origin":"offline","slot_uid":"slot-1234"},
            {"v":1,"id":proto::uuid_v4(),"game":"outro-jogo","type":"game.heartbeat","data":{"state":"menu"},"slot_uid":"slot-1234"}
        ]});
        let (l, _) = c.line("outbox.flush", evs.clone());
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        let st: Vec<&str> = r.iter().skip(1).map(|x| x["data"]["status"].as_str().unwrap()).collect();
        assert_eq!(st, vec!["accepted", "unsupported", "rejected"]);
        // reenviar os mesmos pendentes: o aceito vira "duplicate", o não aplicado continua "unsupported"
        let (l, _) = c.line("outbox.flush", evs);
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!(r[1]["data"]["status"], "duplicate");
        assert_eq!(r[2]["data"]["status"], "unsupported");
    }

    #[test]
    fn reconnect_needs_new_hello_but_keeps_sequence() {
        let (db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        core.reset_connection();
        let (l, _) = c.line("game.heartbeat", json!({"state":"menu"}));
        assert!(matches!(core.handle(&db, &l, proto::now_secs())[..], [Out::Close]));
        let r = replies(&core.key.clone(), &core.handle(&db, &hello(&mut c), proto::now_secs()));
        assert_eq!(r[0]["type"], "launcher.welcome");
        let mut old = Client { key: c.key.clone(), seq: 0 };
        let (l, _) = old.line("game.heartbeat", json!({"state":"menu"}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_REPLAY", "sequência não volta ao zero");
    }

    #[test]
    fn statistics_and_progress_applied_per_slot() {
        let (mut db, mut core, mut c) = setup();
        core.handle(&db, &hello(&mut c), proto::now_secs());
        let (l, _) = c.line("game.started", json!({"mode":"classic","slot_uid":"slot-1234","owner_profile":""}));
        core.handle(&db, &l, proto::now_secs());
        // sem catálogo de estatísticas: guardado no jogo para depois
        let (l, _) = c.line("statistics.updated", json!({"slot_uid":"slot-1234","values":{"runs":3}}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "unsupported");
        // progresso não depende de catálogo
        let (l, _) = c.line("game.progress.updated", json!({"slot_uid":"slot-1234","percent":37,"details":{}}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "accepted");
        statistics::save_catalog(&mut db, "tomb-survivors", &statistics::tests::catalog()).unwrap();
        let (l, _) = c.line("statistics.updated", json!({"slot_uid":"slot-1234","values":{"runs":3,"best_level":12,"top_weapon":"bow"}}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "accepted");
        let (l, _) = c.line("statistics.updated", json!({"slot_uid":"slot-1234","values":{"runs":"muitas"}}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_SCHEMA");
        let (l, _) = c.line("statistics.updated", json!({"slot_uid":"slot-OUTRO","values":{"runs":50}}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_PROFILE_MISMATCH");
        let p = statistics::profile(&db, 1, "tomb-survivors").unwrap();
        let get = |k: &str| p.stats.iter().find(|s| s.key == k).map(|s| s.value.clone());
        assert_eq!((p.slots, get("runs"), get("best_level"), get("progress_percent")), (1, Some(json!(3)), Some(json!(12)), Some(json!(37))));
    }

    #[test]
    fn achievements_applied_with_signed_catalog() {
        let (mut db, mut core, mut c) = setup();
        achievements::save_catalog(&mut db, "tomb-survivors", &achievements::tests::catalog()).unwrap();
        let r = replies(&core.key.clone(), &core.handle(&db, &hello(&mut c), proto::now_secs()));
        assert_eq!(r[0]["data"]["catalog_version"], 1);
        let (l, _) = c.line("game.started", json!({"mode":"classic","slot_uid":"slot-1234","owner_profile":"PERFIL-A"}));
        core.handle(&db, &l, proto::now_secs());
        // ao vivo
        let (l, _) = c.line("achievement.unlocked", json!({"achievement_id":"first_blood","slot_uid":"slot-1234"}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["status"], "accepted");
        assert_eq!(core.fresh_unlocks, vec!["first_blood".to_string()]);
        // de outro slot: recusado
        let (l, _) = c.line("achievement.unlocked", json!({"achievement_id":"relic_hunter","slot_uid":"slot-OUTRO"}));
        assert_eq!(replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()))[0]["data"]["code"], "E_PROFILE_MISMATCH");
        // pendentes: inexistente e reservada recusadas (E_CATALOG), progresso aceito, repetida vira duplicate
        let ok = proto::uuid_v4();
        let ev = |id: &str, kind: &str, d: Value| json!({"v":1,"id":id,"game":"tomb-survivors","type":kind,"data":d,"origin":"offline","slot_uid":"slot-1234"});
        let evs = json!({"events":[
            ev(&proto::uuid_v4(), "achievement.unlocked", json!({"achievement_id":"inventada","slot_uid":"slot-1234"})),
            ev(&proto::uuid_v4(), "achievement.unlocked", json!({"achievement_id":"immortal_100","slot_uid":"slot-1234"})),
            ev(&ok, "achievement.progress.updated", json!({"achievement_id":"relic_hunter","current":12,"target":50,"slot_uid":"slot-1234"})),
            ev(&proto::uuid_v4(), "achievement.progress.updated", json!({"achievement_id":"relic_hunter","current":12,"target":7,"slot_uid":"slot-1234"})),
        ]});
        let (l, _) = c.line("outbox.flush", evs.clone());
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        let st: Vec<(&str, &str)> = r.iter().skip(1).map(|x| (x["data"]["status"].as_str().unwrap(), x["data"]["code"].as_str().unwrap_or(""))).collect();
        assert_eq!(st, vec![("rejected", "E_CATALOG"), ("rejected", "E_CATALOG"), ("accepted", ""), ("rejected", "E_CATALOG")]);
        let (l, _) = c.line("outbox.flush", evs);
        let r = replies(&core.key.clone(), &core.handle(&db, &l, proto::now_secs()));
        assert_eq!(r[3]["data"]["status"], "duplicate");
        let v = achievements::list(&db, 1, "tomb-survivors").unwrap();
        assert!(v[0].unlocked_at.is_some() && v[0].origin.as_deref() == Some("live"));
        assert_eq!((v[1].current, v[1].unlocked_at.is_none()), (12, true));
        assert!(v[2].unlocked_at.is_none(), "reservada continua bloqueada");
    }
}
