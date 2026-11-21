//! Estatísticas dos jogos integrados (Fase 8).
//!
//! - Definições vêm SÓ do manifesto assinado (`catalog.statistics`), como as conquistas.
//! - O jogo manda valores ABSOLUTOS por slot (`statistics.updated`); o NexStage guarda por perfil + slot
//!   (`bridge_stats`) e mostra a soma dos slots do perfil. Números nunca descem num slot (reenvio antigo
//!   não apaga um recorde); textos (`ref`) são substituídos.
//! - Chaves fora do catálogo são ignoradas (jogo mais novo que o catálogo guardado), nunca guardadas.
//! - Progresso geral (`game.progress.updated`) fica como a estatística `progress_percent` do slot.

use super::protocol::Code;
use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;

const TYPES: [&str; 5] = ["counter", "max", "duration_s", "max_duration_s", "ref"];
const MAX_STATS: usize = 300;
const MAX_NUMBER: f64 = 1e12;

fn valid_key(k: &str) -> bool {
    (1..=64).contains(&k.len()) && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn def_ok(d: &Value) -> bool {
    d["key"].as_str().is_some_and(valid_key)
        && d["type"].as_str().is_some_and(|t| TYPES.contains(&t))
        && d["name"].as_object().is_some_and(|o| !o.is_empty() && o.values().all(|v| v.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= 80)))
}

pub fn catalog_version(conn: &Connection, game: &str) -> i64 {
    conn.query_row("SELECT COALESCE(MAX(catalog_version),0) FROM bridge_stat_defs WHERE game_id=?1", [game], |r| r.get(0)).unwrap_or(0)
}

/// Guarda as definições de um manifesto já verificado (mesma regra das conquistas: nunca rebaixa).
pub fn save_catalog(conn: &mut Connection, game: &str, catalog: &Value) -> AppResult<usize> {
    let version = catalog["version"].as_i64().unwrap_or(0);
    let list = catalog["statistics"].as_array().cloned().unwrap_or_default();
    if version <= 0 || list.is_empty() || version < catalog_version(conn, game) {
        return Ok(0);
    }
    let valid: Vec<&Value> = list.iter().take(MAX_STATS).filter(|d| def_ok(d)).collect();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM bridge_stat_defs WHERE game_id=?1", [game])?;
    for d in &valid {
        tx.execute(
            "INSERT OR REPLACE INTO bridge_stat_defs(game_id,stat_key,data,catalog_version) VALUES(?1,?2,?3,?4)",
            params![game, d["key"].as_str().unwrap_or_default(), d.to_string(), version],
        )?;
    }
    tx.commit()?;
    Ok(valid.len())
}

fn defs(conn: &Connection, game: &str) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    if let Ok(mut st) = conn.prepare("SELECT stat_key, data FROM bridge_stat_defs WHERE game_id=?1") {
        if let Ok(rows) = st.query_map([game], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            for (k, d) in rows.flatten() {
                if let Ok(v) = serde_json::from_str(&d) {
                    out.insert(k, v);
                }
            }
        }
    }
    out
}

fn put(conn: &Connection, profile: i64, game: &str, slot: &str, key: &str, value: &Value, numeric: bool) -> rusqlite::Result<()> {
    if numeric {
        let old: Option<String> = conn
            .query_row("SELECT value FROM bridge_stats WHERE profile_id=?1 AND game_id=?2 AND slot_uid=?3 AND key=?4", params![profile, game, slot, key], |r| r.get(0))
            .optional()?;
        if old.and_then(|o| o.parse::<f64>().ok()).is_some_and(|o| o >= value.as_f64().unwrap_or(0.0)) {
            return Ok(()); // número não desce
        }
    }
    let text = match value {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    conn.execute(
        "INSERT INTO bridge_stats(profile_id,game_id,slot_uid,key,value,updated_at) VALUES(?1,?2,?3,?4,?5,datetime('now'))
         ON CONFLICT(profile_id,game_id,slot_uid,key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        params![profile, game, slot, key, text],
    )?;
    Ok(())
}

/// `statistics.updated`. Sem catálogo → `E_CATALOG` só se NENHUMA chave for conhecida; devolve quantas foram guardadas.
pub fn apply(conn: &Connection, profile: i64, game: &str, slot: &str, values: &Value) -> Result<usize, Code> {
    let defs = defs(conn, game);
    let obj = values.as_object().ok_or(Code::Schema)?;
    if obj.len() > MAX_STATS {
        return Err(Code::Schema);
    }
    let mut n = 0;
    for (k, v) in obj {
        let Some(d) = defs.get(k) else { continue };
        let ty = d["type"].as_str().unwrap_or_default();
        let ok = if ty == "ref" {
            v.as_str().is_some_and(|s| !s.is_empty() && s.chars().count() <= 64)
        } else {
            v.as_f64().is_some_and(|x| x.is_finite() && (0.0..=MAX_NUMBER).contains(&x))
        };
        if !ok {
            return Err(Code::Schema);
        }
        put(conn, profile, game, slot, k, v, ty != "ref").map_err(|_| Code::Schema)?;
        n += 1;
    }
    if n == 0 && !obj.is_empty() {
        return Err(Code::Catalog);
    }
    Ok(n)
}

/// `game.progress.updated` → `progress_percent` do slot (0–100, só sobe).
pub fn apply_progress(conn: &Connection, profile: i64, game: &str, slot: &str, percent: f64) -> Result<(), Code> {
    let p = percent.round().clamp(0.0, 100.0);
    put(conn, profile, game, slot, "progress_percent", &json!(p as i64), true).map_err(|_| Code::Schema)
}

/// Uma estatística do perfil (soma dos slots).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatView {
    pub key: String,
    pub name: Value,
    pub kind: String,
    pub group: String,
    /// Número (contadores/recordes/durações) ou texto (`ref`).
    pub value: Value,
    /// Rótulos do texto (`ref`) por idioma, quando o catálogo os traz.
    pub label: Option<Value>,
    pub of: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStats {
    pub slots: i64,
    pub last_update: Option<String>,
    pub stats: Vec<StatView>,
}

/// Estatísticas do perfil: contadores e durações somam os slots; recordes pegam o maior; textos vêm do slot
/// atualizado por último. Na ordem do catálogo.
pub fn profile(conn: &Connection, profile: i64, game: &str) -> AppResult<ProfileStats> {
    let mut st = conn.prepare("SELECT slot_uid, key, value, updated_at FROM bridge_stats WHERE profile_id=?1 AND game_id=?2 ORDER BY updated_at, rowid")?;
    let rows: Vec<(String, String, String, String)> = st
        .query_map(params![profile, game], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let slots: std::collections::HashSet<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    let last_update = rows.iter().map(|r| r.3.clone()).max();
    let mut defs_list: Vec<(String, Value)> = {
        let mut s = conn.prepare("SELECT stat_key, data FROM bridge_stat_defs WHERE game_id=?1 ORDER BY rowid")?;
        let v = s.query_map([game], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        v.into_iter().filter_map(|(k, d)| Some((k, serde_json::from_str(&d).ok()?))).collect()
    };
    let mut stats = Vec::new();
    for (key, d) in defs_list.drain(..) {
        let ty = d["type"].as_str().unwrap_or_default().to_string();
        let vals: Vec<&String> = rows.iter().filter(|r| r.1 == key).map(|r| &r.2).collect();
        if vals.is_empty() {
            continue;
        }
        let value = match ty.as_str() {
            "ref" => json!(vals.last().map(|s| s.as_str()).unwrap_or_default()),
            "counter" | "duration_s" => json!(vals.iter().filter_map(|v| v.parse::<f64>().ok()).sum::<f64>().round() as i64),
            _ => json!(vals.iter().filter_map(|v| v.parse::<f64>().ok()).fold(0.0_f64, f64::max).round() as i64),
        };
        let label = if ty == "ref" { value.as_str().and_then(|v| d["labels"].get(v)).cloned() } else { None };
        stats.push(StatView { key, name: d["name"].clone(), kind: ty, group: d["group"].as_str().unwrap_or_default().to_string(), value, label, of: d["of"].as_i64() });
    }
    Ok(ProfileStats { slots: slots.len() as i64, last_update, stats })
}

/// Um slot do jogo neste perfil, com os números principais (aba Slots da página especial).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotView {
    pub slot_uid: String,
    pub slot_number: Option<i64>,
    pub first_seen: Option<String>,
    pub last_seen: Option<String>,
    pub values: HashMap<String, Value>,
}

/// Slots vistos neste perfil (pelo `game.started` ou por estatísticas), do usado por último ao mais antigo.
pub fn slots(conn: &Connection, profile: i64, game: &str) -> AppResult<Vec<SlotView>> {
    let mut map: Vec<SlotView> = Vec::new();
    let mut st = conn.prepare("SELECT slot_uid, slot_number, first_seen, last_seen FROM bridge_slots WHERE profile_id=?1 AND game_id=?2")?;
    for r in st.query_map(params![profile, game], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))? {
        let (uid, n, f, l) = r?;
        map.push(SlotView { slot_uid: uid, slot_number: n, first_seen: Some(f), last_seen: Some(l), values: HashMap::new() });
    }
    let mut st = conn.prepare("SELECT slot_uid, key, value, updated_at FROM bridge_stats WHERE profile_id=?1 AND game_id=?2")?;
    for r in st.query_map(params![profile, game], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))? {
        let (uid, k, v, at) = r?;
        let i = match map.iter().position(|s| s.slot_uid == uid) {
            Some(i) => i,
            None => {
                map.push(SlotView { slot_uid: uid, slot_number: None, first_seen: None, last_seen: None, values: HashMap::new() });
                map.len() - 1
            }
        };
        let s = &mut map[i];
        if s.last_seen.as_deref().is_none_or(|l| at.as_str() > l) {
            s.last_seen = Some(at);
        }
        s.values.insert(k, v.parse::<f64>().map(|x| json!(x.round() as i64)).unwrap_or(Value::String(v)));
    }
    map.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    Ok(map)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn catalog() -> Value {
        json!({"version": 1, "statistics": [
            {"key":"runs","type":"counter","group":"geral","name":{"pt-BR":"Partidas"}},
            {"key":"best_level","type":"max","group":"recordes","name":{"pt-BR":"Maior nível"}},
            {"key":"total_playtime_s","type":"duration_s","group":"geral","name":{"pt-BR":"Tempo"}},
            {"key":"top_weapon","type":"ref","group":"armas","name":{"pt-BR":"Arma favorita"},"labels":{"bow":{"pt-BR":"Arco"}}},
            {"key":"progress_percent","type":"max","group":"geral","name":{"pt-BR":"Progresso"}},
            {"key":"BAD","type":"counter","name":{"pt-BR":"x"}}
        ]})
    }

    pub fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE profiles(id INTEGER PRIMARY KEY); INSERT INTO profiles(id) VALUES(1),(2);").unwrap();
        c.execute_batch(super::super::store::SCHEMA).unwrap();
        c
    }

    #[test]
    fn slots_list_number_and_values() {
        let mut c = db();
        save_catalog(&mut c, "g", &catalog()).unwrap();
        super::super::store::slot_seen(&c, 1, "g", "s1", Some(2)).unwrap();
        super::super::store::slot_seen(&c, 1, "g", "s1", None).unwrap(); // número continua
        apply(&c, 1, "g", "s1", &json!({"runs": 4, "top_weapon": "bow"})).unwrap();
        apply(&c, 1, "g", "s7", &json!({"runs": 1})).unwrap(); // só estatística, sem game.started
        apply(&c, 2, "g", "s9", &json!({"runs": 9})).unwrap();
        let v = slots(&c, 1, "g").unwrap();
        assert_eq!(v.len(), 2);
        let s1 = v.iter().find(|s| s.slot_uid == "s1").unwrap();
        assert_eq!((s1.slot_number, s1.values["runs"].clone(), s1.values["top_weapon"].clone()), (Some(2), json!(4), json!("bow")));
    }

    #[test]
    fn catalog_validated_and_values_checked() {
        let mut c = db();
        assert_eq!(save_catalog(&mut c, "g", &catalog()).unwrap(), 5, "definição inválida ignorada");
        assert_eq!(apply(&c, 1, "g", "s1", &json!({"runs": 3, "chave_nova": 7})), Ok(1), "chave fora do catálogo ignorada");
        assert_eq!(apply(&c, 1, "g", "s1", &json!({"runs": -1})), Err(Code::Schema));
        assert_eq!(apply(&c, 1, "g", "s1", &json!({"top_weapon": 5})), Err(Code::Schema));
        assert_eq!(apply(&c, 1, "g", "s1", &json!({"so_desconhecida": 1})), Err(Code::Catalog));
        let n: i64 = c.query_row("SELECT COUNT(*) FROM bridge_stats WHERE key='chave_nova'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn profile_sums_slots_and_numbers_never_go_down() {
        let mut c = db();
        save_catalog(&mut c, "g", &catalog()).unwrap();
        apply(&c, 1, "g", "s1", &json!({"runs": 10, "best_level": 20, "total_playtime_s": 600, "top_weapon": "pistol"})).unwrap();
        apply(&c, 1, "g", "s1", &json!({"runs": 4})).unwrap(); // reenvio antigo: não desce
        apply(&c, 1, "g", "s2", &json!({"runs": 5, "best_level": 31, "total_playtime_s": 300, "top_weapon": "bow"})).unwrap();
        apply(&c, 2, "g", "s9", &json!({"runs": 99})).unwrap(); // outro perfil
        apply_progress(&c, 1, "g", "s1", 42.4).unwrap();
        apply_progress(&c, 1, "g", "s1", 10.0).unwrap();
        let p = profile(&c, 1, "g").unwrap();
        let get = |k: &str| p.stats.iter().find(|s| s.key == k).map(|s| s.value.clone());
        assert_eq!(p.slots, 2);
        assert_eq!(get("runs"), Some(json!(15)), "contador soma os slots");
        assert_eq!(get("best_level"), Some(json!(31)), "recorde pega o maior");
        assert_eq!(get("total_playtime_s"), Some(json!(900)));
        assert_eq!(get("progress_percent"), Some(json!(42)), "progresso só sobe");
        assert_eq!(get("top_weapon"), Some(json!("bow")), "texto do slot mais recente");
        assert_eq!(p.stats.iter().find(|s| s.key == "top_weapon").unwrap().label, Some(json!({"pt-BR":"Arco"})));
        assert_eq!(profile(&c, 2, "g").unwrap().stats[0].value, json!(99));
    }
}
