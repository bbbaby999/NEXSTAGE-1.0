//! Nexus Mods — API oficial: v1 REST (validação da chave, categorias) e v2 GraphQL (pesquisa de
//! mods, filtros, requisitos, arquivos). A chave pessoal vai no cabeçalho `apikey` (nunca em URL,
//! log ou mensagem); o app se identifica com `Application-Name`/`Application-Version`.
//!
//! Escopo desta versão: DESCOBERTA e CONSULTA. Nada é baixado, instalado, substituído ou modificado;
//! "Abrir no Nexus Mods" leva à página oficial. `supports_vortex` é exibido e a camada `ModManager`
//! fica prevista para uma integração futura autorizada (ex.: Vortex) — sem tocar em executáveis,
//! DLLs ou componentes anti-cheat.
//!
//! Correspondência: os dados do Nexus não trazem AppID da Steam e o nome sozinho não basta (ex.:
//! "Skyrim" tem versões PC original, Special Edition, PS4 e Switch). Por isso o vínculo jogo ↔ domínio
//! do Nexus é SEMPRE confirmado pelo usuário; o app só sugere candidatos.
//! Conteúdo adulto fica fora das buscas. Respostas em cache (`source_cache`) para poupar requisições.

use super::Source;
use crate::commands::games::ensure_owned;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::net::Gate;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::State;

const V1: &str = "https://api.nexusmods.com/v1";
const V2: &str = "https://api.nexusmods.com/v2/graphql";
static GATE: Gate = Gate::new(600);

/// Integração futura com gerenciadores autorizados (Vortex etc.). Nenhuma implementação nesta versão:
/// o NEXSTAGE não instala mods.
#[allow(dead_code)]
pub trait ModManager {
    fn name(&self) -> &'static str;
    /// Abre o fluxo oficial do gerenciador para o mod (ex.: link `nxm://` tratado pelo Vortex).
    fn open(&self, domain: &str, mod_id: i64) -> AppResult<()>;
}

pub fn validate_format(key: &str) -> AppResult<()> {
    if key.len() < 20 || key.len() > 512 || !key.chars().all(|c| c.is_ascii_alphanumeric() || "+/=-_.".contains(c)) {
        return Err(AppError::Validation("Chave da API do Nexus Mods em formato inválido (copie em nexusmods.com → Configurações → Chaves de API).".into()));
    }
    Ok(())
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
        .build()
        .map_err(|_| offline())
}

fn offline() -> AppError {
    AppError::Other("Nexus Mods: sem conexão. Os dados já carregados continuam disponíveis.".into())
}

fn headers(rb: reqwest::RequestBuilder, key: &str) -> reqwest::RequestBuilder {
    rb.header("apikey", key).header("Application-Name", "NEXSTAGE").header("Application-Version", env!("NX_VERSION")).header("Accept", "application/json")
}

async fn check(r: reqwest::Response) -> AppResult<Value> {
    match r.status().as_u16() {
        401 | 403 => Err(AppError::Validation("Nexus Mods: chave da API recusada. Confira em Configurações → Fontes de dados.".into())),
        404 => Ok(Value::Null),
        429 => {
            let s = GATE.block(crate::net::retry_after(&r));
            Err(AppError::Other(format!("Nexus Mods: limite de requisições atingido. Nova tentativa em {s} s.")))
        }
        s if s >= 500 => Err(AppError::Other("Nexus Mods indisponível no momento. Tente novamente mais tarde.".into())),
        _ => r.json().await.map_err(|_| AppError::Other("Nexus Mods: resposta inesperada.".into())),
    }
}

async fn v1(key: &str, path: &str) -> AppResult<Value> {
    if let Some(s) = GATE.blocked_for() {
        return Err(AppError::Other(format!("Nexus Mods: limite de requisições atingido. Nova tentativa em {s} s.")));
    }
    GATE.wait().await;
    let r = headers(client()?.get(format!("{V1}{path}")), key).send().await.map_err(|_| offline())?;
    check(r).await
}

async fn gql(key: &str, query: &str, variables: Value) -> AppResult<Value> {
    if let Some(s) = GATE.blocked_for() {
        return Err(AppError::Other(format!("Nexus Mods: limite de requisições atingido. Nova tentativa em {s} s.")));
    }
    GATE.wait().await;
    let r = headers(client()?.post(V2), key).json(&json!({ "query": query, "variables": variables })).send().await.map_err(|_| offline())?;
    let v = check(r).await?;
    if let Some(e) = v["errors"].as_array().and_then(|a| a.first()) {
        let msg = e["message"].as_str().unwrap_or("erro");
        return Err(AppError::Other(format!("Nexus Mods: consulta recusada ({}).", msg.chars().take(120).collect::<String>())));
    }
    Ok(v["data"].clone())
}

/// Valida a chave (v1 `users/validate`). Devolve só dados públicos (sem a chave nem o e-mail).
pub async fn validate(key: &str) -> AppResult<Value> {
    let v = v1(key, "/users/validate.json").await?;
    let name = v["name"].as_str().ok_or_else(|| AppError::Other("Nexus Mods: resposta inesperada ao validar a chave.".into()))?;
    Ok(json!({ "user": name, "premium": v["is_premium"].as_bool().unwrap_or(false), "supporter": v["is_supporter"].as_bool().unwrap_or(false) }))
}

fn key(conn: &rusqlite::Connection) -> AppResult<String> {
    super::active(conn, Source::Nexus)?.ok_or_else(|| AppError::Validation("Ative o Nexus Mods em Configurações → Fontes de dados.".into()))
}

fn fail(db: &Db, e: &AppError) {
    if let Ok(c) = db.conn() {
        let _ = super::record(&c, Source::Nexus, Err(&e.to_string()));
    }
}
fn ok(db: &Db) {
    if let Ok(c) = db.conn() {
        let _ = super::record(&c, Source::Nexus, Ok(()));
    }
}

// ---------------------------------------------------------------- jogos e vínculo

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NexusGame {
    pub id: i64,
    pub name: String,
    pub domain: String,
    pub mod_count: i64,
}

pub fn parse_games(v: &Value) -> Vec<NexusGame> {
    v["games"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some(NexusGame { id: g["id"].as_i64()?, name: g["name"].as_str()?.into(), domain: g["domainName"].as_str()?.into(), mod_count: g["modCount"].as_i64().unwrap_or(0) }))
        .collect()
}

/// Termo de busca do jogo: palavras principais, sem tags de região/edição entre parênteses.
pub fn game_term(title: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            c if c.is_alphanumeric() || c == ' ' => out.push(c),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn search_games(db: &Db, key: &str, term: &str) -> AppResult<Vec<NexusGame>> {
    let t = game_term(term);
    if t.is_empty() {
        return Ok(vec![]);
    }
    let ck = format!("games:{}", t.to_lowercase());
    if let Some(v) = super::cache_get(&*db.conn()?, Source::Nexus, &ck, 86400)? {
        return Ok(parse_games(&v));
    }
    // O filtro WILDCARD do Nexus aceita o texto puro (com espaços); asteriscos explícitos zeram o resultado.
    // Poucos resultados: busca também pela palavra mais marcante, para mostrar todas as versões
    // (ex.: "The Elder Scrolls V Skyrim" sozinho só traz a Special Edition, não o Skyrim original).
    let q = "query($filter: GamesSearchFilter, $sort: [GamesSearchSort!], $count: Int) { games(filter: $filter, sort: $sort, count: $count) { nodes { id name domainName modCount } } }";
    let vars = |value: &str| json!({ "filter": { "name": [{ "value": value, "op": "WILDCARD" }] }, "sort": [{ "mods": { "direction": "DESC" } }], "count": 10 });
    let mut v = gql(key, q, vars(&t.to_lowercase())).await?;
    if parse_games(&v).len() < 3 {
        if let Some(word) = t.split_whitespace().filter(|w| w.len() >= 4 && !matches!(w.to_lowercase().as_str(), "edition" | "remastered" | "definitive" | "complete" | "scrolls" | "elder")).max_by_key(|w| w.len()) {
            let extra = gql(key, q, vars(&word.to_lowercase())).await?;
            let mut nodes = v["games"]["nodes"].as_array().cloned().unwrap_or_default();
            for n in extra["games"]["nodes"].as_array().into_iter().flatten() {
                if !nodes.iter().any(|x| x["id"] == n["id"]) {
                    nodes.push(n.clone());
                }
            }
            v = json!({ "games": { "nodes": nodes } });
        }
    }
    super::cache_put(&*db.conn()?, Source::Nexus, &ck, &v)?;
    Ok(parse_games(&v))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStatus {
    pub enabled: bool,
    pub link: Option<NexusGame>,
    /// Sugestões (nunca vinculadas sem confirmação).
    pub candidates: Vec<NexusGame>,
    pub error: Option<String>,
}

fn link_of(conn: &rusqlite::Connection, game_id: i64) -> AppResult<Option<NexusGame>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row("SELECT nexus_game_id, name, domain FROM nexus_links WHERE game_id=?1", [game_id], |r| {
            Ok(NexusGame { id: r.get(0)?, name: r.get(1)?, domain: r.get(2)?, mod_count: 0 })
        })
        .optional()?)
}

#[tauri::command]
pub async fn nexus_link_status(db: State<'_, Db>, profile_id: i64, game_id: i64) -> AppResult<LinkStatus> {
    let (k, link, title) = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        let title: String = conn.query_row("SELECT title FROM games WHERE id=?1", [game_id], |r| r.get(0))?;
        (super::active(&conn, Source::Nexus)?, link_of(&conn, game_id)?, title)
    };
    let Some(k) = k else { return Ok(LinkStatus { enabled: false, link, candidates: vec![], error: None }) };
    if link.is_some() {
        return Ok(LinkStatus { enabled: true, link, candidates: vec![], error: None });
    }
    match search_games(&db, &k, &title).await {
        Ok(c) => Ok(LinkStatus { enabled: true, link: None, candidates: c, error: None }),
        Err(e) => {
            fail(&db, &e);
            Ok(LinkStatus { enabled: true, link: None, candidates: vec![], error: Some(e.to_string()) })
        }
    }
}

#[tauri::command]
pub async fn nexus_search_games(db: State<'_, Db>, term: String) -> AppResult<Vec<NexusGame>> {
    let k = key(&*db.conn()?)?;
    search_games(&db, &k, &term).await.inspect_err(|e| fail(&db, e))
}

/// Confirma (ou desfaz, com `domain = None`) o jogo do Nexus correspondente.
#[tauri::command]
pub fn nexus_set_link(db: State<Db>, profile_id: i64, game_id: i64, domain: Option<String>, nexus_game_id: Option<i64>, name: Option<String>) -> AppResult<()> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    match (domain, nexus_game_id) {
        (Some(d), Some(id)) if !d.is_empty() && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') => {
            conn.execute(
                "INSERT INTO nexus_links(game_id, domain, nexus_game_id, name) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(game_id) DO UPDATE SET domain=excluded.domain, nexus_game_id=excluded.nexus_game_id, name=excluded.name",
                rusqlite::params![game_id, d, id, name.unwrap_or_default()],
            )?;
        }
        (None, _) => {
            conn.execute("DELETE FROM nexus_links WHERE game_id=?1", [game_id])?;
        }
        _ => return Err(AppError::Validation("Jogo do Nexus Mods inválido.".into())),
    }
    Ok(())
}

// ---------------------------------------------------------------- mods

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModItem {
    pub mod_id: i64,
    pub domain: String,
    pub name: String,
    pub summary: String,
    pub picture: Option<String>,
    pub thumbnail: Option<String>,
    pub author: String,
    pub uploader: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub downloads: Option<i64>,
    pub endorsements: Option<i64>,
    pub category: Option<String>,
    pub version: Option<String>,
    pub supports_vortex: bool,
    pub url: String,
}

fn https(v: &Value) -> Option<String> {
    v.as_str().filter(|u| u.starts_with("https://")).map(String::from)
}

pub fn parse_mod(m: &Value, domain: &str) -> Option<ModItem> {
    let id = m["modId"].as_i64()?;
    Some(ModItem {
        mod_id: id,
        domain: domain.into(),
        name: m["name"].as_str()?.trim().to_string(),
        summary: m["summary"].as_str().unwrap_or("").trim().to_string(),
        picture: https(&m["pictureUrl"]),
        thumbnail: https(&m["thumbnailUrl"]).or_else(|| https(&m["pictureUrl"])),
        author: m["author"].as_str().unwrap_or("").to_string(),
        uploader: m["uploader"]["name"].as_str().unwrap_or("").to_string(),
        created_at: m["createdAt"].as_str().map(String::from),
        updated_at: m["updatedAt"].as_str().map(String::from),
        downloads: m["downloads"].as_i64(),
        endorsements: m["endorsements"].as_i64(),
        category: m["modCategory"]["name"].as_str().map(String::from),
        version: m["version"].as_str().filter(|v| !v.is_empty()).map(String::from),
        supports_vortex: m["supportsVortex"].as_bool().unwrap_or(false),
        url: format!("https://www.nexusmods.com/{domain}/mods/{id}"),
    })
}

/// Ordenação da interface → campo do Nexus.
pub fn sort_field(sort: &str) -> &'static str {
    match sort {
        "downloads" => "downloads",
        "endorsements" => "endorsements",
        "updated" => "updatedAt",
        "new" => "createdAt",
        _ => "uniqueDownloads", // "popular": jogadores únicos que baixaram
    }
}

pub fn mods_filter(domain: &str, query: &str, category: &str) -> Value {
    let mut f = json!({ "op": "AND", "gameDomainName": [{ "value": domain, "op": "EQUALS" }], "adultContent": [{ "value": false, "op": "EQUALS" }] });
    let q = query.trim();
    if !q.is_empty() {
        f["nameStemmed"] = json!([{ "value": q.chars().take(80).collect::<String>(), "op": "MATCHES" }]);
    }
    if !category.trim().is_empty() {
        f["categoryName"] = json!([{ "value": category.trim(), "op": "EQUALS" }]);
    }
    f
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsPage {
    pub mods: Vec<ModItem>,
    pub total: i64,
    pub cached: bool,
}

const MOD_FIELDS: &str = "modId name summary pictureUrl thumbnailUrl author uploader { name } createdAt updatedAt downloads endorsements modCategory { name } version supportsVortex";

#[tauri::command]
pub async fn nexus_mods(db: State<'_, Db>, profile_id: i64, game_id: i64, query: String, sort: String, category: String, offset: i64) -> AppResult<ModsPage> {
    let (k, link) = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        (key(&conn)?, link_of(&conn, game_id)?)
    };
    let link = link.ok_or_else(|| AppError::Validation("Confirme primeiro qual é o jogo no Nexus Mods.".into()))?;
    let offset = offset.clamp(0, 10_000);
    let ck = format!("mods:{}:{}:{}:{}:{}", link.domain, query.trim().to_lowercase(), sort, category.trim(), offset);
    let parse = |v: &Value| -> (Vec<ModItem>, i64) {
        let mods = v["mods"]["nodes"].as_array().into_iter().flatten().filter_map(|m| parse_mod(m, &link.domain)).collect();
        (mods, v["mods"]["totalCount"].as_i64().unwrap_or(0))
    };
    if let Some(v) = super::cache_get(&*db.conn()?, Source::Nexus, &ck, 20 * 60)? {
        let (mods, total) = parse(&v);
        return Ok(ModsPage { mods, total, cached: true });
    }
    let q = format!("query($filter: ModsFilter, $sort: [ModsSort!], $count: Int, $offset: Int) {{ mods(filter: $filter, sort: $sort, count: $count, offset: $offset) {{ totalCount nodes {{ {MOD_FIELDS} }} }} }}");
    let vars = json!({ "filter": mods_filter(&link.domain, &query, &category), "sort": [{ sort_field(&sort): { "direction": "DESC" } }], "count": 24, "offset": offset });
    let v = gql(&k, &q, vars).await.inspect_err(|e| fail(&db, e))?;
    super::cache_put(&*db.conn()?, Source::Nexus, &ck, &v)?;
    ok(&db);
    let (mods, total) = parse(&v);
    Ok(ModsPage { mods, total, cached: false })
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub name: String,
    pub version: String,
    pub category: String,
    pub date: Option<i64>,
    pub size_kb: Option<i64>,
    pub primary: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    pub name: String,
    pub url: Option<String>,
    pub notes: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetail {
    pub files: Vec<ModFile>,
    pub requirements: Vec<Requirement>,
    /// Imagens que o autor colocou na descrição do mod ([img]…[/img]), na ordem, só https.
    pub images: Vec<String>,
}

/// Hosts de botões de doação/redes sociais (não são screenshots do mod).
const NOT_SCREENSHOT: [&str; 8] = ["patreon.com", "ko-fi.com", "paypal", "buymeacoffee", "discord", "shields.io", "twitter.com", "youtube.com"];

/// Extrai as URLs das tags [img]…[/img] (com ou sem atributos) da descrição em BBCode.
pub fn description_images(desc: &str) -> Vec<String> {
    let lower = desc.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut pos = 0;
    while let Some(i) = lower[pos..].find("[img") {
        let start = pos + i;
        let Some(close) = lower[start..].find(']') else { break };
        let body = start + close + 1;
        let Some(end) = lower[body..].find("[/img]") else { break };
        let url = desc[body..body + end].trim();
        pos = body + end + 6;
        let ok = url.starts_with("https://") && url.len() < 600 && !url.chars().any(|c| c.is_whitespace() || c == '"' || c == '<');
        let lu = url.to_ascii_lowercase();
        if ok && !NOT_SCREENSHOT.iter().any(|h| lu.contains(h)) && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
            if out.len() >= 40 {
                break;
            }
        }
    }
    out
}

pub fn parse_detail(v: &Value) -> ModDetail {
    let mut files: Vec<ModFile> = v["modFiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["category"].as_str() != Some("ARCHIVED") && f["category"].as_str() != Some("DELETED"))
        .filter_map(|f| {
            Some(ModFile {
                name: f["name"].as_str()?.into(),
                version: f["version"].as_str().unwrap_or("").into(),
                category: f["category"].as_str().unwrap_or("").into(),
                date: f["date"].as_i64(),
                size_kb: f["size"].as_i64(),
                primary: f["primary"].as_i64().unwrap_or(0) == 1,
            })
        })
        .collect();
    files.sort_by(|a, b| b.date.cmp(&a.date));
    let requirements = v["mod"]["modRequirements"]["nexusRequirements"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some(Requirement { name: r["modName"].as_str()?.into(), url: https(&r["url"]), notes: r["notes"].as_str().unwrap_or("").into() }))
        .collect();
    let images = v["mod"]["description"].as_str().map(description_images).unwrap_or_default();
    ModDetail { files, requirements, images }
}

/// Versões disponíveis (arquivos) e requisitos de um mod.
#[tauri::command]
pub async fn nexus_mod_detail(db: State<'_, Db>, nexus_game_id: i64, mod_id: i64) -> AppResult<ModDetail> {
    let k = key(&*db.conn()?)?;
    let ck = format!("detail2:{nexus_game_id}:{mod_id}"); // v2 do cache: inclui a descrição
    if let Some(v) = super::cache_get(&*db.conn()?, Source::Nexus, &ck, 3600)? {
        return Ok(parse_detail(&v));
    }
    let v = gql(
        &k,
        "query($modId: ID!, $gameId: ID!) { mod(modId: $modId, gameId: $gameId) { description modRequirements { nexusRequirements { nodes { modName url notes } } } } modFiles(modId: $modId, gameId: $gameId) { name version category date size primary } }",
        json!({ "modId": mod_id.to_string(), "gameId": nexus_game_id.to_string() }),
    )
    .await
    .inspect_err(|e| fail(&db, e))?;
    super::cache_put(&*db.conn()?, Source::Nexus, &ck, &v)?;
    Ok(parse_detail(&v))
}

/// Categorias de mods do jogo (v1 com a chave; cache de 7 dias).
#[tauri::command]
pub async fn nexus_categories(db: State<'_, Db>, domain: String) -> AppResult<Vec<String>> {
    if domain.is_empty() || !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::Validation("Jogo do Nexus Mods inválido.".into()));
    }
    let k = key(&*db.conn()?)?;
    let ck = format!("categories:{domain}");
    let cached = super::cache_get(&*db.conn()?, Source::Nexus, &ck, 7 * 86400)?; // trava do banco liberada antes da rede
    let v = match cached {
        Some(v) => v,
        None => {
            let v = v1(&k, &format!("/games/{domain}.json")).await.inspect_err(|e| fail(&db, e))?;
            super::cache_put(&*db.conn()?, Source::Nexus, &ck, &v)?;
            v
        }
    };
    let mut names: Vec<String> = v["categories"].as_array().into_iter().flatten().filter_map(|c| c["name"].as_str().map(String::from)).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

// ---------------------------------------------------------------- favoritos

#[tauri::command]
pub fn nexus_favorites(db: State<Db>, profile_id: i64, domain: Option<String>) -> AppResult<Vec<Value>> {
    let conn = db.conn()?;
    let mut st = conn.prepare("SELECT data FROM nexus_favorites WHERE profile_id=?1 AND (?2 IS NULL OR domain=?2) ORDER BY added_at DESC")?;
    let rows = st.query_map(rusqlite::params![profile_id, domain], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().filter_map(|d| serde_json::from_str(&d).ok()).collect())
}

/// Adiciona/remove dos favoritos. Devolve `true` se ficou favoritado.
#[tauri::command]
pub fn nexus_toggle_favorite(db: State<Db>, profile_id: i64, item: Value) -> AppResult<bool> {
    let (Some(domain), Some(mod_id)) = (item["domain"].as_str(), item["modId"].as_i64()) else {
        return Err(AppError::Validation("Mod inválido.".into()));
    };
    let conn = db.conn()?;
    let n = conn.execute("DELETE FROM nexus_favorites WHERE profile_id=?1 AND domain=?2 AND mod_id=?3", rusqlite::params![profile_id, domain, mod_id])?;
    if n > 0 {
        return Ok(false);
    }
    conn.execute("INSERT INTO nexus_favorites(profile_id, domain, mod_id, data) VALUES(?1, ?2, ?3, ?4)", rusqlite::params![profile_id, domain, mod_id, item.to_string()])?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_shapes() {
        let games = json!({"games": {"nodes": [{"id": 1704, "name": "Skyrim Special Edition", "domainName": "skyrimspecialedition", "modCount": 120000}, {"id": 110, "name": "Skyrim", "domainName": "skyrim", "modCount": 73201}]}});
        let g = parse_games(&games);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].domain, "skyrimspecialedition");
        let m = json!({"modId": 12604, "name": "SkyUI", "summary": "Interface", "pictureUrl": "https://staticdelivery.nexusmods.com/x.jpg", "thumbnailUrl": "javascript:alert(1)",
            "uploader": {"name": "schlangster"}, "downloads": 26411632, "endorsements": 507788, "updatedAt": "2026-05-05T22:41:21Z", "modCategory": {"name": "User Interface"}, "version": "6.9", "supportsVortex": true});
        let item = parse_mod(&m, "skyrimspecialedition").unwrap();
        assert_eq!(item.url, "https://www.nexusmods.com/skyrimspecialedition/mods/12604", "link oficial do mod");
        assert_eq!(item.thumbnail.as_deref(), Some("https://staticdelivery.nexusmods.com/x.jpg"), "URL não-https é descartada");
        assert_eq!((item.downloads, item.endorsements), (Some(26411632), Some(507788)));
        let d = parse_detail(&json!({
            "mod": {"modRequirements": {"nexusRequirements": {"nodes": [{"modName": "SKSE64", "url": "", "notes": ""}]}}},
            "modFiles": [{"name": "Old", "version": "5.1", "category": "ARCHIVED", "date": 1, "size": 10, "primary": 0},
                         {"name": "SkyUI", "version": "6.9", "category": "MAIN", "date": 9, "size": 2573, "primary": 1},
                         {"name": "Patch", "version": "1.0", "category": "OPTIONAL", "date": 5, "size": 1, "primary": 0}]}));
        assert_eq!(d.requirements[0].name, "SKSE64");
        assert_eq!(d.requirements[0].url, None);
        assert_eq!(d.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["SkyUI", "Patch"], "arquivados ficam de fora; mais recentes primeiro");
    }

    #[test]
    fn extracts_description_images() {
        let d = "[center][img]https://staticdelivery.nexusmods.com/a.png[/img][/center] [IMG width=50%] https://images.steamusercontent.com/ugc/1/ [/IMG] [img]http://insecure.jpg[/img] [img]https://c5.patreon.com/b.png[/img] [img]https://staticdelivery.nexusmods.com/a.png[/img] [img]javascript:alert(1)[/img]";
        assert_eq!(description_images(d), vec!["https://staticdelivery.nexusmods.com/a.png", "https://images.steamusercontent.com/ugc/1/"]);
        assert!(description_images("[img]sem fim").is_empty());
    }

    #[test]
    fn filters_and_terms() {
        let f = mods_filter("skyrimspecialedition", " map ", "User Interface");
        assert_eq!(f["adultContent"][0]["value"], json!(false), "conteúdo adulto sempre fora");
        assert_eq!(f["nameStemmed"][0]["value"], json!("map"));
        assert_eq!(f["categoryName"][0]["value"], json!("User Interface"));
        assert!(mods_filter("x", "", "").get("nameStemmed").is_none());
        assert_eq!(sort_field("endorsements"), "endorsements");
        assert_eq!(sort_field("qualquer"), "uniqueDownloads");
        assert_eq!(game_term("The Elder Scrolls V: Skyrim (USA) [!]"), "The Elder Scrolls V Skyrim");
        assert!(validate_format("curta").is_err());
        assert!(validate_format("abcDEF123456789+/=abcdefghijKLMN").is_ok());
    }

    #[test]
    fn favorites_and_links_in_db() {
        let c = super::super::tests::mem();
        c.execute_batch("INSERT INTO games(id, title, platform) VALUES(1, 'Skyrim', 'PC');").unwrap();
        c.execute("INSERT INTO nexus_links(game_id, domain, nexus_game_id, name) VALUES(1, 'skyrimspecialedition', 1704, 'Skyrim SE')", []).unwrap();
        let l = link_of(&c, 1).unwrap().unwrap();
        assert_eq!((l.domain.as_str(), l.id), ("skyrimspecialedition", 1704));
        assert!(link_of(&c, 2).unwrap().is_none());
    }
/// Contra a API real (v2 é pública para leitura): cargo test nexus_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn nexus_live() {
        tauri::async_runtime::block_on(async {
            let c = client().unwrap();
            let post = |q: String, v: Value| { let c = c.clone(); async move {
                let r: Value = c.post(V2).header("Application-Name", "NEXSTAGE").json(&json!({"query": q, "variables": v})).send().await.unwrap().json().await.unwrap();
                assert!(r["errors"].is_null(), "erro da API: {}", r["errors"]);
                r["data"].clone()
            }};
            let g = post("query($filter: GamesSearchFilter, $sort: [GamesSearchSort!], $count: Int) { games(filter: $filter, sort: $sort, count: $count) { nodes { id name domainName modCount } } }".into(),
                json!({"filter": {"name": [{"value": "skyrim", "op": "WILDCARD"}]}, "sort": [{"mods": {"direction": "DESC"}}], "count": 10})).await;
            let games = parse_games(&g);
            println!("jogos skyrim: {:?}", games.iter().map(|x| (&x.name, &x.domain)).collect::<Vec<_>>());
            assert!(games.len() > 1, "ambíguo de propósito: precisa de confirmação do usuário");
            let q = format!("query($filter: ModsFilter, $sort: [ModsSort!], $count: Int, $offset: Int) {{ mods(filter: $filter, sort: $sort, count: $count, offset: $offset) {{ totalCount nodes {{ {MOD_FIELDS} }} }} }}");
            let m = post(q, json!({"filter": mods_filter("skyrimspecialedition", "", ""), "sort": [{sort_field("endorsements"): {"direction": "DESC"}}], "count": 3, "offset": 0})).await;
            let mods: Vec<ModItem> = m["mods"]["nodes"].as_array().unwrap().iter().filter_map(|x| parse_mod(x, "skyrimspecialedition")).collect();
            for x in &mods { println!("  {} · {:?} downloads · {:?} endossos · {:?} · {}", x.name, x.downloads, x.endorsements, x.category, x.url); }
            assert_eq!(mods.len(), 3);
            let d = post("query($modId: ID!, $gameId: ID!) { mod(modId: $modId, gameId: $gameId) { description modRequirements { nexusRequirements { nodes { modName url notes } } } } modFiles(modId: $modId, gameId: $gameId) { name version category date size primary } }".into(),
                json!({"modId": mods[0].mod_id.to_string(), "gameId": "1704"})).await;
            let det = parse_detail(&d);
            println!("  requisitos de {}: {:?}", mods[0].name, det.requirements.iter().map(|r| &r.name).collect::<Vec<_>>());
            println!("  versões: {:?}", det.files.iter().take(3).map(|f| (&f.name, &f.version, &f.category)).collect::<Vec<_>>());
            assert!(!det.files.is_empty());
            println!("  screenshots da descrição: {} (ex.: {:?})", det.images.len(), det.images.first());
            assert!(!det.images.is_empty(), "a descrição do SkyUI traz screenshots");
            assert!(det.images.iter().all(|u| u.starts_with("https://") && !u.contains("patreon")));
        });
    }
}
