//! DLCs dos jogos Steam: aba "DLCs" na página do jogo e aviso de DLC nova na Home.
//!
//! Fonte: `store.steampowered.com/api/dlcforapp` — endpoint público da loja da Steam, sem login nem
//! chave (o mesmo serviço do `appdetails` já usado em `commands::info`). Uma requisição traz a lista
//! completa de DLCs de um jogo (nome, imagem, preço na região da conexão e data de lançamento).
//!
//! O que NÃO é possível (e não é feito): saber quais DLCs o usuário comprou. A Steam só libera isso para
//! quem publica o jogo. "Instalada" vem do appmanifest local (`local::installed_dlc`) e só cobre DLCs com
//! conteúdo baixado.
//!
//! DLC nova: a primeira consulta de um jogo grava a lista como referência (nada é "novo"); DLCs que
//! aparecem depois ficam marcadas como novas por 30 dias. Lançamentos dos últimos 14 dias (data da Steam)
//! também contam como novos, mesmo na primeira consulta.
//!
//! Custo: cache global por jogo (não depende do perfil) com validade de 24 h. A verificação em segundo
//! plano é espaçada (fila própria, 1 requisição a cada 1,5 s), limitada por rodada e para no primeiro
//! sinal de falta de rede ou limite de requisições.

use crate::db::Db;
use crate::error::AppResult;
use crate::net::Gate;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS steam_dlc_cache (
    appid       INTEGER PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,           -- epoch da última consulta bem-sucedida
    data        TEXT NOT NULL               -- lista JSON de `Dlc`
);
-- Quando cada DLC apareceu pela primeira vez (0 = já existia na primeira consulta do jogo).
CREATE TABLE IF NOT EXISTS steam_dlc_seen (
    appid       INTEGER NOT NULL,
    dlc_appid   INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL,
    PRIMARY KEY (appid, dlc_appid)
);
"#;

const TTL: i64 = 24 * 3600;
const MANUAL_TTL: i64 = 5 * 60;
const NEW_FOR: i64 = 30 * 86400;
const RECENT_RELEASE: i64 = 14 * 86400;
const MAX_PER_RUN: usize = 120;
static GATE: Gate = Gate::new(1500);
static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub currency: String,
    /// Valores em centavos (como a Steam informa).
    pub initial: i64,
    #[serde(rename = "final")]
    pub final_: i64,
    pub discount: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dlc {
    pub id: i64,
    pub name: String,
    pub image: Option<String>,
    pub price: Option<Price>,
    /// epoch do lançamento na Steam (None = não informado).
    pub release: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum FetchError {
    Offline,
    RateLimited(u64),
    Unavailable(u16),
    BadResponse,
}

impl FetchError {
    pub fn message(&self) -> String {
        match self {
            FetchError::Offline => "Steam: sem conexão. Exibindo as DLCs já consultadas.".into(),
            FetchError::RateLimited(s) => format!("Steam: limite de requisições atingido. Nova tentativa em {s} s."),
            FetchError::Unavailable(c) => format!("Steam: serviço indisponível no momento (HTTP {c})."),
            FetchError::BadResponse => "Steam: resposta inesperada. Tente novamente mais tarde.".into(),
        }
    }
}

fn http() -> Option<&'static reqwest::Client> {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
            .build()
            .ok()
    })
    .as_ref()
}

/// Interpreta a resposta do `dlcforapp`. `None` = formato inesperado.
pub fn parse(v: &Value) -> Option<Vec<Dlc>> {
    // status 1 = ok; outro valor (app inexistente/sem página na loja) = sem DLCs
    if v["status"].as_i64() != Some(1) {
        return v.is_object().then(Vec::new);
    }
    let list = match &v["dlc"] {
        Value::Array(a) => a,
        Value::Null => return Some(Vec::new()),
        _ => return None,
    };
    let mut out = Vec::new();
    for d in list {
        let Some(id) = d["id"].as_i64().filter(|i| *i > 0) else { continue };
        let name = d["name"].as_str().unwrap_or("").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let p = &d["price_overview"];
        let price = p["currency"].as_str().filter(|c| c.len() == 3).map(|c| Price {
            currency: c.to_string(),
            initial: p["initial"].as_i64().unwrap_or(0),
            final_: p["final"].as_i64().unwrap_or(0),
            discount: p["discount_percent"].as_i64().unwrap_or(0),
        });
        let release = match &d["release_date"]["steam"] {
            Value::String(s) => s.trim().parse::<i64>().ok(),
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
        .filter(|t| *t > 0);
        let image = d["header_image"].as_str().filter(|u| u.starts_with("https://")).map(str::to_string);
        out.push(Dlc { id, name: name.chars().take(200).collect(), image, price, release });
    }
    Some(out)
}

/// Consulta a Steam (uma requisição, pela fila própria).
pub async fn fetch(appid: i64, lang: &str) -> Result<Vec<Dlc>, FetchError> {
    if let Some(s) = GATE.blocked_for() {
        return Err(FetchError::RateLimited(s));
    }
    GATE.wait().await;
    let client = http().ok_or(FetchError::Offline)?;
    let resp = client
        .get("https://store.steampowered.com/api/dlcforapp/")
        .query(&[("appid", appid.to_string().as_str()), ("l", lang)])
        .send()
        .await
        .map_err(|_| FetchError::Offline)?;
    let status = resp.status().as_u16();
    if status == 429 || status == 403 {
        return Err(FetchError::RateLimited(GATE.block(crate::net::retry_after(&resp))));
    }
    if status >= 500 {
        return Err(FetchError::Unavailable(status));
    }
    let v: Value = resp.json().await.map_err(|e| if e.is_decode() { FetchError::BadResponse } else { FetchError::Offline })?;
    parse(&v).ok_or(FetchError::BadResponse)
}

/// Grava a lista e registra quando cada DLC apareceu (a primeira consulta vira referência).
pub fn save(conn: &Connection, appid: i64, list: &[Dlc], at: i64) -> AppResult<()> {
    let first_time: bool = !conn
        .query_row("SELECT EXISTS(SELECT 1 FROM steam_dlc_cache WHERE appid=?1)", [appid], |r| r.get::<_, bool>(0))?;
    conn.execute(
        "INSERT INTO steam_dlc_cache(appid, fetched_at, data) VALUES(?1,?2,?3)
         ON CONFLICT(appid) DO UPDATE SET fetched_at=excluded.fetched_at, data=excluded.data",
        params![appid, at, serde_json::to_string(list).unwrap_or_else(|_| "[]".into())],
    )?;
    let seen = if first_time { 0 } else { at };
    let mut st = conn.prepare("INSERT OR IGNORE INTO steam_dlc_seen(appid, dlc_appid, first_seen) VALUES(?1,?2,?3)")?;
    for d in list {
        st.execute(params![appid, d.id, seen])?;
    }
    Ok(())
}

fn cached(conn: &Connection, appid: i64) -> AppResult<Option<(i64, Vec<Dlc>)>> {
    let row: Option<(i64, String)> = conn
        .query_row("SELECT fetched_at, data FROM steam_dlc_cache WHERE appid=?1", [appid], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row.map(|(at, data)| (at, serde_json::from_str(&data).unwrap_or_default())))
}

fn first_seen_map(conn: &Connection, appid: i64) -> AppResult<HashMap<i64, i64>> {
    let mut st = conn.prepare("SELECT dlc_appid, first_seen FROM steam_dlc_seen WHERE appid=?1")?;
    let rows = st.query_map([appid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Nova: apareceu depois da primeira consulta (há menos de 30 dias) ou foi lançada nos últimos 14 dias.
pub fn is_new(d: &Dlc, first_seen: i64, at: i64) -> bool {
    (first_seen > 0 && at - first_seen < NEW_FOR) || d.release.is_some_and(|r| r <= at && at - r < RECENT_RELEASE)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DlcItem {
    #[serde(flatten)]
    pub dlc: Dlc,
    pub installed: bool,
    pub new: bool,
    /// Lançamento ainda no futuro ("Em breve").
    pub upcoming: bool,
    /// Quando o NEXSTAGE viu a DLC pela primeira vez (0 = desde a primeira consulta).
    pub first_seen: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcView {
    pub appid: i64,
    pub fetched_at: Option<i64>,
    pub items: Vec<DlcItem>,
    /// Falha na última consulta (os itens mostrados são do cache).
    pub error: Option<String>,
}

fn items(list: Vec<Dlc>, seen: &HashMap<i64, i64>, installed: &HashSet<i64>, at: i64) -> Vec<DlcItem> {
    let mut out: Vec<DlcItem> = list
        .into_iter()
        .map(|d| {
            let fs = seen.get(&d.id).copied().unwrap_or(0);
            DlcItem { installed: installed.contains(&d.id), new: is_new(&d, fs, at), upcoming: d.release.is_some_and(|r| r > at), first_seen: fs, dlc: d }
        })
        .collect();
    // em breve primeiro, depois do lançamento mais recente para o mais antigo (sem data no fim)
    out.sort_by(|a, b| b.upcoming.cmp(&a.upcoming).then(b.dlc.release.unwrap_or(0).cmp(&a.dlc.release.unwrap_or(0))).then(a.dlc.name.cmp(&b.dlc.name)));
    out
}

pub fn view(conn: &Connection, appid: i64, installed: &HashSet<i64>, error: Option<String>) -> AppResult<DlcView> {
    let (fetched_at, list) = match cached(conn, appid)? {
        Some((at, l)) => (Some(at), l),
        None => (None, Vec::new()),
    };
    let seen = first_seen_map(conn, appid)?;
    Ok(DlcView { appid, fetched_at, items: items(list, &seen, installed, now()), error })
}

/// Página do jogo: usa o cache e consulta a Steam se ele estiver vencido (ou `refresh`).
pub async fn game_dlc(db: &Db, appid: i64, refresh: bool, lang: &str) -> AppResult<DlcView> {
    let age = { cached(&*db.conn()?, appid)?.map(|(at, _)| now() - at) };
    let stale = match age {
        None => true,
        Some(a) => a > TTL || (refresh && a > MANUAL_TTL),
    };
    let mut error = None;
    if stale {
        match fetch(appid, lang).await {
            Ok(list) => save(&*db.conn()?, appid, &list, now())?,
            Err(e) => error = Some(e.message()),
        }
    }
    let installed = super::local::installed_dlc(appid);
    view(&*db.conn()?, appid, &installed, error)
}

// ---------- verificação em segundo plano (DLC nova) ----------

static RUNNING: AtomicBool = AtomicBool::new(false);

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Consulta os jogos Steam do perfil com cache vencido (não bloqueia). `false` = já havia uma em andamento.
pub fn start(app: &AppHandle, profile_id: i64, lang: &'static str) -> bool {
    if RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _g = Guard;
        let stale: Vec<i64> = {
            let db = app.state::<Db>();
            let Ok(conn) = db.conn() else { return };
            let limit = now() - TTL;
            super::news::candidates(&conn, profile_id)
                .unwrap_or_default()
                .into_iter()
                .filter(|a| {
                    let at: Option<i64> = conn.query_row("SELECT fetched_at FROM steam_dlc_cache WHERE appid=?1", [a], |r| r.get(0)).optional().ok().flatten();
                    at.map_or(true, |t| t < limit)
                })
                .take(MAX_PER_RUN)
                .collect()
        };
        let mut changed = false;
        for appid in stale {
            match fetch(appid, lang).await {
                Ok(list) => {
                    let db = app.state::<Db>();
                    if let Ok(conn) = db.conn() {
                        changed |= save(&conn, appid, &list, now()).is_ok();
                    };
                }
                // sem rede ou limite da Steam: para e tenta de novo mais tarde
                Err(FetchError::Offline | FetchError::RateLimited(_)) => break,
                Err(_) => {}
            }
        }
        if changed {
            let _ = app.emit("steam-dlc-updated", json!({ "profileId": profile_id }));
        }
    });
    true
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDlc {
    pub game_id: i64,
    pub game_title: String,
    pub cover: Option<String>,
    pub appid: i64,
    #[serde(flatten)]
    pub item: DlcItem,
}

/// DLCs novas dos jogos Steam da biblioteca do perfil (só cache; funciona offline).
pub fn recent_new(conn: &Connection, profile_id: i64, limit: usize) -> AppResult<Vec<NewDlc>> {
    let at = now();
    let mut out = Vec::new();
    let mut game = conn.prepare(
        "SELECT g.id, g.title, g.cover FROM steam_links sl JOIN games g ON g.id = sl.game_id
           JOIN profile_games pg ON pg.game_id = g.id AND pg.profile_id = ?1 WHERE sl.appid = ?2 LIMIT 1",
    )?;
    for appid in super::news::candidates(conn, profile_id)? {
        let Some((_, list)) = cached(conn, appid)? else { continue };
        let seen = first_seen_map(conn, appid)?;
        let fresh: Vec<DlcItem> = items(list, &seen, &HashSet::new(), at).into_iter().filter(|i| i.new).collect();
        if fresh.is_empty() {
            continue;
        }
        let Some((game_id, game_title, cover)) = game
            .query_row(params![profile_id, appid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))
            .optional()?
        else {
            continue;
        };
        let cover = crate::images::resolve(cover);
        for item in fresh {
            out.push(NewDlc { game_id, game_title: game_title.clone(), cover: cover.clone(), appid, item });
        }
    }
    // mais recente primeiro: data de lançamento ou, sem ela, quando apareceu
    let key = |n: &NewDlc| n.item.dlc.release.filter(|r| *r <= at).unwrap_or(0).max(n.item.first_seen);
    out.sort_by(|a, b| key(b).cmp(&key(a)));
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    fn dlc(id: i64, release: Option<i64>) -> Dlc {
        Dlc { id, name: format!("DLC {id}"), image: None, price: None, release }
    }

    #[test]
    fn parses_store_response() {
        let v = json!({ "status": 1, "appid": "1245620", "dlc": [
            { "id": 2778580, "name": "Shadow of the Erdtree", "header_image": "https://x/h.jpg",
              "price_overview": { "currency": "BRL", "initial": 18290, "final": 9145, "discount_percent": 50 },
              "release_date": { "steam": "1718920817" } },
            { "id": 1536500, "name": "Heart of Russia", "platforms": { "windows": true } },
            { "id": 0, "name": "inválida" },
            { "id": 5, "name": "  " }
        ]});
        let l = parse(&v).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].price.as_ref().map(|p| (p.final_, p.discount, p.currency.as_str())), Some((9145, 50, "BRL")));
        assert_eq!(l[0].release, Some(1718920817));
        assert_eq!((l[1].price.clone(), l[1].release), (None, None), "sem preço e sem data: campos vazios");
        assert_eq!(parse(&json!({ "status": 1, "dlc": null })).unwrap().len(), 0);
        assert_eq!(parse(&json!({ "status": 0 })).unwrap().len(), 0, "app sem página: sem DLCs");
        assert!(parse(&json!({ "status": 1, "dlc": "x" })).is_none(), "formato inesperado");
        assert!(parse(&json!([])).is_none());
    }

    #[test]
    fn first_fetch_is_baseline_and_later_ones_are_new() {
        let c = db();
        let t0 = 1_800_000_000;
        save(&c, 10, &[dlc(1, Some(t0 - 400 * 86400)), dlc(2, Some(t0 - 3 * 86400))], t0).unwrap();
        let v = view(&c, 10, &HashSet::from([1]), None).unwrap();
        let by = |id: i64| v.items.iter().find(|i| i.dlc.id == id).unwrap().clone();
        // relógio real (now) está antes de t0 nos testes: avalia com `items` no tempo controlado
        let seen = first_seen_map(&c, 10).unwrap();
        let at = items(cached(&c, 10).unwrap().unwrap().1, &seen, &HashSet::new(), t0);
        assert!(!at.iter().find(|i| i.dlc.id == 1).unwrap().new, "antiga na primeira consulta: referência");
        assert!(at.iter().find(|i| i.dlc.id == 2).unwrap().new, "lançada há 3 dias: nova mesmo na primeira consulta");
        assert!(by(1).installed && !by(2).installed);

        // segunda consulta traz uma DLC a mais (sem data): nova por 30 dias
        let t1 = t0 + 86400;
        save(&c, 10, &[dlc(1, None), dlc(2, None), dlc(3, None)], t1).unwrap();
        let seen = first_seen_map(&c, 10).unwrap();
        assert_eq!((seen[&1], seen[&3]), (0, t1));
        let l = items(cached(&c, 10).unwrap().unwrap().1, &seen, &HashSet::new(), t1 + 10 * 86400);
        assert!(l.iter().find(|i| i.dlc.id == 3).unwrap().new);
        let l = items(cached(&c, 10).unwrap().unwrap().1, &seen, &HashSet::new(), t1 + 31 * 86400);
        assert!(!l.iter().find(|i| i.dlc.id == 3).unwrap().new, "deixa de ser nova depois de 30 dias");
    }

    #[test]
    fn upcoming_first_then_newest() {
        let at = 1_800_000_000;
        let l = items(vec![dlc(1, Some(at - 100)), dlc(2, None), dlc(3, Some(at + 86400)), dlc(4, Some(at - 10))], &HashMap::new(), &HashSet::new(), at);
        assert_eq!(l.iter().map(|i| i.dlc.id).collect::<Vec<_>>(), vec![3, 4, 1, 2]);
        assert!(l[0].upcoming && !l[0].new, "em breve não é nova");
    }

    /// Consulta real (rede): cargo test steam_dlc_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn steam_dlc_live() {
        let l = tauri::async_runtime::block_on(fetch(1245620, "brazilian")).unwrap();
        for d in &l {
            println!("{} | {} | {:?} | {:?}", d.id, d.name, d.price, d.release);
        }
        assert!(l.len() >= 2);
        println!("instaladas (1091500): {:?}", crate::steam::local::installed_dlc(1091500));
    }
}
