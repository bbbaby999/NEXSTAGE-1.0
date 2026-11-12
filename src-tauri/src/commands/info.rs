//! Informações do jogo (estilo página da Steam): descrição, gêneros, notas,
//! capturas de tela e links. A fonte automática é a loja pública da Steam
//! (sem chave de API), que cobre jogos de PC. Para os demais há edição manual.

use super::games::ensure_owned;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::State;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameInfo {
    pub source: String,
    pub external_id: Option<String>,
    pub matched_name: String,
    pub description: String,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub release_date: Option<String>,
    pub metacritic_score: Option<i64>,
    pub metacritic_url: Option<String>,
    /// Origem da nota do Metacritic: "steam" | "rawg" | "manual" (ver commands/metacritic.rs).
    pub metacritic_source: Option<String>,
    /// Plataforma a que a nota se refere (o Metacritic avalia cada versão separadamente).
    pub metacritic_platform: Option<String>,
    /// Última verificação (mesmo quando o jogo não tem nota publicada).
    pub metacritic_checked_at: Option<String>,
    /// Jogo confirmado no RAWG (identificação estável: as atualizações não dependem mais do nome).
    pub rawg_id: Option<i64>,
    /// Nota geral do RAWG e notas por versão (cache para trocar a plataforma sem rede).
    pub metacritic_overall: Option<i64>,
    pub metacritic_overall_url: Option<String>,
    pub metacritic_platforms: Vec<McPlatform>,
    /// Dados do IGDB (ver sources/igdb.rs): cache, nunca sobrescreve campos personalizados.
    pub igdb: Option<crate::sources::igdb::IgdbData>,
    pub review_summary: Option<String>,
    pub review_positive: Option<i64>,
    pub review_negative: Option<i64>,
    pub screenshots: Vec<String>,
    pub header_image: Option<String>,
    pub store_url: Option<String>,
    pub hltb: Option<Hltb>,
    /// Screenshots encontradas na internet (fora da Steam): ver `webshots.rs`.
    pub web_screenshots: Vec<super::webshots::WebShot>,
    /// Screenshots que o usuário removeu da galeria do NEXSTAGE (URLs). Não voltam em novas buscas.
    pub removed_screenshots: Vec<String>,
}

/// Metascore de uma versão do jogo (plataforma do RAWG).
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct McPlatform {
    pub slug: String,
    pub name: String,
    pub score: i64,
    pub url: Option<String>,
}

fn net(e: reqwest::Error) -> AppError {
    AppError::Other(format!("Falha de rede ao consultar a Steam: {e}"))
}

pub(crate) fn load(conn: &Connection, game_id: i64) -> AppResult<Option<GameInfo>> {
    let data: Option<String> = conn
        .query_row("SELECT data FROM game_info WHERE game_id=?1", [game_id], |r| r.get(0))
        .optional()?;
    Ok(data.and_then(|d| serde_json::from_str(&d).ok()))
}

/// Uma atualização da Steam sem Metascore não apaga a nota obtida pelo RAWG ou digitada à mão.
pub(crate) fn keep_metacritic(new: &mut GameInfo, old: &GameInfo) {
    if new.metacritic_score.is_none() && old.metacritic_score.is_some() && old.metacritic_source.as_deref() != Some("steam") {
        new.metacritic_score = old.metacritic_score;
        new.metacritic_url = old.metacritic_url.clone();
        new.metacritic_source = old.metacritic_source.clone();
        new.metacritic_platform = old.metacritic_platform.clone();
    }
    if new.metacritic_checked_at.is_none() {
        new.metacritic_checked_at = old.metacritic_checked_at.clone();
    }
    // dados do IGDB nunca vêm da Steam: preserva sempre
    if new.igdb.is_none() {
        new.igdb = old.igdb.clone();
    }
    // identificação e notas do RAWG nunca vêm da Steam: preserva sempre
    new.rawg_id = new.rawg_id.or(old.rawg_id);
    if new.metacritic_platforms.is_empty() {
        new.metacritic_platforms = old.metacritic_platforms.clone();
        new.metacritic_overall = new.metacritic_overall.or(old.metacritic_overall);
        new.metacritic_overall_url = new.metacritic_overall_url.clone().or_else(|| old.metacritic_overall_url.clone());
    }
}

pub(crate) fn store(conn: &Connection, game_id: i64, info: &GameInfo) -> AppResult<()> {
    let json = serde_json::to_string(info).map_err(|e| AppError::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO game_info(game_id,data) VALUES(?1,?2)
         ON CONFLICT(game_id) DO UPDATE SET data=excluded.data, fetched_at=datetime('now')",
        params![game_id, json],
    )?;
    Ok(())
}

/// Remove tags HTML simples e entidades comuns.
fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"").replace("&amp;", "&").replace("&#39;", "'").replace("&nbsp;", " ")
}

fn translate_review(s: &str) -> Option<String> {
    Some(
        match s {
            "Overwhelmingly Positive" => "Extremamente positivas",
            "Very Positive" => "Muito positivas",
            "Positive" => "Positivas",
            "Mostly Positive" => "Majoritariamente positivas",
            "Mixed" => "Mistas",
            "Mostly Negative" => "Majoritariamente negativas",
            "Negative" => "Negativas",
            "Very Negative" => "Muito negativas",
            "Overwhelmingly Negative" => "Extremamente negativas",
            _ => return None,
        }
        .to_string(),
    )
}

async fn get_json(client: &reqwest::Client, url: &str, q: &[(&str, &str)]) -> AppResult<Value> {
    client
        .get(url)
        .query(q)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?
        .json::<Value>()
        .await
        .map_err(net)
}

fn steam_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(12)).user_agent(concat!("NEXSTAGE/", env!("NX_VERSION"))).build().map_err(net)
}

/// Busca na loja da Steam (resultados brutos de `storesearch`).
async fn steam_search(client: &reqwest::Client, term: &str) -> AppResult<Vec<Value>> {
    let search = get_json(client, "https://store.steampowered.com/api/storesearch/", &[("term", term), ("l", "brazilian"), ("cc", "br")]).await?;
    Ok(search["items"].as_array().cloned().unwrap_or_default())
}

/// Busca + escolha + detalhes. `strict`: só aceita correspondência segura (usado pela classificação
/// automática e pelo Metacritic). A busca manual da página do jogo NÃO usa isto: mostra a lista
/// (`steam_store_search`) e o usuário escolhe.
pub(crate) async fn steam_lookup(term: &str, strict: bool) -> AppResult<GameInfo> {
    let client = steam_client()?;
    let items = steam_search(&client, term).await?;
    let id = if strict {
        pick_match(term, &items)
    } else {
        items.first().and_then(|i| i["id"].as_i64())
    }
    .ok_or_else(|| {
        AppError::NotFound(if strict {
            "Sem correspondência segura na Steam.".into()
        } else {
            "Não encontrei esse jogo na Steam. Tente outro nome.".into()
        })
    })?;
    steam_details(&client, id, term, strict).await
}

/// Detalhes de um app da loja da Steam (descrição, gêneros, notas, capturas, avaliações).
async fn steam_details(client: &reqwest::Client, id: i64, term: &str, strict: bool) -> AppResult<GameInfo> {
    let ids = id.to_string();

    let det = get_json(
        &client,
        "https://store.steampowered.com/api/appdetails",
        &[("appids", ids.as_str()), ("l", "brazilian"), ("cc", "br")],
    )
    .await?;
    let d = &det[ids.as_str()]["data"];
    if d.is_null() {
        return Err(AppError::NotFound("A Steam não retornou dados para esse jogo.".into()));
    }
    if strict && d["type"].as_str() != Some("game") {
        return Err(AppError::NotFound("A correspondência não é um jogo base (pode ser DLC ou demo).".into()));
    }

    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let genres = d["genres"]
        .as_array()
        .map(|a| a.iter().filter_map(|g| g["description"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    let screenshots = d["screenshots"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["path_full"].as_str().map(String::from)).take(12).collect())
        .unwrap_or_default();

    let mut info = GameInfo {
        source: "steam".into(),
        external_id: Some(ids.clone()),
        matched_name: d["name"].as_str().unwrap_or(term).to_string(),
        description: clean(d["short_description"].as_str().unwrap_or("")),
        genres,
        developers: strings(&d["developers"]),
        release_date: d["release_date"]["date"].as_str().map(String::from),
        metacritic_score: d["metacritic"]["score"].as_i64(),
        metacritic_url: d["metacritic"]["url"].as_str().map(String::from),
        metacritic_source: d["metacritic"]["score"].as_i64().map(|_| "steam".to_string()),
        metacritic_platform: d["metacritic"]["score"].as_i64().map(|_| "PC".to_string()),
        screenshots,
        header_image: d["header_image"].as_str().map(String::from),
        store_url: Some(format!("https://store.steampowered.com/app/{ids}")),
        ..Default::default()
    };

    // Avaliações dos jogadores (opcional: falhas aqui não invalidam o restante).
    let url = format!("https://store.steampowered.com/appreviews/{ids}");
    if let Ok(rev) = get_json(
        &client,
        &url,
        &[("json", "1"), ("language", "all"), ("purchase_type", "all"), ("num_per_page", "0")],
    )
    .await
    {
        let q = &rev["query_summary"];
        info.review_positive = q["total_positive"].as_i64();
        info.review_negative = q["total_negative"].as_i64();
        info.review_summary = q["review_score_desc"].as_str().and_then(translate_review);
    }
    Ok(info)
}

#[tauri::command]
pub fn get_game_info(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<Option<GameInfo>> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    load(&conn, game_id)
}

#[tauri::command]
pub async fn fetch_game_info(
    db: State<'_, Db>,
    profile_id: i64,
    game_id: i64,
    term: String,
    appid: Option<i64>,
) -> AppResult<GameInfo> {
    {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
    }
    let term = term.trim().to_string();
    if term.is_empty() && appid.is_none() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    // jogo escolhido na lista de resultados (sem appid: comportamento antigo, primeiro resultado)
    let mut info = match appid {
        Some(id) if id > 0 => steam_details(&steam_client()?, id, &term, false).await?,
        _ => steam_lookup(&term, false).await?,
    };
    let conn = db.conn()?;
    // Preserva descrição/gêneros digitados manualmente se a Steam não trouxe nada.
    if let Some(old) = load(&conn, game_id)? {
        keep_metacritic(&mut info, &old);
        info.hltb = old.hltb;
        info.web_screenshots = old.web_screenshots;
        info.removed_screenshots = old.removed_screenshots;
        info.screenshots.retain(|u| !info.removed_screenshots.contains(u));
        if info.description.is_empty() {
            info.description = old.description;
        }
        if info.genres.is_empty() {
            info.genres = old.genres;
        }
    }
    store(&conn, game_id, &info)?;
    Ok(info)
}

/// Resultado da busca na loja da Steam para o usuário escolher.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreHit {
    pub appid: i64,
    pub name: String,
    pub image: Option<String>,
    pub price: Option<String>,
    pub platforms: Vec<String>,
}

pub(crate) fn parse_store_hits(items: &[Value]) -> Vec<StoreHit> {
    items
        .iter()
        .filter(|i| i["type"].as_str().unwrap_or("app") == "app")
        .filter_map(|i| {
            let appid = i["id"].as_i64()?;
            let plats = &i["platforms"];
            let platforms = [("windows", "Windows"), ("mac", "macOS"), ("linux", "Linux")]
                .iter()
                .filter(|(k, _)| plats[*k].as_bool() == Some(true))
                .map(|(_, l)| l.to_string())
                .collect();
            let price = i["price"]["final"].as_i64().map(|c| format!("R$ {:.2}", c as f64 / 100.0).replace('.', ","));
            Some(StoreHit {
                appid,
                name: i["name"].as_str()?.to_string(),
                image: Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg")),
                price,
                platforms,
            })
        })
        .collect()
}

/// Lista de jogos encontrados na loja da Steam para o nome informado (o usuário escolhe um).
#[tauri::command]
pub async fn steam_store_search(term: String) -> AppResult<Vec<StoreHit>> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let items = steam_search(&steam_client()?, &term).await?;
    Ok(parse_store_hits(&items))
}

#[tauri::command]
pub fn save_manual_info(
    db: State<Db>,
    profile_id: i64,
    game_id: i64,
    description: String,
    genres: Vec<String>,
) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    let mut info = load(&conn, game_id)?.unwrap_or_default();
    if info.source.is_empty() {
        info.source = "manual".into();
    }
    info.description = description.trim().chars().take(2000).collect();
    info.genres = genres
        .into_iter()
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .take(8)
        .collect();
    store(&conn, game_id, &info)?;
    Ok(info)
}

/// Baixa a capa vertical da Steam e a grava como capa do jogo.
#[tauri::command]
pub async fn apply_steam_cover(db: State<'_, Db>, profile_id: i64, game_id: i64) -> AppResult<()> {
    let ext = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        load(&conn, game_id)?.and_then(|i| i.external_id)
    }
    .ok_or_else(|| AppError::Validation("Busque as informações na Steam antes de usar a capa.".into()))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
        .build()
        .map_err(net)?;
    let url = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{ext}/library_600x900.jpg");
    let resp = client.get(&url).send().await.map_err(net)?;
    if !resp.status().is_success() {
        return Err(AppError::NotFound("A Steam não tem capa vertical para esse jogo.".into()));
    }
    let bytes = resp.bytes().await.map_err(net)?;
    if bytes.len() > 3_000_000 {
        return Err(AppError::Other("A capa baixada é grande demais.".into()));
    }
    let cover = crate::images::store_bytes(&bytes, "image/jpeg")?;
    let conn = db.conn()?;
    conn.execute("UPDATE games SET cover=?1 WHERE id=?2", params![cover, game_id])?;
    Ok(())
}

/// Tempo para zerar (segundos). `source`: "manual" ou "howlongtobeat" (obtido por versões antigas;
/// mantido, identificado como tal). Os tempos do IGDB ficam em `IgdbData::time_to_beat`.
/// `updated_at`: quando os tempos foram gravados.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Hltb {
    pub source: String,
    pub game_id: Option<i64>,
    pub name: String,
    pub main_seconds: Option<i64>,
    pub extra_seconds: Option<i64>,
    pub completionist_seconds: Option<i64>,
    pub all_seconds: Option<i64>,
    pub updated_at: Option<String>,
}

// Consulta automática ao HowLongToBeat REMOVIDA: usava o endpoint interno do site e contornava a
// proteção anti-automação dele. Ver `sources::hltb` (provedor preparado e desativado).

/// Tempos digitados à mão (em horas).
#[tauri::command]
pub fn save_manual_hltb(
    db: State<Db>,
    profile_id: i64,
    game_id: i64,
    main: Option<f64>,
    extra: Option<f64>,
    completionist: Option<f64>,
    all: Option<f64>,
) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    let secs = |h: Option<f64>| h.filter(|v| *v > 0.0 && *v < 100000.0).map(|v| (v * 3600.0).round() as i64);
    let mut info = load(&conn, game_id)?.unwrap_or_default();
    if info.source.is_empty() {
        info.source = "manual".into();
    }
    let (m, e, c, a) = (secs(main), secs(extra), secs(completionist), secs(all));
    info.hltb = if m.is_none() && e.is_none() && c.is_none() && a.is_none() {
        None
    } else {
        Some(Hltb {
            source: "manual".into(),
            game_id: None,
            name: String::new(),
            main_seconds: m,
            extra_seconds: e,
            completionist_seconds: c,
            all_seconds: a,
            updated_at: Some(crate::retro::util::fmt_epoch(crate::retro::util::now_epoch())),
        })
    };
    store(&conn, game_id, &info)?;
    Ok(info)
}

// ---------- classificação automática (correspondência estrita) ----------

pub(crate) fn tokens(s: &str) -> Vec<String> {
    let mut t = s.to_lowercase();
    for p in [
        "™", "®", "©", "game of the year edition", "game of the year", "goty edition", "goty",
        "definitive edition", "complete edition", "ultimate edition", "deluxe edition",
        "enhanced edition", "remastered", "director's cut", "directors cut", "special edition",
        "anniversary edition", "digital edition", "gold edition",
    ] {
        t = t.replace(p, " ");
    }
    t.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "the")
        .map(|w| {
            match w {
                "ii" => "2", "iii" => "3", "iv" => "4", "vi" => "6", "vii" => "7", "viii" => "8", "ix" => "9",
                _ => w,
            }
            .to_string()
        })
        .collect()
}

/// 1.0 = nomes equivalentes após normalizar; 0.0 = números diferentes (sequências distintas).
pub(crate) fn name_score(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let sa: HashSet<String> = tokens(a).into_iter().collect();
    let sb: HashSet<String> = tokens(b).into_iter().collect();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let nums = |s: &HashSet<String>| -> HashSet<String> {
        s.iter().filter(|t| t.chars().all(|c| c.is_ascii_digit())).cloned().collect()
    };
    if nums(&sa) != nums(&sb) {
        return 0.0;
    }
    if sa == sb {
        return 1.0;
    }
    sa.intersection(&sb).count() as f64 / sa.union(&sb).count() as f64
}

/// Escolhe o resultado da busca só quando há segurança: nome equivalente e único, ou muito
/// parecido (>= 0.8) com folga sobre o segundo colocado. Caso contrário, `None`.
pub(crate) fn pick_match(term: &str, items: &[Value]) -> Option<i64> {
    let mut scored: Vec<(f64, i64)> = items
        .iter()
        .take(8)
        .filter(|i| i["type"].as_str().unwrap_or("app") == "app")
        .filter_map(|i| Some((name_score(term, i["name"].as_str()?), i["id"].as_i64()?)))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let best = *scored.first()?;
    if best.0 >= 1.0 {
        return if scored.iter().filter(|s| s.0 >= 1.0).count() == 1 { Some(best.1) } else { None };
    }
    let second = scored.get(1).map(|s| s.0).unwrap_or(0.0);
    if best.0 >= 0.8 && best.0 - second >= 0.1 { Some(best.1) } else { None }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyResult {
    /// "updated" | "skipped" (já tinha gêneros) | "uncertain" (sem correspondência segura)
    pub status: String,
    pub matched_name: Option<String>,
    pub genres: Vec<String>,
}

/// Classifica o jogo pelos gêneros da Steam. Nunca sobrescreve gêneros já existentes (inclusive
/// os editados à mão) e não grava nada quando a correspondência não é segura.
#[tauri::command]
pub async fn auto_classify(db: State<'_, Db>, profile_id: i64, game_id: i64, term: String) -> AppResult<ClassifyResult> {
    let old = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        load(&conn, game_id)?
    };
    if let Some(o) = &old {
        if !o.genres.is_empty() {
            return Ok(ClassifyResult { status: "skipped".into(), matched_name: None, genres: o.genres.clone() });
        }
    }
    let mut info = match steam_lookup(term.trim(), true).await {
        Ok(i) => i,
        Err(AppError::NotFound(_)) => {
            return Ok(ClassifyResult { status: "uncertain".into(), matched_name: None, genres: vec![] })
        }
        Err(e) => return Err(e),
    };
    if let Some(o) = old {
        keep_metacritic(&mut info, &o);
        if !o.description.is_empty() {
            info.description = o.description;
        }
        info.hltb = o.hltb;
        info.web_screenshots = o.web_screenshots;
        info.removed_screenshots = o.removed_screenshots;
        info.screenshots.retain(|u| !info.removed_screenshots.contains(u));
    }
    let conn = db.conn()?;
    store(&conn, game_id, &info)?;
    Ok(ClassifyResult { status: "updated".into(), matched_name: Some(info.matched_name.clone()), genres: info.genres.clone() })
}

#[cfg(test)]
mod match_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn store_hits_list_only_apps() {
        let items = vec![
            json!({"type": "app", "id": 620, "name": "Portal 2", "price": {"final": 3699}, "platforms": {"windows": true, "mac": true, "linux": false}}),
            json!({"type": "sub", "id": 1, "name": "Pacote"}),
            json!({"type": "app", "id": 400, "name": "Portal"}),
        ];
        let h = parse_store_hits(&items);
        assert_eq!(h.len(), 2, "pacotes (sub/bundle) ficam de fora");
        assert_eq!((h[0].appid, h[0].price.as_deref(), h[0].platforms.clone()), (620, Some("R$ 36,99"), vec!["Windows".to_string(), "macOS".to_string()]));
        assert!(h[1].image.as_deref().unwrap().ends_with("/400/header.jpg"));
    }

    #[test]
    fn exact_after_normalization_is_accepted() {
        let items = vec![
            json!({"id":1,"name":"The Witcher® 3: Wild Hunt","type":"app"}),
            json!({"id":2,"name":"The Witcher 3: Wild Hunt - Blood and Wine","type":"app"}),
        ];
        assert_eq!(pick_match("Witcher 3 Wild Hunt GOTY", &items), Some(1));
    }

    #[test]
    fn different_numbers_or_extra_words_are_rejected() {
        let items = vec![json!({"id":3,"name":"Resident Evil 3","type":"app"})];
        assert_eq!(pick_match("Resident Evil 4", &items), None);
        let items = vec![json!({"id":4,"name":"DOOM Eternal","type":"app"})];
        assert_eq!(pick_match("Doom", &items), None);
    }

    #[test]
    fn ambiguous_exact_matches_are_rejected() {
        let items = vec![json!({"id":5,"name":"Prey","type":"app"}), json!({"id":6,"name":"Prey","type":"app"})];
        assert_eq!(pick_match("Prey", &items), None);
    }
}

// ---------- Fandom ----------

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn is_fandom_url(u: &str) -> bool {
    u.strip_prefix("https://")
        .map(|r| r.split('/').next().unwrap_or(""))
        .map(|host| host.ends_with(".fandom.com") && !host.starts_with('.'))
        .unwrap_or(false)
}

/// Subdomínios prováveis da wiki do jogo no Fandom, do mais específico ao mais geral:
/// título inteiro ("eldenring", "elden-ring"), sem subtítulo ("Elden Ring: Nightreign" → "eldenring")
/// e a franquia (primeira palavra relevante: "The Witcher 3" → "witcher").
fn fandom_candidates(title: &str) -> Vec<String> {
    let clean: String = {
        let mut out = String::new();
        let mut depth = 0;
        for c in title.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = (depth - 1).max(0),
                _ if depth > 0 => {}
                c => out.push(c),
            }
        }
        out
    };
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_ascii_lowercase()).collect()
    };
    let all = words(&clean);
    let main = words(clean.split([':', '–', '—']).next().unwrap_or(&clean));
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if s.len() >= 3 && !out.contains(&s) {
            out.push(s);
        }
    };
    push(all.join(""));
    push(all.join("-"));
    push(main.join(""));
    if let Some(f) = all.iter().find(|w| !matches!(w.as_str(), "the" | "a" | "an" | "of") && !w.chars().all(|c| c.is_ascii_digit())) {
        push(f.clone());
    }
    out.truncate(5);
    out
}

/// Busca de verdade do Fandom (todas as wikis) — usada quando nenhuma wiki é confirmada.
fn fandom_search_url(title: &str) -> String {
    format!("https://community.fandom.com/wiki/Special:Search?query={}&scope=cross-wiki", pct(title))
}

/// Procura a página do jogo no Fandom pela API oficial do MediaWiki de cada wiki (`api.php`).
/// As páginas do Fandom bloqueiam acesso automatizado (Cloudflare) e a antiga API de busca de wikis
/// também; a API de cada wiki continua aberta. Para cada subdomínio provável: confirma que a wiki
/// existe (`siteinfo`) e procura o artigo do jogo nela (`opensearch`); aceita só nome equivalente.
async fn fandom_lookup(title: &str) -> Option<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(6)).user_agent(concat!("NEXSTAGE/", env!("NX_VERSION"))).build().ok()?;
    let main_title = title.split([':', '–', '—']).next().unwrap_or(title).trim().to_string();
    for sub in fandom_candidates(title) {
        let api = format!("https://{sub}.fandom.com/api.php");
        let Ok(r) = client.get(&api).query(&[("action", "query"), ("meta", "siteinfo"), ("siprop", "general"), ("format", "json")]).send().await else { continue };
        if !r.status().is_success() {
            continue; // 404 = a wiki não existe
        }
        let Ok(site) = r.json::<Value>().await else { continue };
        let general = &site["query"]["general"];
        let Some(sitename) = general["sitename"].as_str() else { continue };
        // artigo do jogo dentro da wiki (ex.: wiki da franquia "witcher" → "The Witcher 3: Wild Hunt")
        if let Ok(r) = client.get(&api).query(&[("action", "opensearch"), ("search", title), ("limit", "5"), ("namespace", "0"), ("format", "json")]).send().await {
            if let Ok(v) = r.json::<Value>().await {
                let names = v[1].as_array().cloned().unwrap_or_default();
                let urls = v[3].as_array().cloned().unwrap_or_default();
                for (n, u) in names.iter().zip(urls.iter()) {
                    let (Some(n), Some(u)) = (n.as_str(), u.as_str()) else { continue };
                    if (name_score(title, n) >= 0.8 || name_score(&main_title, n) >= 1.0) && is_fandom_url(u) {
                        return Some(u.to_string());
                    }
                }
            }
        }
        // sem artigo com o nome, mas a própria wiki é do jogo (ex.: "Elden Ring Wiki")
        let wiki_name = sitename.replace("Wiki", "").replace("wiki", "");
        if name_score(title, &wiki_name) >= 1.0 || name_score(&main_title, &wiki_name) >= 1.0 {
            let base = general["base"].as_str().map(String::from).unwrap_or_else(|| format!("https://{sub}.fandom.com/"));
            if is_fandom_url(&base) {
                return Some(base);
            }
        }
    }
    None
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FandomLink {
    pub url: String,
    /// true = página/wiki identificada; false = busca do Fandom com o título.
    pub direct: bool,
}

#[tauri::command]
pub async fn fandom_link(title: String) -> AppResult<FandomLink> {
    let t = title.trim().to_string();
    if t.is_empty() {
        return Err(AppError::Validation("Jogo sem título.".into()));
    }
    Ok(match fandom_lookup(&t).await {
        Some(url) => FandomLink { url, direct: true },
        // a página inicial do Fandom ignora "?s=": sem wiki confirmada, abre a busca real em todas as wikis
        None => FandomLink { url: fandom_search_url(&t), direct: false },
    })
}

#[cfg(test)]
mod fandom_tests {
    use super::*;

    #[test]
    fn encodes_search_and_checks_hosts() {
        assert_eq!(pct("Elden Ring: Nightreign"), "Elden+Ring%3A+Nightreign");
        assert!(is_fandom_url("https://eldenring.fandom.com/"));
        assert!(!is_fandom_url("https://evil.com/eldenring.fandom.com/"));
        assert!(!is_fandom_url("http://eldenring.fandom.com/"));
    }

    #[test]
    fn wiki_candidates_and_search_url() {
        assert_eq!(fandom_candidates("Elden Ring: Nightreign"), vec!["eldenringnightreign", "elden-ring-nightreign", "eldenring", "elden"]);
        assert_eq!(fandom_candidates("The Witcher 3: Wild Hunt (GOTY)"), vec!["thewitcher3wildhunt", "the-witcher-3-wild-hunt", "thewitcher3", "witcher"]);
        assert!(fandom_search_url("Elden Ring").starts_with("https://community.fandom.com/wiki/Special:Search?query=Elden+Ring"));
        assert!(!fandom_search_url("x").contains("www.fandom.com/?s="), "nunca a página inicial");
    }

    /// Contra o Fandom real: cargo test fandom_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fandom_live() {
        tauri::async_runtime::block_on(async {
            for t in ["Elden Ring", "The Witcher 3: Wild Hunt", "Hollow Knight", "Jogo Que Nao Existe Zzq"] {
                println!("{t} -> {:?}", fandom_lookup(t).await);
            }
        });
    }
}
