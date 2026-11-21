//! Pesquisa inteligente de imagens de jogos (capas, fundos e screenshots), sempre com escolha manual:
//!  1. `search_games`: pesquisa o título e devolve uma LISTA de jogos (capa, título, plataforma, ano);
//!  2. `game_images`: carrega as imagens SÓ do jogo escolhido;
//!  3. nada é gravado sem confirmação: `add_web_screenshots` (galeria da página do jogo) e
//!     `fetch_image_data` (capa/fundo em "Editar jogo", que só são salvos ao confirmar o formulário).
//!
//! Fontes especializadas em jogos:
//!  - Steam (loja pública, sem chave): jogos de PC — capas verticais, fundo (hero) e screenshots.
//!    DLCs/pacotes e jogos com conteúdo sexual adulto (descritores 3 e 4) ficam fora.
//!  - libretro-thumbnails (GitHub, sem chave): capas, capturas de jogo e telas de título por sistema
//!    (consoles retrô), com os nomes oficiais dos lançamentos.
//!  - Wikipédia em inglês (API oficial, sem chave): capa e capturas de jogo do artigo de cada jogo.
//!  - SteamGridDB (chave pessoal gratuita, opcional): banco de capas e fundos feitos para launchers,
//!    com os filtros de conteúdo adulto/humor ligados.
//!  - RAWG (chave pessoal gratuita, opcional): banco de dados de jogos com screenshots.
//! O mesmo jogo vindo de várias fontes vira um único resultado (as imagens são somadas), mas versões
//! de plataformas diferentes continuam separadas.

use super::games::ensure_owned;
use super::info::{self, GameInfo};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, State};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct WebShot {
    /// Imagem em tamanho cheio.
    pub url: String,
    /// Miniatura (pode ser a própria imagem).
    pub thumb: String,
    /// "steam" | "libretro" | "wikipedia" | "steamgriddb" | "rawg" (e "wikimedia" em registros antigos)
    pub source: String,
    /// Página de origem (créditos / licença), quando existir.
    pub page: Option<String>,
    /// Descrição curta (lançamento, região, nome do arquivo...).
    pub title: String,
    /// "snap" (jogo) | "title" (tela de título) | "cover" | "background" | "image"
    pub kind: String,
}

/// Onde buscar as imagens de um jogo encontrado (uma fonte + o identificador nela).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source: String,
    pub id: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameHit {
    pub title: String,
    pub platform: Option<String>,
    pub year: Option<i32>,
    pub cover: Option<String>,
    pub sources: Vec<SourceRef>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageSet {
    pub covers: Vec<WebShot>,
    pub backgrounds: Vec<WebShot>,
    pub screenshots: Vec<WebShot>,
}

const UA: &str = concat!("NEXSTAGE/", env!("NX_VERSION"), " (desktop game launcher)");
const STEAM_CDN: &str = "https://cdn.cloudflare.steamstatic.com/steam/apps";
const WIKI: &str = "https://en.wikipedia.org/w/api.php";
const GH_API: &str = "https://api.github.com/repos/libretro-thumbnails";
const GH_RAW: &str = "https://raw.githubusercontent.com/libretro-thumbnails";
const SGDB: &str = "https://www.steamgriddb.com/api/v2";
const RAWG: &str = "https://api.rawg.io/api";
const MAX_HITS: usize = 24;
/// Tamanho máximo de uma imagem baixada para capa/fundo.
const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;

fn net(e: reqwest::Error) -> AppError {
    // sem a URL: a do RAWG leva a chave pessoal na query
    AppError::Other(format!("Falha de rede ao pesquisar imagens: {}", e.without_url()))
}

pub(crate) fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent(UA).build().map_err(net)
}

async fn get_json(req: reqwest::RequestBuilder) -> Option<Value> {
    req.send().await.ok()?.error_for_status().ok()?.json().await.ok()
}

/// Roda as consultas em paralelo; uma fonte que falha (rede, limite de uso) só não contribui.
async fn all<T: Send + 'static>(futs: Vec<std::pin::Pin<Box<dyn Future<Output = T> + Send>>>) -> Vec<T> {
    let handles: Vec<_> = futs.into_iter().map(tauri::async_runtime::spawn).collect();
    let mut out = Vec::new();
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

/// Codifica um segmento de caminho de URL (espaço = %20).
pub(crate) fn penc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// O termo aparece no nome como frase (palavras em sequência)? "Spider-Man" casa com
/// "Marvel's Spider-Man 2", mas não com "Man Face Spider I".
pub fn contains_all(term: &str, name: &str) -> bool {
    let want = info::tokens(term);
    let have = info::tokens(name);
    !want.is_empty() && have.windows(want.len()).any(|w| w == want.as_slice())
}

/// Primeiro ano plausível (1970–2039) num texto: "Nov 4, 2022" → 2022.
pub fn year_in(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(3)).find_map(|i| {
        let w = &s[i..i + 4];
        let edge = |j: usize| j >= b.len() || !b[j].is_ascii_digit();
        let ok = w.bytes().all(|c| c.is_ascii_digit()) && (i == 0 || !b[i - 1].is_ascii_digit()) && edge(i + 4);
        ok.then(|| w.parse::<i32>().ok()).flatten().filter(|y| (1970..2040).contains(y))
    })
}

/// Ano de um instante Unix (segundos), no calendário civil (UTC).
pub fn unix_year(secs: i64) -> i32 {
    // algoritmo "days from civil" invertido (Howard Hinnant)
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32
}

fn shot(url: &str, thumb: &str, source: &str, kind: &str, title: &str, page: Option<String>) -> WebShot {
    WebShot { url: url.into(), thumb: thumb.into(), source: source.into(), page, title: title.into(), kind: kind.into() }
}

// =============================================================================================
// Steam

async fn steam_details(c: &reqwest::Client, id: &str, filters: &str) -> Option<Value> {
    let v = get_json(c.get("https://store.steampowered.com/api/appdetails").query(&[("appids", id), ("filters", filters), ("l", "english")])).await?;
    let d = &v[id];
    (d["success"] == true).then(|| d["data"].clone())
}

/// Jogo de verdade (não DLC/trilha/pacote) e sem conteúdo sexual adulto.
pub fn steam_acceptable(d: &Value) -> bool {
    let adult = d["content_descriptors"]["ids"].as_array().map(|a| a.iter().any(|x| x == 3 || x == 4)).unwrap_or(false);
    d["type"] == "game" && !adult
}

async fn steam_search(c: reqwest::Client, term: String) -> Vec<GameHit> {
    let term_f = term.clone();
    let Some(v) = get_json(c.get("https://store.steampowered.com/api/storesearch/").query(&[("term", term.as_str()), ("l", "english"), ("cc", "US")])).await else {
        return vec![];
    };
    let ids: Vec<String> = v["items"].as_array().into_iter().flatten().take(10).filter_map(|i| i["id"].as_i64().map(|n| n.to_string())).collect();
    let futs = ids
        .into_iter()
        .map(|id| {
            let c = c.clone();
            Box::pin(async move {
                let d = steam_details(&c, &id, "basic,release_date,content_descriptors").await?;
                if !steam_acceptable(&d) {
                    return None;
                }
                Some(GameHit {
                    title: d["name"].as_str()?.to_string(),
                    platform: Some("PC".into()),
                    year: d["release_date"]["date"].as_str().and_then(year_in),
                    cover: Some(format!("{STEAM_CDN}/{id}/library_600x900.jpg")),
                    sources: vec![SourceRef { source: "steam".into(), id }],
                })
            }) as std::pin::Pin<Box<dyn Future<Output = Option<GameHit>> + Send>>
        })
        .collect();
    // a busca da loja é "frouxa" (ex.: "Man Face Spider I" para "Spider-Man"): o nome precisa conter o termo
    all(futs).await.into_iter().flatten().filter(|h| contains_all(&term_f, &h.title)).collect()
}

async fn exists(c: &reqwest::Client, url: &str) -> bool {
    c.head(url).send().await.map(|r| r.status().is_success()).unwrap_or(false)
}

async fn steam_images(c: reqwest::Client, id: String) -> ImageSet {
    let mut set = ImageSet::default();
    let page = Some(format!("https://store.steampowered.com/app/{id}"));
    for (file, kind) in [("library_600x900_2x.jpg", "cover"), ("library_600x900.jpg", "cover"), ("library_hero.jpg", "background")] {
        let url = format!("{STEAM_CDN}/{id}/{file}");
        if exists(&c, &url).await {
            let s = shot(&url, &url, "steam", kind, "Steam", page.clone());
            if kind == "cover" {
                if set.covers.is_empty() { set.covers.push(s); } // 2x e 1x são a mesma arte: fica a maior
            } else {
                set.backgrounds.push(s);
            }
        }
    }
    if let Some(d) = steam_details(&c, &id, "basic,screenshots,content_descriptors").await {
        if !steam_acceptable(&d) {
            return ImageSet::default();
        }
        if let Some(h) = d["header_image"].as_str() {
            set.backgrounds.push(shot(h, h, "steam", "background", "Steam (cabeçalho)", page.clone()));
        }
        for s in d["screenshots"].as_array().into_iter().flatten() {
            if let (Some(full), Some(th)) = (s["path_full"].as_str(), s["path_thumbnail"].as_str()) {
                set.screenshots.push(shot(full, th, "steam", "snap", "Steam", page.clone()));
            }
        }
    }
    set
}

// =============================================================================================
// libretro-thumbnails (consoles retrô)

/// Plataforma do NEXSTAGE → sistema no libretro-thumbnails. PC/PS4/PS5/Switch/Xbox Series não têm acervo lá.
pub fn libretro_system(platform: &str) -> Option<&'static str> {
    Some(match platform {
        "Playstation 1" => "Sony - PlayStation",
        "Playstation 2" => "Sony - PlayStation 2",
        "Playstation 3" => "Sony - PlayStation 3",
        "PSP" => "Sony - PlayStation Portable",
        "Ps Vita" => "Sony - PlayStation Vita",
        "Super Nintendo" => "Nintendo - Super Nintendo Entertainment System",
        "Nintendinho" => "Nintendo - Nintendo Entertainment System",
        "GameCube" => "Nintendo - GameCube",
        "Wii" => "Nintendo - Wii",
        "Wii U" => "Nintendo - Wii U",
        "Nintendo DS" => "Nintendo - Nintendo DS",
        "Nintendo 3DS" => "Nintendo - Nintendo 3DS",
        "Game Boy Advance" => "Nintendo - Game Boy Advance",
        "Game Boy Color" => "Nintendo - Game Boy Color",
        "Game Boy" => "Nintendo - Game Boy",
        "Nintendo 64" => "Nintendo - Nintendo 64",
        "Mega Drive" => "Sega - Mega Drive - Genesis",
        "Dreamcast" => "Sega - Dreamcast",
        "XBOX" => "Microsoft - Xbox",
        "Xbox 360" => "Microsoft - Xbox 360",
        _ => return None,
    })
}

/// Repositório do sistema: "Sony - PlayStation 2" → "Sony_-_PlayStation_2".
fn repo(system: &str) -> String {
    system.replace(' ', "_")
}

/// Imagens de uma pasta (resposta de `git/trees/{sha}`): nome sem ".png" → tamanho em bytes.
/// Só arquivos comuns: o acervo usa links simbólicos para lançamentos repetidos.
#[derive(Default)]
pub struct Folder {
    pub names: Vec<String>,
    pub sizes: HashMap<String, u64>,
}

pub fn parse_tree(v: &Value) -> Folder {
    let mut f = Folder::default();
    for e in v["tree"].as_array().into_iter().flatten() {
        if e["type"] != "blob" || e["mode"] != "100644" {
            continue;
        }
        let Some(name) = e["path"].as_str().and_then(|p| p.strip_suffix(".png")) else { continue };
        f.sizes.insert(name.to_string(), e["size"].as_u64().unwrap_or(0));
        f.names.push(name.to_string());
    }
    f
}

/// Nome-base de um lançamento: "Shadow of the Colossus (USA) (Demo)" → "Shadow of the Colossus".
fn base_name(n: &str) -> &str {
    n.find(" (").map(|i| &n[..i]).unwrap_or(n).trim()
}

/// Versões que não interessam (demonstrações, protótipos, quiosques...).
fn is_noise(n: &str) -> bool {
    let l = n.to_lowercase();
    ["(demo", "(beta", "(proto", "(kiosk", "(taikenban", "(sample", "(preview", "(trial"].iter().any(|t| l.contains(t))
}

/// Ordem de preferência das regiões (o restante vem depois, em ordem alfabética).
fn region_rank(n: &str) -> u8 {
    if !n.contains(" (") { 0 } else if n.contains("(USA") { 1 } else if n.contains("(Europe") { 2 } else if n.contains("(World") { 3 } else if n.contains("(Brazil") { 4 } else if n.contains("(Japan") { 6 } else { 5 }
}

/// Jogos (nomes-base) que contêm o termo, cada um com o melhor lançamento (região) para a capa.
pub fn libretro_titles(term: &str, names: &[String], max: usize) -> Vec<(String, String)> {
    let mut best: HashMap<String, String> = HashMap::new();
    for n in names.iter().filter(|n| !is_noise(n)) {
        let base = base_name(n);
        if !contains_all(term, base) {
            continue;
        }
        let e = best.entry(base.to_string()).or_insert_with(|| n.clone());
        if (region_rank(n), n.as_str()) < (region_rank(e), e.as_str()) && n.contains(" (") || !e.contains(" (") && n.contains(" (") {
            *e = n.clone();
        }
    }
    let mut v: Vec<(String, String)> = best.into_iter().collect();
    v.sort_by(|a, b| info::name_score(term, &b.0).partial_cmp(&info::name_score(term, &a.0)).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    v.truncate(max);
    v
}

/// Lançamentos de UM jogo (nome-base exato), sem repetidos (mesmo tamanho de arquivo = mesma imagem).
pub fn releases_of(base: &str, f: &Folder, max: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut v: Vec<&String> = f.names.iter().filter(|n| !is_noise(n) && base_name(n) == base).collect();
    if v.iter().any(|n| n.contains(" (")) {
        v.retain(|n| n.contains(" ("));
    }
    v.sort_by(|a, b| region_rank(a).cmp(&region_rank(b)).then(a.cmp(b)));
    v.into_iter().filter(|n| seen.insert(f.sizes.get(*n).copied().unwrap_or(0))).take(max).cloned().collect()
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Folder>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<Folder>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Índice de uma pasta. Baixado uma vez por execução (alguns sistemas têm milhares de títulos e a
/// API pública do GitHub limita as consultas por hora).
async fn folder(c: &reqwest::Client, system: &str, name: &str) -> Option<Arc<Folder>> {
    let key = format!("{system}/{name}");
    if let Some(f) = cache().lock().ok()?.get(&key) {
        return Some(f.clone());
    }
    let gh = |url: String| c.get(url).header("Accept", "application/vnd.github+json");
    let r = repo(system);
    let root = get_json(gh(format!("{GH_API}/{r}/git/trees/master"))).await?;
    let sha = root["tree"].as_array()?.iter().find(|e| e["path"] == name && e["type"] == "tree")?["sha"].as_str()?.to_string();
    let f = Arc::new(parse_tree(&get_json(gh(format!("{GH_API}/{r}/git/trees/{sha}"))).await?));
    cache().lock().ok()?.insert(key, f.clone());
    Some(f)
}

fn libretro_url(system: &str, dir: &str, name: &str) -> String {
    format!("{GH_RAW}/{}/master/{dir}/{}.png", repo(system), penc(name))
}

async fn libretro_search(c: reqwest::Client, term: String, platform: String) -> Vec<GameHit> {
    let Some(system) = libretro_system(&platform) else { return vec![] };
    let Some(f) = folder(&c, system, "Named_Boxarts").await else { return vec![] };
    libretro_titles(&term, &f.names, 10)
        .into_iter()
        .map(|(base, rel)| GameHit {
            title: base.clone(),
            platform: Some(platform.clone()),
            year: None,
            cover: Some(libretro_url(system, "Named_Boxarts", &rel)),
            sources: vec![SourceRef { source: "libretro".into(), id: format!("{platform}\u{1f}{base}") }],
        })
        .collect()
}

async fn libretro_images(c: reqwest::Client, id: String) -> ImageSet {
    let mut set = ImageSet::default();
    let Some((platform, base)) = id.split_once('\u{1f}') else { return set };
    let Some(system) = libretro_system(platform) else { return set };
    let page = Some(format!("https://github.com/libretro-thumbnails/{}", repo(system)));
    for (dir, kind, max) in [("Named_Boxarts", "cover", 6), ("Named_Snaps", "snap", 8), ("Named_Titles", "title", 2)] {
        let Some(f) = folder(&c, system, dir).await else { continue };
        for n in releases_of(base, &f, max) {
            let url = libretro_url(system, dir, &n);
            let s = shot(&url, &url, "libretro", kind, &n, page.clone());
            match kind {
                "cover" => set.covers.push(s),
                _ => set.screenshots.push(s),
            }
        }
    }
    set
}

// =============================================================================================
// Wikipédia (inglês)

/// Remove o sufixo de desambiguação: "Spider-Man (2018 video game)" → "Spider-Man".
fn wiki_base(title: &str) -> &str {
    title.find(" (").map(|i| &title[..i]).unwrap_or(title).trim()
}

/// O resultado da busca é o artigo de UM jogo (e não lista, franquia, filme, personagem...)?
/// Ano do jogo: no título ("(2018 video game)") ou na frase de abertura do artigo
/// ("X is a 2005 action-adventure game").
pub fn wiki_year(title: &str, snippet: &str) -> Option<i32> {
    if let Some(y) = title.find(" (").and_then(|i| year_in(&title[i..])) {
        return Some(y);
    }
    opening_year(snippet)
}

/// "… is a(n) 2005 … game": ano da frase de abertura, só quando ela descreve UM jogo.
fn opening_year(snippet: &str) -> Option<i32> {
    let plain = strip_tags(snippet).to_lowercase();
    let i = plain.find(" is a ").map(|i| i + 6).or_else(|| plain.find(" is an ").map(|i| i + 7))?;
    let rest = &plain[i..];
    let end = rest.find(". ").unwrap_or(rest.len()).min(140);
    let sentence = &rest[..end];
    if !sentence.contains("game") || sentence.contains("series") || sentence.contains("franchise") {
        return None;
    }
    year_in(&sentence[..sentence.len().min(12)]) // o ano vem logo depois de "is a"
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in s.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// O resultado da busca é o artigo de UM jogo (e não lista, franquia/série, filme, personagem...)?
pub fn wiki_is_game(title: &str, snippet: &str) -> bool {
    let l = title.to_lowercase();
    let sn = strip_tags(snippet).to_lowercase();
    let bad = l.starts_with("list of")
        || ["(film", "(series", "(franchise", "(character", "(soundtrack", "(album", "(comics", "(tv series"].iter().any(|x| l.contains(x))
        || sn.contains("is a series") || sn.contains("media franchise");
    !bad && (l.contains("video game)") || opening_year(snippet).is_some())
}

/// Palavras que indicam imagem que NÃO é captura de jogo (capa, logotipo, pessoas, produtos...).
const NOT_GAMEPLAY: [&str; 24] = [
    "logo", "icon", "cover", "box art", "boxart", "box", "poster", "cartridge", "disc", "console", "controller",
    "flag", "cosplay", "convention", "expo", "booth", "statue", "figure", "merch", "design", "concept", "artwork",
    "signature", "headshot",
];

pub fn looks_like_gameplay(file_title: &str) -> bool {
    let l = file_title.to_lowercase();
    !NOT_GAMEPLAY.iter().any(|b| l.contains(b))
}

async fn wiki(c: &reqwest::Client, q: &[(&str, &str)]) -> Option<Value> {
    get_json(c.get(WIKI).query(q)).await
}

async fn wiki_search(c: reqwest::Client, term: String) -> Vec<GameHit> {
    let search = format!("{term} video game");
    let Some(s) = wiki(&c, &[("action", "query"), ("format", "json"), ("list", "search"), ("srlimit", "15"), ("srsearch", search.as_str())]).await else {
        return vec![];
    };
    let arts: Vec<(String, Option<i32>)> = s["query"]["search"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let t = r["title"].as_str()?;
            let snip = r["snippet"].as_str().unwrap_or("");
            (wiki_is_game(t, snip) && contains_all(&term, wiki_base(t))).then(|| (t.to_string(), wiki_year(t, snip)))
        })
        .take(10)
        .collect();
    if arts.is_empty() {
        return vec![];
    }
    // capas (imagem principal de cada artigo, inclusive as de uso justo) em uma consulta
    let joined = arts.iter().map(|a| a.0.as_str()).collect::<Vec<_>>().join("|");
    let thumbs: HashMap<String, String> = wiki(&c, &[
        ("action", "query"), ("format", "json"), ("titles", joined.as_str()), ("prop", "pageimages"),
        ("piprop", "thumbnail"), ("pithumbsize", "320"), ("pilicense", "any"),
    ])
    .await
    .and_then(|v| {
        v["query"]["pages"].as_object().map(|o| {
            o.values().filter_map(|p| Some((p["title"].as_str()?.to_string(), p["thumbnail"]["source"].as_str()?.to_string()))).collect()
        })
    })
    .unwrap_or_default();
    arts.into_iter()
        .map(|(t, year)| GameHit {
            title: wiki_base(&t).to_string(),
            platform: None,
            year,
            cover: thumbs.get(&t).cloned(),
            sources: vec![SourceRef { source: "wikipedia".into(), id: t }],
        })
        .collect()
}

async fn wiki_images(c: reqwest::Client, article: String) -> ImageSet {
    let mut set = ImageSet::default();
    let page = Some(format!("https://en.wikipedia.org/wiki/{}", penc(&article.replace(' ', "_"))));
    let lead = wiki(&c, &[("action", "query"), ("format", "json"), ("titles", article.as_str()), ("prop", "pageimages"), ("piprop", "original|name"), ("pilicense", "any")])
        .await
        .and_then(|v| {
            let p = v["query"]["pages"].as_object()?.values().next()?.clone();
            Some((p["pageimage"].as_str()?.replace('_', " "), p["original"]["source"].as_str()?.to_string()))
        });
    if let Some((_, url)) = &lead {
        set.covers.push(shot(url, url, "wikipedia", "cover", "Wikipédia (capa)", page.clone()));
    }
    let Some(v) = wiki(&c, &[
        ("action", "query"), ("format", "json"), ("titles", article.as_str()), ("generator", "images"), ("gimlimit", "60"),
        ("prop", "imageinfo"), ("iiprop", "url|size|mime"),
    ])
    .await
    else {
        return set;
    };
    for p in v["query"]["pages"].as_object().into_iter().flat_map(|o| o.values()) {
        let name = p["title"].as_str().unwrap_or("").trim_start_matches("File:");
        let ii = &p["imageinfo"][0];
        let (w, h) = (ii["width"].as_i64().unwrap_or(0), ii["height"].as_i64().unwrap_or(0));
        // imagens LOCAIS (uso justo) do artigo = capturas de jogo; arquivos da Commons são fotos/ícones
        if p["imagerepository"] != "local"
            || !matches!(ii["mime"].as_str(), Some("image/jpeg" | "image/png" | "image/gif"))
            || w < 200
            || h < 120
            || lead.as_ref().map(|l| l.0 == name).unwrap_or(false)
            || !looks_like_gameplay(name)
        {
            continue;
        }
        if let Some(url) = ii["url"].as_str() {
            set.screenshots.push(shot(url, url, "wikipedia", "snap", name.rsplit_once('.').map(|x| x.0).unwrap_or(name), page.clone()));
        }
    }
    set
}

// =============================================================================================
// SteamGridDB (chave opcional)

fn sgdb(c: &reqwest::Client, key: &str, path: &str) -> reqwest::RequestBuilder {
    c.get(format!("{SGDB}{path}")).bearer_auth(key)
}

async fn sgdb_first_grid(c: &reqwest::Client, key: &str, id: i64) -> Option<String> {
    let v = get_json(sgdb(c, key, &format!("/grids/game/{id}?dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false"))).await?;
    v["data"].as_array()?.first()?["thumb"].as_str().map(String::from)
}

pub(crate) async fn sgdb_search(c: reqwest::Client, key: String, term: String) -> Vec<GameHit> {
    let Some(v) = get_json(sgdb(&c, &key, &format!("/search/autocomplete/{}", penc(&term)))).await else { return vec![] };
    let games: Vec<(i64, String, Option<i32>)> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|g| Some((g["id"].as_i64()?, g["name"].as_str()?.to_string(), g["release_date"].as_i64().map(unix_year))))
        .collect();
    let futs = games
        .into_iter()
        .map(|(id, name, year)| {
            let (c, key) = (c.clone(), key.clone());
            Box::pin(async move {
                GameHit {
                    cover: sgdb_first_grid(&c, &key, id).await,
                    title: name,
                    platform: None,
                    year,
                    sources: vec![SourceRef { source: "steamgriddb".into(), id: id.to_string() }],
                }
            }) as std::pin::Pin<Box<dyn Future<Output = GameHit> + Send>>
        })
        .collect();
    all(futs).await
}

/// Logo automática: a primeira logo do primeiro jogo da pesquisa do SteamGridDB (a mesma ordem do
/// seletor de logos). `Ok(None)` = sem jogo ou sem logo; `Err` = falha de rede/limite (tentar depois).
pub(crate) async fn sgdb_first_logo(c: &reqwest::Client, key: &str, term: &str) -> Result<Option<String>, ()> {
    let fetch = |path: String| async move {
        let r = sgdb(c, key, &path).send().await.map_err(|_| ())?;
        match r.status().as_u16() {
            200..=299 => r.json::<Value>().await.map_err(|_| ()),
            404 => Ok(Value::Null),
            _ => Err(()),
        }
    };
    let v = fetch(format!("/search/autocomplete/{}", penc(term.trim()))).await?;
    let Some(id) = v["data"].as_array().and_then(|a| a.first()).and_then(|g| g["id"].as_i64()) else { return Ok(None) };
    let v = fetch(format!("/logos/game/{id}?types=static&nsfw=false&humor=false")).await?;
    Ok(v["data"].as_array().and_then(|a| a.first()).and_then(|g| g["url"].as_str()).filter(|u| u.starts_with("https://")).map(String::from))
}

async fn sgdb_images(c: reqwest::Client, key: String, id: String) -> ImageSet {
    let mut set = ImageSet::default();
    let page = Some(format!("https://www.steamgriddb.com/game/{id}"));
    for (path, kind, max) in [
        (format!("/grids/game/{id}?dimensions=600x900,342x482,660x930&types=static&nsfw=false&humor=false"), "cover", 16),
        (format!("/heroes/game/{id}?types=static&nsfw=false&humor=false"), "background", 12),
    ] {
        let Some(v) = get_json(sgdb(&c, &key, &path)).await else { continue };
        for g in v["data"].as_array().into_iter().flatten().take(max) {
            if let (Some(url), Some(th)) = (g["url"].as_str(), g["thumb"].as_str()) {
                let s = shot(url, th, "steamgriddb", kind, "SteamGridDB", page.clone());
                if kind == "cover" { set.covers.push(s) } else { set.backgrounds.push(s) }
            }
        }
    }
    set
}

// =============================================================================================
// RAWG (chave opcional)

async fn rawg_search(c: reqwest::Client, key: String, term: String) -> Vec<GameHit> {
    let Some(v) = get_json(c.get(format!("{RAWG}/games")).query(&[
        ("key", key.as_str()), ("search", term.as_str()), ("page_size", "12"), ("search_precise", "true"), ("exclude_additions", "true"),
    ]))
    .await
    else {
        return vec![];
    };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["esrb_rating"]["slug"] != "adults-only")
        .filter_map(|g| {
            let plats: Vec<&str> = g["platforms"].as_array().into_iter().flatten().filter_map(|p| p["platform"]["name"].as_str()).take(3).collect();
            Some(GameHit {
                title: g["name"].as_str()?.to_string(),
                platform: (!plats.is_empty()).then(|| plats.join(", ")),
                year: g["released"].as_str().and_then(year_in),
                cover: g["background_image"].as_str().map(String::from),
                sources: vec![SourceRef { source: "rawg".into(), id: g["id"].as_i64()?.to_string() }],
            })
        })
        .collect()
}

async fn rawg_images(c: reqwest::Client, key: String, id: String) -> ImageSet {
    let mut set = ImageSet::default();
    let page = Some(format!("https://rawg.io/games/{id}"));
    if let Some(g) = get_json(c.get(format!("{RAWG}/games/{id}")).query(&[("key", key.as_str())])).await {
        if g["esrb_rating"]["slug"] == "adults-only" {
            return set;
        }
        for k in ["background_image", "background_image_additional"] {
            if let Some(u) = g[k].as_str() {
                set.backgrounds.push(shot(u, u, "rawg", "background", "RAWG", page.clone()));
            }
        }
    }
    if let Some(v) = get_json(c.get(format!("{RAWG}/games/{id}/screenshots")).query(&[("key", key.as_str()), ("page_size", "20")])).await {
        for s in v["results"].as_array().into_iter().flatten() {
            if let Some(u) = s["image"].as_str() {
                set.screenshots.push(shot(u, u, "rawg", "snap", "RAWG", page.clone()));
            }
        }
    }
    set
}

// =============================================================================================
// Junção e comandos

/// Une o mesmo jogo vindo de fontes diferentes e ordena (plataforma do jogo primeiro, depois o nome
/// mais parecido com o termo). Dois resultados só viram um quando têm nome equivalente, ano e
/// plataforma compatíveis E cada um é o ÚNICO candidato do outro naquela fonte — ex.: "Spider-Man"
/// do PS1 (sem ano) não é juntado a nenhum dos três "Spider-Man" da Wikipédia (1982, 2000, 2002).
/// Resultados da mesma fonte nunca são juntados (são jogos/versões distintos).
pub fn merge_hits(groups: Vec<Vec<GameHit>>, term: &str, platform: &str) -> Vec<GameHit> {
    let flat: Vec<GameHit> = groups.into_iter().flatten().collect();
    let src = |h: &GameHit| h.sources.first().map(|s| s.source.clone()).unwrap_or_default();
    let compatible = |a: &GameHit, b: &GameHit| {
        src(a) != src(b)
            && info::name_score(&a.title, &b.title) >= 1.0
            && (a.year.is_none() || b.year.is_none() || a.year == b.year)
            && (a.platform.is_none() || b.platform.is_none() || a.platform == b.platform)
    };
    // candidato único de i dentro da fonte de j?
    let unique_in = |i: usize, j: usize| {
        flat.iter().enumerate().filter(|(k, h)| src(h) == src(&flat[j]) && compatible(&flat[i], h) && *k != i).count() == 1
    };
    let mut parent: Vec<usize> = (0..flat.len()).collect();
    fn root(p: &mut Vec<usize>, i: usize) -> usize {
        let mut r = i;
        while p[r] != r { r = p[r]; }
        p[i] = r;
        r
    }
    for i in 0..flat.len() {
        for j in i + 1..flat.len() {
            if compatible(&flat[i], &flat[j]) && unique_in(i, j) && unique_in(j, i) {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                if ri != rj { parent[rj] = ri; }
            }
        }
    }
    let mut groups: Vec<(usize, GameHit)> = Vec::new();
    for (i, h) in flat.into_iter().enumerate() {
        let r = root(&mut parent, i);
        match groups.iter_mut().find(|g| g.0 == r) {
            Some((_, o)) => {
                for s in h.sources {
                    if !o.sources.contains(&s) { o.sources.push(s); }
                }
                o.cover = o.cover.take().or(h.cover);
                o.year = o.year.or(h.year);
                o.platform = o.platform.take().or(h.platform);
            }
            None => groups.push((r, h)),
        }
    }
    let mut out: Vec<GameHit> = groups.into_iter().map(|g| g.1).collect();
    let rank = |h: &GameHit| {
        let plat = h.platform.as_deref() == Some(platform);
        (if plat { 0 } else { 1 }, -((info::name_score(term, &h.title) * 1000.0) as i64))
    };
    out.sort_by_key(rank); // estável: empates mantêm a ordem das fontes
    out.truncate(MAX_HITS);
    out
}

fn keys(app: &AppHandle) -> (Option<String>, Option<String>) {
    let k = crate::apikeys::get(app);
    (k("sgdb_key"), k("rawg_key"))
}

type BoxFut<T> = std::pin::Pin<Box<dyn Future<Output = T> + Send>>;

/// Pesquisa o título e devolve a lista de jogos encontrados (nenhuma imagem é adicionada aqui).
#[tauri::command]
pub async fn search_games(app: AppHandle, term: String, platform: String) -> AppResult<Vec<GameHit>> {
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let c = client()?;
    let (sgdb_key, rawg_key) = keys(&app);
    let mut futs: Vec<BoxFut<Vec<GameHit>>> = vec![
        Box::pin(steam_search(c.clone(), term.clone())),
        Box::pin(libretro_search(c.clone(), term.clone(), platform.clone())),
        Box::pin(wiki_search(c.clone(), term.clone())),
    ];
    if let Some(k) = sgdb_key {
        futs.push(Box::pin(sgdb_search(c.clone(), k, term.clone())));
    }
    if let Some(k) = rawg_key {
        futs.push(Box::pin(rawg_search(c.clone(), k, term.clone())));
    }
    let hits = merge_hits(all(futs).await, &term, &platform);
    if hits.is_empty() {
        return Err(AppError::NotFound(format!("Nenhum jogo encontrado para \"{term}\". Tente o nome original ou mais curto.")));
    }
    Ok(hits)
}

/// Imagens do jogo escolhido (de todas as fontes em que ele foi encontrado), sem repetidas.
#[tauri::command]
pub async fn game_images(app: AppHandle, sources: Vec<SourceRef>) -> AppResult<ImageSet> {
    let c = client()?;
    let (sgdb_key, rawg_key) = keys(&app);
    let futs: Vec<BoxFut<ImageSet>> = sources
        .into_iter()
        .filter_map(|s| -> Option<BoxFut<ImageSet>> {
            Some(match s.source.as_str() {
                "steam" => Box::pin(steam_images(c.clone(), s.id)),
                "libretro" => Box::pin(libretro_images(c.clone(), s.id)),
                "wikipedia" => Box::pin(wiki_images(c.clone(), s.id)),
                "steamgriddb" => Box::pin(sgdb_images(c.clone(), sgdb_key.clone()?, s.id)),
                "rawg" => Box::pin(rawg_images(c.clone(), rawg_key.clone()?, s.id)),
                _ => return None,
            })
        })
        .collect();
    let mut out = ImageSet::default();
    let mut seen = HashSet::new();
    for set in all(futs).await {
        for (dst, src) in [(&mut out.covers, set.covers), (&mut out.backgrounds, set.backgrounds), (&mut out.screenshots, set.screenshots)] {
            dst.extend(src.into_iter().filter(|s| seen.insert(s.url.clone())));
        }
    }
    if out.covers.is_empty() && out.backgrounds.is_empty() && out.screenshots.is_empty() {
        return Err(AppError::NotFound("Nenhuma imagem encontrada para este jogo nas fontes disponíveis.".into()));
    }
    Ok(out)
}

/// Adiciona à galeria da página do jogo SÓ as imagens confirmadas pelo usuário (sem repetir).
#[tauri::command]
pub fn add_web_screenshots(db: State<Db>, profile_id: i64, game_id: i64, shots: Vec<WebShot>) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    let mut gi = info::load(&conn, game_id)?.unwrap_or_default();
    if gi.source.is_empty() {
        gi.source = "manual".into();
    }
    for s in shots.into_iter().filter(|s| s.url.starts_with("https://")) {
        gi.removed_screenshots.retain(|u| u != &s.url); // escolha explícita desfaz uma remoção antiga
        if !gi.web_screenshots.iter().any(|w| w.url == s.url) && !gi.screenshots.contains(&s.url) {
            gi.web_screenshots.push(s);
        }
    }
    info::store(&conn, game_id, &gi)?;
    Ok(gi)
}

/// Hosts das fontes acima (as imagens escolhidas só podem vir delas).
const IMAGE_HOSTS: [&str; 9] = [
    "steamstatic.com", "steampowered.com", "steamgriddb.com", "rawg.io", "wikimedia.org", "wikipedia.org", "githubusercontent.com", "akamaihd.net",
    "igdb.com",
];

pub fn allowed_image_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").split(':').next().unwrap_or("").to_lowercase();
    IMAGE_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// Baixa uma imagem escolhida (capa/fundo) e devolve um data URL para o formulário "Editar jogo".
/// Nada é salvo aqui: a imagem só substitui a atual quando o usuário salva o formulário.
#[tauri::command]
pub async fn fetch_image_data(url: String) -> AppResult<String> {
    let (bytes, mime) = download_image(&url).await?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(&bytes)))
}

/// Baixa uma imagem de uma das fontes permitidas (bytes + tipo MIME), com limite de tamanho.
pub(crate) async fn download_image(url: &str) -> AppResult<(Vec<u8>, String)> {
    if !allowed_image_url(url) {
        return Err(AppError::Validation("Endereço de imagem não permitido.".into()));
    }
    let r = client()?.get(url).send().await.map_err(net)?.error_for_status().map_err(net)?;
    // Redirecionamentos não podem levar a um endereço fora da lista de fontes permitidas.
    if !allowed_image_url(r.url().as_str()) {
        return Err(AppError::Validation("Endereço de imagem não permitido.".into()));
    }
    let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_string();
    if !mime.starts_with("image/") {
        return Err(AppError::Validation("O endereço não é uma imagem.".into()));
    }
    if r.content_length().map(|n| n as usize > MAX_IMAGE_BYTES).unwrap_or(false) {
        return Err(AppError::Validation("Imagem grande demais (máx. 15 MB).".into()));
    }
    let bytes = r.bytes().await.map_err(net)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(AppError::Validation("Imagem grande demais (máx. 15 MB).".into()));
    }
    Ok((bytes.to_vec(), mime))
}

/// Remove uma screenshot (Steam ou internet) da galeria do NEXSTAGE. Só o registro local é apagado:
/// a imagem continua no site de origem. A URL fica lembrada para não voltar em novas buscas da Steam.
#[tauri::command]
pub fn remove_game_screenshot(db: State<Db>, profile_id: i64, game_id: i64, url: String) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    let mut gi = info::load(&conn, game_id)?.ok_or_else(|| AppError::NotFound("Este jogo não tem screenshots salvas.".into()))?;
    let before = gi.screenshots.len() + gi.web_screenshots.len();
    gi.screenshots.retain(|u| u != &url);
    gi.web_screenshots.retain(|s| s.url != url);
    if gi.screenshots.len() + gi.web_screenshots.len() == before {
        return Err(AppError::NotFound("Screenshot não encontrada.".into()));
    }
    if !gi.removed_screenshots.contains(&url) {
        gi.removed_screenshots.push(url);
    }
    info::store(&conn, game_id, &gi)?;
    Ok(gi)
}

/// Esquece as remoções: as imagens da Steam voltam na próxima busca de informações.
#[tauri::command]
pub fn restore_removed_screenshots(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    let mut gi = info::load(&conn, game_id)?.unwrap_or_default();
    gi.removed_screenshots.clear();
    info::store(&conn, game_id, &gi)?;
    Ok(gi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(title: &str, plat: Option<&str>, year: Option<i32>, src: &str) -> GameHit {
        GameHit { title: title.into(), platform: plat.map(String::from), year, cover: None, sources: vec![SourceRef { source: src.into(), id: title.into() }] }
    }

    #[test]
    fn term_matching_and_years() {
        assert!(contains_all("Spider-Man", "Marvel's Spider-Man 2"));
        assert!(!contains_all("Spider-Man 2", "Spider-Man"));
        assert_eq!(year_in("4 Nov, 2022"), Some(2022));
        assert_eq!(year_in("(2018 video game)"), Some(2018));
        assert_eq!(year_in("12345"), None);
        assert_eq!(unix_year(1_536_278_400), 2018); // 2018-09-07
        assert_eq!(unix_year(946_684_799), 1999); // 1999-12-31 23:59:59
    }

    #[test]
    fn steam_filters_dlc_and_adult_content() {
        assert!(steam_acceptable(&json!({ "type": "game", "content_descriptors": { "ids": [2, 5] } })));
        assert!(!steam_acceptable(&json!({ "type": "dlc" })));
        assert!(!steam_acceptable(&json!({ "type": "game", "content_descriptors": { "ids": [3] } })));
    }

    #[test]
    fn merges_same_game_but_keeps_other_versions_apart() {
        let out = merge_hits(
            vec![
                vec![hit("Marvel's Spider-Man Remastered", Some("PC"), Some(2022), "steam"), hit("Spider-Man 2", Some("PC"), Some(2025), "steam")],
                vec![hit("Spider-Man", Some("Playstation 1"), None, "libretro")],
                vec![hit("Marvel’s Spider-Man Remastered", None, Some(2022), "steamgriddb"), hit("Spider-Man 2", None, Some(2004), "wikipedia")],
            ],
            "Spider-Man",
            "Playstation 1",
        );
        assert_eq!(out[0].platform.as_deref(), Some("Playstation 1")); // plataforma do jogo primeiro
        let rem = out.iter().find(|h| h.title.contains("Remastered")).unwrap();
        assert_eq!(rem.sources.len(), 2); // Steam + SteamGridDB juntos
        assert_eq!(out.iter().filter(|h| h.title == "Spider-Man 2").count(), 2); // anos diferentes: jogos diferentes
    }

    #[test]
    fn ambiguous_matches_stay_separate() {
        let out = merge_hits(
            vec![
                vec![hit("Spider-Man", Some("Playstation 1"), None, "libretro")],
                vec![hit("Spider-Man", None, Some(2002), "wikipedia"), hit("Spider-Man", None, Some(2000), "wikipedia")],
            ],
            "Spider-Man",
            "Playstation 1",
        );
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|h| h.sources.len() == 1));
        assert_eq!(wiki_year("Ultimate Spider-Man", "<span>Ultimate Spider-Man</span> is a 2005 action-adventure game based on the 2002 film"), Some(2005));
        assert_eq!(wiki_year("Spider-Man (2018 video game)", "released in 2020 video game"), Some(2018));
    }

    #[test]
    fn libretro_lists_games_and_their_releases() {
        let names: Vec<String> = ["Spider-Man (USA)", "Spider-Man (Europe)", "Spider-Man 2 - Enter Electro (USA)", "Spider-Man (USA) (Demo)", "Batman (USA)"]
            .iter().map(|s| s.to_string()).collect();
        let t = libretro_titles("Spider-Man", &names, 10);
        assert_eq!(t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["Spider-Man", "Spider-Man 2 - Enter Electro"]);
        assert_eq!(t[0].1, "Spider-Man (USA)");
        let mut f = Folder::default();
        for (n, sz) in [("Spider-Man (USA)", 10), ("Spider-Man (Europe)", 10), ("Spider-Man (Japan)", 20)] {
            f.names.push(n.into());
            f.sizes.insert(n.into(), sz);
        }
        assert_eq!(releases_of("Spider-Man", &f, 5), vec!["Spider-Man (USA)", "Spider-Man (Japan)"]); // Europa = mesma imagem
    }

    #[test]
    fn wikipedia_game_articles_only() {
        assert!(wiki_is_game("Spider-Man (2018 video game)", ""));
        assert!(wiki_is_game("Spider-Man: Web of Shadows", "Spider-Man: Web of Shadows is a 2008 action-adventure video game"));
        assert!(wiki_is_game("Ultimate Spider-Man (video game)", "Ultimate Spider-Man is a 2005 action-adventure game based on"));
        assert!(!wiki_is_game("Marvel's Spider-Man", "Marvel&#039;s Spider-Man is a series of superhero action-adventure video games, media franchise"));
        assert!(!contains_all("Spider-Man", "Man Face Spider I"));
        assert!(!wiki_is_game("Spider-Man (film)", "video game tie-in"));
        assert!(!wiki_is_game("List of Spider-Man video games", ""));
        assert!(looks_like_gameplay("Shadow of the Colossus (2005) - Valus fight"));
        assert!(!looks_like_gameplay("Spider-Man cover"));
    }

    #[test]
    fn parses_libretro_tree_skipping_symlinks() {
        let v = json!({ "tree": [
            { "path": "A (USA).png", "mode": "100644", "type": "blob", "size": 5 },
            { "path": "A (Korea).png", "mode": "120000", "type": "blob", "size": 4 },
            { "path": "sub", "mode": "040000", "type": "tree" }
        ]});
        assert_eq!(parse_tree(&v).names, vec!["A (USA)"]);
        assert_eq!(repo("Sony - PlayStation 2"), "Sony_-_PlayStation_2");
        assert_eq!(penc("Sony - PlayStation 2"), "Sony%20-%20PlayStation%202");
    }

    #[test]
    fn only_known_image_hosts() {
        assert!(allowed_image_url("https://cdn.cloudflare.steamstatic.com/steam/apps/1/library_600x900.jpg"));
        assert!(allowed_image_url("https://cdn2.steamgriddb.com/grid/x.png"));
        assert!(allowed_image_url("https://upload.wikimedia.org/a.jpg"));
        assert!(!allowed_image_url("http://cdn.cloudflare.steamstatic.com/x.jpg"));
        assert!(!allowed_image_url("https://evil.com/steamstatic.com/x.jpg"));
        assert!(!allowed_image_url("https://steamstatic.com.evil.com/x.jpg"));
    }

    /// Rede de verdade: `cargo test --lib webshots::tests::live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live() {
        tauri::async_runtime::block_on(async {
            let c = client().unwrap();
            let groups = vec![
                steam_search(c.clone(), "Spider-Man".into()).await,
                libretro_search(c.clone(), "Spider-Man".into(), "Playstation 1".into()).await,
                wiki_search(c.clone(), "Spider-Man".into()).await,
            ];
            for (i, g) in groups.iter().enumerate() {
                println!("fonte {i}: {:?}", g.iter().map(|h| (&h.title, &h.platform, h.year)).collect::<Vec<_>>());
            }
            let hits = merge_hits(groups, "Spider-Man", "PC");
            println!("JUNTOS ({}):", hits.len());
            for h in &hits {
                println!("  {} | {:?} | {:?} | {:?}", h.title, h.platform, h.year, h.sources.iter().map(|s| &s.source).collect::<Vec<_>>());
            }
            let remastered = hits.iter().find(|h| h.title.contains("Remastered")).expect("Remastered");
            let mut set = ImageSet::default();
            for s in &remastered.sources {
                let part = match s.source.as_str() {
                    "steam" => steam_images(c.clone(), s.id.clone()).await,
                    "wikipedia" => wiki_images(c.clone(), s.id.clone()).await,
                    _ => ImageSet::default(),
                };
                set.covers.extend(part.covers);
                set.backgrounds.extend(part.backgrounds);
                set.screenshots.extend(part.screenshots);
            }
            println!("Remastered: {} capas, {} fundos, {} screenshots", set.covers.len(), set.backgrounds.len(), set.screenshots.len());
            assert!(!set.covers.is_empty() && !set.screenshots.is_empty());
            let ps1 = hits.iter().find(|h| h.platform.as_deref() == Some("Playstation 1")).expect("PS1");
            let l = libretro_images(c.clone(), ps1.sources[0].id.clone()).await;
            println!("PS1 '{}': {} capas, {} screenshots", ps1.title, l.covers.len(), l.screenshots.len());
        });
    }
}
