//! Importação automática por pasta: varredura em segundo plano, pré-visualização, importação e
//! gerenciamento das pastas. Somente LEITURA: os arquivos nunca são copiados, movidos, modificados
//! nem executados; só o caminho real é registrado no banco.

use super::emulators::{load_all, Emulator};
use super::games::{canonical_platform, PLATFORMS};
use crate::db::Db;
use crate::detect;
use crate::error::{AppError, AppResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
pub struct ScanState {
    pub running: AtomicBool,
    pub cancel: AtomicBool,
}

const EXTS: [&str; 8] = ["iso", "bin", "cue", "exe", "nsp", "nup", "pkg", "vpk"];
const SKIP_DIRS: [&str; 10] = [
    "$recycle.bin", "system volume information", "windows", "node_modules", ".git", "_commonredist",
    "redist", "redistributables", "directx", "vcredist",
];
/// Executáveis que não são o jogo (instaladores, desinstaladores, redistribuíveis, relatórios de falha...).
const BAD_EXE: [&str; 14] = [
    "unins", "uninstall", "setup", "install", "vcredist", "vc_redist", "dxsetup", "dotnet", "crashhandler",
    "crashreport", "redist", "oalinst", "physx", "dxwebsetup",
];

/// Chave normalizada para comparar caminhos (minúsculas, barras "/", sem prefixo `\\?\`).
pub fn norm_key(p: &str) -> String {
    let s = p.replace('\\', "/").to_lowercase();
    s.trim_start_matches("//?/").trim_end_matches('/').to_string()
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanItem {
    pub path: String,
    pub name: String,
    pub ext: String,
    /// windows | disc | switch | pkg | vpk
    pub kind: String,
    pub platform: Option<String>,
    /// new | secondary | duplicate | invalid | unrecognized
    pub status: String,
    pub note: String,
    pub size: u64,
    /// Emuladores cadastrados compatíveis (plataforma E formato).
    pub compatible: Vec<i64>,
    pub assigned: Option<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    phase: &'static str,
    scanned: u64,
    found: usize,
    current: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub game_id: i64,
    pub title: String,
    pub path: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Done {
    folder: String,
    folder_id: Option<i64>,
    items: Vec<ScanItem>,
    scanned: u64,
    ignored: u64,
    errors: Vec<String>,
    cancelled: bool,
    missing: Vec<Missing>,
}

struct Cand {
    path: PathBuf,
    ext: String,
    size: u64,
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().replace('_', " ")).unwrap_or_default().trim().to_string()
}

fn compatible(emus: &[Emulator], platform: &str, ext: &str) -> Vec<i64> {
    let disc_fmt = matches!(ext, "bin" | "cue");
    emus.iter()
        .filter(|e| e.platforms.iter().any(|p| p == platform))
        .filter(|e| e.formats.iter().any(|f| f == ext || (disc_fmt && (f == "bin" || f == "cue"))))
        .map(|e| e.id)
        .collect()
}

fn walk(root: &Path, cancel: &AtomicBool, app: &AppHandle) -> (Vec<Cand>, u64, Vec<String>) {
    let mut cands = Vec::new();
    let mut errors = Vec::new();
    let mut scanned = 0u64;
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut last = Instant::now();
    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                if errors.len() < 20 { errors.push(format!("{}: {e}", dir.display())); }
                continue;
            }
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_symlink() {
                continue; // não segue atalhos simbólicos (evita ciclos)
            }
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if depth < 40 && !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push((path, depth + 1));
                }
            } else if ft.is_file() {
                scanned += 1;
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if EXTS.contains(&ext.as_str()) {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    cands.push(Cand { path, ext, size });
                }
            }
            if last.elapsed() > Duration::from_millis(150) {
                last = Instant::now();
                let _ = app.emit("scan-progress", Progress { phase: "walking", scanned, found: cands.len(), current: dir.to_string_lossy().to_string() });
            }
        }
    }
    (cands, scanned, errors)
}

fn item(c: &Cand, kind: &str, platform: Option<&str>, status: &str, note: String) -> ScanItem {
    ScanItem {
        path: c.path.to_string_lossy().to_string(),
        name: stem(&c.path),
        ext: c.ext.clone(),
        kind: kind.into(),
        platform: platform.map(String::from),
        status: status.into(),
        note,
        size: c.size,
        compatible: vec![],
        assigned: None,
    }
}

/// Analisa os candidatos (lê só os cabeçalhos necessários) e monta a lista de resultados.
fn analyze(cands: Vec<Cand>, library: &HashSet<String>, emus: &[Emulator], cancel: &AtomicBool, app: &AppHandle) -> (Vec<ScanItem>, u64) {
    let mut items: Vec<ScanItem> = Vec::new();
    let mut ignored = 0u64;

    // .cue -> .bin referenciados (o .cue é o jogo; o .bin associado não vira outro item)
    let mut bin_of_cue: HashMap<String, String> = HashMap::new();
    let mut covered: HashSet<String> = HashSet::new();
    for c in cands.iter().filter(|c| c.ext == "cue") {
        let mut txt = String::new();
        let _ = File::open(&c.path).and_then(|f| f.take(256 * 1024).read_to_string(&mut txt));
        let dir = c.path.parent().unwrap_or(Path::new(""));
        let refs: Vec<PathBuf> = detect::cue_files(&txt).into_iter().map(|n| dir.join(n)).collect();
        for r in &refs {
            covered.insert(norm_key(&r.to_string_lossy()));
        }
        if let Some(first) = refs.first() {
            bin_of_cue.insert(norm_key(&c.path.to_string_lossy()), first.to_string_lossy().to_string());
        }
    }

    // executáveis agrupados por pasta para sugerir só o principal
    let mut exes_by_dir: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let total = cands.len();
    for (i, c) in cands.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if i % 25 == 0 {
            let _ = app.emit("scan-progress", Progress { phase: "analyzing", scanned: i as u64, found: total, current: c.path.to_string_lossy().to_string() });
        }
        let key = norm_key(&c.path.to_string_lossy());
        match c.ext.as_str() {
            "exe" => {
                let low = stem(&c.path).to_lowercase();
                if BAD_EXE.iter().any(|b| low.contains(b)) {
                    ignored += 1;
                    continue;
                }
                let pe = File::open(&c.path).map_err(|_| "inacessível").and_then(|mut f| detect::detect_pe(&mut f));
                match pe {
                    Ok(info) => {
                        let mut it = item(c, "windows", Some("PC"), "new", format!("Executável de jogo do Windows ({}{})", info.arch, if info.gui { "" } else { ", modo console" }));
                        if !info.gui { it.status = "secondary".into(); }
                        items.push(it);
                        exes_by_dir.entry(c.path.parent().unwrap_or(Path::new("")).to_path_buf()).or_default().push(items.len() - 1);
                    }
                    Err(e) => items.push(item(c, "windows", None, "invalid", format!("Executável inválido: {e}."))),
                }
            }
            "iso" => match File::open(&c.path) {
                Ok(mut f) => {
                    let d = detect::detect_disc(&mut f);
                    match d.platform {
                        Some(p) => items.push(item(c, "disc", Some(p), "new", d.note)),
                        None => items.push(item(c, "disc", None, "unrecognized", d.note)),
                    }
                }
                Err(e) => items.push(item(c, "disc", None, "invalid", format!("Arquivo inacessível: {e}."))),
            },
            "cue" => {
                let bin = bin_of_cue.get(&key).cloned();
                match bin.as_deref().map(|b| File::open(b)) {
                    Some(Ok(mut f)) => {
                        let d = detect::detect_disc(&mut f);
                        match d.platform {
                            Some(p) => items.push(item(c, "disc", Some(p), "new", format!("{} (.cue + .bin)", d.note))),
                            None => items.push(item(c, "disc", None, "unrecognized", d.note)),
                        }
                    }
                    Some(Err(_)) | None => items.push(item(c, "disc", None, "invalid", "O .cue aponta para um .bin que não foi encontrado.".into())),
                }
            }
            "bin" => {
                if covered.contains(&key) {
                    ignored += 1; // faz parte de um .cue: não duplica
                    continue;
                }
                match File::open(&c.path) {
                    Ok(mut f) => {
                        let d = detect::detect_disc(&mut f);
                        match d.platform {
                            Some(p) => items.push(item(c, "disc", Some(p), "new", format!("{} (.bin sem .cue)", d.note))),
                            None => items.push(item(c, "disc", None, "unrecognized", format!("{} (.bin sem .cue)", d.note))),
                        }
                    }
                    Err(e) => items.push(item(c, "disc", None, "invalid", format!("Arquivo inacessível: {e}."))),
                }
            }
            "nup" => match detect::detect_nup(&c.path) {
                Ok(d) => match d.platform {
                    Some(p) => items.push(item(c, "switch", Some(p), "new", d.note)),
                    None => items.push(item(c, "switch", None, "invalid", d.note)),
                },
                Err(e) => items.push(item(c, "switch", None, "invalid", format!("Arquivo inacessível: {e}."))),
            },
            "nsp" | "pkg" | "vpk" => match File::open(&c.path) {
                Ok(mut f) => {
                    let d = match c.ext.as_str() { "nsp" => detect::detect_nsp(&mut f), "pkg" => detect::detect_pkg(&mut f), _ => detect::detect_vpk(&mut f) };
                    let kind = match c.ext.as_str() { "nsp" => "switch", "pkg" => "pkg", _ => "vpk" };
                    match d.platform {
                        Some(p) => {
                            let extra = if c.ext == "nsp" { "" } else { " — precisa ser instalado no emulador" };
                            items.push(item(c, kind, Some(p), "new", format!("{}{extra}", d.note)));
                        }
                        None => items.push(item(c, kind, None, "invalid", d.note)),
                    }
                }
                Err(e) => items.push(item(c, "pkg", None, "invalid", format!("Arquivo inacessível: {e}."))),
            },
            _ => {}
        }
    }

    // vários .exe na mesma pasta: só o mais provável vem marcado como "novo"; os demais ficam "secundários"
    for (dir, idxs) in &exes_by_dir {
        let folder = dir.file_name().map(|s| s.to_string_lossy().to_lowercase().replace(|c: char| !c.is_alphanumeric(), "")).unwrap_or_default();
        let score = |i: usize| -> (bool, bool, u64) {
            let n = items[i].name.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "");
            (items[i].status == "new", !folder.is_empty() && (n.contains(&folder) || folder.contains(&n)), items[i].size)
        };
        if let Some(best) = idxs.iter().copied().max_by_key(|&i| score(i)) {
            for &i in idxs {
                if i != best && items[i].status == "new" {
                    items[i].status = "secondary".into();
                    items[i].note = "Executável secundário nesta pasta (o principal está pré-selecionado).".into();
                }
            }
        }
    }

    // duplicidade (caminho normalizado) e compatibilidade com os emuladores cadastrados
    for it in items.iter_mut() {
        let key = norm_key(&it.path);
        let bin_dup = it.ext == "cue" && {
            let mut txt = String::new();
            let _ = File::open(&it.path).and_then(|f| f.take(256 * 1024).read_to_string(&mut txt));
            let dir = Path::new(&it.path).parent().unwrap_or(Path::new("")).to_path_buf();
            detect::cue_files(&txt).iter().any(|n| library.contains(&norm_key(&dir.join(n).to_string_lossy())))
        };
        if library.contains(&key) || bin_dup {
            it.status = "duplicate".into();
            it.note = "Já está na biblioteca.".into();
        }
        if let Some(p) = &it.platform {
            if it.kind != "windows" {
                it.compatible = compatible(emus, p, &it.ext);
                if it.compatible.len() == 1 { it.assigned = it.compatible.first().copied(); }
            }
        }
    }
    items.sort_by(|a, b| {
        let rank = |p: &Option<String>| p.as_deref().and_then(|p| PLATFORMS.iter().position(|x| *x == p)).unwrap_or(99);
        rank(&a.platform).cmp(&rank(&b.platform)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    (items, ignored)
}

/// Chaves normalizadas de tudo que já está na biblioteca do perfil.
fn library_keys(conn: &rusqlite::Connection, profile_id: i64) -> AppResult<HashSet<String>> {
    let mut st = conn.prepare(
        "SELECT g.executable_path,g.iso_path,g.rom_path FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE pg.profile_id=?1",
    )?;
    let rows = st.query_map([profile_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))?;
    let mut set = HashSet::new();
    for row in rows {
        let (a, b, c) = row?;
        for p in [Some(a), b, c].into_iter().flatten().filter(|p| !p.trim().is_empty()) {
            set.insert(norm_key(&p));
        }
    }
    Ok(set)
}

#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn scan_folder(app: AppHandle, db: State<Db>, st: State<ScanState>, profile_id: i64, path: String, folder_id: Option<i64>) -> AppResult<()> {
    let root = PathBuf::from(path.trim());
    if !root.is_dir() {
        return Err(AppError::Validation("A pasta não existe ou está inacessível.".into()));
    }
    if st.running.swap(true, Ordering::SeqCst) {
        return Err(AppError::Validation("Já existe uma varredura em andamento.".into()));
    }
    st.cancel.store(false, Ordering::SeqCst);
    let (library, emus, missing) = {
        let conn = db.conn()?;
        let missing = match folder_id {
            Some(fid) => {
                let mut s = conn.prepare(
                    "SELECT g.id,g.title,COALESCE(g.iso_path,g.rom_path,g.executable_path)
                     FROM games g JOIN profile_games pg ON pg.game_id=g.id WHERE g.folder_id=?1 AND pg.profile_id=?2",
                )?;
                let rows = s.query_map(params![fid, profile_id], |r| Ok(Missing { game_id: r.get(0)?, title: r.get(1)?, path: r.get(2)? }))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows.into_iter().filter(|m| !Path::new(&m.path).exists()).collect()
            }
            None => vec![],
        };
        (library_keys(&conn, profile_id)?, load_all(&conn)?, missing)
    };
    let a = app.clone();
    std::thread::spawn(move || {
        let st = a.state::<ScanState>();
        // Um pânico na varredura não pode deixar a interface carregando para sempre nem travar `running`.
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (cands, scanned, errors) = walk(&root, &st.cancel, &a);
            let cancelled = st.cancel.load(Ordering::Relaxed);
            let (items, ignored) = if cancelled { (vec![], 0) } else { analyze(cands, &library, &emus, &st.cancel, &a) };
            (items, scanned, ignored, errors, cancelled || st.cancel.load(Ordering::Relaxed))
        }));
        let (items, scanned, ignored, errors, cancelled) = match res {
            Ok(r) => r,
            Err(_) => (vec![], 0, 0, vec!["Erro interno durante a varredura.".to_string()], false),
        };
        st.running.store(false, Ordering::SeqCst);
        let _ = a.emit("scan-done", Done {
            folder: root.to_string_lossy().to_string(), folder_id, items: if cancelled { vec![] } else { items },
            scanned, ignored, errors, cancelled, missing,
        });
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_scan(st: State<ScanState>) {
    st.cancel.store(true, Ordering::SeqCst);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub path: String,
    pub ext: String,
    pub platform: String,
    pub title: String,
    pub emulator_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: u32,
    pub duplicates: u32,
    pub failed: Vec<String>,
    pub folder_id: i64,
}

/// Registra os jogos escolhidos (só caminhos; nenhum arquivo é tocado) e vincula a pasta ao perfil.
#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn import_scan_items(db: State<Db>, profile_id: i64, folder_path: String, items: Vec<ImportItem>) -> AppResult<ImportSummary> {
    let mut conn = db.conn()?;
    let mut known = library_keys(&conn, profile_id)?;
    let tx = conn.transaction()?;
    tx.execute("INSERT OR IGNORE INTO library_folders(profile_id,path) VALUES(?1,?2)", params![profile_id, folder_path])?;
    let folder_id: i64 = tx.query_row("SELECT id FROM library_folders WHERE profile_id=?1 AND path=?2", params![profile_id, folder_path], |r| r.get(0))?;
    tx.execute("UPDATE library_folders SET last_scan_at=datetime('now') WHERE id=?1", [folder_id])?;
    let mut sum = ImportSummary { imported: 0, duplicates: 0, failed: vec![], folder_id };
    for it in items {
        let name = Path::new(&it.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let key = norm_key(&it.path);
        if known.contains(&key) { sum.duplicates += 1; continue; }
        if !Path::new(&it.path).is_file() { sum.failed.push(format!("{name}: arquivo não encontrado")); continue; }
        let ext = it.ext.to_lowercase();
        let Some(platform) = canonical_platform(&it.platform).filter(|p| PLATFORMS.contains(p)) else {
            sum.failed.push(format!("{name}: plataforma inválida"));
            continue;
        };
        let title: String = it.title.trim().chars().take(80).collect();
        let title = if title.is_empty() { stem(Path::new(&it.path)) } else { title };
        let emu = match it.emulator_id {
            Some(e) => tx.query_row("SELECT COUNT(*) FROM emulators WHERE id=?1", [e], |r| r.get::<_, i64>(0)).ok().filter(|n| *n > 0).map(|_| e),
            None => None,
        };
        let (exe, iso, rom, source) = match ext.as_str() {
            "exe" => (it.path.clone(), None, None, "folder-exe"),
            "iso" => (String::new(), Some(it.path.clone()), None, "folder-iso"),
            _ => (String::new(), None, Some(it.path.clone()), "folder-rom"),
        };
        let r = tx.execute(
            "INSERT INTO games(title,platform,executable_path,launch_args,iso_path,rom_path,emulator_id,folder_id,source)
             VALUES(?1,?2,?3,'',?4,?5,?6,?7,?8)",
            params![title, platform, exe, iso, rom, emu, folder_id, source],
        );
        match r {
            Ok(_) => {
                let gid = tx.last_insert_rowid();
                tx.execute("INSERT INTO profile_games(profile_id,game_id) VALUES(?1,?2)", params![profile_id, gid])?;
                known.insert(key);
                sum.imported += 1;
            }
            Err(e) => sum.failed.push(format!("{name}: {e}")),
        }
    }
    tx.commit()?;
    Ok(sum)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub id: i64,
    pub path: String,
    pub added_at: String,
    pub last_scan_at: Option<String>,
    pub games: i64,
    pub exists: bool,
}

#[tauri::command]
pub fn list_library_folders(db: State<Db>, profile_id: i64) -> AppResult<Vec<FolderRow>> {
    let conn = db.conn()?;
    let mut st = conn.prepare(
        "SELECT f.id,f.path,f.added_at,f.last_scan_at,
                (SELECT COUNT(*) FROM games g JOIN profile_games pg ON pg.game_id=g.id WHERE g.folder_id=f.id AND pg.profile_id=f.profile_id)
         FROM library_folders f WHERE f.profile_id=?1 ORDER BY f.added_at DESC",
    )?;
    let rows = st
        .query_map([profile_id], |r| {
            let path: String = r.get(1)?;
            Ok(FolderRow { id: r.get(0)?, exists: Path::new(&path).is_dir(), path, added_at: r.get(2)?, last_scan_at: r.get(3)?, games: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Remove só o vínculo da pasta com a biblioteca. Os jogos já importados permanecem e os arquivos nunca são apagados.
#[tauri::command]
pub fn remove_library_folder(db: State<Db>, profile_id: i64, id: i64) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM library_folders WHERE id=?1 AND profile_id=?2", params![id, profile_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_paths_for_duplicate_detection() {
        assert_eq!(norm_key(r"D:\Jogos\GOD OF WAR.ISO"), norm_key("d:/jogos/god of war.iso/"));
        assert_eq!(norm_key(r"\\?\D:\A\b.iso"), "d:/a/b.iso");
        assert_ne!(norm_key(r"D:\A\b.iso"), norm_key(r"D:\A\c.iso"));
    }

    #[test]
    fn compatibility_needs_platform_and_format() {
        let emu = |id, pl: &[&str], fm: &[&str]| Emulator {
            id, name: format!("e{id}"), exe_path: String::new(),
            platforms: pl.iter().map(|s| s.to_string()).collect(), formats: fm.iter().map(|s| s.to_string()).collect(),
            args: String::new(), cover: None, exe_exists: true, games: 0,
        };
        let list = vec![emu(1, &["Playstation 2"], &["iso", "cue"]), emu(2, &["Playstation 2"], &["iso"]), emu(3, &["PSP"], &["iso"])];
        assert_eq!(compatible(&list, "Playstation 2", "iso"), vec![1, 2]);
        assert_eq!(compatible(&list, "Playstation 2", "bin"), vec![1]); // bin/cue contam como o mesmo formato
        assert!(compatible(&list, "Playstation 3", "iso").is_empty());
        assert!(compatible(&list, "PSP", "nsp").is_empty());
        let sw2 = vec![emu(4, &["Nintendo Switch 2"], &["nsp", "nup"])];
        assert_eq!(compatible(&sw2, "Nintendo Switch 2", "nup"), vec![4]);
    }
}
