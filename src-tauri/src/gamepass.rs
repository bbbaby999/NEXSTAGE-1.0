//! Catálogo do PC Game Pass na Home (EXPERIMENTAL): "Em breve", "Adicionados recentemente" e "Saindo".
//!
//! Fontes: as listas públicas que o próprio site xbox.com usa (`catalog.gamepass.com/sigls/v2`) e o
//! catálogo público da Microsoft Store (`displaycatalog.mp.microsoft.com`) para nome, imagem, editora e
//! data. Nenhuma das duas pede login e nada da conta do usuário é enviado; mas NÃO são APIs documentadas:
//! podem mudar ou sair do ar sem aviso. Por isso:
//!  - a resposta é validada (lista certa, formato esperado); qualquer surpresa vira "indisponível" e a
//!    seção some da Home, sem afetar o resto do app;
//!  - cache de 12 h no banco; falhas mantêm o último catálogo válido;
//!  - nada roda na inicialização: a consulta só começa quando a Home pede, em segundo plano;
//!  - a seção pode ser desligada (Configurações → Contas e plataformas → Xbox).
//!
//! O catálogo não traz a data em que um jogo sai do Game Pass: a lista "Saindo" diz só que sai em breve.
//! Jogos são abertos na Microsoft Store pelo protocolo oficial `ms-windows-store://pdp/?ProductId=`.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS gamepass_cache (
    list        TEXT PRIMARY KEY,           -- 'coming' | 'added' | 'leaving'
    market      TEXT NOT NULL,
    lang        TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    data        TEXT NOT NULL               -- lista JSON de `Item`
);
-- Página de um jogo do catálogo (detalhes completos da Microsoft Store), validade de 24 h.
CREATE TABLE IF NOT EXISTS gamepass_detail (
    id          TEXT PRIMARY KEY,           -- ProductId da Microsoft Store
    market      TEXT NOT NULL,
    lang        TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    data        TEXT NOT NULL               -- JSON de `Detail`
);
"#;

/// Listas do PC Game Pass (ids das listas públicas do xbox.com, versão PC).
pub const LISTS: [(&str, &str); 3] = [
    ("coming", "4165f752-d702-49c8-886b-fb57936f6bae"),
    ("added", "3fdd7f57-7092-4b65-bd40-5a9dac1b2b84"),
    ("leaving", "cc7fc951-d00f-410e-9e02-5e4628e04163"),
];
pub const SETTING: &str = "gamepass_home";
const TTL: i64 = 12 * 3600;
const MANUAL_TTL: i64 = 10 * 60;
const MAX_PER_LIST: usize = 60;
const CHUNK: usize = 40;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub title: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    /// epoch do lançamento do jogo (None = sem data anunciada).
    pub release: Option<i64>,
    /// Último dia da oferta do Game Pass no catálogo ("AAAA-MM-DD"). Só significa SAÍDA para os jogos da lista
    /// "Saindo" (nos demais, a Microsoft renova essa data periodicamente). Ver `leave_date`.
    #[serde(default)]
    pub leaves_on: Option<String>,
}

// ---------- idioma e região ----------

/// Idioma do catálogo a partir do idioma do NEXSTAGE.
pub fn store_lang(app_lang: &str) -> &'static str {
    match app_lang {
        "pt-BR" => "pt-br",
        "es" => "es-es",
        "fr" => "fr-fr",
        "de" => "de-de",
        "it" => "it-it",
        "ru" => "ru-ru",
        "ja" => "ja-jp",
        "ko" => "ko-kr",
        "zh-CN" => "zh-cn",
        _ => "en-us",
    }
}

/// Região (mercado) do Windows: "pt-BR" → "BR". Sem região informada: "US".
pub fn market_from_locale(locale: &str) -> String {
    locale
        .split(['-', '_'])
        .skip(1)
        .find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .map(|p| p.to_ascii_uppercase())
        .unwrap_or_else(|| "US".into())
}

#[cfg(windows)]
fn windows_locale() -> String {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, cch: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 1 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..(n - 1) as usize])
}
#[cfg(not(windows))]
fn windows_locale() -> String {
    String::new()
}

// ---------- consulta ----------

fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(8))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
        .build()
        .map_err(|_| AppError::Other("Game Pass: sem conexão.".into()))
}

async fn get_json(client: &reqwest::Client, url: &str, q: &[(&str, &str)]) -> AppResult<Value> {
    let resp = client.get(url).query(q).send().await.map_err(|_| AppError::Other("Game Pass: sem conexão.".into()))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(AppError::Other(format!("Game Pass: catálogo indisponível no momento (HTTP {status}).")));
    }
    resp.json::<Value>().await.map_err(|_| AppError::Other("Game Pass: o catálogo respondeu num formato inesperado.".into()))
}

fn valid_id(id: &str) -> bool {
    id.len() == 12 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Ids de produto de uma lista. `None` = formato inesperado (ou outra lista no lugar da pedida).
pub fn parse_list(v: &Value, sigl: &str) -> Option<Vec<String>> {
    let a = v.as_array()?;
    let head = a.first()?;
    if !head["siglId"].as_str()?.eq_ignore_ascii_case(sigl) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for x in &a[1..] {
        if let Some(id) = x["id"].as_str().map(str::trim).filter(|i| valid_id(i)) {
            let id = id.to_ascii_uppercase();
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out.truncate(MAX_PER_LIST);
    Some(out)
}

fn epoch(s: &str) -> Option<i64> {
    // "2027-02-23T00:00:00.0000000Z" → dias desde 1970 (algoritmo civil) + horário
    let (y, m, d) = (s.get(0..4)?.parse::<i64>().ok()?, s.get(5..7)?.parse::<i64>().ok()?, s.get(8..10)?.parse::<i64>().ok()?);
    if !(1970..9000).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None; // 9998-12-30 = data ainda não anunciada
    }
    let (hh, mm) = (s.get(11..13).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0), s.get(14..16).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0));
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86400 + hh * 3600 + mm * 60)
}

fn image_url(uri: &str) -> Option<String> {
    image_sized(uri, 360)
}

/// Imagem da Store em https, pedida já na largura usada (o servidor de imagens redimensiona).
fn image_sized(uri: &str, w: u32) -> Option<String> {
    let u = if let Some(rest) = uri.strip_prefix("//") { format!("https://{rest}") } else { uri.to_string() };
    u.starts_with("https://").then(|| if u.contains('?') { u } else { format!("{u}?w={w}") })
}

/// Produto do catálogo da Store → item (só jogos disponíveis para PC). `None` = descartar.
pub fn parse_product(p: &Value) -> Option<Item> {
    let id = p["ProductId"].as_str()?.to_ascii_uppercase();
    let lp = p["LocalizedProperties"].as_array()?.first()?;
    let title = lp["ProductTitle"].as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    // disponível no Windows (PC)? procura a plataforma em qualquer disponibilidade
    let pc = p["DisplaySkuAvailabilities"].as_array().into_iter().flatten().any(|sa| {
        sa["Availabilities"].as_array().into_iter().flatten().any(|a| {
            a["Conditions"]["ClientConditions"]["AllowedPlatforms"].as_array().into_iter().flatten().any(|pl| pl["PlatformName"].as_str() == Some("Windows.Desktop"))
        })
    });
    if !pc {
        return None;
    }
    let images = lp["Images"].as_array();
    let pick = |purpose: &str| images.into_iter().flatten().find(|i| i["ImagePurpose"].as_str() == Some(purpose)).and_then(|i| i["Uri"].as_str()).and_then(image_url);
    let text = |k: &str| lp[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(600).collect::<String>());
    Some(Item {
        id,
        title: title.chars().take(150).collect(),
        publisher: text("PublisherName"),
        description: text("ShortDescription"),
        image: pick("Poster").or_else(|| pick("BoxArt")).or_else(|| pick("BrandedKeyArt")),
        release: p["MarketProperties"].as_array().and_then(|m| m.first()).and_then(|m| m["OriginalReleaseDate"].as_str()).and_then(epoch),
        leaves_on: leave_date(p),
    })
}

/// Data civil ("AAAA-MM-DD") de um epoch em UTC.
fn civil_date(t: i64) -> String {
    let z = t.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Último dia da oferta do Game Pass para PC no catálogo: fim da disponibilidade que oferece a assinatura
/// ("Upsell") no Windows. A Microsoft encerra essas ofertas no fim do dia nos EUA (ex.: 01/12 09:59 UTC =
/// 30/11 à noite no Pacífico); 12 h antes do fim dá o dia anunciado. Sem data (ou "9998"): None.
pub fn leave_date(p: &Value) -> Option<String> {
    let mut end: Option<i64> = None;
    for sa in p["DisplaySkuAvailabilities"].as_array().into_iter().flatten() {
        for a in sa["Availabilities"].as_array().into_iter().flatten() {
            let upsell = a["Remediations"].as_array().into_iter().flatten().any(|r| r["Type"].as_str() == Some("Upsell"));
            let pc = a["Conditions"]["ClientConditions"]["AllowedPlatforms"].as_array().into_iter().flatten().any(|pl| pl["PlatformName"].as_str() == Some("Windows.Desktop"));
            if let Some(e) = a["Conditions"]["EndDate"].as_str().and_then(epoch).filter(|_| upsell && pc) {
                end = Some(end.map_or(e, |x: i64| x.max(e)));
            }
        }
    }
    end.map(|e| civil_date(e - 12 * 3600))
}

/// Busca as três listas e os detalhes dos jogos. Erro só quando nada pôde ser obtido.
async fn fetch_all(lang: &str, market: &str) -> AppResult<HashMap<String, Vec<Item>>> {
    let client = http()?;
    let mut ids_by: Vec<(&str, Vec<String>)> = Vec::new();
    let mut last_err = None;
    for (key, sigl) in LISTS {
        match get_json(&client, "https://catalog.gamepass.com/sigls/v2", &[("id", sigl), ("language", lang), ("market", market)]).await {
            Ok(v) => match parse_list(&v, sigl) {
                Some(ids) => ids_by.push((key, ids)),
                None => last_err = Some(AppError::Other("Game Pass: o catálogo respondeu num formato inesperado.".into())),
            },
            Err(e) => last_err = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if ids_by.is_empty() {
        return Err(last_err.unwrap_or_else(|| AppError::Other("Game Pass: catálogo indisponível no momento.".into())));
    }
    let mut all: Vec<String> = Vec::new();
    for (_, ids) in &ids_by {
        for id in ids {
            if !all.contains(id) {
                all.push(id.clone());
            }
        }
    }
    let mut products: HashMap<String, Item> = HashMap::new();
    for chunk in all.chunks(CHUNK) {
        let joined = chunk.join(",");
        let v = get_json(&client, "https://displaycatalog.mp.microsoft.com/v7.0/products", &[("bigIds", joined.as_str()), ("market", market), ("languages", lang)]).await?;
        let list = v["Products"].as_array().ok_or_else(|| AppError::Other("Game Pass: o catálogo respondeu num formato inesperado.".into()))?;
        for p in list {
            if let Some(it) = parse_product(p) {
                products.insert(it.id.clone(), it);
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(ids_by.into_iter().map(|(k, ids)| (k.to_string(), ids.iter().filter_map(|i| products.get(i).cloned()).collect())).collect())
}

fn save(conn: &Connection, lists: &HashMap<String, Vec<Item>>, market: &str, lang: &str) -> AppResult<()> {
    let at = now();
    for (k, items) in lists {
        conn.execute(
            "INSERT INTO gamepass_cache(list, market, lang, fetched_at, data) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(list) DO UPDATE SET market=excluded.market, lang=excluded.lang, fetched_at=excluded.fetched_at, data=excluded.data",
            params![k, market, lang, at, serde_json::to_string(items).unwrap_or_else(|_| "[]".into())],
        )?;
    }
    Ok(())
}

pub fn enabled(conn: &Connection) -> bool {
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", [SETTING], |r| r.get::<_, String>(0)).optional().ok().flatten().as_deref() != Some("0")
}

/// Cache mais antigo entre as listas (None = nunca buscado) e se ele é do idioma/região atuais.
fn cache_age(conn: &Connection, lang: &str, market: &str) -> AppResult<Option<i64>> {
    let row: Option<(Option<i64>, i64)> = conn
        .query_row(
            "SELECT MIN(fetched_at), SUM(lang<>?1 OR market<>?2) FROM gamepass_cache",
            params![lang, market],
            |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
        )
        .optional()?;
    // cache de versão anterior (sem a data de saída): busca de novo
    let old: bool = conn
        .query_row("SELECT data NOT LIKE '%leavesOn%' FROM gamepass_cache WHERE list='leaving' AND data<>'[]'", [], |r| r.get(0))
        .optional()?
        .unwrap_or(false);
    Ok(match row {
        Some((Some(at), 0)) if !old => Some(now() - at),
        _ => None,
    })
}

// ---------- página de um jogo ----------

const DETAIL_TTL: i64 = 24 * 3600;
const DETAIL_MANUAL_TTL: i64 = 10 * 60;
const MAX_SHOTS: usize = 30;
/// Versão do formato de `Detail` no cache: muda quando a leitura do catálogo melhora (cache antigo é buscado de novo).
const DETAIL_SCHEMA: u32 = 4;
/// Servidor de trailers do Xbox que pode ser tocado dentro do app (HLS com CORS liberado). Os trailers do
/// outro servidor da Microsoft (cdn-dynmedia) recusam acesso fora do site do Xbox e ficam de fora.
const TRAILER_HOST: &str = "https://cdn.trailers.xboxservices.com/";
const MAX_TRAILERS: usize = 6;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub full: String,
    pub thumb: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trailer {
    pub title: String,
    /// Manifesto HLS (.m3u8) no servidor de trailers do Xbox.
    pub hls: String,
    pub poster: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgeRating {
    /// Sistema de classificação (ex.: "DJCTQ" = ClassInd no Brasil, "PEGI", "ESRB").
    pub system: String,
    /// Classificação nesse sistema (ex.: "16", "M").
    pub value: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// Versão do formato (ver `DETAIL_SCHEMA`); cache sem ela é de uma versão anterior.
    #[serde(default)]
    pub schema: u32,
    pub id: String,
    pub title: String,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub release: Option<i64>,
    /// Arte grande do topo da página (16:9).
    pub hero: Option<String>,
    pub poster: Option<String>,
    pub screenshots: Vec<Shot>,
    /// O catálogo tem trailer (que só toca no site do Xbox: streaming HLS/DASH).
    pub has_trailer: bool,
    /// Categorias em inglês, como a Store informa (a interface traduz as conhecidas).
    pub categories: Vec<String>,
    /// Média das avaliações dos usuários da Store (todas as épocas) e quantas avaliações.
    pub rating: Option<f64>,
    pub rating_count: i64,
    /// Maior tamanho de download informado entre os pacotes (bytes).
    pub size: Option<i64>,
    /// Recursos disponíveis no PC, como chaves estáveis (ver `feature_key`).
    pub features: Vec<String>,
    pub age_rating: Option<AgeRating>,
    pub pfn: Option<String>,
    pub has_add_ons: bool,
    /// Último dia da oferta do Game Pass (ver `Item::leaves_on`: só vale como saída na lista "Saindo").
    #[serde(default)]
    pub leaves_on: Option<String>,
    /// Trailers que tocam dentro do app (servidor de trailers do Xbox).
    #[serde(default)]
    pub trailers: Vec<Trailer>,
}

/// Recurso do catálogo → chave estável usada pela interface. Recursos sem interesse ficam de fora.
fn feature_key(name: &str) -> Option<&'static str> {
    Some(match name {
        "SinglePlayer" => "single",
        "XblOnlineCoop" | "CoopSupportOnline" => "coop_online",
        "XblLocalCoop" | "CoopSupportLocal" => "coop_local",
        "XblOnlineMultiPlayer" | "OnlineMultiplayerWithGold" => "mp_online",
        "XblLocalMultiPlayer" | "LocalMultiplayer" => "mp_local",
        "XblCrossPlatformMultiPlayer" | "CrossPlatformMultiPlayer" => "mp_cross",
        "XblCrossPlatformCoop" => "coop_cross",
        "SharedSplitScreen" => "split",
        "XPA" => "xpa",
        "XblAchievements" => "achievements",
        "XblCloudSaves" => "cloud_saves",
        "CapabilityHDR" => "hdr",
        "CapabilityVRR" => "vrr",
        "RayTracing" => "ray_tracing",
        "SpatialSound" => "spatial",
        "DolbyAtmos" => "atmos",
        "DTSX" => "dtsx",
        _ => return None,
    })
}

/// Sistema de classificação etária mais adequado à região (ClassInd no Brasil).
fn rating_system(market: &str) -> &'static [&'static str] {
    match market {
        "BR" => &["DJCTQ", "IARC"],
        "US" | "CA" | "MX" => &["ESRB", "IARC"],
        "DE" => &["USK", "PEGI", "IARC"],
        "JP" => &["CERO", "IARC"],
        "KR" => &["GRB", "IARC"],
        "AU" => &["COB-AU", "IARC"],
        "GB" | "FR" | "ES" | "IT" | "PT" | "NL" | "BE" | "AT" | "CH" | "IE" | "PL" | "SE" | "NO" | "DK" | "FI" | "CZ" | "GR" | "HU" => &["PEGI", "IARC"],
        _ => &["IARC", "PEGI", "ESRB"],
    }
}

/// Produto completo do catálogo → página do jogo. `None` = sem título (formato inesperado).
pub fn parse_detail(p: &Value, market: &str) -> Option<Detail> {
    let id = p["ProductId"].as_str()?.to_ascii_uppercase();
    let lp = p["LocalizedProperties"].as_array()?.first()?;
    let title = lp["ProductTitle"].as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let text = |k: &str, max: usize| lp[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(|s| s.chars().take(max).collect::<String>());
    let images: Vec<&Value> = lp["Images"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let img = |purpose: &str, w: u32| images.iter().find(|i| i["ImagePurpose"].as_str() == Some(purpose)).and_then(|i| i["Uri"].as_str()).and_then(|u| image_sized(u, w));
    // a Store repete cada captura para PC e para Xbox (mesmo endereço): uma de cada
    let mut seen = std::collections::HashSet::new();
    let screenshots = images
        .iter()
        .filter(|i| i["ImagePurpose"].as_str() == Some("Screenshot"))
        .filter_map(|i| i["Uri"].as_str())
        .filter(|u| seen.insert(u.trim_start_matches("https:").trim_start_matches("http:").to_string()))
        .filter_map(|u| Some(Shot { full: image_sized(u, 2560)?, thumb: image_sized(u, 400)? }))
        .take(MAX_SHOTS)
        .collect();
    let mp = p["MarketProperties"].as_array().and_then(|m| m.first());
    let rating = mp.and_then(|m| m["UsageData"].as_array()).and_then(|u| u.iter().find(|x| x["AggregateTimeSpan"].as_str() == Some("AllTime")));
    let rating_count = rating.and_then(|r| r["RatingCount"].as_i64()).unwrap_or(0);
    let age_rating = mp.and_then(|m| m["ContentRatings"].as_array()).and_then(|list| {
        rating_system(market).iter().find_map(|sys| {
            let r = list.iter().find(|r| r["RatingSystem"].as_str() == Some(*sys))?;
            let value = r["RatingId"].as_str()?.rsplit(':').next()?.trim().to_string();
            (!value.is_empty()).then(|| AgeRating { system: sys.to_string(), value })
        })
    });
    let props = &p["Properties"];
    let mut features: Vec<String> = Vec::new();
    for a in props["Attributes"].as_array().into_iter().flatten() {
        // recurso só do console (ex.: 4K no Xbox) não vale para o PC
        let pc = match a["ApplicablePlatforms"].as_array() {
            None => true,
            Some(pl) => pl.iter().any(|x| x.as_str() == Some("Desktop")),
        };
        if let Some(k) = a["Name"].as_str().and_then(feature_key).filter(|_| pc) {
            if !features.iter().any(|f| f == k) {
                features.push(k.to_string());
            }
        }
    }
    let size = p["DisplaySkuAvailabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|sa| sa["Sku"]["Properties"]["Packages"].as_array().into_iter().flatten())
        .filter_map(|pk| pk["MaxDownloadSizeInBytes"].as_i64())
        .filter(|s| *s > 0)
        .max();
    Some(Detail {
        schema: DETAIL_SCHEMA,
        id,
        title: title.chars().take(150).collect(),
        developer: text("DeveloperName", 120),
        publisher: text("PublisherName", 120),
        description: text("ProductDescription", 6000).or_else(|| text("ShortDescription", 600)),
        release: mp.and_then(|m| m["OriginalReleaseDate"].as_str()).and_then(epoch),
        hero: img("SuperHeroArt", 1920).or_else(|| img("TitledHeroArt", 1920)),
        poster: img("Poster", 480).or_else(|| img("BoxArt", 480)),
        screenshots,
        has_trailer: lp["CMSVideos"].as_array().is_some_and(|v| !v.is_empty()) || lp["Videos"].as_array().is_some_and(|v| !v.is_empty()),
        categories: props["Categories"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(str::to_string).take(4).collect(),
        rating: rating.and_then(|r| r["AverageRating"].as_f64()).filter(|r| *r > 0.0 && rating_count > 0),
        rating_count,
        size,
        features,
        age_rating,
        pfn: props["PackageFamilyName"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
        has_add_ons: props["HasAddOns"].as_bool().unwrap_or(false),
        leaves_on: leave_date(p),
        trailers: lp["CMSVideos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| {
                let hls = v["HLS"].as_str()?.trim();
                (hls.starts_with(TRAILER_HOST) && hls.contains(".m3u8")).then(|| Trailer {
                    title: v["Caption"].as_str().map(str::trim).filter(|c| !c.is_empty()).unwrap_or("Trailer").chars().take(120).collect(),
                    hls: hls.to_string(),
                    poster: v["PreviewImage"]["Uri"].as_str().and_then(|u| image_sized(u, 1280)),
                })
            })
            .take(MAX_TRAILERS)
            .collect(),
    })
}

async fn fetch_detail(id: &str, lang: &str, market: &str) -> AppResult<Option<Detail>> {
    let client = http()?;
    let v = get_json(&client, "https://displaycatalog.mp.microsoft.com/v7.0/products", &[("bigIds", id), ("market", market), ("languages", lang)]).await?;
    let list = v["Products"].as_array().ok_or_else(|| AppError::Other("Game Pass: o catálogo respondeu num formato inesperado.".into()))?;
    Ok(list.iter().find(|p| p["ProductId"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(id))).and_then(|p| parse_detail(p, market)))
}

fn cached_detail(conn: &Connection, id: &str, lang: &str) -> AppResult<Option<(i64, Detail)>> {
    let row: Option<(i64, String, String)> = conn
        .query_row("SELECT fetched_at, lang, data FROM gamepass_detail WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    // outro idioma ou formato antigo: vale só como reserva (sem internet); a idade conta como vencida
    Ok(row.and_then(|(at, l, data)| serde_json::from_str::<Detail>(&data).ok().map(|d| (if l == lang && d.schema == DETAIL_SCHEMA { at } else { 0 }, d))))
}

fn save_detail(conn: &Connection, d: &Detail, market: &str, lang: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO gamepass_detail(id, market, lang, fetched_at, data) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(id) DO UPDATE SET market=excluded.market, lang=excluded.lang, fetched_at=excluded.fetched_at, data=excluded.data",
        params![d.id, market, lang, now(), serde_json::to_string(d).unwrap_or_else(|_| "{}".into())],
    )?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailView {
    pub detail: Detail,
    pub fetched_at: i64,
    /// Falha na última consulta (o que aparece é do cache).
    pub error: Option<String>,
    /// Jogo instalado neste PC (pacote da Microsoft Store) e o AUMID para adicioná-lo à biblioteca.
    pub installed: bool,
    pub aumid: Option<String>,
    pub in_library: bool,
    /// Jogo da biblioteca do perfil com o mesmo pacote (para "Jogar").
    pub library_game_id: Option<i64>,
}

/// Jogo da biblioteca do perfil com esta família de pacote (`shell:AppsFolder\<família>!...`).
fn library_game(conn: &Connection, profile_id: i64, family: &str) -> AppResult<Option<i64>> {
    let mut st = conn.prepare("SELECT g.id, g.executable_path FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1 ORDER BY g.id")?;
    let rows = st.query_map([profile_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, exe) = row?;
        if crate::msstore::aumid(&exe).and_then(|a| a.split('!').next()).is_some_and(|f| f.eq_ignore_ascii_case(family)) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Página de um jogo do catálogo: cache de 24 h (`refresh`: 10 min); sem internet, devolve o cache.
#[tauri::command]
pub async fn gamepass_detail(db: tauri::State<'_, Db>, profile_id: i64, id: String, lang: String, refresh: bool) -> AppResult<DetailView> {
    let id = id.trim().to_ascii_uppercase();
    if !valid_id(&id) {
        return Err(AppError::Validation("Produto da Microsoft Store inválido.".into()));
    }
    let lang = store_lang(&lang);
    let market = market_from_locale(&windows_locale());
    let cached = cached_detail(&*db.conn()?, &id, lang)?;
    let ttl = if refresh { DETAIL_MANUAL_TTL } else { DETAIL_TTL };
    let (detail, fetched_at, error) = match cached {
        Some((at, d)) if now() - at < ttl => (d, at, None),
        cached => {
            let mut res = fetch_detail(&id, lang, &market).await;
            // produto sem página nesta região: tenta o catálogo dos EUA
            if market != "US" && matches!(res, Ok(None)) {
                res = fetch_detail(&id, lang, "US").await;
            }
            match (res, cached) {
                (Ok(Some(d)), _) => {
                    save_detail(&*db.conn()?, &d, &market, lang)?;
                    (d, now(), None)
                }
                (Ok(None), Some((at, d))) => (d, at, None),
                (Ok(None), None) => return Err(AppError::NotFound("Jogo não encontrado no catálogo da Microsoft Store.".into())),
                (Err(e), Some((at, d))) => (d, at, Some(e.to_string())),
                (Err(e), None) => return Err(e),
            }
        }
    };
    // instalado neste PC? (mesma família de pacote) e já está na biblioteca do perfil?
    let family = detail.pfn.as_deref().map(str::to_lowercase);
    let aumid = family.as_deref().and_then(|f| crate::msstore::installed_games().into_iter().find(|g| g.family.to_lowercase() == f).map(|g| g.aumid));
    let installed = aumid.is_some() || family.as_deref().is_some_and(|f| crate::msstore::installed_families().contains(f));
    let library_game_id = match &family {
        Some(f) => library_game(&*db.conn()?, profile_id, f)?,
        None => None,
    };
    Ok(DetailView { detail, fetched_at, error, installed, aumid, in_library: library_game_id.is_some(), library_game_id })
}

/// Adiciona o jogo à biblioteca do perfil, instalado ou não (entra como jogo do Xbox, aberto pelo Windows).
/// Não instalado, fica gravado só com a família do pacote; o app é identificado quando ele for instalado.
/// Capa e fundo vêm do catálogo da Microsoft Store (o título fica escrito, sem logo). Já na biblioteca: devolve o jogo existente.
#[tauri::command]
pub fn gamepass_add_to_library(db: tauri::State<Db>, profile_id: i64, id: String) -> AppResult<i64> {
    let id = id.trim().to_ascii_uppercase();
    let mut conn = db.conn()?;
    // dados da página (qualquer idioma serve: só título, imagens e pacote são usados)
    let Some(d) = conn
        .query_row("SELECT data FROM gamepass_detail WHERE id=?1", [&id], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|s| serde_json::from_str::<Detail>(&s).ok())
    else {
        return Err(AppError::NotFound("Abra a página do jogo antes de adicioná-lo (os dados ainda não foram buscados).".into()));
    };
    let Some(pfn) = d.pfn.clone().filter(|p| p.contains('_') && !p.contains(['\\', '/', '!'])) else {
        return Err(AppError::Validation("Este jogo não tem pacote para PC na Microsoft Store.".into()));
    };
    if let Some(existing) = library_game(&conn, profile_id, &pfn)? {
        return Ok(existing);
    }
    let aumid = crate::msstore::installed_games().into_iter().find(|g| g.family.eq_ignore_ascii_case(&pfn)).map(|g| g.aumid).unwrap_or_else(|| format!("{pfn}!"));
    let tx = conn.transaction()?;
    tx.execute(
        // sem logo: o "Logo" do catálogo é o ícone da loja; a Biblioteca mostra o título escrito
        "INSERT INTO games(title, platform, executable_path, launch_args, cover, background, source) VALUES(?1, 'PC', ?2, '', ?3, ?4, 'xbox')",
        params![d.title.chars().take(80).collect::<String>(), format!("{}{aumid}", crate::msstore::PREFIX), d.poster, d.hero],
    )?;
    let gid = tx.last_insert_rowid();
    tx.execute("INSERT INTO profile_games(profile_id, game_id) VALUES(?1, ?2)", params![profile_id, gid])?;
    tx.commit()?;
    Ok(gid)
}

// ---------- verificação em segundo plano ----------

static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Atualiza o catálogo se o cache estiver vencido (12 h; manual: 10 min). Não bloqueia.
pub fn start(app: &AppHandle, app_lang: &str, manual: bool) -> bool {
    let lang = store_lang(app_lang);
    let market = market_from_locale(&windows_locale());
    {
        let db = app.state::<Db>();
        let Ok(conn) = db.conn() else { return false };
        if !enabled(&conn) {
            return false;
        }
        let ttl = if manual { MANUAL_TTL } else { TTL };
        if cache_age(&conn, lang, &market).ok().flatten().is_some_and(|a| a < ttl) {
            return false;
        }
    }
    if RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _g = Guard;
        let _ = app.emit("gamepass-state", json!({ "running": true }));
        let mut result = fetch_all(lang, &market).await;
        // região sem PC Game Pass (listas vazias): usa o catálogo dos EUA
        if market != "US" && result.as_ref().is_ok_and(|m| m.values().all(|l| l.is_empty())) {
            result = fetch_all(lang, "US").await;
        }
        let err = match result {
            Ok(lists) => {
                let db = app.state::<Db>();
                let saved = db.conn().map_err(|e| e.to_string()).and_then(|c| save(&c, &lists, &market, lang).map_err(|e| e.to_string()));
                saved.err()
            }
            Err(e) => Some(e.to_string()),
        };
        if let Ok(mut le) = LAST_ERROR.lock() {
            *le = err;
        }
        let _ = app.emit("gamepass-state", json!({ "running": false }));
        let _ = app.emit("gamepass-updated", json!({}));
    });
    true
}

// ---------- leitura (só cache) ----------

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub enabled: bool,
    pub running: bool,
    pub fetched_at: Option<i64>,
    pub error: Option<String>,
    pub coming: Vec<Item>,
    pub added: Vec<Item>,
    pub leaving: Vec<Item>,
}

pub fn overview(conn: &Connection) -> AppResult<Overview> {
    let mut ov = Overview { enabled: enabled(conn), running: RUNNING.load(Ordering::SeqCst), ..Default::default() };
    if !ov.enabled {
        return Ok(ov);
    }
    let mut st = conn.prepare("SELECT list, fetched_at, data FROM gamepass_cache")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?;
    for row in rows {
        let (list, at, data) = row?;
        let items: Vec<Item> = serde_json::from_str(&data).unwrap_or_default();
        ov.fetched_at = Some(ov.fetched_at.map_or(at, |f: i64| f.min(at)));
        match list.as_str() {
            "coming" => ov.coming = items,
            "added" => ov.added = items,
            "leaving" => ov.leaving = items,
            _ => {}
        }
    }
    ov.error = LAST_ERROR.lock().ok().and_then(|e| e.clone());
    Ok(ov)
}

#[tauri::command(async)]
pub fn gamepass_overview(db: tauri::State<Db>) -> AppResult<Overview> {
    overview(&*db.conn()?)
}

#[tauri::command]
pub fn gamepass_refresh(app: AppHandle, lang: String, manual: bool) -> bool {
    start(&app, &lang, manual)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Videos {
    /// IGDB configurado e ativado em Configurações → Fontes de dados.
    pub available: bool,
    pub videos: Vec<crate::sources::igdb::Video>,
}

/// Trailers do YouTube pelo IGDB (alternativa aos trailers do Xbox): jogo identificado pelo código da
/// Microsoft Store. Cache de 7 dias. Sem IGDB configurado: `available = false`.
#[tauri::command]
pub async fn gamepass_videos(db: tauri::State<'_, Db>, id: String) -> AppResult<Videos> {
    use crate::sources::{self, igdb, Source};
    let id = id.trim().to_ascii_uppercase();
    if !valid_id(&id) {
        return Err(AppError::Validation("Produto da Microsoft Store inválido.".into()));
    }
    let key = format!("ms-videos:{id}");
    let (creds, cached) = {
        let conn = db.conn()?;
        let creds = sources::active(&conn, Source::Igdb)?.and_then(|c| serde_json::from_str::<igdb::Creds>(&c).ok());
        (creds, sources::cache_get(&conn, Source::Igdb, &key, 7 * 86400)?)
    };
    let Some(creds) = creds else { return Ok(Videos { available: false, videos: vec![] }) };
    if let Some(v) = cached.and_then(|v| serde_json::from_value::<Vec<igdb::Video>>(v).ok()) {
        return Ok(Videos { available: true, videos: v });
    }
    let videos = igdb::videos_by_microsoft_id(&creds, &id).await?.unwrap_or_default();
    sources::cache_put(&*db.conn()?, Source::Igdb, &key, &serde_json::to_value(&videos).unwrap_or_default())?;
    Ok(Videos { available: true, videos })
}

/// Pacote do app Xbox para PC (dono do protocolo `msxbox://`).
const XBOX_APP_FAMILY: &str = "microsoft.gamingapp_8wekyb3d8bbwe";

/// Abre o jogo no app Xbox (`msxbox://game/?productId=`), onde se instala e joga com o Game Pass. Sem o app
/// Xbox instalado, abre a página na Microsoft Store. Devolve "xbox" ou "store".
#[tauri::command]
pub fn gamepass_open(app: AppHandle, id: String) -> AppResult<String> {
    let id = id.trim().to_ascii_uppercase();
    if !valid_id(&id) {
        return Err(AppError::Validation("Produto da Microsoft Store inválido.".into()));
    }
    let (url, how) = if crate::msstore::installed_families().contains(XBOX_APP_FAMILY) {
        (format!("msxbox://game/?productId={id}"), "xbox")
    } else {
        (format!("ms-windows-store://pdp/?ProductId={id}"), "store")
    };
    app.opener().open_url(url, None::<&str>).map_err(|_| AppError::Other("Não foi possível abrir o app Xbox.".into()))?;
    Ok(how.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_validation() {
        let sigl = LISTS[0].1;
        let v = json!([{ "siglId": sigl, "title": "Chegando" }, { "id": "9NR13RF1V2WV" }, { "id": "9nr13rf1v2wv" }, { "id": "x" }, {}]);
        assert_eq!(parse_list(&v, sigl).unwrap(), vec![String::from("9NR13RF1V2WV")]);
        assert!(parse_list(&v, LISTS[1].1).is_none(), "outra lista no lugar da pedida");
        assert!(parse_list(&json!({ "type": "Not Found", "status": 404 }), sigl).is_none());
        assert!(parse_list(&json!([]), sigl).is_none());
        assert_eq!(parse_list(&json!([{ "siglId": sigl }]), sigl).unwrap().len(), 0);
    }

    fn product(platform: &str, release: &str) -> Value {
        json!({
            "ProductId": "9pmn4dgrt1ln",
            "LocalizedProperties": [{ "ProductTitle": " Fable ", "PublisherName": "Xbox Game Studios", "ShortDescription": "Uma aventura.",
                "Images": [{ "ImagePurpose": "BoxArt", "Uri": "//store-images.s-microsoft.com/image/box" },
                           { "ImagePurpose": "Poster", "Uri": "//store-images.s-microsoft.com/image/poster" }] }],
            "MarketProperties": [{ "OriginalReleaseDate": release }],
            "DisplaySkuAvailabilities": [{ "Availabilities": [{ "Conditions": { "ClientConditions": { "AllowedPlatforms": [{ "PlatformName": platform }] } } }] }]
        })
    }

    #[test]
    fn product_parsing() {
        let it = parse_product(&product("Windows.Desktop", "2027-02-23T00:00:00.0000000Z")).unwrap();
        assert_eq!((it.id.as_str(), it.title.as_str()), ("9PMN4DGRT1LN", "Fable"));
        assert_eq!(it.image.as_deref(), Some("https://store-images.s-microsoft.com/image/poster?w=360"), "pôster primeiro, https");
        assert_eq!(it.release, Some(1_803_340_800));
        assert_eq!(parse_product(&product("Windows.Desktop", "9998-12-30T00:00:00.0000000Z")).unwrap().release, None, "data não anunciada");
        assert!(parse_product(&product("Windows.Xbox", "2027-02-23T00:00:00Z")).is_none(), "só console: fora");
        assert!(parse_product(&json!({ "ProductId": "X" })).is_none());
    }

    #[test]
    fn locale_and_dates() {
        assert_eq!(market_from_locale("pt-BR"), "BR");
        assert_eq!(market_from_locale("zh-Hans-CN"), "CN");
        assert_eq!(market_from_locale("en"), "US");
        assert_eq!(market_from_locale(""), "US");
        assert_eq!(store_lang("pt-BR"), "pt-br");
        assert_eq!(store_lang("xx"), "en-us");
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2024-03-05T15:00:00.0000000Z"), Some(1_709_650_800));
        assert_eq!(epoch("lixo"), None);
    }

    fn full_product() -> Value {
        json!({
            "ProductId": "9pmn4dgrt1ln",
            "LocalizedProperties": [{
                "ProductTitle": "Fable", "DeveloperName": "Playground Games", "PublisherName": "Xbox Game Studios",
                "ProductDescription": "Torne-se o Herói.", "ShortDescription": "Curta.",
                "Images": [
                    { "ImagePurpose": "SuperHeroArt", "Uri": "//store-images.s-microsoft.com/image/hero" },
                    { "ImagePurpose": "Poster", "Uri": "//store-images.s-microsoft.com/image/poster" },
                    { "ImagePurpose": "Screenshot", "Uri": "//store-images.s-microsoft.com/image/s1", "ImagePositionInfo": "Desktop/1" },
                    { "ImagePurpose": "Screenshot", "Uri": "//store-images.s-microsoft.com/image/s1", "ImagePositionInfo": "Xbox/1" },
                    { "ImagePurpose": "Screenshot", "Uri": "http://inseguro/s2" }
                ],
                "CMSVideos": [{ "HLS": "https://x/a.m3u8", "VideoPurpose": "trailer" },
                              { "HLS": "https://cdn.trailers.xboxservices.com/t/manifest.m3u8", "Caption": "Trailer de lançamento",
                                "PreviewImage": { "Uri": "//store-images.s-microsoft.com/image/prev" } }]
            }],
            "MarketProperties": [{
                "OriginalReleaseDate": "2027-02-23T00:00:00.0000000Z",
                "UsageData": [{ "AggregateTimeSpan": "7Days", "AverageRating": 0, "RatingCount": 0 },
                              { "AggregateTimeSpan": "AllTime", "AverageRating": 3.1, "RatingCount": 566 }],
                "ContentRatings": [{ "RatingSystem": "ESRB", "RatingId": "ESRB:M" }, { "RatingSystem": "DJCTQ", "RatingId": "DJCTQ:16" },
                                   { "RatingSystem": "PEGI", "RatingId": "PEGI:18" }]
            }],
            "Properties": {
                "Categories": ["Action & adventure"], "PackageFamilyName": "Microsoft.FableBaseGame_8wekyb3d8bbwe", "HasAddOns": true,
                "Attributes": [{ "Name": "XPA" }, { "Name": "Capability4k", "ApplicablePlatforms": ["Xbox"] },
                               { "Name": "SinglePlayer", "ApplicablePlatforms": ["Xbox", "Desktop"] }, { "Name": "XboxLive" }, { "Name": "XPA" }]
            },
            "DisplaySkuAvailabilities": [{ "Sku": { "Properties": { "Packages": [{ "MaxDownloadSizeInBytes": 100 }, { "MaxDownloadSizeInBytes": 392753152 }] } } }]
        })
    }

    #[test]
    fn leave_date_from_game_pass_offer() {
        let av = |end: &str, upsell: bool, plat: &str| json!({
            "Conditions": { "EndDate": end, "ClientConditions": { "AllowedPlatforms": [{ "PlatformName": plat }] } },
            "Remediations": if upsell { json!([{ "Type": "Upsell", "BigId": "CFQ7TTC0KGQ8" }]) } else { json!([]) }
        });
        let p = json!({ "DisplaySkuAvailabilities": [{ "Availabilities": [
            av("9998-12-30T23:59:59.9999999Z", false, "Windows.Desktop"),          // compra: sem fim
            av("2026-11-19T19:59:59.0000000Z", true, "Windows.Desktop"),
            av("2026-11-12T09:59:59.0000000Z", true, "Windows.Desktop"),          // janela anterior
            av("2027-05-01T00:00:00.0000000Z", true, "Windows.Xbox")               // só console: ignora
        ] }] });
        assert_eq!(leave_date(&p).as_deref(), Some("2026-11-19"));
        let dec = json!({ "DisplaySkuAvailabilities": [{ "Availabilities": [av("2026-12-01T09:59:59.0000000Z", true, "Windows.Desktop")] }] });
        assert_eq!(leave_date(&dec).as_deref(), Some("2026-11-30"), "fim do dia nos EUA: 30/11");
        assert_eq!(leave_date(&json!({})), None);
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(1_803_340_800), "2027-02-23");
        assert_eq!(civil_date(951_782_400), "2000-02-29");
    }

    #[test]
    fn detail_parsing() {
        let d = parse_detail(&full_product(), "BR").unwrap();
        assert_eq!((d.id.as_str(), d.developer.as_deref(), d.description.as_deref()), ("9PMN4DGRT1LN", Some("Playground Games"), Some("Torne-se o Herói.")));
        assert_eq!(d.hero.as_deref(), Some("https://store-images.s-microsoft.com/image/hero?w=1920"));
        assert_eq!(d.screenshots, vec![Shot { full: String::from("https://store-images.s-microsoft.com/image/s1?w=2560"), thumb: String::from("https://store-images.s-microsoft.com/image/s1?w=400") }], "sem repetição (PC/Xbox) e só https");
        assert!(d.has_trailer && d.has_add_ons);
        assert_eq!(d.trailers, vec![Trailer { title: String::from("Trailer de lançamento"), hls: String::from("https://cdn.trailers.xboxservices.com/t/manifest.m3u8"),
            poster: Some(String::from("https://store-images.s-microsoft.com/image/prev?w=1280")) }], "só o servidor que toca no app");
        assert_eq!((d.rating, d.rating_count, d.size), (Some(3.1), 566, Some(392753152)));
        assert_eq!(d.features, vec![String::from("xpa"), String::from("single")], "sem repetição; recurso só do console fica de fora");
        assert_eq!(d.age_rating, Some(AgeRating { system: String::from("DJCTQ"), value: String::from("16") }), "ClassInd no Brasil");
        assert_eq!(parse_detail(&full_product(), "US").unwrap().age_rating.unwrap().value, "M");
        assert_eq!(parse_detail(&full_product(), "FR").unwrap().age_rating.unwrap().system, "PEGI");
        assert_eq!(d.pfn.as_deref(), Some("Microsoft.FableBaseGame_8wekyb3d8bbwe"));
        assert!(parse_detail(&json!({ "ProductId": "X", "LocalizedProperties": [{ "ProductTitle": " " }] }), "BR").is_none());
    }

    #[test]
    fn detail_cache_by_language() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        let d = parse_detail(&full_product(), "BR").unwrap();
        save_detail(&c, &d, "BR", "pt-br").unwrap();
        let (at, back) = cached_detail(&c, "9PMN4DGRT1LN", "pt-br").unwrap().unwrap();
        assert!(at > 0 && back == d);
        assert_eq!(cached_detail(&c, "9PMN4DGRT1LN", "en-us").unwrap().unwrap().0, 0, "outro idioma: reserva, já vencida");
        assert!(cached_detail(&c, "9NR13RF1V2WV", "pt-br").unwrap().is_none());
    }

    #[test]
    fn cache_roundtrip_and_switch() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT);").unwrap();
        c.execute_batch(SCHEMA).unwrap();
        assert!(enabled(&c), "ligado por padrão");
        assert_eq!(cache_age(&c, "pt-br", "BR").unwrap(), None);
        let item = Item { id: String::from("9PMN4DGRT1LN"), title: String::from("Fable"), publisher: None, description: None, image: None, release: None, leaves_on: None };
        let lists = HashMap::from([(String::from("coming"), vec![item.clone()]), (String::from("added"), vec![]), (String::from("leaving"), vec![])]);
        save(&c, &lists, "BR", "pt-br").unwrap();
        assert!(cache_age(&c, "pt-br", "BR").unwrap().is_some());
        assert_eq!(cache_age(&c, "en-us", "BR").unwrap(), None, "outro idioma: busca de novo");
        let ov = overview(&c).unwrap();
        assert_eq!((ov.coming, ov.added.len()), (vec![item], 0));
        c.execute("INSERT INTO app_settings VALUES(?1,'0')", [SETTING]).unwrap();
        let off = overview(&c).unwrap();
        assert!(!off.enabled && off.coming.is_empty(), "desligado: nada é mostrado");
    }

    /// Consulta real (rede): cargo test gamepass_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn gamepass_live() {
        let m = tauri::async_runtime::block_on(fetch_all("pt-br", "BR")).unwrap();
        for (k, l) in &m {
            println!("{k}: {}", l.len());
            for it in l.iter().take(if k == "leaving" { 20 } else { 2 }) {
                println!("  {} | {} | {:?} | sai {:?}", it.id, it.title, it.release, it.leaves_on);
            }
        }
        assert!(m.values().any(|l| !l.is_empty()));
    }

    /// Página de um jogo, consulta real: cargo test gamepass_detail_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn gamepass_detail_live() {
        let d = tauri::async_runtime::block_on(fetch_detail("9PMN4DGRT1LN", "pt-br", "BR")).unwrap().unwrap();
        println!("{} | {:?} | {:?} | shots {} | trailer {} | {:?} | {:?} ({}) | {:?} | {:?} | {:?}", d.title, d.developer, d.release, d.screenshots.len(), d.has_trailer, d.categories, d.rating, d.rating_count, d.size, d.features, d.age_rating);
        for t in &d.trailers { println!("trailer: {} | {}", t.title, &t.hls[..t.hls.len().min(90)]); }
        assert!(d.hero.is_some() && !d.screenshots.is_empty());
    }
}
