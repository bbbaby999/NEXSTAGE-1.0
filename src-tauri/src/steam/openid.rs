//! Login oficial "Entrar com a Steam" (OpenID 2.0 de steamcommunity.com).
//!
//! Fluxo para app de desktop:
//! 1. abre um receptor HTTP de uso único em 127.0.0.1 (porta aleatória, só a própria máquina);
//! 2. abre o navegador padrão na página oficial de login da Steam — a senha é digitada SÓ no site da
//!    Steam; o NEXSTAGE nunca a vê;
//! 3. a Steam redireciona de volta para o receptor com a asserção OpenID assinada;
//! 4. a asserção é conferida (estado anti-falsificação, return_to, endpoint, formato do claimed_id) e
//!    VALIDADA PELA PRÓPRIA STEAM (`check_authentication`). O resultado é apenas o SteamID64 público.
//! Nenhum cookie, token ou credencial é guardado.

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const OP_ENDPOINT: &str = "https://steamcommunity.com/openid/login";
const NS: &str = "http://specs.openid.net/auth/2.0";
const SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
const TIMEOUT: Duration = Duration::from_secs(300);

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// 128 bits aleatórios (semente do sistema via `RandomState`) em hexadecimal.
fn random_state() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i ^ std::process::id() as u64);
        h.write_u128(Instant::now().elapsed().as_nanos() ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

/// Página exibida no navegador depois do retorno (sem scripts, sem recursos externos).
pub(crate) fn page(ok: bool) -> String {
    let (title, msg) = if ok {
        ("Login concluído", "Pronto! Você já pode fechar esta aba e voltar ao NEXSTAGE.<br><small>Done! You can close this tab and return to NEXSTAGE.</small>")
    } else {
        ("Página não encontrada", "")
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>NEXSTAGE · {title}</title></head>\
         <body style=\"margin:0;height:100vh;display:grid;place-items:center;background:#05060a;color:#eef2ff;font-family:Segoe UI,sans-serif;text-align:center\">\
         <div><h1 style=\"font-weight:300;letter-spacing:.18em\">NEXSTAGE</h1><p>{msg}</p></div></body></html>"
    );
    let status = if ok { "200 OK" } else { "404 Not Found" };
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Lê uma requisição e devolve (caminho, query).
pub(crate) fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf = vec![0u8; 16 * 1024];
    let mut n = 0;
    while n < buf.len() {
        let r = stream.read(&mut buf[n..]).ok()?;
        if r == 0 {
            break;
        }
        n += r;
        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    let line = text.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Some((path.to_string(), query.to_string()))
}

pub struct Started {
    pub listener: TcpListener,
    pub login_url: String,
    pub return_to: String,
    pub state: String,
}

/// Prepara o receptor local e a URL oficial de login.
pub fn start() -> AppResult<Started> {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        // um login anterior ainda está esperando: cancela e assume
        CANCEL.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(400));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
        ACTIVE.store(false, Ordering::SeqCst);
        AppError::Other(format!("Não foi possível preparar o retorno do login da Steam: {e}"))
    })?;
    listener.set_nonblocking(true).ok();
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let state = random_state();
    let return_to = format!("http://127.0.0.1:{port}/steam/openid?state={state}");
    let realm = format!("http://127.0.0.1:{port}/");
    let url = reqwest::Url::parse_with_params(
        OP_ENDPOINT,
        &[
            ("openid.ns", NS),
            ("openid.mode", "checkid_setup"),
            ("openid.return_to", return_to.as_str()),
            ("openid.realm", realm.as_str()),
            ("openid.identity", SELECT),
            ("openid.claimed_id", SELECT),
        ],
    )
    .map_err(|_| AppError::Other("URL de login inválida.".into()))?;
    Ok(Started { listener, login_url: url.to_string(), return_to, state })
}

/// Espera o retorno da Steam (bloqueante; até 5 min ou cancelamento).
pub fn wait_callback(listener: TcpListener) -> AppResult<HashMap<String, String>> {
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            ACTIVE.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    let start = Instant::now();
    loop {
        if CANCEL.load(Ordering::SeqCst) {
            return Err(AppError::Validation("Login da Steam cancelado.".into()));
        }
        if start.elapsed() > TIMEOUT {
            return Err(AppError::Validation("Tempo esgotado esperando o login da Steam. Tente novamente.".into()));
        }
        match listener.accept() {
            Ok((mut s, addr)) => {
                if !addr.ip().is_loopback() {
                    continue;
                }
                s.set_nonblocking(false).ok();
                match read_request(&mut s) {
                    Some((path, query)) if path == "/steam/openid" => {
                        let params = parse_query(&query);
                        let _ = s.write_all(page(true).as_bytes());
                        return Ok(params);
                    }
                    _ => {
                        let _ = s.write_all(page(false).as_bytes()); // favicon etc.
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
            Err(_) => std::thread::sleep(Duration::from_millis(150)),
        }
    }
}

/// Conferências locais da asserção. Devolve o SteamID64.
pub fn check_assertion(p: &HashMap<String, String>, return_to: &str, state: &str) -> AppResult<String> {
    let get = |k: &str| p.get(k).map(String::as_str).unwrap_or("");
    if get("openid.mode") == "cancel" {
        return Err(AppError::Validation("Login cancelado na página da Steam.".into()));
    }
    if get("openid.mode") != "id_res" {
        return Err(AppError::Validation("Resposta de login da Steam inválida.".into()));
    }
    if get("state") != state || get("openid.return_to") != return_to {
        return Err(AppError::Validation("Resposta de login da Steam não corresponde a esta tentativa.".into()));
    }
    if get("openid.op_endpoint") != OP_ENDPOINT || get("openid.ns") != NS {
        return Err(AppError::Validation("Resposta de login não veio da Steam.".into()));
    }
    let claimed = get("openid.claimed_id");
    let id = claimed
        .strip_prefix("https://steamcommunity.com/openid/id/")
        .filter(|id| id.len() == 17 && id.chars().all(|c| c.is_ascii_digit()) && id.starts_with("7656119"))
        .ok_or_else(|| AppError::Validation("Identificador da Steam inválido na resposta de login.".into()))?;
    if get("openid.identity") != claimed {
        return Err(AppError::Validation("Resposta de login da Steam inconsistente.".into()));
    }
    Ok(id.to_string())
}

/// Validação da assinatura pela própria Steam (`check_authentication`).
pub async fn verify_with_steam(p: &HashMap<String, String>) -> AppResult<()> {
    let mut form: Vec<(String, String)> = p.iter().filter(|(k, _)| k.starts_with("openid.")).map(|(k, v)| (k.clone(), v.clone())).collect();
    for f in form.iter_mut() {
        if f.0 == "openid.mode" {
            f.1 = "check_authentication".into();
        }
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|_| AppError::Other("Falha de rede.".into()))?;
    let body = client
        .post(OP_ENDPOINT)
        .form(&form)
        .send()
        .await
        .map_err(|_| AppError::Other("Sem conexão com a Steam para confirmar o login.".into()))?
        .text()
        .await
        .map_err(|_| AppError::Other("Sem conexão com a Steam para confirmar o login.".into()))?;
    if body.lines().any(|l| l.trim() == "is_valid:true") {
        Ok(())
    } else {
        Err(AppError::Validation("A Steam não confirmou este login. Tente novamente.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good(rt: &str, state: &str) -> HashMap<String, String> {
        let id = "https://steamcommunity.com/openid/id/76561197960287930";
        [
            ("openid.mode", "id_res"), ("openid.ns", NS), ("openid.op_endpoint", OP_ENDPOINT), ("openid.claimed_id", id),
            ("openid.identity", id), ("openid.return_to", rt), ("state", state),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn assertion_checks() {
        let rt = "http://127.0.0.1:5555/steam/openid?state=abc";
        assert_eq!(check_assertion(&good(rt, "abc"), rt, "abc").unwrap(), "76561197960287930");
        assert!(check_assertion(&good(rt, "xyz"), rt, "abc").is_err(), "estado diferente = outra tentativa");
        let mut bad = good(rt, "abc");
        bad.insert("openid.op_endpoint".into(), "https://evil.example/openid".into());
        assert!(check_assertion(&bad, rt, "abc").is_err());
        let mut bad = good(rt, "abc");
        bad.insert("openid.claimed_id".into(), "https://steamcommunity.com/openid/id/123".into());
        assert!(check_assertion(&bad, rt, "abc").is_err());
        let mut c = good(rt, "abc");
        c.insert("openid.mode".into(), "cancel".into());
        assert!(check_assertion(&c, rt, "abc").is_err());
    }

    #[test]
    fn query_decoding() {
        let q = parse_query("openid.return_to=http%3A%2F%2F127.0.0.1%3A1%2Fsteam%2Fopenid%3Fstate%3Dabc&state=abc&x=a+b&bad=%zz");
        assert_eq!(q["openid.return_to"], "http://127.0.0.1:1/steam/openid?state=abc");
        assert_eq!(q["x"], "a b");
        assert_eq!(q["bad"], "%zz");
        assert_eq!(random_state().len(), 32);
        assert_ne!(random_state(), random_state());
    }

    #[test]
    fn login_url_is_official() {
        let s = start().unwrap();
        assert!(s.login_url.starts_with("https://steamcommunity.com/openid/login?"));
        assert!(s.login_url.contains("checkid_setup"));
        assert!(s.return_to.starts_with("http://127.0.0.1:"));
        cancel();
        assert!(wait_callback(s.listener).is_err());
    }
}
