//! Atualização dos emuladores cadastrados.
//!
//! - **De onde vem a versão nova:** as versões publicadas (releases) do repositório do emulador no GitHub
//!   (ou num servidor Gitea/Forgejo, como o do Eden). Emuladores reconhecidos já têm a fonte certa
//!   (`preset`); qualquer outro pode receber "dono/repositório" no cadastro.
//! - **Qual versão está instalada:** a que o NEXSTAGE instalou (chave do pacote gravada no banco) ou, sem
//!   ela, a data do executável: se ele é tão novo quanto a versão publicada, está em dia. Isso também evita
//!   "atualizar" para trás quem usa uma compilação de desenvolvimento mais nova que a estável.
//! - **Como instala:** baixa o pacote portátil (.zip/.7z) do Windows, extrai numa pasta temporária, localiza
//!   o mesmo executável dentro dele e copia os arquivos por cima da pasta do emulador, guardando uma cópia
//!   dos que forem substituídos (desfeita em caso de erro). Configurações, BIOS, saves e memory cards do
//!   usuário nunca são substituídos. Instalações em pasta protegida (Arquivos de Programas, via instalador)
//!   ficam só com o aviso.
//! - **Automático:** ao abrir o app (e a cada 6 h) verifica; com "Atualizar automaticamente", instala
//!   sozinho quando o emulador não está aberto e nenhum jogo está em execução.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const UA: &str = concat!("NEXSTAGE/", env!("NX_VERSION"));
/// Maior pacote aceito no download e maior tamanho extraído.
const MAX_DOWNLOAD: u64 = 1536 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 6 * 1024 * 1024 * 1024;
/// Folga entre a data dos arquivos (7-Zip preserva a data da compilação) e a publicação da versão.
const MTIME_SLACK: i64 = 6 * 3600;
/// Consultas repetidas dentro deste intervalo reaproveitam a anterior (limite de 60/h do GitHub sem login).
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
const RECHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

pub const SETTING_CHECK: &str = "emu_update_check";
pub const SETTING_AUTO: &str = "emu_update_auto";

// ---------------------------------------------------------------- fonte

/// Fontes conhecidas: trecho do nome do executável (ou do nome cadastrado) → repositório e, quando há
/// várias variantes do pacote do Windows, um trecho do nome que escolhe a mais compatível.
const PRESETS: &[(&str, &str, Option<&str>)] = &[
    ("pcsx2", "PCSX2/pcsx2", None),
    ("duckstation", "stenzek/duckstation", None),
    ("rpcs3", "RPCS3/rpcs3-binaries-win", None),
    ("ppsspp", "hrydgard/ppsspp", None),
    ("vita3k", "Vita3K/Vita3K", None),
    ("xemu", "xemu-project/xemu", None),
    ("xenia_canary", "xenia-canary/xenia-canary-releases", None),
    ("xenia canary", "xenia-canary/xenia-canary-releases", None),
    ("eden", "https://git.eden-emu.dev/eden-emu/eden", Some("msvc")),
    ("dolphin", DOLPHIN, None),
    ("cemu", "cemu-project/Cemu", None),
    ("melonds", "melonDS-emu/melonDS", None),
    ("mgba", "mgba-emu/mgba", Some("win64")),
    ("azahar", "azahar-emu/azahar", Some("msvc")),
    ("flycast", "flyinghead/flycast", None),
    ("snes9x", "snes9xgit/snes9x", None),
    ("mesen", "SourMesen/Mesen2", None),
    ("lichii", "zortos293/Lichii", Some("x86_64.zip")),
    ("rmg", "Rosalie241/RMG", Some("portable")),
    ("ares", "ares-emulator/ares", Some("x64")),
];

/// Fonte especial: versões do Dolphin publicadas no site oficial (não há pacotes no GitHub).
pub const DOLPHIN: &str = "dolphin";

/// Fonte de atualização reconhecida para o emulador. O executável vem primeiro: o nome cadastrado pode ser
/// genérico (ex.: "Emulador de Switch" apontando para o Eden).
pub fn preset(name: &str, exe: &str) -> Option<(&'static str, Option<&'static str>)> {
    let stem = Path::new(exe).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let name = name.to_lowercase();
    [stem, name].iter().find_map(|t| PRESETS.iter().find(|(k, _, _)| t.contains(k)).map(|(_, s, h)| (*s, *h)))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Host {
    GitHub { owner: String, repo: String },
    Gitea,
    Dolphin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// Endereço da API da última versão estável.
    pub api: String,
    /// Página das versões (para o usuário abrir).
    pub page: String,
    pub hint: Option<&'static str>,
    pub host: Host,
}

fn valid_seg(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// "dono/repositório" (GitHub) ou URL https de um repositório no GitHub ou num Gitea/Forgejo.
pub fn parse_source(raw: &str) -> AppResult<Source> {
    let bad = || AppError::Validation("Fonte de atualização inválida. Use dono/repositório do GitHub ou o endereço https do repositório.".into());
    let s = raw.trim().trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    if s.eq_ignore_ascii_case(DOLPHIN) || s.to_lowercase().starts_with("https://dolphin-emu.org") {
        return Ok(Source {
            api: String::from("https://dolphin-emu.org/update/latest/beta"),
            page: String::from("https://dolphin-emu.org/download/"),
            hint: None,
            host: Host::Dolphin,
        });
    }
    let (host, path) = if let Some(rest) = s.strip_prefix("https://") {
        let (h, p) = rest.split_once('/').ok_or_else(bad)?;
        (h.to_lowercase(), p)
    } else if s.contains("://") {
        return Err(bad());
    } else {
        ("github.com".to_string(), s)
    };
    let segs: Vec<&str> = path.split('/').collect();
    if segs.len() < 2 || !valid_seg(segs[0]) || !valid_seg(segs[1]) {
        return Err(bad());
    }
    // só os dois primeiros trechos importam (aceita colar ".../releases" ou ".../tree/main")
    let (o, r) = (segs[0], segs[1]);
    if !host.chars().all(|c| c.is_ascii_alphanumeric() || "-.:".contains(c)) || host.is_empty() {
        return Err(bad());
    }
    Ok(if host == "github.com" || host == "www.github.com" {
        Source {
            api: format!("https://api.github.com/repos/{o}/{r}/releases/latest"),
            page: format!("https://github.com/{o}/{r}/releases"),
            hint: None,
            host: Host::GitHub { owner: o.to_string(), repo: r.to_string() },
        }
    } else {
        Source {
            api: format!("https://{host}/api/v1/repos/{o}/{r}/releases/latest"),
            page: format!("https://{host}/{o}/{r}/releases"),
            hint: None,
            host: Host::Gitea,
        }
    })
}

/// Fonte efetiva: `update_source` NULL/vazio = automática, "-" = desligada.
fn effective_source(stored: Option<&str>, name: &str, exe: &str) -> Option<AppResult<Source>> {
    match stored.map(str::trim) {
        Some("-") => None,
        Some(s) if !s.is_empty() => Some(parse_source(s)),
        _ => {
            let (src, hint) = preset(name, exe)?;
            Some(parse_source(src).map(|s| Source { hint, ..s }))
        }
    }
}

// ---------------------------------------------------------------- versão publicada

#[derive(Deserialize)]
struct RelJson {
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize, Clone)]
struct AssetJson {
    // só os testes identificam o pacote escolhido pelo id
    #[cfg_attr(not(test), allow(dead_code))]
    id: i64,
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Identifica o pacote (id + data: versões "contínuas" trocam o arquivo mantendo a mesma tag).
    pub key: String,
}

#[derive(Clone, Debug)]
pub struct Latest {
    pub version: String,
    /// Data do pacote (ou da publicação), RFC 3339.
    pub date: Option<String>,
    pub asset: Option<Asset>,
}

const REJECT: &[&str] = &[
    "symbol", "pdb", "dbg", "debug", "arm64", "aarch64", "armhf", "linux", "macos", "mac-", "darwin", "android", "ubuntu",
    "appimage", "installer", "setup", "sse2", "source", "src", "legacy", "rog-ally", "steamdeck", "x86-32", "libretro",
];

/// Pacote portátil do Windows x64 entre os arquivos da versão.
fn pick_asset<'a>(assets: &'a [AssetJson], hint: Option<&str>) -> Option<&'a AssetJson> {
    let ok: Vec<&AssetJson> = assets
        .iter()
        .filter(|a| {
            let n = a.name.to_lowercase();
            (n.ends_with(".zip") || n.ends_with(".7z"))
                && n.contains("win")
                && !REJECT.iter().any(|r| n.contains(r))
                // "win32" é 32 bits, a menos que o nome diga x64 (ex.: "snes9x-1.63-win32-x64.zip")
                && !(n.contains("win32") && !n.contains("x64"))
        })
        .collect();
    hint.and_then(|h| ok.iter().find(|a| a.name.to_lowercase().contains(h)).copied()).or_else(|| ok.first().copied())
}

/// Número de versão escrito no nome do pacote ("pcsx2-v2.8.2-windows…" → "v2.8.2",
/// "rpcs3-v0.0.43-20644-…" → "v0.0.43-20644").
fn version_in(name: &str) -> Option<String> {
    let b = name.as_bytes();
    for i in 0..b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && (b[i - 1].is_ascii_alphanumeric() && b[i - 1] != b'v' && b[i - 1] != b'V')) {
            continue;
        }
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
            j += 1;
        }
        let core = name[i..j].trim_end_matches('.');
        if !core.contains('.') || core.split('.').any(str::is_empty) {
            continue;
        }
        let mut out = core.to_string();
        // número de compilação logo depois ("-20644")
        let tail = &name[i + core.len()..];
        if let Some(rest) = tail.strip_prefix('-') {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 3 && rest[digits.len()..].starts_with(['-', '_']) {
                out = format!("{out}-{digits}");
            }
        }
        let v = i > 0 && (b[i - 1] == b'v' || b[i - 1] == b'V');
        return Some(if v { format!("v{out}") } else { out });
    }
    None
}

fn to_latest(rel: RelJson, hint: Option<&str>) -> Latest {
    let asset = pick_asset(&rel.assets, hint).cloned();
    let date = asset.as_ref().and_then(|a| a.updated_at.clone().or_else(|| a.created_at.clone())).or(rel.published_at.clone());
    let tag = rel.tag_name.trim().to_string();
    let rolling = ["latest", "continuous", "preview", "nightly", "pre-release"].contains(&tag.to_lowercase().as_str()) || tag.starts_with("build-");
    let version = asset
        .as_ref()
        .and_then(|a| version_in(&a.name))
        .or_else(|| (!rolling).then(|| tag.clone()))
        .or_else(|| date.as_deref().map(|d| d.chars().take(10).collect()))
        .unwrap_or(tag);
    Latest {
        version,
        asset: asset.map(|a| Asset { key: asset_key(&a.name, a.updated_at.as_deref().or(a.created_at.as_deref())), name: a.name, url: a.browser_download_url, size: a.size }),
        date,
    }
}

/// Identifica o pacote: nome + data até a hora. Igual pela API e pela página pública (que arredonda a data
/// de outro jeito), e muda quando uma versão "contínua" troca o arquivo mantendo o nome.
fn asset_key(name: &str, date: Option<&str>) -> String {
    format!("{name}@{}", date.map(|d| d.chars().take(13).collect::<String>()).unwrap_or_default())
}

fn api_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent(UA).build().map_err(|e| AppError::Other(e.to_string()))
}

pub(crate) async fn fetch_latest(src: &Source) -> Result<Latest, String> {
    if src.host == Host::Dolphin {
        return fetch_dolphin(src).await;
    }
    match fetch_api(src).await {
        // sem login, a API do GitHub aceita 60 consultas por hora; as páginas públicas das versões não
        // entram nessa conta e trazem os mesmos pacotes
        Err(Api::Limited) => match &src.host {
            Host::GitHub { owner, repo } => fetch_github_page(owner, repo, src.hint).await,
            _ => Err("Limite de consultas ao GitHub atingido. Tente de novo mais tarde.".to_string()),
        },
        Err(Api::Msg(m)) => Err(m),
        Ok(l) => Ok(l),
    }
}

enum Api {
    Limited,
    Msg(String),
}

async fn fetch_api(src: &Source) -> Result<Latest, Api> {
    let c = api_client().map_err(|e| Api::Msg(e.to_string()))?;
    let resp = c
        .get(&src.api)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| Api::Msg("Sem conexão com o servidor de versões do emulador.".to_string()))?;
    match resp.status().as_u16() {
        200 => {}
        403 | 429 => return Err(Api::Limited),
        404 => return Err(Api::Msg("Repositório ou versão publicada não encontrados.".to_string())),
        s => return Err(Api::Msg(format!("HTTP {s}"))),
    }
    let rel: RelJson = resp.json().await.map_err(|e| Api::Msg(e.to_string()))?;
    Ok(to_latest(rel, src.hint))
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Pacotes listados na página pública de uma versão do GitHub (`expanded_assets`): (nome, endereço, data).
fn parse_assets_page(html: &str, owner: &str, repo: &str) -> Vec<(String, String, Option<String>)> {
    const HREF: &str = "href=\"";
    const DT: &str = "datetime=\"";
    let prefix = format!("{HREF}/{owner}/{repo}/releases/download/");
    let starts: Vec<usize> = html.match_indices(&prefix).map(|(i, _)| i).collect();
    let mut out: Vec<(String, String, Option<String>)> = Vec::new();
    for (k, &i) in starts.iter().enumerate() {
        let from = i + HREF.len();
        let Some(end) = html[from..].find('"').map(|e| from + e) else { continue };
        let path = &html[from..end];
        let name = pct_decode(path.rsplit('/').next().unwrap_or(""));
        let until = starts.get(k + 1).copied().unwrap_or(html.len());
        let date = html[end..until].find(DT).and_then(|d| {
            let a = end + d + DT.len();
            html[a..until].find('"').map(|e| html[a..a + e].to_string())
        });
        if !name.is_empty() && !out.iter().any(|(n, _, _)| *n == name) {
            out.push((name, format!("https://github.com{path}"), date));
        }
    }
    out
}

async fn fetch_github_page(owner: &str, repo: &str, hint: Option<&'static str>) -> Result<Latest, String> {
    let offline = |_| "Sem conexão com o servidor de versões do emulador.".to_string();
    let c = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    // /releases/latest redireciona para /releases/tag/<versão>
    let r = c.get(format!("https://github.com/{owner}/{repo}/releases/latest")).send().await.map_err(offline)?;
    let loc = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let Some(tag) = loc.split("/releases/tag/").nth(1).filter(|t| !t.is_empty()).map(str::to_string) else {
        return Err("Repositório ou versão publicada não encontrados.".to_string());
    };
    let html = c
        .get(format!("https://github.com/{owner}/{repo}/releases/expanded_assets/{tag}"))
        .send()
        .await
        .map_err(offline)?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let assets = parse_assets_page(&html, owner, repo)
        .into_iter()
        .enumerate()
        .map(|(i, (name, url, date))| AssetJson { id: i as i64, name, browser_download_url: url, size: 0, updated_at: date, created_at: None })
        .collect();
    Ok(to_latest(RelJson { tag_name: pct_decode(&tag), published_at: None, assets }, hint))
}

#[derive(Deserialize)]
struct DolphinJson {
    shortrev: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    artifacts: Vec<DolphinArtifact>,
}

#[derive(Deserialize)]
struct DolphinArtifact {
    system: String,
    url: String,
}

fn dolphin_latest(j: DolphinJson) -> Latest {
    let key = format!("dolphin@{}", if j.hash.is_empty() { &j.shortrev } else { &j.hash });
    let asset = j.artifacts.iter().find(|a| a.system.eq_ignore_ascii_case("Windows x64")).map(|a| Asset {
        name: a.url.rsplit('/').next().unwrap_or("dolphin.7z").to_string(),
        url: a.url.clone(),
        size: 0,
        key: key.clone(),
    });
    Latest { version: j.shortrev, date: j.date, asset }
}

/// Versões do Dolphin: endpoint do site oficial usado pelo próprio atualizador ("beta" = versões publicadas).
async fn fetch_dolphin(src: &Source) -> Result<Latest, String> {
    let c = api_client().map_err(|e| e.to_string())?;
    let r = c.get(&src.api).send().await.map_err(|_| "Sem conexão com o servidor de versões do emulador.".to_string())?;
    let j: DolphinJson = r.error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    Ok(dolphin_latest(j))
}

// ---------------------------------------------------------------- datas

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "2026-11-21T14:35:04Z" / "2026-11-21T14:35:04+02:00" → segundos Unix.
fn parse_ts(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let base = days_from_civil(n(0, 4)?, n(5, 7)?, n(8, 10)?) * 86400 + n(11, 13)? * 3600 + n(14, 16)? * 60 + n(17, 19)?;
    // fuso: depois dos segundos (e de frações, se houver)
    let rest = s.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let h: i64 = rest.get(1..3)?.parse().ok()?;
            let m: i64 = rest.get(4..6).and_then(|x| x.parse().ok()).unwrap_or(0);
            let o = h * 3600 + m * 60;
            if sign == '+' { o } else { -o }
        }
        _ => 0,
    };
    Some(base - off)
}

fn mtime(p: &Path) -> Option<i64> {
    let t = fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

fn iso(ts: i64) -> String {
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    // civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Há versão nova? A chave instalada pelo NEXSTAGE decide; sem ela (ou com outra), o executável tão
/// novo quanto o pacote publicado conta como em dia.
fn is_newer(stored_key: Option<&str>, exe_mtime: Option<i64>, latest: &Latest) -> bool {
    let Some(a) = &latest.asset else { return false };
    if stored_key == Some(a.key.as_str()) {
        return false;
    }
    match (exe_mtime, latest.date.as_deref().and_then(parse_ts)) {
        (Some(m), Some(p)) => p > m + MTIME_SLACK,
        _ => true,
    }
}

// ---------------------------------------------------------------- estado

#[derive(Default)]
pub struct EmuUpdates {
    /// Instalações novas pelo catálogo em andamento (id do catálogo).
    pub(crate) catalog_busy: Mutex<HashSet<String>>,
    cache: Mutex<HashMap<String, (Instant, Result<Latest, String>)>>,
    busy: Mutex<HashSet<i64>>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmuUpdateStatus {
    pub emulator_id: i64,
    pub name: String,
    /// "off" (desligada) | "unsupported" (sem fonte conhecida) | "upToDate" | "available" | "noPackage" | "error"
    pub state: &'static str,
    pub page: Option<String>,
    /// Versão instalada pelo NEXSTAGE (rótulo) e data do executável.
    pub installed: Option<String>,
    pub installed_date: Option<String>,
    pub latest: Option<String>,
    pub latest_date: Option<String>,
    /// Dá para instalar daqui (pacote .zip/.7z e pasta com permissão de escrita).
    pub installable: bool,
    pub message: Option<String>,
    pub busy: bool,
    #[serde(skip)]
    latest_key: Option<String>,
}

struct Row {
    id: i64,
    name: String,
    exe: String,
    source: Option<String>,
    version: Option<String>,
    key: Option<String>,
}

fn rows(conn: &Connection, only: Option<i64>) -> AppResult<Vec<Row>> {
    let mut st = conn.prepare(
        "SELECT id,name,exe_path,update_source,update_version,update_key FROM emulators WHERE ?1 IS NULL OR id=?1 ORDER BY name COLLATE NOCASE",
    )?;
    let r = st
        .query_map([only], |r| Ok(Row { id: r.get(0)?, name: r.get(1)?, exe: r.get(2)?, source: r.get(3)?, version: r.get(4)?, key: r.get(5)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(r)
}

pub(crate) fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".nexstage-write-test");
    let ok = fs::File::create(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// O executável está aberto? (no Windows, um .exe em execução não abre para escrita: erro 32)
fn exe_in_use(exe: &Path) -> bool {
    match fs::OpenOptions::new().write(true).open(exe) {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(32),
    }
}

fn status_for(app: &AppHandle, row: &Row, latest: Option<Result<Latest, String>>, page: Option<String>) -> EmuUpdateStatus {
    let exe = Path::new(&row.exe);
    let busy = app.state::<EmuUpdates>().busy.lock().map(|b| b.contains(&row.id)).unwrap_or(false);
    let mt = mtime(exe);
    let mut s = EmuUpdateStatus {
        emulator_id: row.id,
        name: row.name.clone(),
        state: "upToDate",
        page,
        installed: row.version.clone(),
        installed_date: mt.map(iso),
        latest: None,
        latest_date: None,
        installable: false,
        message: None,
        busy,
        latest_key: None,
    };
    match latest {
        None => s.state = "error",
        Some(Err(m)) => {
            s.state = "error";
            s.message = Some(m);
        }
        Some(Ok(l)) => {
            s.latest = Some(l.version.clone());
            s.latest_date = l.date.clone();
            s.latest_key = l.asset.as_ref().map(|a| a.key.clone());
            if l.asset.is_none() {
                s.state = "noPackage";
            } else if !exe.is_file() {
                s.state = "error";
                s.message = Some("Executável do emulador não encontrado.".into());
            } else if is_newer(row.key.as_deref(), mt, &l) {
                s.state = "available";
                s.installable = exe.parent().is_some_and(dir_writable);
                if !s.installable {
                    s.message = Some("O emulador está numa pasta protegida (instalado pelo instalador): atualize pela página do emulador.".into());
                }
            }
        }
    }
    s
}

pub(crate) async fn latest_cached(app: &AppHandle, src: &Source, force: bool) -> Result<Latest, String> {
    let st = app.state::<EmuUpdates>();
    if !force {
        if let Some((at, r)) = st.cache.lock().ok().and_then(|c| c.get(&src.api).cloned()) {
            if at.elapsed() < CACHE_TTL {
                return r;
            }
        }
    }
    let r = fetch_latest(src).await;
    if let Ok(mut c) = st.cache.lock() {
        c.insert(src.api.clone(), (Instant::now(), r.clone()));
    }
    r
}

async fn collect(app: &AppHandle, only: Option<i64>, force: bool) -> AppResult<Vec<EmuUpdateStatus>> {
    let list = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        rows(&conn, only)?
    };
    let mut out = Vec::with_capacity(list.len());
    for row in &list {
        let s = match effective_source(row.source.as_deref(), &row.name, &row.exe) {
            None => {
                let mut s = status_for(app, row, None, None);
                s.state = if row.source.as_deref().map(str::trim) == Some("-") { "off" } else { "unsupported" };
                s
            }
            Some(Err(e)) => status_for(app, row, Some(Err(e.to_string())), None),
            Some(Ok(src)) => {
                let l = latest_cached(app, &src, force).await;
                status_for(app, row, Some(l), Some(src.page.clone()))
            }
        };
        out.push(s);
    }
    Ok(out)
}

// ---------------------------------------------------------------- instalação

pub(crate) fn work_dir(app: &AppHandle, name: &str) -> AppResult<PathBuf> {
    let d = app.path().app_data_dir().map_err(|e| AppError::Other(e.to_string()))?.join("emu_updates").join(name);
    Ok(d)
}

fn progress(app: &AppHandle, id: i64, phase: &str, pct: Option<u8>) {
    let _ = app.emit("emulator-update-progress", serde_json::json!({ "id": id, "phase": phase, "pct": pct }));
}

pub(crate) async fn download(a: &Asset, dest: &Path, on_pct: &(dyn Fn(u8) + Send + Sync)) -> AppResult<()> {
    let net = |_| AppError::Other("Falha ao baixar a atualização do emulador (verifique a internet).".into());
    let c = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(90))
        .user_agent(UA)
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let mut resp = c.get(&a.url).send().await.map_err(net)?.error_for_status().map_err(net)?;
    let total = resp.content_length().unwrap_or(a.size).max(1);
    if total > MAX_DOWNLOAD {
        return Err(AppError::Validation("Pacote da atualização grande demais.".into()));
    }
    let mut f = fs::File::create(dest)?;
    let (mut got, mut last) = (0u64, 255u8);
    while let Some(chunk) = resp.chunk().await.map_err(net)? {
        got += chunk.len() as u64;
        if got > MAX_DOWNLOAD {
            return Err(AppError::Validation("Pacote da atualização grande demais.".into()));
        }
        f.write_all(&chunk)?;
        let pct = (got * 100 / total).min(100) as u8;
        if pct != last {
            last = pct;
            on_pct(pct);
        }
    }
    f.flush()?;
    Ok(())
}

/// Caminho relativo seguro de uma entrada do pacote (sem "..", raiz ou unidade).
fn safe_rel(name: &str) -> Option<PathBuf> {
    let mut p = PathBuf::new();
    for seg in name.split(['/', '\\']) {
        match seg {
            "" | "." => continue,
            ".." => return None,
            s if s.contains(':') => return None,
            s => p.push(s),
        }
    }
    (!p.as_os_str().is_empty()).then_some(p)
}

fn copy_limited(r: &mut dyn Read, dest: &Path, total: &mut u64) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(dest)?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        *total += n as u64;
        if *total > MAX_EXTRACTED {
            return Err(std::io::Error::other("pacote extraído grande demais"));
        }
        f.write_all(&buf[..n])?;
    }
    Ok(())
}

pub(crate) fn extract(pkg: &Path, out: &Path) -> AppResult<()> {
    let bad = |e: String| AppError::Other(format!("Não foi possível extrair o pacote: {e}"));
    fs::create_dir_all(out)?;
    let mut total = 0u64;
    let name = pkg.to_string_lossy().to_lowercase();
    if name.ends_with(".7z") {
        let mut ar = sevenz_rust2::ArchiveReader::open(pkg, sevenz_rust2::Password::empty()).map_err(|e| bad(e.to_string()))?;
        ar.for_each_entries(|entry, reader| {
            let Some(rel) = safe_rel(entry.name()) else { return Ok(true) };
            let dest = out.join(rel);
            if entry.is_directory() {
                fs::create_dir_all(&dest)?;
            } else {
                copy_limited(reader, &dest, &mut total)?;
            }
            Ok(true)
        })
        .map_err(|e| bad(e.to_string()))?;
    } else {
        let mut z = zip::ZipArchive::new(fs::File::open(pkg)?).map_err(|e| bad(e.to_string()))?;
        for i in 0..z.len() {
            let mut f = z.by_index(i).map_err(|e| bad(e.to_string()))?;
            let Some(rel) = f.enclosed_name() else { continue };
            let dest = out.join(rel);
            if f.is_dir() {
                fs::create_dir_all(&dest)?;
            } else {
                copy_limited(&mut f, &dest, &mut total).map_err(|e| bad(e.to_string()))?;
            }
        }
    }
    Ok(())
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if e.file_type()?.is_dir() {
            walk(&p, base, out)?;
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

/// Pasta do pacote que corresponde à pasta do emulador: onde está o mesmo executável (o mais raso).
fn find_root(extracted: &Path, exe_name: &str) -> AppResult<PathBuf> {
    find_root_any(extracted, &[exe_name]).map(|(root, _)| root).ok_or_else(|| AppError::Validation(format!("O pacote baixado não contém {exe_name}. Se o executável foi renomeado, atualize pela página do emulador.")))
}

/// Arquivos do usuário que já existem na pasta do emulador e nunca são substituídos.
fn protected(rel: &Path) -> bool {
    const DIRS: &[&str] = &[
        "inis", "memcards", "sstates", "savestates", "saves", "bios", "config", "user", "memstick", "cache", "dev_hdd0", "dev_hdd1",
        "dev_usb000", "ux0", "portable", "covers", "cheats", "patches", "logs", "gamesettings", "textures", "snaps", "screenshots",
    ];
    let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    if comps.len() > 1 && comps[..comps.len() - 1].iter().any(|c| DIRS.contains(&c.as_str())) {
        return true;
    }
    let file = comps.last().map(String::as_str).unwrap_or("");
    let ext = file.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    comps.len() == 1 && (["ini", "cfg", "yml", "yaml", "toml"].contains(&ext) || file.starts_with("portable"))
}

/// Copia o pacote por cima da pasta do emulador. Em caso de erro, desfaz o que já foi feito.
pub(crate) fn apply(root: &Path, target: &Path, backup: &Path) -> AppResult<usize> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    let mut replaced: Vec<PathBuf> = Vec::new();
    let mut created: Vec<PathBuf> = Vec::new();
    let res = (|| -> std::io::Result<usize> {
        let mut n = 0;
        for rel in &files {
            let dest = target.join(rel);
            if dest.exists() {
                if protected(rel) {
                    continue;
                }
                let b = backup.join(rel);
                if let Some(p) = b.parent() {
                    fs::create_dir_all(p)?;
                }
                fs::copy(&dest, &b)?;
                replaced.push(rel.clone());
            } else {
                if let Some(p) = dest.parent() {
                    fs::create_dir_all(p)?;
                }
                created.push(rel.clone());
            }
            fs::copy(root.join(rel), &dest)?;
            n += 1;
        }
        Ok(n)
    })();
    match res {
        Ok(n) => Ok(n),
        Err(e) => {
            for rel in &replaced {
                let _ = fs::copy(backup.join(rel), target.join(rel));
            }
            for rel in &created {
                let _ = fs::remove_file(target.join(rel));
            }
            Err(AppError::Other(format!("A atualização foi desfeita: {e}")))
        }
    }
}

struct BusyGuard<'a>(&'a AppHandle, i64);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut b) = self.0.state::<EmuUpdates>().busy.lock() {
            b.remove(&self.1);
        }
    }
}

async fn install(app: &AppHandle, id: i64) -> AppResult<EmuUpdateStatus> {
    {
        let st = app.state::<EmuUpdates>();
        let mut b = st.busy.lock().map_err(|_| AppError::Other(String::from("estado")))?;
        if !b.insert(id) {
            return Err(AppError::Validation("Este emulador já está sendo atualizado.".into()));
        }
    }
    let _guard = BusyGuard(app, id);
    let row = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        rows(&conn, Some(id))?.pop().ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))?
    };
    let src = match effective_source(row.source.as_deref(), &row.name, &row.exe) {
        Some(s) => s?,
        None => return Err(AppError::Validation("Este emulador não tem fonte de atualização.".into())),
    };
    progress(app, id, "check", None);
    let latest = latest_cached(app, &src, false).await.map_err(AppError::Other)?;
    let asset = latest.asset.clone().ok_or_else(|| AppError::Validation("A versão publicada não tem pacote portátil para Windows.".into()))?;
    let exe = PathBuf::from(&row.exe);
    let exe_name = exe.file_name().and_then(|n| n.to_str()).map(String::from).ok_or_else(|| AppError::NotFound("Executável do emulador não encontrado.".into()))?;
    let dir = exe.parent().map(Path::to_path_buf).filter(|_| exe.is_file()).ok_or_else(|| AppError::NotFound("Executável do emulador não encontrado.".into()))?;
    if !dir_writable(&dir) {
        return Err(AppError::Validation("O emulador está numa pasta protegida (instalado pelo instalador): atualize pela página do emulador.".into()));
    }
    if exe_in_use(&exe) {
        return Err(AppError::Validation("Feche o emulador antes de atualizar.".into()));
    }

    let work = work_dir(app, &id.to_string())?;
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let ext = if asset.name.to_lowercase().ends_with(".7z") { "7z" } else { "zip" };
    let pkg = work.join(format!("package.{ext}"));
    let result = async {
        download(&asset, &pkg, &|pct| progress(app, id, "download", Some(pct))).await?;
        progress(app, id, "install", None);
        let (pkg2, work2, dir2, exe2) = (pkg.clone(), work.clone(), dir.clone(), exe.clone());
        tauri::async_runtime::spawn_blocking(move || -> AppResult<usize> {
            let files = work2.join("files");
            extract(&pkg2, &files)?;
            let root = find_root(&files, &exe_name)?;
            // o emulador pode ter sido aberto durante o download
            if exe_in_use(&exe2) {
                return Err(AppError::Validation("Feche o emulador antes de atualizar.".into()));
            }
            apply(&root, &dir2, &work2.join("backup"))
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }
    .await;
    let _ = fs::remove_dir_all(&work);
    result?;

    {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        conn.execute("UPDATE emulators SET update_version=?1, update_key=?2 WHERE id=?3", params![latest.version, asset.key, id])?;
    }
    progress(app, id, "done", None);
    drop(_guard);
    collect(app, Some(id), false).await?.pop().ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))
}

/// Como `find_root`, aceitando vários nomes de executável (em ordem de preferência). Devolve a pasta e o
/// nome do executável como está no pacote.
pub(crate) fn find_root_any(extracted: &Path, names: &[&str]) -> Option<(PathBuf, String)> {
    let mut files = Vec::new();
    walk(extracted, extracted, &mut files).ok()?;
    names.iter().find_map(|want| {
        files
            .iter()
            .filter(|f| f.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(want)))
            .min_by_key(|f| f.components().count())
            .map(|f| (extracted.join(f.parent().unwrap_or(Path::new(""))), f.file_name().unwrap_or_default().to_string_lossy().into_owned()))
    })
}

// ---------------------------------------------------------------- comandos

/// Situação de atualização de todos os emuladores (`force` = consulta de novo, sem o cache de 10 min).
#[tauri::command]
pub async fn emulator_updates(app: AppHandle, force: bool) -> AppResult<Vec<EmuUpdateStatus>> {
    collect(&app, None, force).await
}

/// Baixa e instala a versão nova do emulador.
#[tauri::command]
pub async fn emulator_update_install(app: AppHandle, id: i64) -> AppResult<EmuUpdateStatus> {
    install(&app, id).await
}

/// Fonte automática para o emulador do formulário (mostrada como sugestão; vazio = não reconhecido).
#[tauri::command]
pub fn emulator_update_preset(name: String, exe_path: String) -> Option<String> {
    preset(&name, &exe_path).map(|(s, _)| s.to_string())
}

/// Normaliza a fonte digitada no cadastro: vazio = automática (NULL), "-" = desligada.
pub fn normalize_source(raw: Option<&str>) -> AppResult<Option<String>> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some("-") => Ok(Some("-".into())),
        Some(s) => parse_source(s).map(|_| Some(s.trim_end_matches('/').to_string())),
    }
}

// ---------------------------------------------------------------- segundo plano

fn setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| r.get(0)).optional().ok().flatten()
}

async fn background_pass(app: &AppHandle) {
    let (check, auto) = {
        let db = app.state::<Db>();
        let Ok(conn) = db.conn() else { return };
        (setting(&conn, SETTING_CHECK).as_deref() != Some("0"), setting(&conn, SETTING_AUTO).as_deref() == Some("1"))
    };
    if !check && !auto {
        return;
    }
    let Ok(list) = collect(app, None, true).await else { return };
    for s in list.into_iter().filter(|s| s.state == "available") {
        let version = s.latest.clone().unwrap_or_default();
        let game_running = app.state::<crate::overlay::Runtime>().is_running();
        if auto && s.installable && !game_running {
            let exe_busy = {
                let db = app.state::<Db>();
                db.conn().ok().and_then(|c| rows(&c, Some(s.emulator_id)).ok()).and_then(|mut r| r.pop()).is_some_and(|r| exe_in_use(Path::new(&r.exe)))
            };
            if !exe_busy {
                match install(app, s.emulator_id).await {
                    Ok(_) => {
                        let _ = app.emit("emulator-update-installed", serde_json::json!({ "id": s.emulator_id, "name": s.name, "version": version }));
                    }
                    Err(e) => {
                        let _ = app.emit("emulator-update-failed", serde_json::json!({ "id": s.emulator_id, "name": s.name, "message": e.to_string() }));
                    }
                }
                continue;
            }
        }
        if !check {
            continue;
        }
        // aviso uma vez por versão
        let Some(key) = s.latest_key.clone() else { continue };
        let k = format!("emu_update_notified:{}", s.emulator_id);
        let db = app.state::<Db>();
        let Ok(conn) = db.conn() else { continue };
        if setting(&conn, &k).as_deref() == Some(key.as_str()) {
            continue;
        }
        let _ = conn.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", params![k, key]);
        let _ = app.emit("emulator-update-available", serde_json::json!({ "id": s.emulator_id, "name": s.name, "version": version }));
    }
}

/// Verificação ao abrir o app (sem atrasar a abertura) e a cada 6 h enquanto ele estiver aberto.
pub fn spawn_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(40)).await;
        loop {
            background_pass(&app).await;
            tokio::time::sleep(RECHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: i64, name: &str, date: &str) -> AssetJson {
        AssetJson { id, name: String::from(name), browser_download_url: format!("https://x/{name}"), size: 1, updated_at: Some(String::from(date)), created_at: None }
    }

    #[test]
    fn sources() {
        let g = parse_source("PCSX2/pcsx2").unwrap();
        assert_eq!(g.api, "https://api.github.com/repos/PCSX2/pcsx2/releases/latest");
        assert_eq!(parse_source("https://github.com/PCSX2/pcsx2/releases/").unwrap(), g);
        let e = parse_source("https://git.eden-emu.dev/eden-emu/eden.git").unwrap();
        assert_eq!(e.api, "https://git.eden-emu.dev/api/v1/repos/eden-emu/eden/releases/latest");
        for bad in ["", "pcsx2", "http://github.com/a/b", "a/..", "a b/c", "https://h/a"] {
            assert!(parse_source(bad).is_err(), "{bad}");
        }
        assert_eq!(normalize_source(Some(" ")).unwrap(), None);
        assert_eq!(normalize_source(Some("-")).unwrap().as_deref(), Some("-"));
    }

    #[test]
    fn presets() {
        assert_eq!(preset("PCSX2", "D:/emu/pcsx2-qt.exe").unwrap().0, "PCSX2/pcsx2");
        assert_eq!(preset("Xbox 360", "D:/x/xenia_canary.exe").unwrap().0, "xenia-canary/xenia-canary-releases");
        assert!(preset("Xbox 360", "D:/x/xenia.exe").is_none());
        assert_eq!(preset("Switch", "D:/eden/eden.exe").unwrap().1, Some("msvc"));
        assert!(preset("Ryujinx", "D:/r/Ryujinx.exe").is_none());
        assert!(effective_source(Some("-"), "PCSX2", "pcsx2-qt.exe").is_none());
    }

    #[test]
    fn picks_windows_portable_package() {
        let pcsx2 = [
            asset(1, "pcsx2-v2.8.2-linux-appimage-x64-Qt.AppImage", "2026-09-04T23:04:48Z"),
            asset(2, "pcsx2-v2.8.2-windows-x64-installer.exe", "2026-09-04T23:04:48Z"),
            asset(3, "pcsx2-v2.8.2-windows-x64-Qt-symbols.7z", "2026-09-04T23:04:48Z"),
            asset(4, "pcsx2-v2.8.2-windows-x64-Qt.7z", "2026-09-04T23:04:48Z"),
        ];
        assert_eq!(pick_asset(&pcsx2, None).unwrap().id, 4);
        let duck = [
            asset(1, "duckstation-windows-arm64-release.zip", "x"),
            asset(2, "duckstation-windows-x64-release-symbols.7z", "x"),
            asset(3, "duckstation-windows-x64-sse2-release.zip", "x"),
            asset(4, "duckstation-windows-x64-release.zip", "x"),
        ];
        assert_eq!(pick_asset(&duck, None).unwrap().id, 4);
        let eden = [asset(1, "Eden-Windows-v0.2.1-amd64-clang-pgo.zip", "x"), asset(2, "Eden-Windows-v0.2.1-amd64-msvc-standard.zip", "x"), asset(3, "Eden-Windows-v0.2.1-arm64-clang-pgo.zip", "x")];
        assert_eq!(pick_asset(&eden, Some("msvc")).unwrap().id, 2);
        let xemu = [asset(1, "xemu-0.8.139-dbg-windows-x86_64.zip", "x"), asset(2, "xemu-0.8.139-windows-x86_64.zip", "x"), asset(3, "xemu-0.8.139-macos-universal-unsigned.zip", "x")];
        assert_eq!(pick_asset(&xemu, None).unwrap().id, 2);
        assert_eq!(pick_asset(&[asset(1, "rpcs3-v0.0.43-20644-3bfac607_win64_msvc.7z", "x")], None).unwrap().id, 1);
        assert_eq!(pick_asset(&[asset(1, "windows-latest.zip", "x"), asset(2, "windows-arm64-latest.zip", "x")], None).unwrap().id, 1);
        assert_eq!(pick_asset(&[asset(1, "xenia_canary_windows_.zip", "x")], None).unwrap().id, 1);
    }

    #[test]
    fn versions() {
        assert_eq!(version_in("pcsx2-v2.8.2-windows-x64-Qt.7z").as_deref(), Some("v2.8.2"));
        assert_eq!(version_in("rpcs3-v0.0.43-20644-3bfac607_win64_msvc.7z").as_deref(), Some("v0.0.43-20644"));
        assert_eq!(version_in("PPSSPP-v1.20.4-Windows-x64.zip").as_deref(), Some("v1.20.4"));
        assert_eq!(version_in("xemu-0.8.139-windows-x86_64.zip").as_deref(), Some("0.8.139"));
        assert_eq!(version_in("duckstation-windows-x64-release.zip"), None);
        assert_eq!(version_in("windows-latest.zip"), None);
        let rel = RelJson { tag_name: String::from("latest"), published_at: None, assets: vec![asset(9, "duckstation-windows-x64-release.zip", "2026-11-19T03:13:53Z")] };
        let l = to_latest(rel, None);
        assert_eq!(l.version, "2026-11-19");
        assert_eq!(l.asset.unwrap().key, "duckstation-windows-x64-release.zip@2026-11-19T03");
    }

    #[test]
    fn dates_and_decision() {
        assert_eq!(parse_ts("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(parse_ts("2026-11-21T14:35:04Z").map(iso).as_deref(), Some("2026-11-21T14:35:04Z"));
        assert_eq!(parse_ts("2026-11-21T16:35:04+02:00"), parse_ts("2026-11-21T14:35:04Z"));
        assert_eq!(parse_ts("2026-11-21T14:35:04.123Z"), parse_ts("2026-11-21T14:35:04Z"));
        let pubd = parse_ts("2026-11-21T14:35:04Z").unwrap();
        let l = Latest { version: String::from("v1"), date: Some(String::from("2026-11-21T14:35:04Z")), asset: Some(Asset { name: String::new(), url: String::new(), size: 0, key: String::from("k") }) };
        assert!(!is_newer(Some("k"), Some(0), &l), "mesma chave");
        assert!(is_newer(None, Some(pubd - 3 * 86400), &l), "executável antigo");
        assert!(!is_newer(None, Some(pubd - 3600), &l), "7-Zip: data da compilação logo antes da publicação");
        assert!(!is_newer(Some("old"), Some(pubd + 86400), &l), "compilação mais nova que a estável");
        assert!(is_newer(None, None, &l));
        assert!(!is_newer(None, None, &Latest { asset: None, ..l.clone() }));
    }

    #[test]
    fn public_page_and_dolphin_sources() {
        let html = r#"<li><a href="/mgba-emu/mgba/releases/download/0.10.5/mGBA-0.10.5-win32.7z" rel="nofollow">x</a>
            <relative-time datetime="2026-03-01T10:00:00Z"></relative-time></li>
            <li><a href="/mgba-emu/mgba/releases/download/0.10.5/mGBA-0.10.5-win64.7z">y</a><span>12 MB</span>
            <relative-time datetime="2026-03-01T10:05:00Z"></relative-time></li>
            <li><a href="/mgba-emu/mgba/archive/refs/tags/0.10.5.zip">Source code</a></li>"#;
        let a = parse_assets_page(html, "mgba-emu", "mgba");
        assert_eq!(a.len(), 2);
        assert_eq!(a[1], (String::from("mGBA-0.10.5-win64.7z"), String::from("https://github.com/mgba-emu/mgba/releases/download/0.10.5/mGBA-0.10.5-win64.7z"), Some(String::from("2026-03-01T10:05:00Z"))));
        assert_eq!(pct_decode("Mesen%202.1.zip"), "Mesen 2.1.zip");
        let j: DolphinJson = serde_json::from_str(r#"{"shortrev":"2609b","date":"2026-11-08T01:30:31.535Z","hash":"3a5b","artifacts":[
            {"system":"Windows arm64","url":"https://dl.dolphin-emu.org/releases/2609b/dolphin-2609b-arm64.7z"},
            {"system":"Windows x64","url":"https://dl.dolphin-emu.org/releases/2609b/dolphin-2609b-x64.7z"}]}"#).unwrap();
        let l = dolphin_latest(j);
        assert_eq!((l.version.as_str(), l.asset.as_ref().unwrap().name.as_str()), ("2609b", "dolphin-2609b-x64.7z"));
        assert_eq!(parse_source("dolphin").unwrap().host, Host::Dolphin);
        assert_eq!(preset("GameCube", "D:/Dolphin-x64/Dolphin.exe").unwrap().0, DOLPHIN);
        assert_eq!(preset("x", "D:/snes9x/snes9x-x64.exe").unwrap().0, "snes9xgit/snes9x");
        // snes9x publica o x64 com "win32" no nome
        let sn = [asset(1, "snes9x-1.63-win32.zip", "x"), asset(2, "snes9x-1.63-libretro-x64.zip", "x"), asset(3, "snes9x-1.63-win32-x64.zip", "x")];
        assert_eq!(pick_asset(&sn, None).unwrap().id, 3);
    }

    #[test]
    fn protects_user_files_and_paths() {
        assert!(protected(Path::new("PCSX2.ini")));
        assert!(protected(Path::new("portable.txt")));
        assert!(protected(Path::new("inis/PCSX2.ini")));
        assert!(protected(Path::new("memcards/Mcd001.ps2")));
        assert!(protected(Path::new("dev_hdd0/game/x.bin")));
        assert!(!protected(Path::new("pcsx2-qt.exe")));
        assert!(!protected(Path::new("resources/gamecontrollerdb.txt")));
        assert!(!protected(Path::new("translations/en.ini")));
        assert_eq!(safe_rel("a/../b"), None);
        assert_eq!(safe_rel("C:/x"), None);
        assert_eq!(safe_rel("/a\\b"), Some(PathBuf::from("a").join("b")));
    }

    #[test]
    fn installs_over_folder_with_rollback_data() {
        let base = std::env::temp_dir().join(format!("nx-emu-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (pkg, target, backup) = (base.join("pkg/PCSX2 v2"), base.join("emu"), base.join("bak"));
        fs::create_dir_all(pkg.join("resources")).unwrap();
        fs::create_dir_all(target.join("inis")).unwrap();
        fs::write(pkg.join("pcsx2-qt.exe"), "new").unwrap();
        fs::write(pkg.join("resources/a.dat"), "res").unwrap();
        fs::write(pkg.join("PCSX2.ini"), "default").unwrap();
        fs::write(target.join("pcsx2-qt.exe"), "old").unwrap();
        fs::write(target.join("PCSX2.ini"), "mine").unwrap();
        let root = find_root(&base.join("pkg"), "PCSX2-QT.EXE").unwrap();
        assert_eq!(root, pkg);
        assert!(find_root(&base.join("pkg"), "other.exe").is_err());
        apply(&root, &target, &backup).unwrap();
        assert_eq!(fs::read_to_string(target.join("pcsx2-qt.exe")).unwrap(), "new");
        assert_eq!(fs::read_to_string(target.join("resources/a.dat")).unwrap(), "res");
        assert_eq!(fs::read_to_string(target.join("PCSX2.ini")).unwrap(), "mine", "configuração do usuário mantida");
        assert_eq!(fs::read_to_string(backup.join("pcsx2-qt.exe")).unwrap(), "old");
        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod e2e {
    use super::*;

    /// Baixa e extrai pacotes reais (rede): `cargo test emu_update::e2e -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn real_packages_extract_and_contain_exe() {
        for (src, hint, exe) in [("PCSX2/pcsx2", None, "pcsx2-qt.exe"), ("stenzek/duckstation", None, "duckstation-qt-x64-ReleaseLTCG.exe")] {
            let s = Source { hint, ..parse_source(src).unwrap() };
            let l = fetch_latest(&s).await.unwrap();
            let a = l.asset.clone().unwrap();
            let dir = std::env::temp_dir().join(format!("nx-e2e-{}", a.key.replace(':', "_")));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let pkg = dir.join(&a.name);
            let bytes = reqwest::get(&a.url).await.unwrap().bytes().await.unwrap();
            fs::write(&pkg, &bytes).unwrap();
            extract(&pkg, &dir.join("files")).unwrap();
            let root = find_root(&dir.join("files"), exe);
            println!("{src}: {} {:?} -> {:?}", l.version, l.date, root);
            assert!(root.is_ok());
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod e2e_presets {
    use super::*;

    /// Todas as fontes automáticas respondem e têm pacote do Windows (rede).
    #[tokio::test]
    #[ignore]
    async fn every_preset_has_a_windows_package() {
        for (name, exe) in [("PCSX2", "pcsx2-qt.exe"), ("DuckStation", "duckstation.exe"), ("RPCS3", "rpcs3.exe"), ("PPSSPP", "PPSSPPWindows64.exe"), ("Vita3K", "Vita3K.exe"), ("xemu", "xemu.exe"), ("Xenia", "xenia_canary.exe"), ("Eden", "eden.exe")] {
            let src = effective_source(None, name, exe).unwrap().unwrap();
            let l = fetch_latest(&src).await.unwrap();
            println!("{name}: {} {:?} -> {:?}", l.version, l.date, l.asset.as_ref().map(|a| &a.name));
            assert!(l.asset.is_some(), "{name}");
        }
    }
}
