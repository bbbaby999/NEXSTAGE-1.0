//! Conquistas dos jogos integrados (Fase 6).
//!
//! - O catálogo vem SÓ do manifesto assinado da versão publicada (`catalog.achievements`), conferido pela
//!   assinatura Ed25519 antes de chegar aqui. O jogo nunca define conquistas pelo Bridge.
//! - Cada desbloqueio enviado pelo jogo é conferido contra o catálogo: id existente, não reservado
//!   (modos futuros), progresso com o mesmo alvo do catálogo. Desbloqueio é permanente e idempotente.
//! - Tudo por perfil do NexStage (`profile_id`); o jogo só conhece a chave opaca do perfil.

use super::protocol::{self as proto, Code};
use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

const RARITIES: [&str; 4] = ["comum", "rara", "epica", "lendaria"];
const MAX_ACHIEVEMENTS: usize = 500;

fn valid_ach_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn text_map_ok(v: &Value, max: usize) -> bool {
    v.as_object().is_some_and(|o| !o.is_empty() && o.values().all(|t| t.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= max)))
}

/// Confere uma definição do catálogo (formato da especificação, §11).
fn def_ok(d: &Value) -> bool {
    d["id"].as_str().is_some_and(valid_ach_id)
        && text_map_ok(&d["name"], 80)
        && text_map_ok(&d["description"], 240)
        && d["rarity"].as_str().is_some_and(|r| RARITIES.contains(&r))
        && d["points"].as_i64().is_some_and(|p| (0..=1000).contains(&p))
        && (d["target"].is_null() || d["target"].as_i64().is_some_and(|t| t > 0))
}

/// Versão do catálogo guardada (0 = nenhum ainda: eventos de conquista ficam guardados no jogo).
pub fn catalog_version(conn: &Connection, game: &str) -> i64 {
    conn.query_row("SELECT COALESCE(MAX(catalog_version),0) FROM bridge_achievements WHERE game_id=?1", [game], |r| r.get(0)).unwrap_or(0)
}

/// Guarda o catálogo de um manifesto já verificado. Só substitui por um catálogo de versão igual ou maior
/// (voltar o jogo de versão não apaga conquistas novas). Devolve quantas definições ficaram guardadas.
pub fn save_catalog(conn: &mut Connection, game: &str, catalog: &Value) -> AppResult<usize> {
    let version = catalog["version"].as_i64().unwrap_or(0);
    let list = catalog["achievements"].as_array().cloned().unwrap_or_default();
    if version <= 0 || list.is_empty() || version < catalog_version(conn, game) {
        return Ok(0);
    }
    let valid: Vec<&Value> = list.iter().take(MAX_ACHIEVEMENTS).filter(|d| def_ok(d)).collect();
    if valid.len() != list.len().min(MAX_ACHIEVEMENTS) {
        crate::diaglog::write("aviso", "bridge", &format!("catálogo de {game}: {} definição(ões) inválida(s) ignorada(s)", list.len() - valid.len()));
    }
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM bridge_achievements WHERE game_id=?1", [game])?;
    for d in &valid {
        tx.execute(
            "INSERT OR REPLACE INTO bridge_achievements(game_id,achievement_id,data,catalog_version) VALUES(?1,?2,?3,?4)",
            params![game, d["id"].as_str().unwrap_or_default(), d.to_string(), version],
        )?;
    }
    tx.commit()?;
    Ok(valid.len())
}

fn def_of(conn: &Connection, game: &str, id: &str) -> Option<Value> {
    let s: Option<String> = conn
        .query_row("SELECT data FROM bridge_achievements WHERE game_id=?1 AND achievement_id=?2", params![game, id], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    s.and_then(|s| serde_json::from_str(&s).ok())
}

/// Resultado de aplicar um evento de conquista.
#[derive(Debug, PartialEq, Eq)]
pub enum Applied {
    /// Primeira vez neste perfil (o launcher avisa — Fase 7).
    New,
    /// Já estava registrado (reenvio, outro slot do mesmo perfil, etc.): confirmado sem mudar nada.
    Already,
}

/// `achievement.unlocked`. `unlocked_at` vem do jogo; se faltar, for inválido ou estiver no futuro, usa agora.
pub fn apply_unlock(conn: &Connection, profile: i64, game: &str, data: &Value, origin: &str, event_id: &str, now: i64) -> Result<Applied, Code> {
    let id = data["achievement_id"].as_str().unwrap_or_default();
    let def = def_of(conn, game, id).ok_or(Code::Catalog)?;
    if def["reserved"].as_bool().unwrap_or(false) {
        return Err(Code::Catalog);
    }
    let at = data["unlocked_at"].as_str().and_then(proto::parse_ts).filter(|t| *t <= now + proto::TS_WINDOW_SECS).unwrap_or(now);
    let origin = if origin == "offline" { "offline" } else { "live" };
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO bridge_unlocks(profile_id,game_id,achievement_id,unlocked_at,origin,event_id) VALUES(?1,?2,?3,?4,?5,?6)",
            params![profile, game, id, proto::ts_from_secs(at), origin, event_id],
        )
        .map_err(|_| Code::Schema)?;
    if let Some(t) = def["target"].as_i64() {
        let _ = conn.execute(
            "INSERT INTO bridge_progress(profile_id,game_id,achievement_id,current,target,updated_at) VALUES(?1,?2,?3,?4,?4,datetime('now'))
             ON CONFLICT(profile_id,game_id,achievement_id) DO UPDATE SET current=excluded.current, target=excluded.target, updated_at=excluded.updated_at",
            params![profile, game, id, t],
        );
    }
    Ok(if n == 1 { Applied::New } else { Applied::Already })
}

/// `achievement.progress.updated`: só sobe; alvo tem de ser o do catálogo; não desbloqueia sozinho
/// (o jogo manda o desbloqueio explícito ao chegar no alvo).
pub fn apply_progress(conn: &Connection, profile: i64, game: &str, data: &Value) -> Result<Applied, Code> {
    let id = data["achievement_id"].as_str().unwrap_or_default();
    let def = def_of(conn, game, id).ok_or(Code::Catalog)?;
    let target = def["target"].as_i64().ok_or(Code::Catalog)?;
    if def["reserved"].as_bool().unwrap_or(false) || data["target"].as_i64() != Some(target) {
        return Err(Code::Catalog);
    }
    let current = data["current"].as_i64().unwrap_or(0).clamp(0, target);
    let n = conn
        .execute(
            "INSERT INTO bridge_progress(profile_id,game_id,achievement_id,current,target,updated_at) VALUES(?1,?2,?3,?4,?5,datetime('now'))
             ON CONFLICT(profile_id,game_id,achievement_id) DO UPDATE SET current=excluded.current, target=excluded.target, updated_at=excluded.updated_at
             WHERE excluded.current > bridge_progress.current",
            params![profile, game, id, current, target],
        )
        .map_err(|_| Code::Schema)?;
    Ok(if n == 1 { Applied::New } else { Applied::Already })
}

/// Texto do catálogo no idioma do app (o catálogo traz pt-BR e en; os outros idiomas usam o inglês).
pub fn pick_lang(m: &Value, lang: &str) -> String {
    let get = |k: &str| m.get(k).and_then(Value::as_str).map(str::to_string);
    get(lang)
        .or_else(|| if lang.is_empty() || lang.starts_with("pt") { get("pt-BR") } else { get("en") })
        .or_else(|| get("en"))
        .or_else(|| get("pt-BR"))
        .or_else(|| m.as_object().and_then(|o| o.values().find_map(|v| v.as_str().map(str::to_string))))
        .unwrap_or_default()
}

/// Até 5 cartões de conquista recém-desbloqueadas (as demais viram "e mais N" no último). Épicas e lendárias
/// usam o estilo raro (dourado, som próprio).
pub fn toasts(conn: &Connection, game: &str, game_name: &str, ids: &[String], lang: &str) -> Vec<crate::shot_toast::AchievementToast> {
    let mut out: Vec<crate::shot_toast::AchievementToast> = ids
        .iter()
        .filter_map(|id| def_of(conn, game, id))
        .take(5)
        .map(|d| crate::shot_toast::AchievementToast {
            source: "nexstage".into(),
            icon: None,
            title: pick_lang(&d["name"], lang),
            game: game_name.to_string(),
            points: d["points"].as_i64().unwrap_or(0),
            hardcore: false,
            rare: matches!(d["rarity"].as_str(), Some("epica" | "lendaria")),
            global_pct: None,
            more: 0,
            grade: None,
        })
        .collect();
    if let Some(last) = out.last_mut() {
        last.more = ids.len().saturating_sub(5);
    }
    out
}

/// Uma conquista do catálogo com o estado do perfil (página do jogo no NexStage).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementView {
    pub id: String,
    pub name: Value,
    pub description: Value,
    pub category: String,
    pub rarity: String,
    pub points: i64,
    pub glyph: String,
    pub hidden: bool,
    pub reserved: bool,
    pub target: Option<i64>,
    pub current: i64,
    pub unlocked_at: Option<String>,
    pub origin: Option<String>,
}

pub fn list(conn: &Connection, profile: i64, game: &str) -> AppResult<Vec<AchievementView>> {
    let mut st = conn.prepare(
        "SELECT a.data, u.unlocked_at, u.origin, COALESCE(p.current,0) FROM bridge_achievements a
         LEFT JOIN bridge_unlocks u ON u.game_id=a.game_id AND u.achievement_id=a.achievement_id AND u.profile_id=?1
         LEFT JOIN bridge_progress p ON p.game_id=a.game_id AND p.achievement_id=a.achievement_id AND p.profile_id=?1
         WHERE a.game_id=?2 ORDER BY a.rowid",
    )?;
    let rows = st.query_map(params![profile, game], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (data, unlocked_at, origin, current) = row?;
        let Ok(d) = serde_json::from_str::<Value>(&data) else { continue };
        let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
        out.push(AchievementView {
            id: s("id"),
            name: d["name"].clone(),
            description: d["description"].clone(),
            category: s("category"),
            rarity: s("rarity"),
            points: d["points"].as_i64().unwrap_or(0),
            glyph: s("glyph"),
            hidden: d["hidden"].as_bool().unwrap_or(false),
            reserved: d["reserved"].as_bool().unwrap_or(false),
            target: d["target"].as_i64(),
            current,
            unlocked_at,
            origin,
        });
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn catalog() -> Value {
        json!({"version": 1, "achievements": [
            {"id":"first_blood","name":{"pt-BR":"Primeiro Sangue"},"description":{"pt-BR":"Derrote o primeiro inimigo."},"category":"combate","rarity":"comum","points":15,"target":null},
            {"id":"relic_hunter","name":{"pt-BR":"Caçador"},"description":{"pt-BR":"50 relíquias."},"category":"exploracao","rarity":"rara","points":30,"target":50},
            {"id":"immortal_100","name":{"pt-BR":"Imortal"},"description":{"pt-BR":"Modo infinito."},"category":"desafio","rarity":"lendaria","points":180,"target":null,"reserved":true}
        ]})
    }

    pub fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE profiles(id INTEGER PRIMARY KEY); INSERT INTO profiles(id) VALUES(1),(2);").unwrap();
        c.execute_batch(super::super::store::SCHEMA).unwrap();
        c
    }

    #[test]
    fn catalog_saved_validated_and_never_downgraded() {
        let mut c = db();
        let mut cat = catalog();
        cat["achievements"].as_array_mut().unwrap().push(json!({"id":"BAD id","name":{"pt-BR":"x"},"description":{"pt-BR":"x"},"rarity":"comum","points":1}));
        assert_eq!(save_catalog(&mut c, "g", &cat).unwrap(), 3, "definição inválida ignorada");
        assert_eq!(catalog_version(&c, "g"), 1);
        let old = json!({"version": 0, "achievements": [{"id":"x","name":{"pt-BR":"x"},"description":{"pt-BR":"x"},"rarity":"comum","points":1}]});
        assert_eq!(save_catalog(&mut c, "g", &old).unwrap(), 0, "catálogo mais antigo não substitui");
        assert_eq!(list(&c, 1, "g").unwrap().len(), 3);
        assert_eq!(save_catalog(&mut c, "outro", &json!({"version":0,"achievements":[]})).unwrap(), 0);
    }

    #[test]
    fn unlock_checked_against_catalog_and_idempotent() {
        let mut c = db();
        save_catalog(&mut c, "g", &catalog()).unwrap();
        let now = proto::now_secs();
        let d = json!({"achievement_id":"first_blood","slot_uid":"s1","unlocked_at": proto::ts_from_secs(now - 3600)});
        assert_eq!(apply_unlock(&c, 1, "g", &d, "offline", "ev-00000001", now), Ok(Applied::New));
        assert_eq!(apply_unlock(&c, 1, "g", &d, "live", "ev-00000002", now), Ok(Applied::Already));
        assert_eq!(apply_unlock(&c, 2, "g", &d, "live", "ev-00000003", now), Ok(Applied::New), "outro perfil desbloqueia separado");
        let v = list(&c, 1, "g").unwrap();
        assert_eq!(v[0].unlocked_at.as_deref(), Some(proto::ts_from_secs(now - 3600).as_str()), "data original do jogo (offline)");
        assert_eq!(v[0].origin.as_deref(), Some("offline"));
        assert!(v[1].unlocked_at.is_none());
        // inexistente, reservada
        assert_eq!(apply_unlock(&c, 1, "g", &json!({"achievement_id":"nope","slot_uid":"s1"}), "live", "ev-00000004", now), Err(Code::Catalog));
        assert_eq!(apply_unlock(&c, 1, "g", &json!({"achievement_id":"immortal_100","slot_uid":"s1"}), "live", "ev-00000005", now), Err(Code::Catalog));
        // data no futuro vira agora
        let f = json!({"achievement_id":"relic_hunter","slot_uid":"s1","unlocked_at": proto::ts_from_secs(now + 86400)});
        apply_unlock(&c, 1, "g", &f, "live", "ev-00000006", now).unwrap();
        let v = list(&c, 1, "g").unwrap();
        assert_eq!(v[1].unlocked_at.as_deref(), Some(proto::ts_from_secs(now).as_str()));
        assert_eq!(v[1].current, 50, "desbloqueio completa o progresso");
    }

    #[test]
    fn toasts_use_app_language_and_summarize() {
        let mut c = db();
        let mut cat = catalog();
        cat["achievements"][0]["name"]["en"] = json!("First Blood");
        cat["achievements"][1]["rarity"] = json!("epica");
        save_catalog(&mut c, "g", &cat).unwrap();
        let ids: Vec<String> = ["first_blood", "relic_hunter"].iter().map(|s| s.to_string()).collect();
        let t = toasts(&c, "g", "Tomb Survivors", &ids, "en");
        assert_eq!((t[0].title.as_str(), t[0].source.as_str(), t[0].points, t[0].rare), ("First Blood", "nexstage", 15, false));
        assert!(t[1].rare, "épica usa o estilo raro");
        assert_eq!(toasts(&c, "g", "T", &ids, "pt-BR")[0].title, "Primeiro Sangue");
        assert_eq!(toasts(&c, "g", "T", &ids, "ja")[1].title, "Caçador", "sem inglês, cai no português");
        let many: Vec<String> = std::iter::repeat("first_blood".to_string()).take(8).collect();
        let t = toasts(&c, "g", "T", &many, "pt-BR");
        assert_eq!((t.len(), t[4].more), (5, 3), "até 5 cartões, o resto no último");
        assert!(toasts(&c, "g", "T", &["inventada".to_string()], "pt-BR").is_empty());
    }

    #[test]
    fn progress_only_goes_up_and_needs_catalog_target() {
        let mut c = db();
        save_catalog(&mut c, "g", &catalog()).unwrap();
        let p = |cur: i64, t: i64| json!({"achievement_id":"relic_hunter","current":cur,"target":t,"slot_uid":"s1"});
        assert_eq!(apply_progress(&c, 1, "g", &p(20, 50)), Ok(Applied::New));
        assert_eq!(apply_progress(&c, 1, "g", &p(10, 50)), Ok(Applied::Already), "não desce");
        assert_eq!(apply_progress(&c, 1, "g", &p(30, 99)), Err(Code::Catalog), "alvo diferente do catálogo");
        assert_eq!(apply_progress(&c, 1, "g", &p(999, 50)), Ok(Applied::New));
        assert_eq!(list(&c, 1, "g").unwrap()[1].current, 50, "limitado ao alvo");
        assert!(list(&c, 1, "g").unwrap()[1].unlocked_at.is_none(), "progresso não desbloqueia sozinho");
        assert_eq!(apply_progress(&c, 1, "g", &json!({"achievement_id":"first_blood","current":1,"target":1})), Err(Code::Catalog), "sem alvo no catálogo");
    }
}
