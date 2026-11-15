//! Login da conta Microsoft (Xbox) e tokens do Xbox Live.
//!
//! Fluxo (OAuth 2.0 "authorization code" + PKCE, app público, sem segredo embutido):
//! 1. receptor HTTP de uso único em `localhost` (porta aleatória, só a própria máquina);
//! 2. o navegador padrão abre a página OFICIAL de login da Microsoft: a senha é digitada só lá;
//! 3. a Microsoft devolve um código ao receptor; o código + o verificador PKCE viram tokens
//!    (o verificador nunca sai do NEXSTAGE, então um código interceptado não serve para nada);
//! 4. token da Microsoft → token de usuário do Xbox Live → token XSTS (identidade: XUID e gamertag).
//!
//! Só o refresh token é guardado, cifrado pela DPAPI (`vault`); tokens de acesso ficam só na memória.
//! O aplicativo do Azure (Client ID) é do distribuidor do NEXSTAGE: embutido na compilação
//! (`NEXSTAGE_XBOX_CLIENT_ID`) ou informado em Configurações.

use crate::error::{AppError, AppResult};
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const USER_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
/// Caminho do retorno. O redirecionamento registrado no Azure é `http://localhost` (sem caminho): a
/// Microsoft ignora a porta em `localhost`, mas exige o mesmo caminho, então o retorno chega na raiz.
const CALLBACK: &str = "/";
const TIMEOUT: Duration = Duration::from_secs(300);

/// Chave em `app_settings` com o Client ID informado pelo usuário (não é segredo).
pub const CLIENT_ID_SETTING: &str = "xbox_client_id";

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Client ID embutido na compilação (variável de ambiente `NEXSTAGE_XBOX_CLIENT_ID` no build).
pub fn embedded_client_id() -> Option<&'static str> {
    option_env!("NEXSTAGE_XBOX_CLIENT_ID").map(str::trim).filter(|s| valid_client_id(s))
}

/// Client ID no formato GUID (00000000-0000-0000-0000-000000000000).
pub fn valid_client_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Client ID em uso: o informado em Configurações tem prioridade sobre o embutido.
pub fn client_id(conn: &rusqlite::Connection) -> Option<String> {
    let saved: Option<String> = conn
        .query_row("SELECT value FROM app_settings WHERE key=?1", [CLIENT_ID_SETTING], |r| r.get(0))
        .ok()
        .map(|s: String| s.trim().to_string())
        .filter(|s| valid_client_id(s));
    saved.or_else(|| embedded_client_id().map(String::from))
}

fn http() -> AppResult<&'static reqwest::Client> {
    static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| AppError::Other("Falha de rede.".into()))
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_b64(n: usize) -> AppResult<String> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|_| AppError::Other("Falha ao gerar dados aleatórios.".into()))?;
    Ok(b64(&buf))
}

/// Desafio PKCE (S256) do verificador.
fn challenge(verifier: &str) -> String {
    b64(&Sha256::digest(verifier.as_bytes()))
}

pub struct Pending {
    listeners: Vec<TcpListener>,
    pub login_url: String,
    redirect: String,
    state: String,
    verifier: String,
    client_id: String,
}

/// Prepara o receptor local e a URL oficial de login.
pub fn start(client_id: &str) -> AppResult<Pending> {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst); // um login anterior ainda esperava: cancela e assume
        std::thread::sleep(Duration::from_millis(400));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let fail = |e: std::io::Error| {
        ACTIVE.store(false, Ordering::SeqCst);
        AppError::Other(format!("Não foi possível preparar o retorno do login da Microsoft: {e}"))
    };
    // "localhost" pode resolver para IPv4 ou IPv6 no navegador: escuta nos dois, na mesma porta.
    let v4 = TcpListener::bind("127.0.0.1:0").map_err(fail)?;
    let port = v4.local_addr().map(|a| a.port()).unwrap_or(0);
    let mut listeners = vec![v4];
    if let Ok(v6) = TcpListener::bind(("::1", port)) {
        listeners.push(v6);
    }
    for l in &listeners {
        l.set_nonblocking(true).ok();
    }
    let state = random_b64(16)?;
    let verifier = random_b64(48)?;
    let redirect = format!("http://localhost:{port}");
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE,
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("response_mode", "query"),
            ("scope", SCOPE),
            ("state", state.as_str()),
            ("code_challenge", challenge(&verifier).as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ],
    )
    .map_err(|_| AppError::Other("URL de login inválida.".into()))?;
    Ok(Pending { listeners, login_url: url.to_string(), redirect, state, verifier, client_id: client_id.to_string() })
}

/// Confere o retorno da Microsoft e devolve o código de autorização.
fn check_callback(q: &std::collections::HashMap<String, String>, state: &str) -> AppResult<String> {
    let get = |k: &str| q.get(k).map(String::as_str).unwrap_or("");
    if get("state") != state {
        return Err(AppError::Validation("Resposta de login da Microsoft não corresponde a esta tentativa.".into()));
    }
    match get("error") {
        "" => {}
        "access_denied" => return Err(AppError::Validation("Login cancelado na página da Microsoft.".into())),
        "unauthorized_client" | "invalid_client" | "invalid_request" => {
            return Err(AppError::Validation(
                "A Microsoft recusou o aplicativo. Confira o Client ID e se o app do Azure aceita contas Microsoft pessoais, com o redirecionamento http://localhost (plataforma “Aplicativos móveis e da área de trabalho”).".into(),
            ))
        }
        _ => return Err(AppError::Validation("A Microsoft não concluiu o login. Tente novamente.".into())),
    }
    let code = get("code");
    if code.is_empty() {
        return Err(AppError::Validation("A Microsoft não concluiu o login. Tente novamente.".into()));
    }
    Ok(code.to_string())
}

/// Espera o retorno do navegador (bloqueante; até 5 min ou cancelamento). Devolve o código.
pub fn wait(p: &Pending) -> AppResult<String> {
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
            return Err(AppError::Validation("Login da conta Xbox cancelado.".into()));
        }
        if start.elapsed() > TIMEOUT {
            return Err(AppError::Validation("Tempo esgotado esperando o login da Microsoft. Tente novamente.".into()));
        }
        let mut got = false;
        for l in &p.listeners {
            let Ok((mut s, addr)) = l.accept() else { continue };
            got = true;
            if !addr.ip().is_loopback() {
                continue;
            }
            s.set_nonblocking(false).ok();
            match crate::steam::openid::read_request(&mut s) {
                Some((path, query)) if path == CALLBACK => {
                    let q = crate::steam::openid::parse_query(&query);
                    let r = check_callback(&q, &p.state);
                    let _ = s.write_all(crate::steam::openid::page(r.is_ok()).as_bytes());
                    return r;
                }
                _ => {
                    let _ = s.write_all(crate::steam::openid::page(false).as_bytes()); // favicon etc.
                }
            }
        }
        if !got {
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

/// Tokens da conta Microsoft.
pub struct MsTokens {
    pub access: String,
    pub refresh: String,
}

async fn token_request(form: &[(&str, &str)]) -> AppResult<MsTokens> {
    let resp = http()?
        .post(TOKEN)
        .form(form)
        .send()
        .await
        .map_err(|_| AppError::Other("Sem conexão com a Microsoft. Verifique a internet.".into()))?;
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if let (Some(a), Some(r)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) {
        return Ok(MsTokens { access: a.to_string(), refresh: r.to_string() });
    }
    Err(match v["error"].as_str().unwrap_or("") {
        "invalid_grant" => AppError::Validation("A sessão da conta Xbox expirou ou foi revogada. Entre novamente em Configurações → Contas e plataformas.".into()),
        "invalid_client" | "unauthorized_client" => AppError::Validation(
            "A Microsoft recusou o aplicativo. Confira o Client ID e se o app do Azure aceita contas Microsoft pessoais, com o redirecionamento http://localhost (plataforma “Aplicativos móveis e da área de trabalho”).".into(),
        ),
        _ => AppError::Other("A Microsoft não emitiu os tokens da conta. Tente novamente.".into()),
    })
}

/// Troca o código (com o verificador PKCE) pelos tokens.
pub async fn exchange(p: &Pending, code: &str) -> AppResult<MsTokens> {
    token_request(&[
        ("client_id", p.client_id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", p.redirect.as_str()),
        ("code_verifier", p.verifier.as_str()),
        ("scope", SCOPE),
    ])
    .await
}

/// Novo token de acesso pelo refresh token (a Microsoft devolve também um refresh token novo).
pub async fn refresh(client_id: &str, refresh_token: &str) -> AppResult<MsTokens> {
    token_request(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("scope", SCOPE)]).await
}

/// Identidade e token do Xbox Live (XSTS), válidos por algumas horas.
#[derive(Clone)]
pub struct Xsts {
    pub uhs: String,
    pub token: String,
    pub xuid: String,
    pub gamertag: String,
    pub expires: Instant,
}

impl Xsts {
    pub fn header(&self) -> String {
        format!("XBL3.0 x={};{}", self.uhs, self.token)
    }
    pub fn valid(&self) -> bool {
        Instant::now() + Duration::from_secs(300) < self.expires
    }
}

/// Mensagem do erro XSTS (`XErr`) mais comuns.
fn xerr_message(code: i64) -> String {
    match code {
        2148916233 => "Esta conta Microsoft ainda não tem perfil Xbox. Crie um em xbox.com e tente novamente.".into(),
        2148916235 => "O Xbox Live não está disponível no país desta conta.".into(),
        2148916236 | 2148916237 => "Esta conta precisa de verificação de idade em xbox.com antes de ser usada.".into(),
        2148916238 => "Conta infantil: um adulto da Família Microsoft precisa permitir o acesso em account.microsoft.com/family.".into(),
        _ => "O Xbox Live recusou o login desta conta.".into(),
    }
}

/// Token da Microsoft → token de usuário do Xbox Live → XSTS.
pub async fn xsts(ms_access: &str) -> AppResult<Xsts> {
    let offline = || AppError::Other("Sem conexão com o Xbox Live. Verifique a internet.".into());
    let user: Value = http()?
        .post(USER_AUTH)
        .header("x-xbl-contract-version", "1")
        .json(&json!({
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms_access}") }
        }))
        .send()
        .await
        .map_err(|_| offline())?
        .json()
        .await
        .map_err(|_| AppError::Other("O Xbox Live recusou o login desta conta.".into()))?;
    let user_token = user["Token"].as_str().ok_or_else(|| AppError::Other("O Xbox Live recusou o login desta conta.".into()))?;
    let resp = http()?
        .post(XSTS_AUTH)
        .header("x-xbl-contract-version", "1")
        .json(&json!({
            "RelyingParty": "http://xboxlive.com",
            "TokenType": "JWT",
            "Properties": { "UserTokens": [user_token], "SandboxId": "RETAIL" }
        }))
        .send()
        .await
        .map_err(|_| offline())?;
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(code) = v["XErr"].as_i64() {
        return Err(AppError::Validation(xerr_message(code)));
    }
    parse_xsts(&v).ok_or_else(|| AppError::Other("O Xbox Live recusou o login desta conta.".into()))
}

fn parse_xsts(v: &Value) -> Option<Xsts> {
    let xui = &v["DisplayClaims"]["xui"][0];
    let s = |x: &Value| x.as_str().filter(|s| !s.is_empty()).map(String::from);
    let expires = v["NotAfter"].as_str().and_then(crate::retro::util::parse_epoch).map(|e| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Instant::now() + Duration::from_secs((e - now).clamp(0, 24 * 3600) as u64)
    });
    Some(Xsts {
        uhs: s(&xui["uhs"])?,
        token: s(&v["Token"])?,
        xuid: s(&xui["xid"])?,
        gamertag: s(&xui["gtg"]).unwrap_or_default(),
        expires: expires.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn client_id_format() {
        assert!(valid_client_id("0123abcd-4567-89ab-cdef-0123456789ab"));
        assert!(!valid_client_id("0123abcd-4567-89ab-cdef-0123456789a"));
        assert!(!valid_client_id("não-é-guid"));
    }

    #[test]
    fn pkce_challenge_rfc7636() {
        // exemplo do apêndice B da RFC 7636
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn login_url_and_callback() {
        let p = start("0123abcd-4567-89ab-cdef-0123456789ab").unwrap();
        assert!(p.login_url.starts_with("https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?"));
        assert!(p.login_url.contains("code_challenge_method=S256") && p.login_url.contains("XboxLive.signin"));
        assert!(!p.login_url.contains(&p.verifier), "o verificador PKCE nunca vai na URL");
        let q = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        assert_eq!(check_callback(&q(&[("state", &p.state), ("code", "abc")]), &p.state).unwrap(), "abc");
        assert!(check_callback(&q(&[("state", "outro"), ("code", "abc")]), &p.state).is_err(), "estado diferente = outra tentativa");
        assert!(check_callback(&q(&[("state", &p.state), ("error", "access_denied")]), &p.state).is_err());
        cancel();
        assert!(wait(&p).is_err());
    }

    #[test]
    fn xsts_claims() {
        let v = json!({ "Token": "tok", "NotAfter": "2099-01-01T00:00:00.0000000Z",
            "DisplayClaims": { "xui": [{ "uhs": "123", "xid": "2535400000000000", "gtg": "Nome" }] } });
        let x = parse_xsts(&v).unwrap();
        assert_eq!((x.uhs.as_str(), x.xuid.as_str(), x.gamertag.as_str()), ("123", "2535400000000000", "Nome"));
        assert_eq!(x.header(), "XBL3.0 x=123;tok");
        assert!(x.valid());
        assert!(parse_xsts(&json!({})).is_none());
    }
}
