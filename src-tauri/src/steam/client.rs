//! Serviço único de acesso às APIs oficiais da Steam:
//! - Steam Web API (api.steampowered.com): IPlayerService, ISteamUser, ISteamUserStats. Os endpoints
//!   de biblioteca/perfil exigem uma chave da Web API: o NEXSTAGE NÃO embute chave compartilhada e não
//!   tem servidor próprio, então usa a chave PESSOAL do usuário (opcional), guardada no cofre do Windows.
//! - Perfil público da Comunidade Steam em XML (steamcommunity.com/profiles/<id>?xml=1), sem chave,
//!   para nome/avatar/privacidade quando não há chave.
//! A chave vai só na query HTTPS; erros nunca incluem a URL (`without_url`).

use crate::error::AppError;
use crate::net::Gate;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

const API: &str = "https://api.steampowered.com/";
static GATE: Gate = Gate::new(250);
static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub enum SteamError {
    NoKey,
    InvalidKey,
    /// Perfil ou detalhes de jogos privados (a Steam não devolve os dados).
    Private,
    /// O jogo não tem conquistas/estatísticas.
    NoStats,
    RateLimited(u64),
    Offline,
    Unavailable(u16),
    BadResponse,
}

impl SteamError {
    pub fn message(&self) -> String {
        match self {
            SteamError::NoKey => "Steam: esta função precisa da sua chave pessoal da Web API (Configurações → Contas e plataformas → Steam).".into(),
            SteamError::InvalidKey => "Steam: a chave da Web API foi recusada. Confira a chave em steamcommunity.com/dev/apikey.".into(),
            SteamError::Private => "Steam: dados indisponíveis porque o perfil ou os detalhes de jogos estão privados.".into(),
            SteamError::NoStats => "Este jogo não tem conquistas ou estatísticas na Steam.".into(),
            SteamError::RateLimited(s) => format!("Steam: limite de requisições atingido. Nova tentativa em {s} s."),
            SteamError::Offline => "Steam: sem conexão. Exibindo os dados já sincronizados.".into(),
            SteamError::Unavailable(c) => format!("Steam: serviço indisponível no momento (HTTP {c})."),
            SteamError::BadResponse => "Steam: resposta inesperada. Tente novamente mais tarde.".into(),
        }
    }
}

impl From<SteamError> for AppError {
    fn from(e: SteamError) -> Self {
        AppError::Other(e.message())
    }
}

pub fn blocked_for() -> Option<u64> {
    GATE.blocked_for()
}

fn http() -> Result<&'static reqwest::Client, SteamError> {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
            .build()
            .ok()
    })
    .as_ref()
    .ok_or(SteamError::Offline)
}

fn net_err(e: reqwest::Error) -> SteamError {
    if e.without_url().is_decode() { SteamError::BadResponse } else { SteamError::Offline }
}

/// Chamada genérica. Devolve (status, corpo JSON) para que cada endpoint interprete 400/403
/// (que, nas estatísticas, significam "sem estatísticas" ou "perfil privado").
async fn call(path: &str, key: Option<&str>, params: &[(&str, String)]) -> Result<(u16, Value), SteamError> {
    if let Some(s) = GATE.blocked_for() {
        return Err(SteamError::RateLimited(s));
    }
    GATE.wait().await;
    let mut q: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    if let Some(k) = key {
        q.push(("key", k));
    }
    q.push(("format", "json"));
    let resp = http()?.get(format!("{API}{path}")).query(&q).send().await.map_err(net_err)?;
    let status = resp.status().as_u16();
    if status == 429 {
        return Err(SteamError::RateLimited(GATE.block(crate::net::retry_after(&resp))));
    }
    if status >= 500 {
        return Err(SteamError::Unavailable(status));
    }
    let text = resp.text().await.map_err(net_err)?;
    let v = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    if status == 401 || (status == 403 && key.is_some() && v.is_null()) {
        // 403 com página HTML (sem JSON) = chave recusada
        return Err(SteamError::InvalidKey);
    }
    Ok((status, v))
}

fn need_key(key: Option<&str>) -> Result<&str, SteamError> {
    key.filter(|k| !k.is_empty()).ok_or(SteamError::NoKey)
}

/// ISteamUser/GetPlayerSummaries: nome, avatar, URL, visibilidade (3 = público).
pub async fn player_summary(key: Option<&str>, steamid: &str) -> Result<Value, SteamError> {
    let k = need_key(key)?;
    let (st, v) = call("ISteamUser/GetPlayerSummaries/v2/", Some(k), &[("steamids", steamid.into())]).await?;
    if st == 403 {
        return Err(SteamError::InvalidKey);
    }
    v["response"]["players"].as_array().and_then(|a| a.first()).cloned().ok_or(SteamError::BadResponse)
}

/// IPlayerService/GetOwnedGames (com nomes/ícones e jogos gratuitos já jogados).
/// `Err(Private)` quando a Steam não devolve a lista (detalhes de jogos privados).
pub async fn owned_games(key: Option<&str>, steamid: &str) -> Result<Vec<Value>, SteamError> {
    let k = need_key(key)?;
    let (st, v) = call(
        "IPlayerService/GetOwnedGames/v1/",
        Some(k),
        &[("steamid", steamid.into()), ("include_appinfo", "1".into()), ("include_played_free_games", "1".into())],
    )
    .await?;
    if st == 403 {
        return Err(SteamError::InvalidKey);
    }
    let r = &v["response"];
    if r.get("game_count").is_none() {
        return Err(SteamError::Private);
    }
    Ok(r["games"].as_array().cloned().unwrap_or_default())
}

/// IPlayerService/GetRecentlyPlayedGames (últimas 2 semanas).
pub async fn recently_played(key: Option<&str>, steamid: &str) -> Result<Vec<Value>, SteamError> {
    let k = need_key(key)?;
    let (_, v) = call("IPlayerService/GetRecentlyPlayedGames/v1/", Some(k), &[("steamid", steamid.into()), ("count", "0".into())]).await?;
    if v["response"].get("total_count").is_none() {
        return Err(SteamError::Private);
    }
    Ok(v["response"]["games"].as_array().cloned().unwrap_or_default())
}

fn stats_error(st: u16, v: &Value) -> Option<SteamError> {
    let msg = v["playerstats"]["error"].as_str().or_else(|| v["error"].as_str()).unwrap_or("").to_lowercase();
    if msg.contains("not public") || msg.contains("private") || st == 403 {
        Some(SteamError::Private)
    } else if msg.contains("no stats") || msg.contains("not found") || st == 400 {
        Some(SteamError::NoStats)
    } else if v["playerstats"]["success"] == Value::Bool(false) {
        Some(SteamError::BadResponse)
    } else {
        None
    }
}

/// ISteamUserStats/GetSchemaForGame: nomes, descrições e ícones das conquistas + nomes das estatísticas.
pub async fn schema(key: Option<&str>, appid: i64, lang: &str) -> Result<Value, SteamError> {
    let k = need_key(key)?;
    let (st, v) = call("ISteamUserStats/GetSchemaForGame/v2/", Some(k), &[("appid", appid.to_string()), ("l", lang.into())]).await?;
    if st == 403 {
        return Err(SteamError::InvalidKey);
    }
    if st == 400 {
        return Err(SteamError::NoStats);
    }
    Ok(v["game"].clone())
}

/// ISteamUserStats/GetPlayerAchievements: estado e data de cada conquista do jogador.
pub async fn player_achievements(key: Option<&str>, steamid: &str, appid: i64) -> Result<Vec<Value>, SteamError> {
    let k = need_key(key)?;
    let (st, v) = call("ISteamUserStats/GetPlayerAchievements/v1/", Some(k), &[("steamid", steamid.into()), ("appid", appid.to_string())]).await?;
    if let Some(e) = stats_error(st, &v) {
        return Err(e);
    }
    Ok(v["playerstats"]["achievements"].as_array().cloned().unwrap_or_default())
}

/// ISteamUserStats/GetUserStatsForGame: estatísticas públicas do jogador no jogo.
pub async fn user_stats(key: Option<&str>, steamid: &str, appid: i64) -> Result<Vec<Value>, SteamError> {
    let k = need_key(key)?;
    let (st, v) = call("ISteamUserStats/GetUserStatsForGame/v2/", Some(k), &[("steamid", steamid.into()), ("appid", appid.to_string())]).await?;
    if let Some(e) = stats_error(st, &v) {
        return Err(e);
    }
    Ok(v["playerstats"]["stats"].as_array().cloned().unwrap_or_default())
}

/// Porcentagem global de cada conquista (não precisa de chave).
pub async fn global_percentages(appid: i64) -> Result<Vec<Value>, SteamError> {
    let (st, v) = call("ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/", None, &[("gameid", appid.to_string())]).await?;
    if st >= 400 {
        return Err(SteamError::NoStats);
    }
    Ok(v["achievementpercentages"]["achievements"].as_array().cloned().unwrap_or_default())
}

/// ISteamNews/GetNewsForApp (sem chave): anúncios oficiais publicados pelo desenvolvedor na Steam
/// (feed `steam_community_announcements`), mais recentes primeiro, com o texto completo (BBCode).
/// App sem notícias ou inexistente: lista vazia.
pub async fn news_for_app(appid: i64, count: u32) -> Result<Vec<Value>, SteamError> {
    let (st, v) = call(
        "ISteamNews/GetNewsForApp/v2/",
        None,
        &[("appid", appid.to_string()), ("count", count.to_string()), ("maxlength", "0".into()), ("feeds", "steam_community_announcements".into())],
    )
    .await?;
    if st >= 400 {
        return Ok(Vec::new());
    }
    Ok(v["appnews"]["newsitems"].as_array().cloned().unwrap_or_default())
}

/// Perfil público em XML (sem chave). Devolve (nome, avatar, privacidade).
pub async fn community_profile(steamid: &str) -> Result<(String, Option<String>, String), SteamError> {
    if let Some(s) = GATE.blocked_for() {
        return Err(SteamError::RateLimited(s));
    }
    GATE.wait().await;
    let resp = http()?
        .get(format!("https://steamcommunity.com/profiles/{steamid}/"))
        .query(&[("xml", "1")])
        .send()
        .await
        .map_err(net_err)?;
    let st = resp.status().as_u16();
    if st == 429 {
        return Err(SteamError::RateLimited(GATE.block(crate::net::retry_after(&resp))));
    }
    if st >= 500 {
        return Err(SteamError::Unavailable(st));
    }
    let body = resp.text().await.map_err(net_err)?;
    parse_community_xml(&body).ok_or(SteamError::BadResponse)
}

fn xml_tag(body: &str, tag: &str) -> Option<String> {
    let start = body.find(&format!("<{tag}>"))? + tag.len() + 2;
    let end = body[start..].find(&format!("</{tag}>"))? + start;
    let raw = body[start..end].trim();
    let raw = raw.strip_prefix("<![CDATA[").and_then(|r| r.strip_suffix("]]>")).unwrap_or(raw);
    Some(raw.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").trim().to_string())
}

pub fn parse_community_xml(body: &str) -> Option<(String, Option<String>, String)> {
    let name = xml_tag(body, "steamID")?;
    let avatar = xml_tag(body, "avatarFull").filter(|a| a.starts_with("https://"));
    let privacy = xml_tag(body, "privacyState").unwrap_or_else(|| "unknown".into());
    Some((name, avatar, privacy))
}

/// Idioma da interface → idioma das APIs da Steam.
pub fn steam_lang(lang: &str) -> &'static str {
    match lang {
        "pt-BR" => "brazilian",
        "es" => "latam",
        "fr" => "french",
        "de" => "german",
        "it" => "italian",
        "ru" => "russian",
        "ja" => "japanese",
        "ko" => "koreana",
        "zh-CN" => "schinese",
        _ => "english",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn interprets_stats_errors() {
        assert_eq!(stats_error(403, &json!({"playerstats": {"error": "Profile is not public", "success": false}})), Some(SteamError::Private));
        assert_eq!(stats_error(400, &json!({"playerstats": {"error": "Requested app has no stats", "success": false}})), Some(SteamError::NoStats));
        assert_eq!(stats_error(200, &json!({"playerstats": {"success": true, "achievements": []}})), None);
    }

    #[test]
    fn community_xml() {
        let x = "<?xml version=\"1.0\"?><profile><steamID64>76561197960287930</steamID64><steamID><![CDATA[Rabscuttle & Co]]></steamID>\
                 <privacyState>public</privacyState><avatarFull><![CDATA[https://avatars.steamstatic.com/abc_full.jpg]]></avatarFull></profile>";
        let (n, a, p) = parse_community_xml(x).unwrap();
        assert_eq!((n.as_str(), a.as_deref(), p.as_str()), ("Rabscuttle & Co", Some("https://avatars.steamstatic.com/abc_full.jpg"), "public"));
        assert!(parse_community_xml("<response><error>The specified profile could not be found.</error></response>").is_none());
    }

    #[test]
    fn messages_never_leak_key() {
        for e in [SteamError::InvalidKey, SteamError::NoKey, SteamError::Private, SteamError::Offline, SteamError::RateLimited(9)] {
            assert!(!e.message().to_lowercase().contains("key="));
        }
        assert_eq!(steam_lang("pt-BR"), "brazilian");
        assert_eq!(steam_lang("xx"), "english");
    }
}
