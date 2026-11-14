//! Troféus de PS Vita (Vita3K) e PS3 (RPCS3), lidos dos arquivos que os próprios emuladores gravam.
//!
//! Os troféus vêm do jogo: cada jogo traz a lista oficial (TROPHY.TRP), e o emulador emula o sistema de
//! troféus do console, gravando a lista instalada (`TROP*.SFM` + ícones) e o progresso (`TROPUSR.DAT`):
//! - Vita3K: `<pref-path>/ux0/user/<user-id>/trophy/conf/<NPCOMMID>/` e `.../trophy/data/<NPCOMMID>/TROPUSR.DAT`
//!   (`pref-path` e `user-id` vêm do `config.yml` do Vita3K);
//! - RPCS3: `<dev_hdd0>/home/<usuário>/trophy/<NPCOMMID>/` (lista, ícones e progresso na mesma pasta;
//!   `dev_hdd0` vem do `vfs.yml`, padrão: ao lado do `rpcs3.exe`).
//! Nada é enviado à internet nem alterado: o NEXSTAGE só lê. Os ícones são copiados para um cache do app
//! (a página do jogo só exibe imagens da pasta do NEXSTAGE).
//!
//! Qual conjunto de troféus é de qual jogo (nesta ordem):
//! 1. vínculo aprendido (`trophy_links`): o conjunto que mudou enquanto o jogo, aberto pelo NEXSTAGE, rodava;
//! 2. Vita3K: o ID do jogo (ex.: `-r PCSA00011`) leva a `ux0/app/<ID>/sce_sys/trophy/<NPCOMMID>`;
//!    RPCS3: a pasta `PS3_GAME/TROPDIR/<NPCOMMID>` dentro da ISO (ou da pasta do jogo);
//! 3. nome do jogo igual ao nome do conjunto de troféus.

pub mod parse;
pub mod watch;

use crate::db::Db;
use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Emu {
    Vita3k,
    Rpcs3,
}

impl Emu {
    pub fn label(self) -> &'static str {
        match self {
            Emu::Vita3k => "Vita3K",
            Emu::Rpcs3 => "RPCS3",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Emu::Vita3k => "vita3k",
            Emu::Rpcs3 => "rpcs3",
        }
    }
    pub fn of(name: &str, exe: &str) -> Option<Emu> {
        match crate::emu_fullscreen::detect(name, exe)? {
            crate::emu_fullscreen::Kind::Vita3k => Some(Emu::Vita3k),
            crate::emu_fullscreen::Kind::Rpcs3 => Some(Emu::Rpcs3),
            _ => None,
        }
    }
}

/// Onde um emulador cadastrado guarda os troféus.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Root {
    pub emu: Emu,
    /// Vita3K: `pref-path`; RPCS3: `dev_hdd0`.
    pub base: PathBuf,
}

/// Lê `chave: valor` de um YAML simples (só o primeiro nível que importa aqui).
fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let t = l.trim_start();
        t.strip_prefix(key).and_then(|r| r.strip_prefix(':')).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
    })
}

impl Root {
    pub fn from_emulator(emu: Emu, exe: &Path) -> Option<Root> {
        let dir = exe.parent()?;
        let base = match emu {
            Emu::Vita3k => {
                // config.yml ao lado do executável (portátil) ou em %APPDATA%\Vita3K
                let appdata = std::env::var_os("APPDATA").map(PathBuf::from).map(|a| a.join("Vita3K"));
                let cfgs = [Some(dir.join("config.yml")), appdata.as_ref().map(|a| a.join("config.yml"))];
                let pref = cfgs.iter().flatten().find_map(|c| std::fs::read_to_string(c).ok().and_then(|t| yaml_value(&t, "pref-path")));
                match pref.filter(|p| !p.is_empty()) {
                    Some(p) => PathBuf::from(p),
                    None if dir.join("ux0").is_dir() => dir.to_path_buf(),
                    None => appdata.map(|a| a.join("Vita3K")).unwrap_or_else(|| dir.to_path_buf()),
                }
            }
            Emu::Rpcs3 => {
                let cfgs = [dir.join("config").join("vfs.yml"), dir.join("vfs.yml")];
                let hdd = cfgs.iter().find_map(|c| std::fs::read_to_string(c).ok().and_then(|t| yaml_value(&t, "/dev_hdd0/")));
                match hdd.filter(|p| !p.is_empty()) {
                    Some(p) => PathBuf::from(p.replace("$(EmulatorDir)", &format!("{}/", dir.display()))),
                    None => dir.join("dev_hdd0"),
                }
            }
        };
        Some(Root { emu, base })
    }

    /// Pastas que contêm uma subpasta por conjunto de troféus (com a lista e os ícones).
    pub fn conf_dirs(&self) -> Vec<PathBuf> {
        match self.emu {
            // todos os usuários do emulador, o padrão primeiro (Vita3K "00", RPCS3 "00000001")
            Emu::Vita3k => users(&self.base.join("ux0").join("user"), "00").into_iter().map(|u| u.join("trophy").join("conf")).collect(),
            Emu::Rpcs3 => users(&self.base.join("home"), "00000001").into_iter().map(|u| u.join("trophy")).collect(),
        }
    }

    /// Pasta do progresso (`TROPUSR.DAT`) de um conjunto, a partir da pasta da lista.
    pub fn usr_file(&self, conf_dir: &Path) -> PathBuf {
        match self.emu {
            // .../trophy/conf/<ID> → .../trophy/data/<ID>/TROPUSR.DAT
            Emu::Vita3k => {
                let id = conf_dir.file_name().unwrap_or_default();
                conf_dir.parent().and_then(|c| c.parent()).map(|t| t.join("data").join(id).join("TROPUSR.DAT")).unwrap_or_default()
            }
            Emu::Rpcs3 => conf_dir.join("TROPUSR.DAT"),
        }
    }

    /// Pasta da lista de um conjunto, se o emulador já a instalou (o jogo foi aberto ao menos uma vez).
    pub fn find_conf(&self, comm_id: &str) -> Option<PathBuf> {
        self.conf_dirs().into_iter().map(|d| d.join(comm_id)).find(|p| p.is_dir())
    }

    pub fn unlocks(&self, conf_dir: &Path) -> Option<parse::Unlocks> {
        self.unlocks_file(&self.usr_file(conf_dir))
    }

    pub fn unlocks_file(&self, usr: &Path) -> Option<parse::Unlocks> {
        let b = std::fs::read(usr).ok()?;
        match self.emu {
            Emu::Vita3k => parse::vita_usr(&b),
            Emu::Rpcs3 => parse::ps3_usr(&b),
        }
    }
}

fn users(dir: &Path, default: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
    v.sort_by_key(|p| (p.file_name().and_then(|n| n.to_str()) != Some(default), p.clone()));
    v
}

/// Lista de troféus no idioma do app (com fallback), lendo os `.SFM` da pasta do conjunto.
pub fn load_conf(dir: &Path, lang: &str) -> Option<parse::Conf> {
    let mut names: Vec<String> = parse::lang_code(lang).iter().map(|c| format!("TROP_{c:02}.SFM")).collect();
    names.push("TROP.SFM".into());
    names.push("TROPCONF.SFM".into());
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut others: Vec<String> = rd
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| n.starts_with("TROP_") && n.ends_with(".SFM"))
            .collect();
        others.sort();
        names.extend(others);
    }
    let mut fallback = None;
    for n in names {
        let Ok(text) = std::fs::read_to_string(dir.join(&n)) else { continue };
        let c = parse::conf(&text);
        if c.trophies.iter().any(|t| !t.name.is_empty()) {
            return Some(c);
        }
        if fallback.is_none() && !c.trophies.is_empty() {
            fallback = Some(c);
        }
    }
    fallback
}

/// Só o nome do conjunto (para comparar com o nome do jogo).
fn set_title(dir: &Path) -> Option<String> {
    for n in ["TROP.SFM", "TROPCONF.SFM"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(n)) {
            let c = parse::conf(&t);
            if !c.title.is_empty() {
                return Some(c.title);
            }
        }
    }
    None
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Mesmo jogo pelo nome? Igual, ou um contém o outro (ex.: "Diablo III: Reaper of Souls" e
/// "Diablo III: Reaper of Souls Ultimate Evil Edition"), desde que o menor seja longo o bastante.
pub fn title_match(game: &str, set: &str) -> usize {
    let (g, s) = (norm(game), norm(set));
    if g.is_empty() || s.is_empty() {
        return 0;
    }
    if g == s {
        return 1000;
    }
    let short = g.len().min(s.len());
    if short >= 8 && (g.contains(&s) || s.contains(&g)) {
        return short;
    }
    0
}

/// ID de jogo do PS Vita/PS3 (4 letras + 5 números, ex.: PCSA00011) no texto.
fn title_id(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(8)).find_map(|i| {
        let w = &b[i..i + 9];
        let ok = w[..4].iter().all(|c| c.is_ascii_uppercase()) && w[4..].iter().all(|c| c.is_ascii_digit());
        let edge_l = i == 0 || !b[i - 1].is_ascii_alphanumeric();
        let edge_r = i + 9 == b.len() || !b[i + 9].is_ascii_alphanumeric();
        (ok && edge_l && edge_r).then(|| String::from_utf8_lossy(w).into_owned())
    })
}

fn first_subdir(p: &Path) -> Option<String> {
    let mut v: Vec<String> = std::fs::read_dir(p).ok()?.flatten().filter(|e| e.path().is_dir()).filter_map(|e| e.file_name().to_str().map(String::from)).collect();
    v.sort();
    v.into_iter().next()
}

/// RPCS3: `PS3_GAME/TROPDIR/<ID>` dentro da ISO.
fn ps3_iso_comm(iso: &Path) -> Option<String> {
    use crate::detect::{layout, list_dir, sector, u32le};
    let mut f = std::fs::File::open(iso).ok()?;
    let l = layout(&mut f)?;
    let pvd = sector(&mut f, &l, 16)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let root = list_dir(&mut f, &l, u32le(&pvd, 158) as u64, u32le(&pvd, 166) as usize);
    let game = root.iter().find(|e| e.0 == "PS3_GAME")?;
    let gl = list_dir(&mut f, &l, game.1 as u64, game.2 as usize);
    let trop = gl.iter().find(|e| e.0 == "TROPDIR")?;
    let mut ids: Vec<String> = list_dir(&mut f, &l, trop.1 as u64, trop.2 as usize).into_iter().map(|e| e.0).filter(|n| n.starts_with("NP")).collect();
    ids.sort();
    ids.into_iter().next()
}

/// RPCS3: jogo em pasta (EBOOT.BIN): procura `TROPDIR` subindo a partir do arquivo.
fn ps3_folder_comm(file: &Path) -> Option<String> {
    let mut cur = file.parent();
    for _ in 0..4 {
        let d = cur?;
        for cand in [d.join("TROPDIR"), d.join("PS3_GAME").join("TROPDIR")] {
            if let Some(id) = first_subdir(&cand) {
                return Some(id);
            }
        }
        cur = d.parent();
    }
    None
}

/// Conjunto de troféus de um jogo.
pub struct Resolved {
    pub root: Root,
    /// `None`: o jogo é desse emulador, mas o conjunto não foi identificado.
    pub comm_id: Option<String>,
}

/// Emulador (Vita3K/RPCS3) de um jogo e o conjunto de troféus dele.
pub fn resolve(conn: &Connection, game_id: i64) -> AppResult<Option<Resolved>> {
    let row = conn
        .query_row(
            "SELECT g.title, g.executable_path, g.launch_args, COALESCE(g.iso_path, g.rom_path), e.name, e.exe_path, e.args, tl.comm_id
             FROM games g LEFT JOIN emulators e ON e.id = g.emulator_id LEFT JOIN trophy_links tl ON tl.game_id = g.id
             WHERE g.id = ?1",
            [game_id],
            |r| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, Option<String>>(7)?,
            )),
        )
        .optional()?;
    let Some((title, exe, args, file, emu_name, emu_exe, emu_args, link)) = row else { return Ok(None) };
    // emulador associado; sem associação, o executável do próprio jogo pode ser o emulador
    let (emu, emu_path) = match (emu_name.as_deref(), emu_exe.as_deref()) {
        (Some(n), Some(p)) => match Emu::of(n, p) {
            Some(e) => (e, p.to_string()),
            None => return Ok(None),
        },
        _ => match Emu::of("", &exe) {
            Some(e) => (e, exe.clone()),
            None => return Ok(None),
        },
    };
    let Some(root) = Root::from_emulator(emu, Path::new(&emu_path)) else { return Ok(None) };
    if let Some(id) = link {
        return Ok(Some(Resolved { root, comm_id: Some(id) }));
    }
    let specific = match emu {
        Emu::Vita3k => {
            let text = format!("{args} {} {} {exe}", emu_args.unwrap_or_default(), file.as_deref().unwrap_or(""));
            title_id(&text).and_then(|tid| first_subdir(&root.base.join("ux0").join("app").join(tid).join("sce_sys").join("trophy")))
        }
        Emu::Rpcs3 => {
            let f = file.as_deref().filter(|f| !f.is_empty()).map(PathBuf::from).or_else(|| (!exe.is_empty() && Path::new(&exe) != Path::new(&emu_path)).then(|| PathBuf::from(&exe)));
            f.and_then(|f| {
                if f.extension().is_some_and(|x| x.eq_ignore_ascii_case("iso")) { ps3_iso_comm(&f) } else { ps3_folder_comm(&f) }
            })
        }
    };
    if specific.is_some() {
        return Ok(Some(Resolved { root, comm_id: specific }));
    }
    // pelo nome: o conjunto instalado com o nome mais parecido
    let mut best: Option<(usize, String)> = None;
    for d in root.conf_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Some(id) = e.file_name().to_str().map(String::from) else { continue };
            let score = set_title(&e.path()).map(|t| title_match(&title, &t)).unwrap_or(0);
            if score > 0 && best.as_ref().map_or(true, |b| score > b.0) {
                best = Some((score, id));
            }
        }
    }
    Ok(Some(Resolved { root, comm_id: best.map(|b| b.1) }))
}

/// Guarda o vínculo aprendido jogo → conjunto (sobrepõe a detecção automática).
pub fn learn(conn: &Connection, game_id: i64, comm_id: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO trophy_links(game_id, comm_id) VALUES(?1, ?2) ON CONFLICT(game_id) DO UPDATE SET comm_id=excluded.comm_id",
        params![game_id, comm_id],
    )?;
    Ok(())
}

fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Copia um ícone para o cache do app (`trophy-icons/<emulador>-<ID>/`) e devolve o caminho da cópia.
pub fn cached_icon(app: &AppHandle, root: &Root, conf_dir: &Path, comm_id: &str, file: &str) -> Option<String> {
    if !safe_id(comm_id) {
        return None;
    }
    let src = conf_dir.join(file);
    let meta = std::fs::metadata(&src).ok()?;
    let dir = app.path().app_data_dir().ok()?.join("trophy-icons").join(format!("{}-{comm_id}", root.emu.key()));
    let dst = dir.join(file);
    let fresh = std::fs::metadata(&dst).map(|m| m.len() == meta.len()).unwrap_or(false);
    if !fresh {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::copy(&src, &dst).ok()?;
    }
    Some(dst.to_string_lossy().into_owned())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Trophy {
    pub id: u32,
    pub name: String,
    pub detail: String,
    /// "P" | "G" | "S" | "B"
    pub grade: String,
    pub hidden: bool,
    pub unlocked: bool,
    /// Segundos Unix (quando o emulador grava a data).
    pub unlocked_at: Option<i64>,
    pub icon: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTrophies {
    /// "Vita3K" | "RPCS3"
    pub emulator: &'static str,
    /// "ok" | "notInstalled" (conjunto conhecido, mas o jogo ainda não foi aberto no emulador) | "notFound"
    pub status: &'static str,
    pub comm_id: Option<String>,
    pub title: Option<String>,
    pub icon: Option<String>,
    pub total: usize,
    pub unlocked: usize,
    pub trophies: Vec<Trophy>,
}

fn lang(app: &AppHandle) -> String {
    crate::overlay::setting(app, "language").unwrap_or_else(|| "pt-BR".into())
}

/// Monta os troféus de um conjunto (lista + progresso + ícones em cache).
pub fn build(app: &AppHandle, root: &Root, comm_id: &str) -> Option<(parse::Conf, Vec<Trophy>, Option<String>)> {
    let dir = root.find_conf(comm_id)?;
    let conf = load_conf(&dir, &lang(app))?;
    let got = root.unlocks(&dir).unwrap_or_default();
    let list = conf
        .trophies
        .iter()
        .map(|d| Trophy {
            id: d.id,
            name: d.name.clone(),
            detail: d.detail.clone(),
            grade: d.grade.clone(),
            hidden: d.hidden,
            unlocked: got.contains_key(&d.id),
            unlocked_at: got.get(&d.id).copied().flatten(),
            icon: cached_icon(app, root, &dir, comm_id, &format!("TROP{:03}.PNG", d.id)),
        })
        .collect();
    let icon = cached_icon(app, root, &dir, comm_id, "ICON0.PNG");
    Some((conf, list, icon))
}

/// Troféus de um jogo para a página do jogo. `None`: o jogo não é de Vita3K/RPCS3.
#[tauri::command]
pub fn game_trophies(app: AppHandle, db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<Option<GameTrophies>> {
    let resolved = {
        let conn = db.conn()?;
        crate::commands::games::ensure_owned(&conn, profile_id, game_id)?;
        resolve(&conn, game_id)?
    };
    let Some(Resolved { root, comm_id }) = resolved else { return Ok(None) };
    let empty = |status, comm_id| GameTrophies { emulator: root.emu.label(), status, comm_id, title: None, icon: None, total: 0, unlocked: 0, trophies: vec![] };
    let Some(id) = comm_id else { return Ok(Some(empty("notFound", None))) };
    let Some((conf, trophies, icon)) = build(&app, &root, &id) else { return Ok(Some(empty("notInstalled", Some(id)))) };
    Ok(Some(GameTrophies {
        emulator: root.emu.label(),
        status: "ok",
        comm_id: Some(id),
        title: Some(conf.title).filter(|t| !t.is_empty()),
        icon,
        total: trophies.len(),
        unlocked: trophies.iter().filter(|t| t.unlocked).count(),
        trophies,
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrophySetSummary {
    pub emulator: &'static str,
    pub comm_id: String,
    pub title: String,
    pub icon: Option<String>,
    pub total: usize,
    pub unlocked: usize,
    /// Último desbloqueio (segundos Unix).
    pub last_unlock_at: Option<i64>,
    /// Jogo da biblioteca deste perfil ligado ao conjunto.
    pub library_game_id: Option<i64>,
    pub trophies: Vec<Trophy>,
}

/// Aba Conquistas → Troféus: todos os conjuntos que o Vita3K/RPCS3 cadastrados já instalaram (jogos
/// abertos ao menos uma vez no emulador), com os troféus de cada um.
#[tauri::command]
pub fn trophy_overview(app: AppHandle, db: State<Db>, profile_id: i64) -> AppResult<Vec<TrophySetSummary>> {
    let (roots, links) = {
        let conn = db.conn()?;
        let mut st = conn.prepare("SELECT name, exe_path FROM emulators")?;
        let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        let mut roots: Vec<Root> = Vec::new();
        for (n, e) in rows {
            if let Some(r) = Emu::of(&n, &e).and_then(|k| Root::from_emulator(k, Path::new(&e))) {
                if !roots.contains(&r) {
                    roots.push(r);
                }
            }
        }
        // jogos da biblioteca deste perfil → conjunto
        let mut st = conn.prepare("SELECT game_id FROM profile_games WHERE profile_id=?1")?;
        let ids: Vec<i64> = st.query_map([profile_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut links: Vec<(Emu, String, i64)> = Vec::new();
        for g in ids {
            if let Ok(Some(Resolved { root, comm_id: Some(c) })) = resolve(&conn, g) {
                links.push((root.emu, c, g));
            }
        }
        (roots, links)
    };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in &roots {
        for d in root.conf_dirs() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let Some(id) = e.file_name().to_str().map(String::from) else { continue };
                if !e.path().is_dir() || !seen.insert((root.emu, id.clone())) {
                    continue;
                }
                let Some((conf, trophies, icon)) = build(&app, root, &id) else { continue };
                if trophies.is_empty() {
                    continue;
                }
                out.push(TrophySetSummary {
                    emulator: root.emu.label(),
                    title: if conf.title.is_empty() { id.clone() } else { conf.title },
                    library_game_id: links.iter().find(|l| l.0 == root.emu && l.1 == id).map(|l| l.2),
                    comm_id: id,
                    icon,
                    total: trophies.len(),
                    unlocked: trophies.iter().filter(|t| t.unlocked).count(),
                    last_unlock_at: trophies.iter().filter_map(|t| t.unlocked_at).max(),
                    trophies,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_title_ids() {
        assert_eq!(title_id(r#""C:\Vita3K\Vita3K.exe" -r PCSA00011"#).as_deref(), Some("PCSA00011"));
        assert_eq!(title_id("D:/roms/BLUS30001/PS3_GAME/USRDIR/EBOOT.BIN").as_deref(), Some("BLUS30001"));
        assert_eq!(title_id("XPCSA000111"), None);
        assert_eq!(title_id("sem id"), None);
    }

    #[test]
    fn matches_titles() {
        assert_eq!(title_match("Deadpool", "Deadpool"), 1000);
        assert!(title_match("Diablo III: Reaper of Souls Ultimate Evil Edition", "Diablo III: Reaper of Souls") > 0);
        assert_eq!(title_match("Doom", "Doom II"), 0); // curto demais para "contém"
        assert_eq!(title_match("Gran Turismo 6", "Gravity Rush"), 0);
    }

    #[test]
    fn reads_yaml_values() {
        let y = "sys-lang: 17\npref-path: C:\\Users\\X\\Vita3K\\\nuser-id: 00\n";
        assert_eq!(yaml_value(y, "pref-path").as_deref(), Some("C:\\Users\\X\\Vita3K\\"));
        assert_eq!(yaml_value(y, "user-id").as_deref(), Some("00"));
        let v = "/dev_hdd0/: $(EmulatorDir)dev_hdd0/\n/dev_hdd1/: x\n";
        assert_eq!(yaml_value(v, "/dev_hdd0/").as_deref(), Some("$(EmulatorDir)dev_hdd0/"));
    }
}

