//! Cache local do Xbox (por perfil) e ligação com a biblioteca do NEXSTAGE.
//!
//! - `xbox_titles`: jogos do histórico da conta, com resumo de conquistas e os minutos jogados registrados
//!   pelo Xbox — dado EXTERNO: nunca altera `profile_games.playtime_seconds` (histórico local).
//! - `xbox_achievements`: conquistas de um jogo, buscadas quando a página do jogo é aberta.
//! - Jogo da biblioteca ↔ título: pela família do pacote (jogo adicionado pelo atalho do app Xbox,
//!   `shell:AppsFolder\<família>!<app>` = `pfn` do título) ou, sem ela, pelo nome igual.

use crate::error::AppResult;
use crate::retro::link::norm_title;
use crate::retro::util::{int, parse_epoch, text};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

pub const PLATFORM: &str = "xbox";

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS xbox_titles (
    profile_id      INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    title_id        TEXT NOT NULL,
    pfn             TEXT,
    name            TEXT NOT NULL,
    image           TEXT,
    ach_current     INTEGER,
    ach_total       INTEGER,
    gs_current      INTEGER,
    gs_total        INTEGER,
    last_played     INTEGER,            -- epoch
    minutes         INTEGER,            -- MinutesPlayed do Xbox (NULL = não informado)
    ach_fetched_at  TEXT,
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, title_id)
);
CREATE TABLE IF NOT EXISTS xbox_achievements (
    profile_id          INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    title_id            TEXT NOT NULL,
    ach_id              TEXT NOT NULL,
    name                TEXT NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    locked_description  TEXT NOT NULL DEFAULT '',
    icon                TEXT,
    secret              INTEGER NOT NULL DEFAULT 0,
    gamerscore          INTEGER NOT NULL DEFAULT 0,
    rarity              REAL,
    unlocked_at         INTEGER,          -- epoch; NULL = pendente
    sort                INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (profile_id, title_id, ach_id)
);
"#;

/// Colunas adicionadas depois (bancos antigos): tipo do título ("Game", "App"...) e aparelhos em que ele
/// foi jogado ("PC", "XboxSeries"...), usados para listar jogos de PC jogados e não instalados.
pub fn migrate(conn: &Connection) -> AppResult<()> {
    if !crate::db::has_column(conn, "xbox_titles", "kind")? {
        conn.execute_batch("ALTER TABLE xbox_titles ADD COLUMN kind TEXT; ALTER TABLE xbox_titles ADD COLUMN devices TEXT;")?;
    }
    Ok(())
}

/// Aparelhos informados pelo Xbox para o título, separados por vírgula (None = não informado).
fn devices_of(t: &Value) -> Option<String> {
    let d: Vec<String> = t["devices"].as_array()?.iter().filter_map(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    Some(d.join(","))
}

/// Grava o histórico de jogos (mantém a data da última busca de conquistas de cada um).
pub fn store_titles(conn: &Connection, profile_id: i64, titles: &[Value], minutes: &std::collections::HashMap<String, i64>) -> AppResult<usize> {
    let mut n = 0;
    for t in titles {
        let id = text(&t["titleId"]);
        let name = text(&t["name"]);
        if id.is_empty() || name.is_empty() {
            continue;
        }
        let a = &t["achievement"];
        let num = |v: &Value| (!v.is_null()).then(|| int(v));
        let image = Some(text(&t["displayImage"])).filter(|s| s.starts_with("https://"));
        let last = parse_epoch(&text(&t["titleHistory"]["lastTimePlayed"])).filter(|e| *e > 0);
        conn.execute(
            "INSERT INTO xbox_titles(profile_id,title_id,pfn,name,image,ach_current,ach_total,gs_current,gs_total,last_played,minutes,kind,devices,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,datetime('now'))
             ON CONFLICT(profile_id,title_id) DO UPDATE SET pfn=excluded.pfn,name=excluded.name,image=excluded.image,
               ach_current=excluded.ach_current,ach_total=excluded.ach_total,gs_current=excluded.gs_current,gs_total=excluded.gs_total,
               last_played=excluded.last_played,minutes=COALESCE(excluded.minutes,xbox_titles.minutes),
               kind=COALESCE(excluded.kind,xbox_titles.kind),devices=COALESCE(excluded.devices,xbox_titles.devices),updated_at=excluded.updated_at",
            params![
                profile_id, id, Some(text(&t["pfn"])).filter(|s| !s.is_empty()), name, image,
                num(&a["currentAchievements"]), num(&a["totalAchievements"]), num(&a["currentGamerscore"]), num(&a["totalGamerscore"]),
                last, minutes.get(&id), Some(text(&t["type"])).filter(|s| !s.is_empty()), devices_of(t),
            ],
        )?;
        n += 1;
    }
    Ok(n)
}

/// Substitui as conquistas de um jogo e atualiza o resumo dele.
pub fn store_achievements(conn: &Connection, profile_id: i64, title_id: &str, list: &[Value]) -> AppResult<()> {
    conn.execute("DELETE FROM xbox_achievements WHERE profile_id=?1 AND title_id=?2", params![profile_id, title_id])?;
    let (mut got, mut gs_got, mut gs_total) = (0i64, 0i64, 0i64);
    for (i, a) in list.iter().enumerate() {
        let id = text(&a["id"]);
        if id.is_empty() {
            continue;
        }
        let gs: i64 = a["rewards"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["type"].as_str() == Some("Gamerscore"))
            .filter_map(|r| text(&r["value"]).parse::<i64>().ok())
            .sum();
        let unlocked = (a["progressState"].as_str() == Some("Achieved"))
            .then(|| parse_epoch(&text(&a["progression"]["timeUnlocked"])).filter(|e| *e > 0).unwrap_or(0));
        let icon = a["mediaAssets"].as_array().into_iter().flatten().find(|m| m["type"].as_str() == Some("Icon")).map(|m| text(&m["url"]));
        let rarity = a["rarity"]["currentPercentage"].as_f64();
        gs_total += gs;
        if unlocked.is_some() {
            got += 1;
            gs_got += gs;
        }
        conn.execute(
            "INSERT OR REPLACE INTO xbox_achievements(profile_id,title_id,ach_id,name,description,locked_description,icon,secret,gamerscore,rarity,unlocked_at,sort)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                profile_id, title_id, id, text(&a["name"]), text(&a["description"]), text(&a["lockedDescription"]),
                icon.filter(|u| u.starts_with("https://")), a["isSecret"].as_bool().unwrap_or(false) as i64, gs, rarity, unlocked, i as i64,
            ],
        )?;
    }
    conn.execute(
        "UPDATE xbox_titles SET ach_fetched_at=datetime('now'), ach_current=?3, ach_total=?4, gs_current=?5, gs_total=?6
         WHERE profile_id=?1 AND title_id=?2",
        params![profile_id, title_id, got, list.len() as i64, gs_got, gs_total],
    )?;
    Ok(())
}

pub fn set_minutes(conn: &Connection, profile_id: i64, title_id: &str, minutes: i64) -> AppResult<()> {
    conn.execute("UPDATE xbox_titles SET minutes=?3 WHERE profile_id=?1 AND title_id=?2", params![profile_id, title_id, minutes])?;
    Ok(())
}

pub fn clear_profile(conn: &Connection, profile_id: i64) -> AppResult<()> {
    for t in ["xbox_titles", "xbox_achievements"] {
        conn.execute(&format!("DELETE FROM {t} WHERE profile_id=?1"), [profile_id])?;
    }
    Ok(())
}

pub fn title_count(conn: &Connection, profile_id: i64) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM xbox_titles WHERE profile_id=?1", [profile_id], |r| r.get(0))?)
}

/// Família do pacote ("Nome_Editor") de um jogo cadastrado pelo atalho do app Xbox.
fn family_of_exe(exe: &str) -> Option<String> {
    crate::msstore::aumid(exe).and_then(|a| a.split('!').next()).map(|f| f.to_lowercase())
}

/// Título do Xbox ligado ao jogo da biblioteca (família do pacote; senão, nome igual em jogo de PC/Xbox).
pub fn title_for_game(conn: &Connection, profile_id: i64, game_id: i64) -> AppResult<Option<String>> {
    let Some((exe, title, platform)) = conn
        .query_row(
            "SELECT g.executable_path, g.title, g.platform FROM profile_games pg JOIN games g ON g.id=pg.game_id
             WHERE pg.profile_id=?1 AND pg.game_id=?2",
            params![profile_id, game_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    if let Some(fam) = family_of_exe(&exe) {
        let hit: Option<String> = conn
            .query_row("SELECT title_id FROM xbox_titles WHERE profile_id=?1 AND lower(pfn)=?2", params![profile_id, fam], |r| r.get(0))
            .optional()?;
        if hit.is_some() {
            return Ok(hit);
        }
    }
    if !matches!(platform.as_str(), "PC" | "XBOX" | "Xbox 360" | "Xbox Series") {
        return Ok(None);
    }
    let key = norm_title(&title).join(" ");
    if key.is_empty() {
        return Ok(None);
    }
    let mut st = conn.prepare("SELECT title_id, name FROM xbox_titles WHERE profile_id=?1 ORDER BY last_played DESC")?;
    let rows = st.query_map([profile_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, name) = row?;
        if norm_title(&name).join(" ") == key {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// IDs das conquistas já obtidas no cache (antes de uma consulta do aviso em tempo real).
pub fn unlocked_ids(conn: &Connection, profile_id: i64, title_id: &str) -> AppResult<std::collections::HashSet<String>> {
    let mut st = conn.prepare("SELECT ach_id FROM xbox_achievements WHERE profile_id=?1 AND title_id=?2 AND unlocked_at IS NOT NULL")?;
    let ids = st.query_map(params![profile_id, title_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Conquista recém-desbloqueada, para o aviso por cima do jogo.
pub struct FreshUnlock {
    pub name: String,
    pub icon: Option<String>,
    pub gamerscore: i64,
    pub rarity: Option<f64>,
    pub game: String,
}

/// Obtidas agora que não estavam em `before` e foram desbloqueadas a partir de `since` (epoch).
/// Sem data informada (0) também contam: a consulta acabou de vê-las mudar de estado.
pub fn fresh_unlocks(conn: &Connection, profile_id: i64, title_id: &str, before: &std::collections::HashSet<String>, since: i64) -> AppResult<Vec<FreshUnlock>> {
    let mut st = conn.prepare(
        "SELECT a.ach_id, a.name, a.icon, a.gamerscore, a.rarity, t.name FROM xbox_achievements a
         JOIN xbox_titles t ON t.profile_id=a.profile_id AND t.title_id=a.title_id
         WHERE a.profile_id=?1 AND a.title_id=?2 AND a.unlocked_at IS NOT NULL AND (a.unlocked_at=0 OR a.unlocked_at>=?3)
         ORDER BY a.unlocked_at",
    )?;
    let rows = st.query_map(params![profile_id, title_id, since], |r| {
        Ok((r.get::<_, String>(0)?, FreshUnlock { name: r.get(1)?, icon: r.get(2)?, gamerscore: r.get(3)?, rarity: r.get(4)?, game: r.get(5)? }))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, f) = row?;
        if !before.contains(&id) {
            out.push(f);
        }
    }
    Ok(out)
}

/// Busca de conquistas está velha (ou nunca feita)?
pub fn achievements_stale(conn: &Connection, profile_id: i64, title_id: &str, max_minutes: i64) -> AppResult<bool> {
    let fresh: bool = conn
        .query_row(
            "SELECT ach_fetched_at IS NOT NULL AND ach_fetched_at > datetime('now', ?3) FROM xbox_titles WHERE profile_id=?1 AND title_id=?2",
            params![profile_id, title_id, format!("-{max_minutes} minutes")],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false);
    Ok(!fresh)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxAch {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub secret: bool,
    pub gamerscore: i64,
    pub rarity: Option<f64>,
    pub unlocked: bool,
    /// epoch (0 = desbloqueada sem data informada)
    pub unlocked_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XboxGameData {
    pub title_id: String,
    pub name: String,
    pub image: Option<String>,
    pub ach_current: Option<i64>,
    pub ach_total: Option<i64>,
    pub gs_current: Option<i64>,
    pub gs_total: Option<i64>,
    pub minutes: Option<i64>,
    pub last_played: Option<i64>,
    pub fetched_at: Option<String>,
    pub achievements: Vec<XboxAch>,
    /// Falha na atualização (os dados mostrados são do cache).
    pub error: Option<String>,
}

pub fn game_data(conn: &Connection, profile_id: i64, title_id: &str) -> AppResult<Option<XboxGameData>> {
    let Some(mut d) = conn
        .query_row(
            "SELECT name,image,ach_current,ach_total,gs_current,gs_total,minutes,last_played,ach_fetched_at FROM xbox_titles WHERE profile_id=?1 AND title_id=?2",
            params![profile_id, title_id],
            |r| {
                Ok(XboxGameData {
                    title_id: title_id.to_string(),
                    name: r.get(0)?,
                    image: r.get(1)?,
                    ach_current: r.get(2)?,
                    ach_total: r.get(3)?,
                    gs_current: r.get(4)?,
                    gs_total: r.get(5)?,
                    minutes: r.get(6)?,
                    last_played: r.get(7)?,
                    fetched_at: r.get(8)?,
                    achievements: vec![],
                    error: None,
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut st = conn.prepare(
        "SELECT ach_id,name,description,locked_description,icon,secret,gamerscore,rarity,unlocked_at FROM xbox_achievements
         WHERE profile_id=?1 AND title_id=?2 ORDER BY sort",
    )?;
    let rows = st.query_map(params![profile_id, title_id], |r| {
        let unlocked_at: Option<i64> = r.get(8)?;
        let (desc, locked): (String, String) = (r.get(2)?, r.get(3)?);
        Ok(XboxAch {
            id: r.get(0)?,
            name: r.get(1)?,
            // pendente: a descrição "bloqueada" do jogo, quando existe (evita spoiler)
            description: if unlocked_at.is_none() && !locked.is_empty() { locked } else { desc },
            icon: r.get(4)?,
            secret: r.get::<_, i64>(5)? != 0,
            gamerscore: r.get(6)?,
            rarity: r.get(7)?,
            unlocked: unlocked_at.is_some(),
            unlocked_at,
        })
    })?;
    d.achievements = rows.collect::<Result<_, _>>()?;
    Ok(Some(d))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotInstalled {
    pub title_id: String,
    pub name: String,
    pub image: Option<String>,
    pub pfn: String,
    pub last_played: Option<i64>,
    pub minutes: Option<i64>,
}

/// Jogos de PC do histórico da conta que não estão instalados neste PC (`installed` = famílias de pacote
/// instaladas, em minúsculas). Só títulos do tipo jogo, com pacote da Microsoft Store e jogados no PC.
/// Devolve também quantos títulos ainda não têm os aparelhos gravados (cache anterior a esta versão:
/// a próxima sincronização completa).
pub fn not_installed(conn: &Connection, profile_id: i64, installed: &std::collections::HashSet<String>) -> AppResult<(Vec<NotInstalled>, usize)> {
    let mut st = conn.prepare(
        "SELECT title_id, name, image, pfn, last_played, minutes, devices FROM xbox_titles
         WHERE profile_id=?1 AND pfn IS NOT NULL AND trim(pfn)<>'' AND COALESCE(kind,'Game')='Game'
         ORDER BY COALESCE(last_played,0) DESC, name",
    )?;
    let rows = st.query_map([profile_id], |r| {
        Ok((
            NotInstalled { title_id: r.get(0)?, name: r.get(1)?, image: r.get(2)?, pfn: r.get(3)?, last_played: r.get(4)?, minutes: r.get(5)? },
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    let (mut out, mut unknown) = (Vec::new(), 0usize);
    for row in rows {
        let (t, devices) = row?;
        if installed.contains(&t.pfn.to_lowercase()) {
            continue;
        }
        match devices {
            None => unknown += 1,
            Some(d) if d.split(',').any(|x| x.trim().eq_ignore_ascii_case("PC")) => out.push(t),
            Some(_) => {} // jogado só no console
        }
    }
    Ok((out, unknown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE profiles(id INTEGER PRIMARY KEY);
             CREATE TABLE games(id INTEGER PRIMARY KEY, title TEXT, platform TEXT, executable_path TEXT);
             CREATE TABLE profile_games(profile_id INTEGER, game_id INTEGER);
             INSERT INTO profiles VALUES(1);
             INSERT INTO games VALUES(1,'Forza Horizon 6','PC','shell:AppsFolder\\Microsoft.ForteBaseGame_8wekyb3d8bbwe!Forzahorizon6'),
                                     (2,'Halo: Infinite','PC','C:/halo.exe'),(3,'Halo Infinite','Playstation 2','x');
             INSERT INTO profile_games VALUES(1,1),(1,2),(1,3);",
        )
        .unwrap();
        c.execute_batch(SCHEMA).unwrap();
        migrate(&c).unwrap();
        c
    }

    #[test]
    fn links_by_package_family_then_title() {
        let c = db();
        let titles = vec![
            json!({ "titleId": "111", "name": "FH6 (nome diferente)", "type": "Game", "pfn": "Microsoft.ForteBaseGame_8wekyb3d8bbwe",
                    "achievement": { "currentAchievements": 3, "totalAchievements": 50, "currentGamerscore": 40, "totalGamerscore": 1000 },
                    "titleHistory": { "lastTimePlayed": "2026-11-01T10:00:00.000Z" } }),
            json!({ "titleId": "222", "name": "Halo Infinite", "type": "Game" }),
        ];
        assert_eq!(store_titles(&c, 1, &titles, &HashMap::from([("111".to_string(), 600)])).unwrap(), 2);
        assert_eq!(title_for_game(&c, 1, 1).unwrap().as_deref(), Some("111"), "pela família do pacote");
        assert_eq!(title_for_game(&c, 1, 2).unwrap().as_deref(), Some("222"), "pelo nome (pontuação ignorada)");
        assert_eq!(title_for_game(&c, 1, 3).unwrap(), None, "outra plataforma não liga pelo nome");
        let d = game_data(&c, 1, "111").unwrap().unwrap();
        assert_eq!((d.minutes, d.ach_current, d.gs_total), (Some(600), Some(3), Some(1000)));
        assert!(achievements_stale(&c, 1, "111", 30).unwrap());
    }

    #[test]
    fn played_on_pc_but_not_installed() {
        let c = db();
        let titles = vec![
            json!({ "titleId": "1", "name": "Instalado", "type": "Game", "pfn": "A.Jogo_8wekyb3d8bbwe", "devices": ["PC"] }),
            json!({ "titleId": "2", "name": "Só no PC", "type": "Game", "pfn": "B.Jogo_8wekyb3d8bbwe", "devices": ["PC", "XboxSeries"],
                    "titleHistory": { "lastTimePlayed": "2026-11-01T10:00:00.000Z" } }),
            json!({ "titleId": "3", "name": "Só console", "type": "Game", "pfn": "C.Jogo_8wekyb3d8bbwe", "devices": ["XboxSeries"] }),
            json!({ "titleId": "4", "name": "App", "type": "App", "pfn": "D.App_8wekyb3d8bbwe", "devices": ["PC"] }),
            json!({ "titleId": "5", "name": "Sem pacote", "type": "Game", "devices": ["PC"] }),
            json!({ "titleId": "6", "name": "Cache antigo", "type": "Game", "pfn": "F.Jogo_8wekyb3d8bbwe" }),
        ];
        store_titles(&c, 1, &titles, &HashMap::new()).unwrap();
        let installed = std::collections::HashSet::from([String::from("a.jogo_8wekyb3d8bbwe")]);
        let (list, unknown) = not_installed(&c, 1, &installed).unwrap();
        assert_eq!(list.iter().map(|t| t.title_id.as_str()).collect::<Vec<_>>(), vec!["2"]);
        assert_eq!(unknown, 1, "sem aparelhos informados: aguarda a sincronização");
        // nova sincronização sem o campo não apaga os aparelhos já gravados
        store_titles(&c, 1, &[json!({ "titleId": "2", "name": "Só no PC", "pfn": "B.Jogo_8wekyb3d8bbwe" })], &HashMap::new()).unwrap();
        assert_eq!(not_installed(&c, 1, &installed).unwrap().0.len(), 1);
    }

    #[test]
    fn achievements_summary_and_spoilers() {
        let c = db();
        store_titles(&c, 1, &[json!({ "titleId": "111", "name": "Jogo" })], &HashMap::new()).unwrap();
        let list = vec![
            json!({ "id": "1", "name": "Primeira", "description": "Feita", "lockedDescription": "Faça algo", "progressState": "Achieved",
                    "progression": { "timeUnlocked": "2026-10-10T12:00:00.0000000Z" }, "rewards": [{ "type": "Gamerscore", "value": "10" }],
                    "mediaAssets": [{ "type": "Icon", "url": "https://images-eds-ssl.xboxlive.com/a.png" }], "rarity": { "currentPercentage": 52.5 } }),
            json!({ "id": "2", "name": "Segunda", "description": "Revelação", "lockedDescription": "Segredo", "progressState": "NotStarted",
                    "progression": { "timeUnlocked": "0001-01-01T00:00:00.0000000Z" }, "rewards": [{ "type": "Gamerscore", "value": "20" }], "isSecret": true }),
        ];
        store_achievements(&c, 1, "111", &list).unwrap();
        let d = game_data(&c, 1, "111").unwrap().unwrap();
        assert_eq!((d.ach_current, d.ach_total, d.gs_current, d.gs_total), (Some(1), Some(2), Some(10), Some(30)));
        assert!(d.achievements[0].unlocked && d.achievements[0].unlocked_at.unwrap() > 0);
        assert_eq!(d.achievements[1].description, "Segredo", "pendente mostra a descrição bloqueada");
        assert!(d.achievements[1].secret && !d.achievements[1].unlocked);
        assert!(!achievements_stale(&c, 1, "111", 30).unwrap());

        // aviso em tempo real: só o que não estava obtido antes e é recente
        let at = d.achievements[0].unlocked_at.unwrap();
        let fresh = fresh_unlocks(&c, 1, "111", &Default::default(), at - 60).unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!((fresh[0].gamerscore, fresh[0].game.as_str()), (10, "Jogo"));
        assert!(fresh_unlocks(&c, 1, "111", &Default::default(), at + 60).unwrap().is_empty(), "antiga não avisa");
        assert!(fresh_unlocks(&c, 1, "111", &unlocked_ids(&c, 1, "111").unwrap(), 0).unwrap().is_empty(), "já conhecida não avisa");
    }
}
