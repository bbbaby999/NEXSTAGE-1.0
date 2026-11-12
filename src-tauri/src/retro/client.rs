//! Serviço único de acesso à API Web oficial do RetroAchievements (https://retroachievements.org/API/).
//!
//! Todas as requisições passam por aqui:
//! - espaçamento mínimo entre chamadas e bloqueio temporário ao receber HTTP 429 (respeita Retry-After);
//! - validação das respostas e erros traduzidos (credenciais inválidas, sem internet, API indisponível);
//! - a chave da API vai só na query da requisição HTTPS: nunca é registrada, nem aparece em mensagens
//!   de erro (os erros do reqwest são convertidos com `without_url()` e a URL nunca é formatada).

use crate::error::AppError;
use serde_json::Value;
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

const BASE: &str = "https://retroachievements.org/API/";
pub const SITE: &str = "https://retroachievements.org";
pub const MEDIA: &str = "https://media.retroachievements.org";

/// Credenciais em memória. `Debug` nunca mostra a chave.
#[derive(Clone)]
pub struct Creds {
    pub user: String,
    key: String,
}
impl Creds {
    pub fn new(user: &str, key: &str) -> Self {
        Self { user: user.trim().to_string(), key: key.trim().to_string() }
    }
}
impl fmt::Debug for Creds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Creds {{ user: {:?}, key: <oculta> }}", self.user)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaError {
    InvalidCredentials,
    UserNotFound,
    RateLimited(u64),
    Offline,
    Unavailable(u16),
    BadResponse,
}

impl RaError {
    pub fn message(&self) -> String {
        match self {
            RaError::InvalidCredentials => "RetroAchievements: usuário ou chave da API inválidos. Confira os dados em Configurações → RetroAchievements.".into(),
            RaError::UserNotFound => "RetroAchievements: usuário não encontrado.".into(),
            RaError::RateLimited(s) => format!("RetroAchievements: limite de requisições atingido. Nova tentativa em {s} s."),
            RaError::Offline => "RetroAchievements: sem conexão com o servidor. Exibindo os dados já sincronizados.".into(),
            RaError::Unavailable(code) => format!("RetroAchievements: serviço indisponível no momento (HTTP {code}). Tente novamente mais tarde."),
            RaError::BadResponse => "RetroAchievements: resposta inesperada da API. Tente novamente mais tarde.".into(),
        }
    }
    /// Falhas temporárias (vale tentar de novo mais tarde, sem incomodar o usuário).
    pub fn is_transient(&self) -> bool {
        matches!(self, RaError::Offline | RaError::RateLimited(_) | RaError::Unavailable(_) | RaError::BadResponse)
    }
}

impl From<RaError> for AppError {
    fn from(e: RaError) -> Self {
        AppError::Other(e.message())
    }
}

static GATE: crate::net::Gate = crate::net::Gate::new(400);
static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();

fn http() -> Result<&'static reqwest::Client, RaError> {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION"), " (RetroAchievements integration)"))
            .build()
            .ok()
    })
    .as_ref()
    .ok_or(RaError::Offline)
}

/// Segundos restantes de bloqueio por limite de requisições (`None` = liberado).
pub fn blocked_for() -> Option<u64> {
    GATE.blocked_for()
}

/// Erros do reqwest → `RaError`, descartando a URL (que contém a chave).
fn net_err(e: reqwest::Error) -> RaError {
    let e = e.without_url();
    if e.is_decode() {
        RaError::BadResponse
    } else {
        RaError::Offline
    }
}

/// Interpreta o corpo da resposta (validação comum a todos os endpoints).
pub fn check_body(status: u16, body: &str) -> Result<Value, RaError> {
    match status {
        401 | 403 => return Err(RaError::InvalidCredentials),
        404 => return Err(RaError::UserNotFound),
        429 => return Err(RaError::RateLimited(60)),
        s if s >= 500 => return Err(RaError::Unavailable(s)),
        s if s >= 400 => return Err(RaError::BadResponse),
        _ => {}
    }
    let v: Value = serde_json::from_str(body).map_err(|_| RaError::BadResponse)?;
    if let Some(msg) = v.get("Error").and_then(Value::as_str).or_else(|| v.get("message").and_then(Value::as_str)) {
        let m = msg.to_lowercase();
        if m.contains("api key") || m.contains("unauthenticated") || m.contains("unauthorized") || m.contains("invalid") && m.contains("key") {
            return Err(RaError::InvalidCredentials);
        }
        if m.contains("not found") {
            return Err(RaError::UserNotFound);
        }
        return Err(RaError::BadResponse);
    }
    Ok(v)
}

async fn call(endpoint: &str, creds: &Creds, params: &[(&str, String)]) -> Result<Value, RaError> {
    if creds.key.is_empty() || creds.user.is_empty() {
        return Err(RaError::InvalidCredentials);
    }
    if let Some(s) = blocked_for() {
        return Err(RaError::RateLimited(s));
    }
    // espaçamento entre chamadas (intervalo mínimo de 400 ms; o RA pede uso moderado)
    GATE.wait().await;
    let mut q: Vec<(&str, &str)> = vec![("y", creds.key.as_str())];
    q.extend(params.iter().map(|(k, v)| (*k, v.as_str())));
    let resp = http()?.get(format!("{BASE}{endpoint}")).query(&q).send().await.map_err(net_err)?;
    let status = resp.status().as_u16();
    if status == 429 {
        let secs = GATE.block(crate::net::retry_after(&resp));
        return Err(RaError::RateLimited(secs));
    }
    let body = resp.text().await.map_err(net_err)?;
    check_body(status, &body)
}

// ---------------------------------------------------------------- endpoints usados

/// Perfil do usuário (pontos, foto, membro desde, presença).
pub async fn user_profile(c: &Creds) -> Result<Value, RaError> {
    let v = call("API_GetUserProfile.php", c, &[("u", c.user.clone())]).await?;
    if v.get("User").and_then(Value::as_str).map(str::is_empty).unwrap_or(true) {
        return Err(RaError::UserNotFound);
    }
    Ok(v)
}

/// Conquistas obtidas entre duas datas (epoch). Lista com dados completos de cada desbloqueio.
pub async fn earned_between(c: &Creds, from: i64, to: i64) -> Result<Vec<Value>, RaError> {
    let v = call("API_GetAchievementsEarnedBetween.php", c, &[("u", c.user.clone()), ("f", from.to_string()), ("t", to.to_string())]).await?;
    v.as_array().cloned().ok_or(RaError::BadResponse)
}

/// Progresso em todos os jogos do usuário (paginado de 500 em 500; no máximo 10 páginas).
pub async fn completion_progress(c: &Creds) -> Result<Vec<Value>, RaError> {
    let mut out = Vec::new();
    for page in 0..10 {
        let v = call("API_GetUserCompletionProgress.php", c, &[("u", c.user.clone()), ("c", "500".into()), ("o", (page * 500).to_string())]).await?;
        let results = v.get("Results").and_then(Value::as_array).ok_or(RaError::BadResponse)?;
        out.extend(results.iter().cloned());
        let total = super::util::int(&v["Total"]) as usize;
        if results.len() < 500 || out.len() >= total {
            break;
        }
    }
    Ok(out)
}

/// Jogo + todas as conquistas, com as datas de desbloqueio do usuário.
pub async fn game_info_user(c: &Creds, game_id: i64) -> Result<Value, RaError> {
    let v = call("API_GetGameInfoAndUserProgress.php", c, &[("g", game_id.to_string()), ("u", c.user.clone()), ("a", "1".into())]).await?;
    if super::util::int(&v["ID"]) == 0 && v.get("Title").is_none() {
        return Err(RaError::BadResponse);
    }
    Ok(v)
}

/// Catálogo de jogos COM conquistas de um console, incluindo os hashes oficiais de cada jogo.
pub async fn game_list(c: &Creds, console_id: i64) -> Result<Vec<Value>, RaError> {
    let v = call("API_GetGameList.php", c, &[("i", console_id.to_string()), ("h", "1".into()), ("f", "1".into())]).await?;
    v.as_array().cloned().ok_or(RaError::BadResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_responses() {
        assert_eq!(check_body(401, "{}"), Err(RaError::InvalidCredentials));
        assert_eq!(check_body(200, r#"{"message":"Unauthenticated.","errors":[]}"#), Err(RaError::InvalidCredentials));
        assert_eq!(check_body(200, r#"{"Success":false,"Error":"Invalid API Key"}"#), Err(RaError::InvalidCredentials));
        assert_eq!(check_body(503, "<html>"), Err(RaError::Unavailable(503)));
        assert_eq!(check_body(200, "<html>manutenção</html>"), Err(RaError::BadResponse));
        assert_eq!(check_body(429, ""), Err(RaError::RateLimited(60)));
        assert!(check_body(200, r#"[{"AchievementID":1}]"#).is_ok());
    }

    #[test]
    fn debug_never_shows_key() {
        let c = Creds::new("Jogador", "SEGREDO123456");
        assert!(!format!("{c:?}").contains("SEGREDO"));
        for e in [RaError::InvalidCredentials, RaError::Offline, RaError::RateLimited(30), RaError::Unavailable(502), RaError::BadResponse] {
            assert!(!e.message().contains("SEGREDO"));
        }
    }
}
