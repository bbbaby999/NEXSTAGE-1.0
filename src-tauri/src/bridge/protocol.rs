//! Protocolo do NexStage Game Bridge v1: envelope assinado, campos da mensagem, formatos dos eventos.
//! Especificação: `docs/bridge/NEXSTAGE_GAME_BRIDGE.md`. Este arquivo não conhece nenhum jogo específico.

use ring::hmac;
use serde_json::{json, Value};

pub const PROTOCOL: i64 = 1;
pub const MAX_LINE: usize = 64 * 1024;
/// Janela aceita entre o horário da mensagem e o relógio do PC (mensagens ao vivo).
pub const TS_WINDOW_SECS: i64 = 600;
/// Máximo de eventos dentro de um `outbox.flush`.
pub const MAX_FLUSH: usize = 200;

/// Códigos de erro do protocolo (§14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Size,
    Auth,
    ProtoVersion,
    Session,
    Replay,
    Schema,
    Catalog,
    ProfileMismatch,
    Rate,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Size => "E_SIZE",
            Code::Auth => "E_AUTH",
            Code::ProtoVersion => "E_PROTO_VERSION",
            Code::Session => "E_SESSION",
            Code::Replay => "E_REPLAY",
            Code::Schema => "E_SCHEMA",
            Code::Catalog => "E_CATALOG",
            Code::ProfileMismatch => "E_PROFILE_MISMATCH",
            Code::Rate => "E_RATE",
        }
    }
    /// Erros que encerram a conexão (os demais viram um `launcher.ack` recusado).
    pub fn fatal(self) -> bool {
        matches!(self, Code::Size | Code::Auth | Code::Session)
    }
}

/// Mensagem já verificada (assinatura conferida) e com os campos do envelope validados.
#[derive(Debug, Clone)]
#[allow(dead_code)] // origin/t passam a ser usados ao aplicar eventos offline (Fase 6)
pub struct Msg {
    pub id: String,
    pub seq: i64,
    pub ts: String,
    pub game: String,
    pub session: String,
    pub kind: String,
    pub data: Value,
    pub origin: String,
    pub at: Option<String>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

pub fn sign(key: &[u8], payload: &str) -> String {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    hex(hmac::sign(&k, payload.as_bytes()).as_ref())
}

/// Linha NDJSON assinada (resposta do launcher).
pub fn envelope(key: &[u8], payload: &Value) -> String {
    let p = payload.to_string();
    let mac = sign(key, &p);
    let mut line = json!({ "p": p, "mac": mac }).to_string();
    line.push('\n');
    line
}

/// Confere tamanho, formato do envelope e assinatura (tempo constante). Devolve a carga útil.
pub fn verify_envelope(line: &str, key: &[u8]) -> Result<Value, Code> {
    if line.len() > MAX_LINE {
        return Err(Code::Size);
    }
    let env: Value = serde_json::from_str(line).map_err(|_| Code::Auth)?;
    let p = env.get("p").and_then(Value::as_str).ok_or(Code::Auth)?;
    let mac = env.get("mac").and_then(Value::as_str).and_then(unhex).ok_or(Code::Auth)?;
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&k, p.as_bytes(), &mac).map_err(|_| Code::Auth)?;
    serde_json::from_str(p).map_err(|_| Code::Schema)
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// Campos obrigatórios do envelope (§3). `v` diferente → `E_PROTO_VERSION`.
pub fn parse_msg(p: &Value) -> Result<Msg, Code> {
    if p.get("v").and_then(Value::as_i64) != Some(PROTOCOL) {
        return Err(Code::ProtoVersion);
    }
    let id = s(p, "id").filter(|x| valid_id(x)).ok_or(Code::Schema)?;
    let seq = p.get("seq").and_then(Value::as_i64).filter(|x| *x > 0).ok_or(Code::Schema)?;
    let ts = s(p, "ts").ok_or(Code::Schema)?;
    let game = s(p, "game").ok_or(Code::Schema)?;
    let session = s(p, "session").ok_or(Code::Schema)?;
    let kind = s(p, "type").filter(|t| !t.is_empty() && t.len() <= 64).ok_or(Code::Schema)?;
    let data = p.get("data").cloned().filter(Value::is_object).ok_or(Code::Schema)?;
    let origin = s(p, "origin").unwrap_or_else(|| "live".into());
    if origin != "live" && origin != "offline" {
        return Err(Code::Schema);
    }
    Ok(Msg { id, seq, ts, game, session, kind, data, origin, at: s(p, "at") })
}

/// Identificadores de evento: 8 a 64 caracteres [A-Za-z0-9-].
pub fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn str_field(d: &Value, k: &str, max: usize) -> bool {
    d.get(k).and_then(Value::as_str).is_some_and(|x| !x.is_empty() && x.len() <= max)
}

/// Formato do `data` de cada tipo de evento enviado pelo jogo (§5).
/// `None` = tipo desconhecido (responde `unsupported`, sem derrubar a sessão).
pub fn check_data(kind: &str, d: &Value) -> Option<Result<(), Code>> {
    let ok = match kind {
        "hello" => d.get("game_version").is_some_and(Value::is_string) && d.get("protocol").is_some_and(Value::is_array),
        "game.started" => str_field(d, "slot_uid", 64) && str_field(d, "mode", 32),
        "game.heartbeat" => str_field(d, "state", 16),
        "game.closed" => str_field(d, "reason", 32),
        "game.version.changed" => str_field(d, "from", 32) && str_field(d, "to", 32),
        "game.progress.updated" => str_field(d, "slot_uid", 64) && d.get("percent").and_then(Value::as_f64).is_some_and(|p| (0.0..=100.0).contains(&p)),
        "achievement.unlocked" => str_field(d, "achievement_id", 64) && str_field(d, "slot_uid", 64),
        "achievement.progress.updated" => {
            str_field(d, "achievement_id", 64)
                && d.get("current").and_then(Value::as_i64).is_some_and(|c| c >= 0)
                && d.get("target").and_then(Value::as_i64).is_some_and(|t| t > 0)
        }
        "statistics.updated" => str_field(d, "slot_uid", 64) && d.get("values").is_some_and(Value::is_object),
        "outbox.flush" => d.get("events").and_then(Value::as_array).is_some_and(|e| e.len() <= MAX_FLUSH),
        _ => return None,
    };
    Some(if ok { Ok(()) } else { Err(Code::Schema) })
}

/// Tipos que o launcher aplica (desde a Fase 8, todos os do protocolo v1). Os demais são respondidos `unsupported`
/// (o jogo os mantém na caixa de saída até a fase que os aplica — ver docs, "Estado da implementação").
pub fn applied(kind: &str) -> bool {
    matches!(
        kind,
        "hello" | "game.started" | "game.heartbeat" | "game.closed" | "game.version.changed" | "outbox.flush" | "achievement.unlocked" | "achievement.progress.updated" | "statistics.updated"
            | "game.progress.updated"
    )
}

// ------------------------------------------------------------ horário (sem dependências)

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` → segundos desde 1970 (UTC).
pub fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') || b[13] != b':' || b[16] != b':' || !s.ends_with('Z') {
        return None;
    }
    let n = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn ts_from_secs(t: i64) -> String {
    let days = t.div_euclid(86400);
    let rem = t.rem_euclid(86400);
    // inverso de days_from_civil
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// UUID v4 aleatório.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::getrandom(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn random_key() -> Vec<u8> {
    let mut k = vec![0u8; 32];
    let _ = getrandom::getrandom(&mut k);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_roundtrip_and_tamper() {
        let key = random_key();
        let line = envelope(&key, &json!({"v": 1, "type": "x"}));
        assert!(verify_envelope(line.trim_end(), &key).is_ok());
        let other = random_key();
        assert_eq!(verify_envelope(line.trim_end(), &other).unwrap_err(), Code::Auth);
        let tampered = line.replace("\\\"x\\\"", "\\\"y\\\"");
        assert_eq!(verify_envelope(tampered.trim_end(), &key).unwrap_err(), Code::Auth);
        assert_eq!(verify_envelope("lixo", &key).unwrap_err(), Code::Auth);
        assert_eq!(verify_envelope(&"a".repeat(MAX_LINE + 1), &key).unwrap_err(), Code::Size);
    }

    #[test]
    fn timestamps() {
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ts("2026-11-20T15:00:00Z"), Some(1795186800));
        assert_eq!(parse_ts("2026-11-20T15:00:00.123Z"), Some(1795186800));
        assert_eq!(parse_ts("2026-13-20T15:00:00Z"), None);
        assert_eq!(parse_ts("ontem"), None);
        assert_eq!(ts_from_secs(1795186800), "2026-11-20T15:00:00Z");
        let now = now_secs();
        assert_eq!(parse_ts(&ts_from_secs(now)), Some(now));
    }

    #[test]
    fn envelope_fields_and_schemas() {
        let base = json!({"v":1,"id":uuid_v4(),"seq":1,"ts":"2026-11-20T15:00:00Z","game":"g","session":"s","type":"game.heartbeat","data":{"state":"menu"}});
        let m = parse_msg(&base).unwrap();
        assert_eq!((m.kind.as_str(), m.origin.as_str()), ("game.heartbeat", "live"));
        let mut v2 = base.clone();
        v2["v"] = json!(2);
        assert_eq!(parse_msg(&v2).unwrap_err(), Code::ProtoVersion);
        let mut bad = base.clone();
        bad["id"] = json!("x");
        assert_eq!(parse_msg(&bad).unwrap_err(), Code::Schema);
        assert_eq!(check_data("game.heartbeat", &json!({"state":"menu"})), Some(Ok(())));
        assert_eq!(check_data("game.heartbeat", &json!({})), Some(Err(Code::Schema)));
        assert_eq!(check_data("achievement.progress.updated", &json!({"achievement_id":"a","current":-1,"target":5})), Some(Err(Code::Schema)));
        assert_eq!(check_data("algo.novo", &json!({})), None);
        assert!(uuid_v4().len() == 36 && valid_id(&uuid_v4()));
    }
}
