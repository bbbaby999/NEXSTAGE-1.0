//! IGDB — API oficial (api.igdb.com/v4), autenticação OAuth 2.0 da Twitch (client credentials).
//!
//! - Credenciais (Client ID + Client Secret) ficam cifradas no cofre (ver `sources`); o token de
//!   acesso fica só em memória e é renovado quando expira ou é recusado.
//! - Limite oficial: 4 requisições/s → `Gate` de 260 ms; HTTP 429 bloqueia temporariamente.
//! - Identificação: jogo confirmado (`igdb.id`) → consulta direta; senão busca + `matching`
//!   (o AppID da Steam, quando o jogo tem, é identificador forte); ambíguo → o usuário escolhe.
//! - Dados ficam em `GameInfo.igdb` (cache, offline). Campos personalizados nunca são sobrescritos:
//!   descrição/gêneros/desenvolvedora/lançamento só são preenchidos se estiverem vazios; capa e fundo
//!   só mudam por ação explícita do usuário (`igdb_apply_image`).

use super::matching::{self, Matchable};
use super::Source;
use crate::commands::games::ensure_owned;
use crate::commands::info::{self, GameInfo};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::net::Gate;
use crate::retro::util::{fmt_epoch, now_epoch};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;

const API: &str = "https://api.igdb.com/v4";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
static GATE: Gate = Gate::new(260);
static TOKEN: Mutex<Option<(String, String, Instant)>> = Mutex::new(None); // (client_id, token, expira)

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Creds {
    pub client_id: String,
    pub client_secret: String,
}

pub fn validate_format(cid: &str, sec: &str) -> AppResult<()> {
    let ok = |s: &str| (20..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric());
    if !ok(cid) || !ok(sec) {
        return Err(AppError::Validation("Client ID ou Client Secret em formato inválido (copie do console da Twitch: dev.twitch.tv/console/apps).".into()));
    }
    Ok(())
}

pub fn forget_token() {
    *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
        .build()
        .map_err(|_| AppError::Other("Falha de rede.".into()))
}

/// Erros sem URL nem corpo da requisição (o segredo vai no corpo do pedido de token).
fn offline() -> AppError {
    AppError::Other("IGDB: sem conexão. Os dados já salvos continuam disponíveis.".into())
}

async fn token(c: &reqwest::Client, creds: &Creds) -> AppResult<String> {
    if let Some((cid, tok, exp)) = TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if cid == creds.client_id && exp > Instant::now() {
            return Ok(tok);
        }
    }
    let r = c
        .post(TOKEN_URL)
        .form(&[("client_id", creds.client_id.as_str()), ("client_secret", creds.client_secret.as_str()), ("grant_type", "client_credentials")])
        .send()
        .await
        .map_err(|_| offline())?;
    match r.status().as_u16() {
        400 | 401 | 403 => return Err(AppError::Validation("IGDB: Client ID ou Client Secret recusados pela Twitch.".into())),
        s if s >= 500 => return Err(AppError::Other("Twitch indisponível no momento. Tente novamente mais tarde.".into())),
        _ => {}
    }
    let v: Value = r.json().await.map_err(|_| offline())?;
    let tok = v["access_token"].as_str().ok_or_else(|| AppError::Other("IGDB: resposta inesperada da Twitch.".into()))?.to_string();
    let secs = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(120);
    *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = Some((creds.client_id.clone(), tok.clone(), Instant::now() + Duration::from_secs(secs)));
    Ok(tok)
}

enum QErr {
    App(AppError),
    /// 400: consulta não aceita (ex.: campo que a API renomeou) — permite tentar a versão reduzida.
    BadQuery,
}

async fn query(creds: &Creds, endpoint: &str, body: &str) -> Result<Value, QErr> {
    let c = http().map_err(QErr::App)?;
    for attempt in 0..2 {
        if let Some(s) = GATE.blocked_for() {
            return Err(QErr::App(AppError::Other(format!("IGDB: limite de requisições atingido. Nova tentativa em {s} s."))));
        }
        GATE.wait().await;
        let tok = token(&c, creds).await.map_err(QErr::App)?;
        let r = c
            .post(format!("{API}/{endpoint}"))
            .header("Client-ID", &creds.client_id)
            .header("Authorization", format!("Bearer {tok}"))
            .header("Accept", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| QErr::App(offline()))?;
        match r.status().as_u16() {
            401 | 403 if attempt == 0 => {
                forget_token(); // token expirado/revogado: pede outro uma vez
                continue;
            }
            401 | 403 => return Err(QErr::App(AppError::Validation("IGDB: acesso recusado. Teste as credenciais em Configurações → Fontes de dados.".into()))),
            429 => {
                let s = GATE.block(crate::net::retry_after(&r));
                return Err(QErr::App(AppError::Other(format!("IGDB: limite de requisições atingido. Nova tentativa em {s} s."))));
            }
            400 => return Err(QErr::BadQuery),
            s if s >= 500 => return Err(QErr::App(AppError::Other("IGDB indisponível no momento. Os dados já salvos continuam disponíveis.".into()))),
            _ => return r.json().await.map_err(|_| QErr::App(AppError::Other("IGDB: resposta inesperada.".into()))),
        }
    }
    Err(QErr::App(offline()))
}

/// Consulta com versão reduzida de reserva (a API renomeia campos de tempos em tempos).
async fn query_fallback(creds: &Creds, endpoint: &str, full: &str, reduced: &str) -> AppResult<Value> {
    match query(creds, endpoint, full).await {
        Ok(v) => Ok(v),
        Err(QErr::BadQuery) => match query(creds, endpoint, reduced).await {
            Ok(v) => Ok(v),
            Err(QErr::BadQuery) => Err(AppError::Other("IGDB: a consulta não foi aceita pela API.".into())),
            Err(QErr::App(e)) => Err(e),
        },
        Err(QErr::App(e)) => Err(e),
    }
}

pub async fn test(creds: &Creds) -> AppResult<()> {
    forget_token();
    match query(creds, "games", "fields name; limit 1;").await {
        Ok(_) => Ok(()),
        Err(QErr::App(e)) => Err(e),
        Err(QErr::BadQuery) => Err(AppError::Other("IGDB: a consulta de teste não foi aceita.".into())),
    }
}

/// Plataforma do NEXSTAGE → ID da plataforma no IGDB.
pub fn platform_id(platform: &str) -> Option<i64> {
    Some(match crate::commands::games::canonical_platform(platform)? {
        "PC" => 6,
        "Playstation 1" => 7,
        "Playstation 2" => 8,
        "Playstation 3" => 9,
        "Playstation 4" => 48,
        "Playstation 5" => 167,
        "PSP" => 38,
        "Ps Vita" => 46,
        "XBOX" => 11,
        "Xbox 360" => 12,
        "Xbox Series" => 169,
        "Nintendo Switch" => 130,
        "Nintendo Switch 2" => 508,
        "Wii U" => 41,
        "Wii" => 5,
        "GameCube" => 21,
        "Nintendo 3DS" => 37,
        "Nintendo DS" => 20,
        "Game Boy Advance" => 24,
        "Game Boy Color" => 22,
        "Game Boy" => 33,
        "Nintendo 64" => 4,
        "Mega Drive" => 29,
        "Dreamcast" => 23,
        "Super Nintendo" => 19,
        "Nintendinho" => 18,
        _ => return None,
    })
}

pub fn image(id: &str, size: &str) -> String {
    format!("https://images.igdb.com/igdb/image/upload/{size}/{id}.jpg")
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Video {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct TimeToBeat {
    pub hastily: Option<i64>,
    pub normally: Option<i64>,
    pub completely: Option<i64>,
    pub count: Option<i64>,
}

/// Dados do IGDB guardados nas informações do jogo.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct IgdbData {
    pub id: i64,
    pub name: String,
    pub url: Option<String>,
    pub summary: String,
    pub storyline: String,
    pub cover: Option<String>,
    pub artworks: Vec<String>,
    pub screenshots: Vec<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub genres: Vec<String>,
    pub platforms: Vec<String>,
    pub release_date: Option<String>,
    pub franchises: Vec<String>,
    pub series: Vec<String>,
    /// "ESRB: M", "PEGI: 18"...
    pub age_ratings: Vec<String>,
    pub videos: Vec<Video>,
    pub time_to_beat: Option<TimeToBeat>,
    pub fetched_at: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: i64,
    pub name: String,
    pub year: Option<i64>,
    pub platforms: Vec<String>,
    pub image: Option<String>,
    pub on_platform: bool,
    /// Mesmo AppID da Steam do jogo da biblioteca.
    pub steam_match: bool,
}

impl Matchable for Candidate {
    fn id(&self) -> i64 { self.id }
    fn name(&self) -> &str { &self.name }
    fn year(&self) -> Option<i64> { self.year }
    fn on_platform(&self) -> bool { self.on_platform }
    fn strong(&self) -> bool { self.steam_match }
}

fn strs(v: &Value, path: &str) -> Vec<String> {
    v[path].as_array().into_iter().flatten().filter_map(|x| x["name"].as_str().map(String::from)).collect()
}

/// Algum registro externo é o AppID da Steam informado (campo novo `external_game_source` ou antigo `category`; 1 = Steam).
fn has_steam(g: &Value, appid: Option<i64>) -> bool {
    let Some(a) = appid else { return false };
    g["external_games"].as_array().into_iter().flatten().any(|e| {
        let steam = e["external_game_source"].as_i64() == Some(1) || e["category"].as_i64() == Some(1);
        steam && e["uid"].as_str().map(|u| u == a.to_string()).unwrap_or(false)
    })
}

pub fn parse_candidate(g: &Value, plat: Option<i64>, appid: Option<i64>) -> Option<Candidate> {
    let plats: Vec<&Value> = g["platforms"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    Some(Candidate {
        id: g["id"].as_i64()?,
        name: g["name"].as_str()?.to_string(),
        year: g["first_release_date"].as_i64().map(|t| fmt_epoch(t)[..4].parse().unwrap_or(0)).filter(|y| *y > 1970),
        platforms: plats.iter().filter_map(|p| p["name"].as_str().map(String::from)).collect(),
        image: g["cover"]["image_id"].as_str().map(|i| image(i, "t_cover_small")),
        on_platform: plat.map(|p| plats.iter().any(|x| x["id"].as_i64() == Some(p))).unwrap_or(true),
        steam_match: has_steam(g, appid),
    })
}

/// Data em formato que `games::parse_release` entende ("18 Oct 2005").
fn release_text(epoch: i64) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let d = fmt_epoch(epoch);
    let (y, m, day) = (&d[..4], d[5..7].parse::<usize>().unwrap_or(1), &d[8..10]);
    format!("{} {} {y}", day.trim_start_matches('0'), M[(m.max(1) - 1).min(11)])
}

pub fn parse_detail(g: &Value) -> IgdbData {
    let companies = |role: &str| -> Vec<String> {
        g["involved_companies"].as_array().into_iter().flatten().filter(|c| c[role].as_bool() == Some(true)).filter_map(|c| c["company"]["name"].as_str().map(String::from)).collect()
    };
    let imgs = |k: &str, size: &str| -> Vec<String> {
        g[k].as_array().into_iter().flatten().filter_map(|x| x["image_id"].as_str().map(|i| image(i, size))).take(20).collect()
    };
    let age_ratings = g["age_ratings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let org = a["organization"]["name"].as_str()?;
            let rating = a["rating_category"]["rating"].as_str()?;
            Some(format!("{org}: {rating}"))
        })
        .collect();
    IgdbData {
        id: g["id"].as_i64().unwrap_or(0),
        name: g["name"].as_str().unwrap_or("").to_string(),
        url: g["url"].as_str().filter(|u| u.starts_with("https://")).map(String::from),
        summary: g["summary"].as_str().unwrap_or("").trim().to_string(),
        storyline: g["storyline"].as_str().unwrap_or("").trim().to_string(),
        cover: g["cover"]["image_id"].as_str().map(|i| image(i, "t_cover_big_2x")),
        artworks: imgs("artworks", "t_1080p"),
        screenshots: imgs("screenshots", "t_1080p"),
        developers: companies("developer"),
        publishers: companies("publisher"),
        genres: strs(g, "genres"),
        platforms: strs(g, "platforms"),
        release_date: g["first_release_date"].as_i64().map(release_text),
        franchises: strs(g, "franchises"),
        series: strs(g, "collections"),
        age_ratings,
        videos: g["videos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| Some(Video { id: v["video_id"].as_str()?.to_string(), name: v["name"].as_str().unwrap_or("Trailer").to_string() }))
            .filter(|v| v.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .take(8)
            .collect(),
        time_to_beat: None,
        fetched_at: Some(fmt_epoch(now_epoch())),
    }
}

/// Preenche só o que estiver vazio nas informações do jogo (dados personalizados são preservados).
pub fn fill_empty(info: &mut GameInfo, d: &IgdbData) {
    if info.description.trim().is_empty() && !d.summary.is_empty() {
        info.description = d.summary.chars().take(2000).collect();
    }
    if info.genres.is_empty() {
        info.genres = d.genres.iter().take(8).cloned().collect();
    }
    if info.developers.is_empty() {
        info.developers = d.developers.clone();
    }
    if info.release_date.is_none() {
        info.release_date = d.release_date.clone();
    }
}

const BASE_FIELDS: &str = "name,url,summary,storyline,cover.image_id,artworks.image_id,screenshots.image_id,involved_companies.company.name,involved_companies.developer,involved_companies.publisher,genres.name,platforms.name,platforms.id,first_release_date,franchises.name,collections.name,videos.video_id,videos.name";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbResult {
    /// "found" | "choose" | "not_found" | "disabled"
    pub status: String,
    pub info: GameInfo,
    pub candidates: Vec<Candidate>,
}

/// Busca/atualiza os dados do IGDB do jogo. `igdb_id` = escolha do usuário; `reidentify` ignora o salvo.
#[tauri::command]
pub async fn igdb_fetch(db: State<'_, Db>, profile_id: i64, game_id: i64, term: String, igdb_id: Option<i64>, reidentify: Option<bool>) -> AppResult<IgdbResult> {
    let (creds, platform, appid, mut stored) = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        let platform: String = conn.query_row("SELECT platform FROM games WHERE id=?1", [game_id], |r| r.get(0))?;
        let appid = crate::steam::store::appid_of(&conn, game_id)?.or_else(|| info::load(&conn, game_id).ok().flatten().and_then(|i| i.external_id).and_then(|e| e.parse().ok()));
        (super::active(&conn, Source::Igdb)?, platform, appid, info::load(&conn, game_id)?.unwrap_or_default())
    };
    let Some(creds) = creds.and_then(|c| serde_json::from_str::<Creds>(&c).ok()) else {
        return Ok(IgdbResult { status: "disabled".into(), info: stored, candidates: vec![] });
    };
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let plat = platform_id(&platform);
    let known = if reidentify == Some(true) { None } else { stored.igdb.as_ref().map(|d| d.id).filter(|i| *i > 0) };
    enum Resolve {
        Id(i64),
        NotFound,
        Choose(Vec<Candidate>),
    }
    let result: AppResult<Resolve> = async {
        if let Some(id) = igdb_id.or(known) {
            return Ok(Resolve::Id(id));
        }
        let esc = term.replace('"', "");
        let full = format!("search \"{esc}\"; fields name,first_release_date,platforms.name,platforms.id,cover.image_id,external_games.uid,external_games.external_game_source; limit 15;");
        let reduced = format!("search \"{esc}\"; fields name,first_release_date,platforms.name,platforms.id,cover.image_id,external_games.uid,external_games.category; limit 15;");
        let v = query_fallback(&creds, "games", &full, &reduced).await?;
        let mut cands: Vec<Candidate> = v.as_array().into_iter().flatten().filter_map(|g| parse_candidate(g, plat, appid)).collect();
        let year = stored.release_date.as_deref().and_then(crate::commands::games::parse_release).as_deref().and_then(matching::year_of);
        // "Não é este jogo?": nunca escolhe sozinho — mostra a lista para o usuário decidir
        if reidentify != Some(true) {
            if let Some(id) = matching::pick_confident(&term, &cands, year) {
                return Ok(Resolve::Id(id));
            }
        }
        if cands.is_empty() {
            return Ok(Resolve::NotFound);
        }
        matching::rank_for_display(&term, &mut cands);
        cands.truncate(8);
        Ok(Resolve::Choose(cands))
    }
    .await;
    let id = match result {
        Ok(Resolve::Id(id)) => id,
        Ok(Resolve::NotFound) => return Ok(IgdbResult { status: "not_found".into(), info: stored, candidates: vec![] }),
        Ok(Resolve::Choose(candidates)) => return Ok(IgdbResult { status: "choose".into(), info: stored, candidates }),
        Err(e) => {
            let _ = super::record(&*db.conn()?, Source::Igdb, Err(&e.to_string()));
            return Err(e);
        }
    };
    let full = format!("fields {BASE_FIELDS},age_ratings.organization.name,age_ratings.rating_category.rating; where id = {id};");
    let reduced = format!("fields {BASE_FIELDS}; where id = {id};");
    let v = match query_fallback(&creds, "games", &full, &reduced).await {
        Ok(v) => v,
        Err(e) => {
            let _ = super::record(&*db.conn()?, Source::Igdb, Err(&e.to_string()));
            return Err(e);
        }
    };
    let Some(g) = v.as_array().and_then(|a| a.first()) else {
        // ID salvo não existe mais: identifica de novo na próxima vez
        if let Some(d) = stored.igdb.as_mut() {
            d.id = 0;
        }
        info::store(&*db.conn()?, game_id, &stored)?;
        return Ok(IgdbResult { status: "not_found".into(), info: stored, candidates: vec![] });
    };
    let mut data = parse_detail(g);
    // tempo para zerar (API oficial); falha aqui não invalida o resto
    if let Ok(t) = query(&creds, "game_time_to_beats", &format!("fields hastily,normally,completely,count; where game_id = {id};")).await {
        if let Some(x) = t.as_array().and_then(|a| a.first()) {
            let pos = |k: &str| x[k].as_i64().filter(|v| *v > 0);
            let ttb = TimeToBeat { hastily: pos("hastily"), normally: pos("normally"), completely: pos("completely"), count: pos("count") };
            if ttb.hastily.or(ttb.normally).or(ttb.completely).is_some() {
                data.time_to_beat = Some(ttb);
            }
        }
    }
    fill_empty(&mut stored, &data);
    if stored.source.is_empty() {
        stored.source = "manual".into();
    }
    stored.igdb = Some(data);
    let conn = db.conn()?;
    info::store(&conn, game_id, &stored)?;
    super::record(&conn, Source::Igdb, Ok(()))?;
    Ok(IgdbResult { status: "found".into(), info: stored, candidates: vec![] })
}

/// Pesquisa livre por nome (página Zerei): sem filtro de plataforma e sem gravar nada.
pub async fn search_titles(creds: &Creds, term: &str, limit: u32) -> AppResult<Vec<Candidate>> {
    let esc = term.replace('"', "");
    let q = format!("search \"{esc}\"; fields name,first_release_date,platforms.name,platforms.id,cover.image_id; limit {limit};");
    let v = query_fallback(creds, "games", &q, &q).await?;
    Ok(v.as_array().into_iter().flatten().filter_map(|g| parse_candidate(g, None, None)).collect())
}

/// Vídeos (YouTube) de um jogo identificado pelo código da Microsoft Store (`external_games`, fonte 11 =
/// Microsoft): identificação exata, sem depender do nome. `None` = o IGDB não conhece esse código.
pub async fn videos_by_microsoft_id(creds: &Creds, product_id: &str) -> AppResult<Option<Vec<Video>>> {
    let uid: String = product_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if uid.is_empty() {
        return Ok(None);
    }
    // a API trocou `category` por `external_game_source`: a versão reduzida usa o campo antigo
    let full = format!("fields game; where uid = \"{uid}\" & external_game_source = 11; limit 1;");
    let reduced = format!("fields game; where uid = \"{uid}\" & category = 11; limit 1;");
    let ext = query_fallback(creds, "external_games", &full, &reduced).await?;
    let Some(game) = ext.as_array().and_then(|a| a.first()).and_then(|e| e["game"].as_i64()) else { return Ok(None) };
    let q = format!("fields videos.video_id,videos.name; where id = {game};");
    let v = query_fallback(creds, "games", &q, &q).await?;
    let videos = v.as_array().and_then(|a| a.first()).map(|g| parse_detail(g).videos).unwrap_or_default();
    Ok(Some(videos))
}

/// Usa uma imagem do IGDB como capa ou fundo do jogo — só por ação explícita do usuário.
#[tauri::command]
pub fn igdb_apply_image(db: State<Db>, profile_id: i64, game_id: i64, kind: String, url: String) -> AppResult<()> {
    if !url.starts_with("https://images.igdb.com/") {
        return Err(AppError::Validation("Imagem inválida.".into()));
    }
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    match kind.as_str() {
        "cover" => conn.execute("UPDATE games SET cover=?2 WHERE id=?1", rusqlite::params![game_id, url])?,
        "background" => conn.execute("UPDATE games SET background=?2, bg_x=50, bg_y=50 WHERE id=?1", rusqlite::params![game_id, url])?,
        _ => return Err(AppError::Validation("Tipo de imagem inválido.".into())),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detail() -> Value {
        json!({
            "id": 1942, "name": "The Witcher 3: Wild Hunt", "url": "https://www.igdb.com/games/the-witcher-3-wild-hunt",
            "summary": "RPG de mundo aberto.", "storyline": "Geralt procura Ciri.", "cover": {"image_id": "co1wyy"},
            "artworks": [{"image_id": "ar1"}], "screenshots": [{"image_id": "sc1"}, {"image_id": "sc2"}],
            "involved_companies": [{"company": {"name": "CD Projekt RED"}, "developer": true, "publisher": false},
                                   {"company": {"name": "CD Projekt"}, "developer": false, "publisher": true}],
            "genres": [{"name": "Role-playing (RPG)"}], "platforms": [{"id": 6, "name": "PC (Microsoft Windows)"}],
            "first_release_date": 1431993600, "franchises": [{"name": "The Witcher"}], "collections": [{"name": "The Witcher"}],
            "age_ratings": [{"organization": {"name": "ESRB"}, "rating_category": {"rating": "M"}}],
            "videos": [{"video_id": "c0i88t0Kacs", "name": "Trailer"}, {"video_id": "bad id!", "name": "x"}]
        })
    }

    #[test]
    fn parses_details_safely() {
        let d = parse_detail(&detail());
        assert_eq!(d.developers, vec!["CD Projekt RED"]);
        assert_eq!(d.publishers, vec!["CD Projekt"]);
        assert_eq!(d.age_ratings, vec!["ESRB: M"]);
        assert_eq!(d.videos.len(), 1, "IDs de vídeo inválidos são descartados");
        assert!(d.cover.unwrap().ends_with("/t_cover_big_2x/co1wyy.jpg"));
        assert_eq!(d.release_date.as_deref(), Some("19 May 2015"));
        assert_eq!(crate::commands::games::parse_release("19 May 2015").as_deref(), Some("2015-05-19"));
    }

    #[test]
    fn never_overwrites_custom_data() {
        let d = parse_detail(&detail());
        let mut custom = GameInfo { description: "Minha descrição".into(), genres: vec!["Ação".into()], ..Default::default() };
        fill_empty(&mut custom, &d);
        assert_eq!((custom.description.as_str(), custom.genres.clone()), ("Minha descrição", vec!["Ação".to_string()]));
        assert_eq!(custom.developers, vec!["CD Projekt RED"], "campo vazio é preenchido");
        let mut empty = GameInfo::default();
        fill_empty(&mut empty, &d);
        assert_eq!(empty.description, "RPG de mundo aberto.");
    }

    #[test]
    fn steam_appid_is_a_strong_match() {
        let g = json!({"id": 5, "name": "DOOM", "platforms": [{"id": 6, "name": "PC"}],
                        "external_games": [{"uid": "379720", "external_game_source": 1}]});
        let c = parse_candidate(&g, Some(6), Some(379720)).unwrap();
        assert!(c.steam_match && c.on_platform);
        let legacy = json!({"id": 6, "name": "DOOM", "external_games": [{"uid": "379720", "category": 1}]});
        assert!(parse_candidate(&legacy, Some(6), Some(379720)).unwrap().steam_match);
        assert!(!parse_candidate(&g, Some(6), Some(1)).unwrap().steam_match);
        assert!(validate_format("abc", "def").is_err());
        assert!(validate_format("abcdefghij0123456789abcd", "abcdefghij0123456789abcdefgh").is_ok());
        assert!(crate::commands::games::PLATFORMS.iter().all(|p| platform_id(p).is_some()));
    }
}
