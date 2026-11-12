//! Associação de jogos da biblioteca com jogos do RetroAchievements.
//!
//! Regras:
//! 1. Hash oficial idêntico (arquivo do jogo → `rcheevos`) = associação automática (sem ambiguidade:
//!    o RA só aceita conquistas para esses hashes).
//! 2. Sem hash correspondente: no máximo SUGESTÕES por nome, que só valem depois de confirmadas.
//! 3. O usuário pode confirmar, escolher outro jogo, desfazer ou marcar "sem suporte".
//! Jogos de plataformas sem RetroAchievements nunca são tocados.

use super::client::{self, Creds, RaError};
use super::hash::{self, console_for_platform};
use super::store::{self, GameRow};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    pub game_id: i64,
    /// "linked" | "suggested" | "none" | "unsupported" (marcado pelo usuário) | "platform" (plataforma sem RA)
    pub status: String,
    /// "hash" | "manual" quando associado.
    pub method: Option<String>,
    pub console_id: Option<i64>,
    pub ra_game: Option<GameRow>,
    pub hash: Option<String>,
    pub hash_error: Option<String>,
    /// O hash do arquivo não está entre os hashes oficiais do jogo associado manualmente.
    pub hash_mismatch: bool,
    pub candidates: Vec<GameRow>,
    pub catalog_ready: bool,
    pub catalog_error: Option<String>,
}

struct GameFile {
    title: String,
    platform: String,
    path: Option<String>,
}

fn game_file(conn: &Connection, profile_id: i64, game_id: i64) -> AppResult<GameFile> {
    conn.query_row(
        "SELECT g.title, g.platform, COALESCE(g.iso_path, g.rom_path) FROM games g JOIN profile_games pg ON pg.game_id=g.id
         WHERE pg.profile_id=?1 AND g.id=?2",
        params![profile_id, game_id],
        |r| Ok(GameFile { title: r.get(0)?, platform: r.get(1)?, path: r.get(2)? }),
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Jogo não encontrado na biblioteca.".into()))
}

/// Assinatura do arquivo (tamanho + data de modificação): muda quando o arquivo é trocado.
fn file_sig(path: &str) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let mt = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    Some(format!("{}:{mt}", m.len()))
}

/// Hash do arquivo do jogo, reaproveitando o cálculo anterior se o arquivo não mudou.
/// A leitura do disco acontece FORA do lock do banco.
pub fn file_hash(db: &Db, game_id: i64, console: i64, path: &str) -> AppResult<(Option<String>, Option<String>)> {
    let Some(sig) = file_sig(path) else { return Ok((None, Some("Arquivo do jogo não encontrado.".into()))) };
    {
        let conn = db.conn()?;
        let cached: Option<(String, Option<String>, Option<String>)> = conn
            .query_row("SELECT file_sig, hash, error FROM ra_file_hashes WHERE game_id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        if let Some((s, h, e)) = cached {
            if s == sig {
                return Ok((h, e));
            }
        }
    }
    let (h, e) = match hash::hash_file(console, Path::new(path)) {
        Ok(h) => (Some(h), None),
        Err(e) => (None, Some(e)),
    };
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO ra_file_hashes(game_id, file_sig, hash, error) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(game_id) DO UPDATE SET file_sig=excluded.file_sig, hash=excluded.hash, error=excluded.error",
        params![game_id, sig, h, e],
    )?;
    Ok((h, e))
}

/// Baixa o catálogo (com hashes) do console se o cache tiver mais de 7 dias. Falhas não apagam o cache.
pub async fn ensure_catalog(db: &Db, creds: &Creds, console: i64) -> Result<(), RaError> {
    let fresh = db.conn().ok().and_then(|c| store::catalog_fresh(&c, console, 7).ok()).unwrap_or(false);
    if fresh {
        return Ok(());
    }
    let rows = client::game_list(creds, console).await?;
    if let Ok(mut conn) = db.conn() {
        if let Ok(tx) = conn.transaction() {
            if store::store_catalog(&tx, console, &rows).is_ok() {
                let _ = tx.commit();
            }
        }
    }
    Ok(())
}

/// Normaliza títulos para comparar: minúsculas, sem tags de região/versão, sem pontuação.
pub fn norm_title(s: &str) -> Vec<String> {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => out.push(' '),
        }
    }
    out.split_whitespace()
        .filter(|w| !matches!(*w, "the" | "a" | "an" | "of" | "and" | "e" | "o" | "de" | "do" | "da"))
        .map(|w| match w {
            "ii" => "2".to_string(),
            "iii" => "3".to_string(),
            "iv" => "4".to_string(),
            "v" => "5".to_string(),
            "x" => "10".to_string(),
            w => w.to_string(),
        })
        .collect()
}

/// Semelhança entre títulos (0–1): Jaccard das palavras, com bônus para igualdade exata.
pub fn title_score(a: &str, b: &str) -> f64 {
    let (x, y) = (norm_title(a), norm_title(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    if x == y {
        return 1.0;
    }
    let inter = x.iter().filter(|w| y.contains(w)).count() as f64;
    let uni = (x.len() + y.len()) as f64 - inter;
    inter / uni
}

fn suggestions(conn: &Connection, profile_id: i64, console: i64, title: &str) -> AppResult<Vec<GameRow>> {
    let all = store::search_catalog(conn, profile_id, console, "", 200).ok().unwrap_or_default();
    // o catálogo pode ter milhares de jogos: busca ampla pela 1ª palavra relevante + pontuação local
    let key = norm_title(title).into_iter().find(|w| w.len() >= 3).unwrap_or_default();
    let mut pool = if key.is_empty() { all } else { store::search_catalog(conn, profile_id, console, &key, 200)? };
    pool.retain(|g| !g.title.starts_with('~')); // ~Hack~, ~Homebrew~, ~Prototype~...
    let mut scored: Vec<(f64, GameRow)> = pool.into_iter().map(|g| (title_score(title, &g.title), g)).filter(|(s, _)| *s >= 0.5).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(5).map(|(_, g)| g).collect())
}

/// Decide o estado da associação com o que está em cache (sem rede).
pub fn resolve(conn: &Connection, profile_id: i64, game_id: i64, hash: Option<String>, hash_error: Option<String>) -> AppResult<LinkInfo> {
    let gf = game_file(conn, profile_id, game_id)?;
    let console = console_for_platform(&gf.platform);
    let mut info = LinkInfo {
        game_id, status: "platform".into(), method: None, console_id: console, ra_game: None, hash: hash.clone(), hash_error,
        hash_mismatch: false, candidates: Vec::new(), catalog_ready: false, catalog_error: None,
    };
    let Some(console) = console else { return Ok(info) };
    info.catalog_ready = store::catalog_exists(conn, console)?;
    let link: Option<(Option<i64>, String, Option<String>)> = conn
        .query_row("SELECT ra_game_id, status, hash FROM ra_links WHERE game_id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let by_hash: Option<i64> = match &hash {
        Some(h) => conn.query_row("SELECT ra_game_id FROM ra_hashes WHERE md5=?1 AND console_id=?2", params![h, console], |r| r.get(0)).optional()?,
        None => None,
    };
    match link {
        Some((_, s, _)) if s == "unsupported" => {
            info.status = "unsupported".into();
            return Ok(info);
        }
        Some((Some(rid), s, _)) if s == "manual" => {
            info.status = "linked".into();
            info.method = Some(s);
            info.hash_mismatch = info.catalog_ready && hash.is_some() && by_hash != Some(rid);
            info.ra_game = store::game(conn, profile_id, rid)?;
            return Ok(info);
        }
        Some((Some(rid), s, old_hash)) if s == "hash" && (hash.is_none() || old_hash == hash) && (by_hash.is_none() || by_hash == Some(rid)) => {
            // associação por hash continua válida (o arquivo não mudou, ou não pôde ser relido agora)
            info.status = "linked".into();
            info.method = Some(s);
            info.ra_game = store::game(conn, profile_id, rid)?;
            return Ok(info);
        }
        Some((_, s, _)) if s == "hash" => {
            // arquivo trocado e o novo hash não confirma mais o jogo: desfaz a associação automática
            conn.execute("DELETE FROM ra_links WHERE game_id=?1", [game_id])?;
        }
        _ => {}
    }
    if let (Some(rid), Some(h)) = (by_hash, &hash) {
        conn.execute(
            "INSERT INTO ra_links(game_id, ra_game_id, status, hash, file_sig, checked_at) VALUES(?1, ?2, 'hash', ?3, NULL, datetime('now'))
             ON CONFLICT(game_id) DO UPDATE SET ra_game_id=excluded.ra_game_id, status='hash', hash=excluded.hash, checked_at=excluded.checked_at",
            params![game_id, rid, h],
        )?;
        info.status = "linked".into();
        info.method = Some("hash".into());
        info.ra_game = store::game(conn, profile_id, rid)?;
        return Ok(info);
    }
    info.candidates = suggestions(conn, profile_id, console, &gf.title)?;
    info.status = if info.candidates.is_empty() { "none" } else { "suggested" }.into();
    Ok(info)
}

/// Estado completo da associação de um jogo: calcula o hash (se preciso), atualiza o catálogo
/// (se houver conta e o cache estiver velho) e resolve. Funciona offline com o que estiver em cache.
pub async fn link_status(app: &AppHandle, profile_id: i64, game_id: i64) -> AppResult<LinkInfo> {
    let db = app.state::<Db>();
    let (gf, creds) = {
        let conn = db.conn()?;
        (game_file(&conn, profile_id, game_id)?, store::creds(&conn, profile_id)?)
    };
    let Some(console) = console_for_platform(&gf.platform) else {
        let conn = db.conn()?;
        return resolve(&conn, profile_id, game_id, None, None);
    };
    let mut catalog_error = None;
    if let Some(c) = &creds {
        if let Err(e) = ensure_catalog(&db, c, console).await {
            catalog_error = Some(e.message());
        }
    }
    let (h, he) = match gf.path.clone() {
        // leitura de disco: fora da thread assíncrona
        Some(p) => {
            let a = app.clone();
            tauri::async_runtime::spawn_blocking(move || file_hash(&a.state::<Db>(), game_id, console, &p))
                .await
                .map_err(|_| AppError::Other("Falha ao calcular o hash do jogo.".into()))??
        }
        None => (None, None),
    };
    let conn = db.conn()?;
    let mut info = resolve(&conn, profile_id, game_id, h, he)?;
    if creds.is_none() && !info.catalog_ready {
        catalog_error = Some("Conecte uma conta do RetroAchievements para baixar o catálogo de jogos compatíveis.".into());
    }
    info.catalog_error = catalog_error;
    Ok(info)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub checked: usize,
    pub linked: usize,
    pub suggested: usize,
    pub not_found: usize,
    pub errors: Vec<String>,
}

/// Identifica todos os jogos compatíveis da biblioteca do perfil (só associa automaticamente por hash).
pub async fn scan_library(app: &AppHandle, profile_id: i64) -> AppResult<ScanReport> {
    let db = app.state::<Db>();
    let (games, creds): (Vec<(i64, String, String)>, Option<Creds>) = {
        let conn = db.conn()?;
        let mut st = conn.prepare("SELECT g.id, g.title, g.platform FROM games g JOIN profile_games pg ON pg.game_id=g.id WHERE pg.profile_id=?1")?;
        let rows = st.query_map([profile_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        (rows, store::creds(&conn, profile_id)?)
    };
    let creds = creds.ok_or_else(|| AppError::Validation("Conecte sua conta do RetroAchievements em Configurações para identificar os jogos.".into()))?;
    let mut rep = ScanReport::default();
    let games: Vec<(i64, String, i64)> = games.into_iter().filter_map(|(id, t, p)| console_for_platform(&p).map(|c| (id, t, c))).collect();
    let mut consoles: Vec<i64> = games.iter().map(|g| g.2).collect();
    consoles.sort();
    consoles.dedup();
    for c in consoles {
        if let Err(e) = ensure_catalog(&db, &creds, c).await {
            rep.errors.push(e.message());
            if matches!(e, RaError::InvalidCredentials | RaError::RateLimited(_)) {
                break;
            }
        }
    }
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = a.state::<Db>();
        for (gid, title, console) in games {
            rep.checked += 1;
            let info = (|| -> AppResult<LinkInfo> {
                let path = game_file(&*db.conn()?, profile_id, gid)?.path;
                let (h, he) = match path { Some(p) => file_hash(&db, gid, console, &p)?, None => (None, None) };
                let conn = db.conn()?;
                resolve(&conn, profile_id, gid, h, he)
            })();
            match info {
                Ok(i) if i.status == "linked" => rep.linked += 1,
                Ok(i) if i.status == "suggested" => rep.suggested += 1,
                Ok(i) if i.status == "unsupported" => {}
                Ok(i) => {
                    rep.not_found += 1;
                    if let Some(e) = i.hash_error {
                        if rep.errors.len() < 8 {
                            rep.errors.push(format!("{title}: {e}"));
                        }
                    }
                }
                Err(e) => {
                    if rep.errors.len() < 8 {
                        rep.errors.push(format!("{title}: {e}"));
                    }
                }
            }
        }
        rep
    })
    .await
    .map_err(|_| AppError::Other("Falha ao identificar os jogos.".into()))
}

/// Associação manual: `ra_game_id` = confirmar/escolher; `None` + `unsupported` = marcar sem suporte;
/// `None` sem `unsupported` = desfazer (volta à identificação automática).
pub fn set_link(conn: &Connection, profile_id: i64, game_id: i64, ra_game_id: Option<i64>, unsupported: bool) -> AppResult<()> {
    let gf = game_file(conn, profile_id, game_id)?;
    let console = console_for_platform(&gf.platform)
        .ok_or_else(|| AppError::Validation("Esta plataforma não tem suporte no RetroAchievements.".into()))?;
    match (ra_game_id, unsupported) {
        (Some(rid), _) => {
            let ok: Option<i64> = conn.query_row("SELECT console_id FROM ra_games WHERE ra_game_id=?1", [rid], |r| r.get(0)).optional()?;
            match ok {
                None => return Err(AppError::NotFound("Jogo do RetroAchievements não encontrado no catálogo.".into())),
                Some(c) if c != 0 && c != console => {
                    return Err(AppError::Validation("O jogo escolhido é de outro console no RetroAchievements.".into()))
                }
                _ => {}
            }
            conn.execute(
                "INSERT INTO ra_links(game_id, ra_game_id, status, checked_at) VALUES(?1, ?2, 'manual', datetime('now'))
                 ON CONFLICT(game_id) DO UPDATE SET ra_game_id=excluded.ra_game_id, status='manual', checked_at=excluded.checked_at",
                params![game_id, rid],
            )?;
        }
        (None, true) => {
            conn.execute(
                "INSERT INTO ra_links(game_id, ra_game_id, status, checked_at) VALUES(?1, NULL, 'unsupported', datetime('now'))
                 ON CONFLICT(game_id) DO UPDATE SET ra_game_id=NULL, status='unsupported', checked_at=excluded.checked_at",
                [game_id],
            )?;
        }
        (None, false) => {
            conn.execute("DELETE FROM ra_links WHERE game_id=?1", [game_id])?;
        }
    }
    Ok(())
}

/// Jogos do RA associados a jogos da biblioteca do perfil (para priorizar detalhes na sincronização).
pub fn linked_ra_games(conn: &Connection, profile_id: i64) -> AppResult<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT DISTINCT l.ra_game_id FROM ra_links l JOIN profile_games pg ON pg.game_id=l.game_id
         WHERE pg.profile_id=?1 AND l.status IN ('hash','manual') AND l.ra_game_id IS NOT NULL",
    )?;
    let rows = st.query_map([profile_id], |r| r.get(0))?.collect::<Result<Vec<i64>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> Connection {
        let c = store::tests::mem();
        c.execute_batch(
            "INSERT INTO games(id, title, platform, iso_path) VALUES(1, 'Shadow of the Colossus (USA)', 'Playstation 2', NULL),
               (2, 'Halo 3', 'Xbox 360', NULL), (3, 'Jogo Desconhecido', 'Playstation 2', NULL);
             INSERT INTO profile_games(profile_id, game_id) VALUES(1,1),(1,2),(1,3);",
        )
        .unwrap();
        store::store_catalog(&c, 21, &[
            json!({"ID": 10, "Title": "Shadow of the Colossus", "Hashes": ["00000000000000000000000000000aaa"]}),
            json!({"ID": 11, "Title": "~Hack~ Shadow of the Colossus Hard", "Hashes": []}),
            json!({"ID": 12, "Title": "Ico", "Hashes": ["00000000000000000000000000000bbb"]}),
        ]).unwrap();
        c
    }

    #[test]
    fn exact_hash_links_automatically() {
        let c = setup();
        let i = resolve(&c, 1, 3, Some("00000000000000000000000000000bbb".into()), None).unwrap();
        assert_eq!(i.status, "linked");
        assert_eq!(i.method.as_deref(), Some("hash"));
        assert_eq!(i.ra_game.unwrap().id, 12);
    }

    #[test]
    fn title_only_match_is_just_a_suggestion() {
        let c = setup();
        let i = resolve(&c, 1, 1, Some("ffffffffffffffffffffffffffffffff".into()), None).unwrap();
        assert_eq!(i.status, "suggested");
        assert_eq!(i.candidates.len(), 1, "hacks (~Hack~) não entram nas sugestões");
        assert_eq!(i.candidates[0].id, 10);
        let n: i64 = c.query_row("SELECT COUNT(*) FROM ra_links", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "sugestão nunca é gravada como associação");
        // confirmação manual
        set_link(&c, 1, 1, Some(10), false).unwrap();
        let i = resolve(&c, 1, 1, Some("ffffffffffffffffffffffffffffffff".into()), None).unwrap();
        assert_eq!((i.status.as_str(), i.method.as_deref(), i.hash_mismatch), ("linked", Some("manual"), true));
        // marcar sem suporte e desfazer
        set_link(&c, 1, 1, None, true).unwrap();
        assert_eq!(resolve(&c, 1, 1, None, None).unwrap().status, "unsupported");
        set_link(&c, 1, 1, None, false).unwrap();
        assert_eq!(resolve(&c, 1, 1, None, None).unwrap().status, "suggested");
    }

    #[test]
    fn unsupported_platform_is_untouched() {
        let c = setup();
        assert_eq!(resolve(&c, 1, 2, None, None).unwrap().status, "platform");
        assert!(set_link(&c, 1, 2, Some(10), false).is_err());
    }

    #[test]
    fn changed_file_drops_stale_hash_link() {
        let c = setup();
        assert_eq!(resolve(&c, 1, 3, Some("00000000000000000000000000000bbb".into()), None).unwrap().status, "linked");
        let i = resolve(&c, 1, 3, Some("cccccccccccccccccccccccccccccccc".into()), None).unwrap();
        assert_ne!(i.status, "linked");
    }

    #[test]
    fn title_normalization() {
        assert_eq!(title_score("Final Fantasy X (USA) [!]", "Final Fantasy X"), 1.0);
        assert_eq!(title_score("Gran Turismo III", "Gran Turismo 3"), 1.0);
        assert!(title_score("Ico", "Shadow of the Colossus") < 0.5);
    }
}
