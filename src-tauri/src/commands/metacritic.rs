//! Nota do Metacritic (Metascore) na página de cada jogo.
//!
//! O Metacritic não oferece API pública e os termos do site proíbem coleta automatizada, então o
//! NEXSTAGE NÃO lê páginas do metacritic.com. Fontes oficiais que repassam o Metascore:
//! - RAWG (principal; chave pessoal gratuita, a mesma da pesquisa de imagens): nota geral e nota POR
//!   PLATAFORMA (`metacritic_platforms`) — o Metacritic avalia cada versão separadamente;
//! - loja da Steam (reserva para jogos de PC quando não há chave do RAWG): `metacritic.score`.
//!
//! Identificação confiável no RAWG:
//! 1. jogo já confirmado (`rawg_id` salvo) → consulta direta pelo ID, sem depender do nome;
//! 2. senão, candidatos da busca filtrada pela plataforma do jogo + tentativa pelo slug exato,
//!    pontuados por nome (`info::name_score`), plataforma e ano; só escolhe sozinho com folga clara;
//! 3. ambíguo → devolve os candidatos para o usuário escolher (nada é gravado até lá).
//! As notas de todas as versões ficam em cache: trocar a plataforma do jogo recalcula na hora (offline).

use super::games::ensure_owned;
use super::info::{self, GameInfo, McPlatform};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::retro::util::{fmt_epoch, now_epoch};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::{AppHandle, State};

const RAWG: &str = "https://api.rawg.io/api";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: i64,
    pub name: String,
    pub year: Option<i64>,
    pub platforms: Vec<String>,
    pub metacritic: Option<i64>,
    pub image: Option<String>,
    /// O candidato existe na plataforma do jogo.
    pub on_platform: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetacriticResult {
    /// "found" | "no_score" (identificado, sem Metascore nesta plataforma) | "choose" (ambíguo: escolha
    /// entre `candidates`) | "not_found" | "no_source" (sem chave do RAWG para console)
    pub status: String,
    pub info: GameInfo,
    pub matched_name: Option<String>,
    pub candidates: Vec<Candidate>,
}

/// Plataforma do NEXSTAGE → slug da plataforma no RAWG.
pub fn rawg_slug(platform: &str) -> Option<&'static str> {
    Some(match crate::commands::games::canonical_platform(platform)? {
        "PC" => "pc",
        "XBOX" => "xbox-old",
        "Xbox 360" => "xbox360",
        "Xbox Series" => "xbox-series-x",
        "Playstation 1" => "playstation1",
        "Playstation 2" => "playstation2",
        "Playstation 3" => "playstation3",
        "Playstation 4" => "playstation4",
        "Playstation 5" => "playstation5",
        "PSP" => "psp",
        "Ps Vita" => "ps-vita",
        "Nintendo Switch" => "nintendo-switch",
        "Wii U" => "wii-u",
        "Wii" => "wii",
        "GameCube" => "gamecube",
        "Nintendo 3DS" => "nintendo-3ds",
        "Nintendo DS" => "nintendo-ds",
        "Game Boy Advance" => "game-boy-advance",
        "Game Boy Color" => "game-boy-color",
        "Game Boy" => "game-boy",
        "Nintendo 64" => "nintendo-64",
        "Mega Drive" => "genesis",
        "Dreamcast" => "dreamcast",
        "Super Nintendo" => "snes",
        "Nintendinho" => "nes",
        _ => return None,
    })
}

/// ID numérico da plataforma no RAWG (filtro da busca).
fn rawg_platform_id(slug: &str) -> Option<&'static str> {
    Some(match slug {
        "pc" => "4",
        "playstation5" => "187",
        "playstation4" => "18",
        "playstation3" => "16",
        "playstation2" => "15",
        "playstation1" => "27",
        "psp" => "17",
        "ps-vita" => "19",
        "xbox-series-x" => "186",
        "xbox360" => "14",
        "xbox-old" => "80",
        "nintendo-switch" => "7",
        "wii-u" => "10",
        "wii" => "11",
        "gamecube" => "105",
        "nintendo-3ds" => "8",
        "nintendo-ds" => "9",
        "game-boy-advance" => "24",
        "game-boy-color" => "43",
        "game-boy" => "26",
        "nintendo-64" => "83",
        "genesis" => "167",
        "dreamcast" => "106",
        "snes" => "79",
        "nes" => "49",
        _ => return None,
    })
}

/// O slug do RAWG vale para a plataforma ("playstation1" às vezes aparece como "playstation").
fn same_platform(rawg: &str, ours: &str) -> bool {
    rawg == ours || (ours == "playstation1" && rawg == "playstation")
}

/// "Shadow of the Colossus (USA)" → "shadow-of-the-colossus" (formato dos slugs do RAWG).
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            '\'' | '’' => {}
            _ => {
                if !out.ends_with('-') && !out.is_empty() {
                    out.push('-');
                }
            }
        }
    }
    out.trim_end_matches('-').to_string()
}

use crate::sources::matching::{year_of, Matchable};
pub use crate::sources::matching::pick_confident;

impl Matchable for Candidate {
    fn id(&self) -> i64 { self.id }
    fn name(&self) -> &str { &self.name }
    fn year(&self) -> Option<i64> { self.year }
    fn on_platform(&self) -> bool { self.on_platform }
}

fn candidate(g: &Value, slug: Option<&str>) -> Option<Candidate> {
    let plats: Vec<&Value> = g["platforms"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    Some(Candidate {
        id: g["id"].as_i64()?,
        name: g["name"].as_str()?.to_string(),
        year: g["released"].as_str().and_then(year_of),
        platforms: plats.iter().filter_map(|p| p["platform"]["name"].as_str().map(String::from)).collect(),
        metacritic: g["metacritic"].as_i64().filter(|s| (1..=100).contains(s)),
        image: g["background_image"].as_str().filter(|u| u.starts_with("https://")).map(String::from),
        on_platform: slug.map(|s| plats.iter().any(|p| same_platform(p["platform"]["slug"].as_str().unwrap_or(""), s))).unwrap_or(true),
    })
}

/// Lê as notas de um jogo do RAWG: geral + por plataforma.
pub fn parse_scores(g: &Value) -> (Option<i64>, Option<String>, Vec<McPlatform>) {
    let overall = g["metacritic"].as_i64().filter(|s| (1..=100).contains(s));
    let url = g["metacritic_url"].as_str().filter(|u| u.starts_with("https://")).map(String::from);
    let plats = g["metacritic_platforms"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(McPlatform {
                slug: p["platform"]["slug"].as_str()?.to_string(),
                name: p["platform"]["name"].as_str().unwrap_or("").to_string(),
                score: p["metascore"].as_i64().filter(|s| (1..=100).contains(s))?,
                url: p["url"].as_str().filter(|u| u.starts_with("https://")).map(String::from),
            })
        })
        .collect();
    (overall, url, plats)
}

/// Aplica ao jogo a nota da plataforma escolhida, a partir do cache (sem rede).
/// Sem nota daquela versão: usa a geral do RAWG, identificada como tal (plataforma `None`).
/// Nota vinda da Steam vale só para PC: em outra plataforma é retirada (precisa buscar no RAWG).
/// Nota manual não é alterada.
pub fn apply_platform(i: &mut GameInfo, platform: &str) {
    let slug = rawg_slug(platform);
    match i.metacritic_source.as_deref() {
        Some("manual") => {}
        Some("rawg") => {
            let per = slug.and_then(|s| i.metacritic_platforms.iter().find(|p| same_platform(&p.slug, s)).cloned());
            match per {
                Some(p) => {
                    i.metacritic_score = Some(p.score);
                    i.metacritic_url = p.url.or_else(|| i.metacritic_overall_url.clone());
                    i.metacritic_platform = Some(p.name);
                }
                None => {
                    i.metacritic_score = i.metacritic_overall;
                    i.metacritic_url = i.metacritic_overall_url.clone();
                    i.metacritic_platform = None;
                }
            }
        }
        Some("steam") if slug != Some("pc") => {
            i.metacritic_score = None;
            i.metacritic_url = None;
            i.metacritic_platform = None;
            i.metacritic_source = None;
            i.metacritic_checked_at = None; // a página do jogo busca de novo, agora no RAWG
        }
        _ => {}
    }
}

/// Chamado quando o usuário troca a plataforma em "Editar jogo".
pub fn on_platform_changed(conn: &Connection, game_id: i64, platform: &str) -> AppResult<()> {
    if let Some(mut i) = info::load(conn, game_id)? {
        apply_platform(&mut i, platform);
        info::store(conn, game_id, &i)?;
    }
    Ok(())
}

fn rawg_key(app: &AppHandle) -> Option<String> {
    crate::apikeys::get(app)("rawg_key")
}

/// Erro de rede sem a URL (a do RAWG contém a chave).
fn net(_: reqwest::Error) -> AppError {
    AppError::Other("Sem conexão para buscar a nota do Metacritic. A nota salva continua disponível.".into())
}

struct Rawg {
    c: reqwest::Client,
    key: String,
}

impl Rawg {
    fn new(key: String) -> AppResult<Self> {
        Ok(Self { c: reqwest::Client::builder().timeout(Duration::from_secs(15)).user_agent("NEXSTAGE").build().map_err(net)?, key })
    }
    /// GET com tratamento de chave recusada / limite / inexistente (`Ok(None)` = 404).
    async fn get(&self, path: &str, q: &[(&str, &str)]) -> AppResult<Option<Value>> {
        let mut query: Vec<(&str, &str)> = vec![("key", self.key.as_str())];
        query.extend_from_slice(q);
        let r = self.c.get(format!("{RAWG}{path}")).query(&query).send().await.map_err(net)?;
        match r.status().as_u16() {
            401 | 403 => Err(AppError::Validation("A chave do RAWG foi recusada. Confira em Configurações → Pesquisa de imagens.".into())),
            404 => Ok(None),
            429 => Err(AppError::Other("RAWG: limite de requisições atingido. Tente novamente mais tarde.".into())),
            s if s >= 500 => Err(AppError::Other("RAWG indisponível no momento. A nota salva continua disponível.".into())),
            _ => Ok(Some(r.json().await.map_err(net)?)),
        }
    }
    async fn search(&self, term: &str, slug: Option<&str>, exact: bool) -> AppResult<Vec<Value>> {
        let mut q: Vec<(&str, &str)> = vec![("search", term), ("page_size", "15"), ("search_precise", "true"), ("exclude_additions", "true")];
        if exact {
            q.push(("search_exact", "true"));
        }
        if let Some(p) = slug.and_then(rawg_platform_id) {
            q.push(("platforms", p));
        }
        Ok(self.get("/games", &q).await?.and_then(|v| v["results"].as_array().cloned()).unwrap_or_default())
    }
    /// Candidatos: busca exata e precisa (filtradas pela plataforma), busca precisa SEM o filtro de plataforma
    /// (o RAWG nem sempre lista o jogo na plataforma certa — filtrado, devolve parecidos sem relação) e o slug
    /// exato do título. Quem chama descarta os que não se parecem com o nome pesquisado.
    async fn candidates(&self, term: &str, slug: Option<&str>) -> AppResult<Vec<Candidate>> {
        let mut raw = self.search(term, slug, true).await?;
        if raw.len() < 3 {
            raw.extend(self.search(term, slug, false).await?);
        }
        if slug.is_some() {
            raw.extend(self.search(term, None, false).await?);
        }
        if let Some(g) = self.get(&format!("/games/{}", slugify(term)), &[]).await? {
            raw.push(g);
        }
        let mut out: Vec<Candidate> = Vec::new();
        for g in &raw {
            if let Some(c) = candidate(g, slug) {
                if !out.iter().any(|o| o.id == c.id) {
                    out.push(c);
                }
            }
        }
        Ok(out)
    }
}

/// Semelhança mínima de nome para um candidato aparecer (abaixo disso a pontuação já é zero).
const MIN_NAME: f64 = 0.5;

/// Busca/atualiza a nota. `rawg_id` = escolha do usuário entre os candidatos; `reidentify` ignora o
/// jogo já confirmado (para corrigir uma identificação).
#[tauri::command]
pub async fn fetch_metacritic(
    app: AppHandle,
    db: State<'_, Db>,
    profile_id: i64,
    game_id: i64,
    term: String,
    rawg_id: Option<i64>,
    reidentify: Option<bool>,
) -> AppResult<MetacriticResult> {
    let (platform, mut stored) = {
        let conn = db.conn()?;
        ensure_owned(&conn, profile_id, game_id)?;
        let platform: String = conn.query_row("SELECT platform FROM games WHERE id=?1", [game_id], |r| r.get(0))?;
        (platform, info::load(&conn, game_id)?.unwrap_or_default())
    };
    let term = term.trim().to_string();
    if term.is_empty() {
        return Err(AppError::Validation("Informe o nome do jogo.".into()));
    }
    let slug = rawg_slug(&platform);
    let now = fmt_epoch(now_epoch());
    let save = |db: &Db, i: &mut GameInfo| -> AppResult<()> {
        if i.source.is_empty() {
            i.source = "manual".into();
        }
        info::store(&*db.conn()?, game_id, i)
    };

    if let Some(key) = rawg_key(&app) {
        let rawg = Rawg::new(key)?;
        let known = if reidentify == Some(true) { None } else { stored.rawg_id };
        let id = match rawg_id.or(known) {
            Some(id) => id,
            None => {
                let mut cands = rawg.candidates(&term, slug).await?;
                // só nomes parecidos com o pesquisado (mesmo corte da pontuação): nada de lista sem relação
                cands.retain(|c| crate::commands::info::name_score(&term, &c.name) >= MIN_NAME);
                let year = stored.release_date.as_deref().and_then(crate::commands::games::parse_release).as_deref().and_then(year_of);
                match pick_confident(&term, &cands, year) {
                    Some(id) => id,
                    None if cands.is_empty() => {
                        return Ok(MetacriticResult { status: "not_found".into(), info: stored, matched_name: None, candidates: vec![] });
                    }
                    None => {
                        // ambíguo: o usuário escolhe (os da plataforma do jogo primeiro)
                        let mut c = cands;
                        crate::sources::matching::rank_for_display(&term, &mut c);
                        c.truncate(8);
                        return Ok(MetacriticResult { status: "choose".into(), info: stored, matched_name: None, candidates: c });
                    }
                }
            }
        };
        let Some(g) = rawg.get(&format!("/games/{id}"), &[]).await? else {
            // ID salvo não existe mais no RAWG: identifica de novo na próxima busca
            stored.rawg_id = None;
            save(&db, &mut stored)?;
            return Ok(MetacriticResult { status: "not_found".into(), info: stored, matched_name: None, candidates: vec![] });
        };
        let (overall, url, plats) = parse_scores(&g);
        stored.rawg_id = Some(id);
        stored.metacritic_overall = overall;
        stored.metacritic_overall_url = url;
        stored.metacritic_platforms = plats;
        stored.metacritic_source = Some("rawg".into());
        stored.metacritic_checked_at = Some(now);
        apply_platform(&mut stored, &platform);
        save(&db, &mut stored)?;
        let status = if stored.metacritic_score.is_some() { "found" } else { "no_score" };
        return Ok(MetacriticResult { status: status.into(), matched_name: g["name"].as_str().map(String::from), info: stored, candidates: vec![] });
    }

    // Sem chave do RAWG: só PC, pela loja da Steam
    if slug != Some("pc") {
        return Ok(MetacriticResult { status: "no_source".into(), info: stored, matched_name: None, candidates: vec![] });
    }
    match info::steam_lookup(&term, true).await {
        Ok(i) => {
            if stored.metacritic_source.as_deref() != Some("manual") || i.metacritic_score.is_some() {
                stored.metacritic_score = i.metacritic_score;
                stored.metacritic_url = i.metacritic_url;
                stored.metacritic_platform = i.metacritic_score.map(|_| "PC".into());
                stored.metacritic_source = i.metacritic_score.map(|_| "steam".into());
            }
            stored.metacritic_checked_at = Some(now);
            save(&db, &mut stored)?;
            let status = if stored.metacritic_score.is_some() { "found" } else { "no_score" };
            Ok(MetacriticResult { status: status.into(), info: stored, matched_name: Some(i.matched_name), candidates: vec![] })
        }
        Err(AppError::NotFound(_)) => Ok(MetacriticResult { status: "not_found".into(), info: stored, matched_name: None, candidates: vec![] }),
        Err(_) => Err(AppError::Other("Sem conexão para buscar a nota do Metacritic. A nota salva continua disponível.".into())),
    }
}

/// Nota informada à mão (0 a 100) ou remoção (`None`).
#[tauri::command]
pub fn save_manual_metacritic(db: State<Db>, profile_id: i64, game_id: i64, score: Option<i64>) -> AppResult<GameInfo> {
    let conn = db.conn()?;
    ensure_owned(&conn, profile_id, game_id)?;
    if let Some(s) = score {
        if !(0..=100).contains(&s) {
            return Err(AppError::Validation("A nota do Metacritic vai de 0 a 100.".into()));
        }
    }
    let mut i = info::load(&conn, game_id)?.unwrap_or_default();
    if i.source.is_empty() {
        i.source = "manual".into();
    }
    i.metacritic_score = score;
    i.metacritic_source = score.map(|_| "manual".into());
    i.metacritic_platform = None;
    if score.is_none() {
        i.metacritic_url = None;
    }
    i.metacritic_checked_at = Some(fmt_epoch(now_epoch()));
    info::store(&conn, game_id, &i)?;
    Ok(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cand(id: i64, name: &str, year: i64, on: bool) -> Candidate {
        Candidate { id, name: name.into(), year: Some(year), platforms: vec![], metacritic: None, image: None, on_platform: on }
    }

    #[test]
    fn picks_only_when_confident() {
        // nome idêntico, na plataforma, sem empate → escolhe
        let c = vec![cand(1, "Shadow of the Colossus", 2005, true), cand(2, "Shadow of the Colossus (2018)", 2018, false)];
        assert_eq!(pick_confident("Shadow of the Colossus", &c, None), Some(1));
        // remake e original com o mesmo nome na mesma plataforma → sem o ano, pergunta
        let c = vec![cand(1, "Resident Evil 4", 2005, true), cand(2, "Resident Evil 4", 2023, true)];
        assert_eq!(pick_confident("Resident Evil 4", &c, None), None);
        // com o ano conhecido, o empate se resolve
        assert_eq!(pick_confident("Resident Evil 4", &c, Some(2023)), Some(2));
        // o único com o nome não existe na plataforma do jogo → não escolhe sozinho
        let c = vec![cand(1, "Halo 3", 2007, false)];
        assert_eq!(pick_confident("Halo 3", &c, None), None);
        // sequência diferente nunca é confundida
        let c = vec![cand(1, "God of War II", 2007, true)];
        assert_eq!(pick_confident("God of War", &c, None), None);
    }

    #[test]
    fn slugs_like_rawg() {
        assert_eq!(slugify("Shadow of the Colossus (USA)"), "shadow-of-the-colossus");
        assert_eq!(slugify("Tony Hawk's Pro Skater 2"), "tony-hawks-pro-skater-2");
        assert_eq!(slugify("Metal Gear Solid: Snake Eater"), "metal-gear-solid-snake-eater");
    }

    fn rawg_info() -> GameInfo {
        let g = json!({
            "metacritic": 90, "metacritic_url": "https://www.metacritic.com/game/x",
            "metacritic_platforms": [
                {"metascore": 93, "url": "https://www.metacritic.com/game/playstation-2/x", "platform": {"slug": "playstation2", "name": "PlayStation 2"}},
                {"metascore": 85, "url": "https://www.metacritic.com/game/pc/x", "platform": {"slug": "pc", "name": "PC"}}
            ]
        });
        let (overall, url, plats) = parse_scores(&g);
        GameInfo { metacritic_overall: overall, metacritic_overall_url: url, metacritic_platforms: plats, metacritic_source: Some("rawg".into()), rawg_id: Some(7), ..Default::default() }
    }

    #[test]
    fn score_follows_the_selected_platform() {
        let mut i = rawg_info();
        apply_platform(&mut i, "Playstation 2");
        assert_eq!((i.metacritic_score, i.metacritic_platform.as_deref()), (Some(93), Some("PlayStation 2")));
        assert!(i.metacritic_url.as_deref().unwrap().contains("playstation-2"));
        apply_platform(&mut i, "PC");
        assert_eq!((i.metacritic_score, i.metacritic_platform.as_deref()), (Some(85), Some("PC")));
        // versão sem nota própria: nota geral, sem fingir que é daquela plataforma
        apply_platform(&mut i, "Xbox 360");
        assert_eq!((i.metacritic_score, i.metacritic_platform), (Some(90), None));
    }

    #[test]
    fn steam_score_is_pc_only_and_manual_is_kept() {
        let mut s = GameInfo { metacritic_score: Some(80), metacritic_source: Some("steam".into()), metacritic_checked_at: Some("x".into()), ..Default::default() };
        apply_platform(&mut s, "PC");
        assert_eq!(s.metacritic_score, Some(80));
        apply_platform(&mut s, "Playstation 3");
        assert_eq!((s.metacritic_score, s.metacritic_checked_at.as_deref()), (None, None), "nota da versão de PC não vale para PS3");
        let mut m = GameInfo { metacritic_score: Some(70), metacritic_source: Some("manual".into()), ..Default::default() };
        apply_platform(&mut m, "PSP");
        assert_eq!(m.metacritic_score, Some(70));
    }

    #[test]
    fn candidate_platform_and_scores() {
        let g = json!({"id": 5, "name": "Ico", "released": "2001-09-24", "metacritic": 90,
            "platforms": [{"platform": {"slug": "playstation2", "name": "PlayStation 2"}}]});
        let c = candidate(&g, Some("playstation2")).unwrap();
        assert!(c.on_platform && c.year == Some(2001) && c.metacritic == Some(90));
        assert!(!candidate(&g, Some("pc")).unwrap().on_platform);
        assert!(parse_scores(&json!({"metacritic": 0})).0.is_none(), "0 = sem nota");
        // Switch 2: sem slug/ID confirmado no RAWG; a busca roda sem filtro de plataforma.
        assert!(crate::commands::games::PLATFORMS.iter().filter(|p| **p != "Nintendo Switch 2").all(|p| rawg_slug(p).and_then(rawg_platform_id).is_some()));
    }
}
