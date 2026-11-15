//! Serviços do Xbox Live usados pelo NEXSTAGE (somente leitura, com o token XSTS do próprio usuário):
//! - titlehub: histórico de jogos (nome, imagem, família do pacote, resumo de conquistas, último acesso);
//! - achievements: lista de conquistas de um jogo (com data de desbloqueio e raridade);
//! - userstats: minutos jogados por jogo (estatística `MinutesPlayed`);
//! - profile: gamertag, imagem e gamerscore.
//! Esses serviços não têm documentação pública oficial; respostas inesperadas viram erro tratável,
//! nunca pânico. Ritmo: intervalo mínimo entre chamadas e pausa no HTTP 429.

use super::auth::Xsts;
use crate::error::{AppError, AppResult};
use crate::net::Gate;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

static GATE: Gate = Gate::new(350);

fn http() -> AppResult<&'static reqwest::Client> {
    static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| AppError::Other("Falha de rede.".into()))
}

/// Idioma das respostas (nomes e descrições de conquistas).
pub fn xbox_lang(lang: &str) -> &'static str {
    match lang {
        "en" => "en-US",
        "es" => "es-ES",
        "fr" => "fr-FR",
        "de" => "de-DE",
        "it" => "it-IT",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "ru" => "ru-RU",
        "zh-CN" => "zh-CN",
        _ => "pt-BR",
    }
}

async fn call(req: reqwest::RequestBuilder) -> AppResult<Value> {
    if let Some(s) = GATE.blocked_for() {
        return Err(AppError::Other(format!("O Xbox Live pediu uma pausa nas consultas. Tente de novo em {s} s.")));
    }
    GATE.wait().await;
    let resp = req.send().await.map_err(|_| AppError::Other("Sem conexão com o Xbox Live. Verifique a internet.".into()))?;
    match resp.status().as_u16() {
        200..=299 => resp.json().await.map_err(|_| AppError::Other("Resposta inesperada do Xbox Live.".into())),
        401 | 403 => Err(AppError::Validation("O Xbox Live recusou o acesso. Entre novamente na conta Xbox em Configurações.".into())),
        429 => {
            let s = GATE.block(crate::net::retry_after(&resp));
            Err(AppError::Other(format!("O Xbox Live pediu uma pausa nas consultas. Tente de novo em {s} s.")))
        }
        404 => Ok(Value::Null),
        _ => Err(AppError::Other("O Xbox Live não respondeu como esperado. Tente mais tarde.".into())),
    }
}

fn get(x: &Xsts, url: &str, contract: &str, lang: &str) -> AppResult<reqwest::RequestBuilder> {
    Ok(http()?
        .get(url)
        .header("Authorization", x.header())
        .header("x-xbl-contract-version", contract)
        .header("Accept-Language", lang)
        .header("Accept", "application/json"))
}

/// Histórico de jogos da conta (só itens do tipo jogo).
pub async fn title_history(x: &Xsts, lang: &str) -> AppResult<Vec<Value>> {
    let url = format!("https://titlehub.xboxlive.com/users/xuid({})/titles/titlehistory/decoration/achievement,image,detail?maxItems=1000", x.xuid);
    let v = call(get(x, &url, "2", lang)?).await?;
    Ok(v["titles"].as_array().cloned().unwrap_or_default().into_iter().filter(|t| t["type"].as_str() == Some("Game")).collect())
}

/// Conquistas de um jogo (paginadas pelo `continuationToken`; até 10 páginas).
pub async fn achievements(x: &Xsts, title_id: &str, lang: &str) -> AppResult<Vec<Value>> {
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..10 {
        let mut url = format!("https://achievements.xboxlive.com/users/xuid({})/achievements?titleId={title_id}&maxItems=500", x.xuid);
        if let Some(t) = &token {
            url.push_str(&format!("&continuationToken={}", urlencode(t)));
        }
        let v = call(get(x, &url, "2", lang)?).await?;
        out.extend(v["achievements"].as_array().cloned().unwrap_or_default());
        token = v["pagingInfo"]["continuationToken"].as_str().filter(|t| !t.is_empty()).map(String::from);
        if token.is_none() {
            break;
        }
    }
    Ok(out)
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Minutos jogados por jogo (`MinutesPlayed`), em lotes. Jogos sem a estatística ficam de fora.
pub async fn minutes_played(x: &Xsts, title_ids: &[String]) -> AppResult<HashMap<String, i64>> {
    let mut out = HashMap::new();
    for chunk in title_ids.chunks(50) {
        let body = json!({
            "arrangebyfield": "xuid",
            "xuids": [x.xuid],
            "stats": chunk.iter().map(|t| json!({ "name": "MinutesPlayed", "titleId": t })).collect::<Vec<_>>(),
        });
        let req = http()?
            .post("https://userstats.xboxlive.com/batch")
            .header("Authorization", x.header())
            .header("x-xbl-contract-version", "2")
            .json(&body);
        let v = call(req).await?;
        out.extend(parse_minutes(&v));
    }
    Ok(out)
}

fn parse_minutes(v: &Value) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for coll in v["statlistscollection"].as_array().into_iter().flatten() {
        for s in coll["stats"].as_array().into_iter().flatten() {
            let id = s["titleid"].as_str().or_else(|| s["titleId"].as_str()).unwrap_or("");
            let val = s["value"].as_str().and_then(|v| v.parse::<i64>().ok()).or_else(|| s["value"].as_i64());
            if let (false, Some(m)) = (id.is_empty(), val) {
                out.insert(id.to_string(), m);
            }
        }
    }
    out
}

/// Perfil público da própria conta: (gamertag, imagem, gamerscore).
pub async fn profile(x: &Xsts) -> AppResult<(Option<String>, Option<String>, Option<i64>)> {
    let url = format!("https://profile.xboxlive.com/users/xuid({})/profile/settings?settings=Gamertag,GameDisplayPicRaw,Gamerscore", x.xuid);
    let v = call(get(x, &url, "3", "en-US")?).await?;
    let mut gt = None;
    let mut pic = None;
    let mut gs = None;
    for s in v["profileUsers"][0]["settings"].as_array().into_iter().flatten() {
        let val = s["value"].as_str().unwrap_or("");
        match s["id"].as_str().unwrap_or("") {
            "Gamertag" if !val.is_empty() => gt = Some(val.to_string()),
            "GameDisplayPicRaw" if val.starts_with("https://") => pic = Some(val.to_string()),
            "Gamerscore" => gs = val.parse().ok(),
            _ => {}
        }
    }
    Ok((gt, pic, gs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_from_batch() {
        let v = json!({ "statlistscollection": [{ "arrangebyfieldid": "1", "stats": [
            { "name": "MinutesPlayed", "titleid": "1234", "value": "605" },
            { "name": "MinutesPlayed", "titleid": "99", "value": null }
        ] }] });
        let m = parse_minutes(&v);
        assert_eq!(m.get("1234"), Some(&605));
        assert!(!m.contains_key("99"));
    }

    #[test]
    fn encodes_continuation() {
        assert_eq!(urlencode("a+b/c="), "a%2Bb%2Fc%3D");
    }
}
