//! Catálogo de emuladores: os cartões que já vêm prontos na tela de Emuladores.
//!
//! Para cada emulador conhecido, o NEXSTAGE sabe de onde baixar a versão portátil do Windows (a mesma fonte
//! da atualização, `emu_update`), qual é o executável dentro do pacote e como deixá-lo em modo portátil
//! (configurações e dados na própria pasta). Assim o usuário pode:
//! - **Baixar e instalar** pelo app: vai para a pasta de emuladores do NEXSTAGE (ou a escolhida em
//!   `emu_install_dir`) e o cadastro é criado já com a versão registrada para as atualizações;
//! - **Localizar** uma instalação que já tem: escolhendo o executável ou com a busca automática no PC
//!   (programas instalados no registro do Windows, atalhos do menu Iniciar/Área de Trabalho e pastas comuns).
//!
//! Plataformas, formatos e argumentos de cada emulador continuam nas sugestões do frontend
//! (`src/data/emulatorPresets.ts`, campo `catalog`): o cadastro chega pronto em `EmulatorInput`.

use crate::commands::emulators::{self, Emulator, EmulatorInput};
use crate::db::Db;
use crate::emu_update::{self, EmuUpdates, Source};
use crate::error::{AppError, AppResult};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

pub const SETTING_DIR: &str = "emu_install_dir";

/// O que deixa o emulador em modo portátil (criado na instalação pelo app; nunca substituído depois).
#[derive(Clone, Copy)]
enum Portable {
    File(&'static str),
    Dir(&'static str),
    /// Já é portátil por padrão.
    None,
}

pub struct Entry {
    pub id: &'static str,
    /// Pasta dentro da pasta de emuladores.
    folder: &'static str,
    source: &'static str,
    hint: Option<&'static str>,
    /// Executáveis possíveis (em ordem de preferência), no pacote e numa instalação existente.
    pub exes: &'static [&'static str],
    portable: Portable,
}

pub const CATALOG: &[Entry] = &[
    Entry { id: "pcsx2", folder: "PCSX2", source: "PCSX2/pcsx2", hint: None, exes: &["pcsx2-qt.exe", "pcsx2-qtx64-avx2.exe", "pcsx2-qtx64.exe"], portable: Portable::File("portable.txt") },
    Entry {
        id: "duckstation",
        folder: "DuckStation",
        source: "stenzek/duckstation",
        hint: None,
        exes: &["duckstation-qt-x64-ReleaseLTCG.exe", "duckstation-qt.exe"],
        portable: Portable::File("portable.txt"),
    },
    Entry { id: "rpcs3", folder: "RPCS3", source: "RPCS3/rpcs3-binaries-win", hint: None, exes: &["rpcs3.exe"], portable: Portable::None },
    Entry { id: "ppsspp", folder: "PPSSPP", source: "hrydgard/ppsspp", hint: None, exes: &["PPSSPPWindows64.exe"], portable: Portable::None },
    Entry { id: "vita3k", folder: "Vita3K", source: "Vita3K/Vita3K", hint: None, exes: &["Vita3K.exe"], portable: Portable::None },
    Entry { id: "eden", folder: "Eden", source: "https://git.eden-emu.dev/eden-emu/eden", hint: Some("msvc"), exes: &["eden.exe"], portable: Portable::Dir("user") },
    Entry { id: "xemu", folder: "xemu", source: "xemu-project/xemu", hint: None, exes: &["xemu.exe"], portable: Portable::None },
    Entry { id: "xenia", folder: "Xenia Canary", source: "xenia-canary/xenia-canary-releases", hint: None, exes: &["xenia_canary.exe"], portable: Portable::File("portable.txt") },
    Entry { id: "dolphin", folder: "Dolphin", source: emu_update::DOLPHIN, hint: None, exes: &["Dolphin.exe"], portable: Portable::File("portable.txt") },
    Entry { id: "cemu", folder: "Cemu", source: "cemu-project/Cemu", hint: None, exes: &["Cemu.exe"], portable: Portable::Dir("portable") },
    Entry { id: "melonds", folder: "melonDS", source: "melonDS-emu/melonDS", hint: None, exes: &["melonDS.exe"], portable: Portable::None },
    Entry { id: "mgba", folder: "mGBA", source: "mgba-emu/mgba", hint: Some("win64"), exes: &["mGBA.exe"], portable: Portable::File("portable.ini") },
    Entry { id: "azahar", folder: "Azahar", source: "azahar-emu/azahar", hint: Some("msvc"), exes: &["azahar.exe", "azahar-qt.exe"], portable: Portable::Dir("user") },
    Entry { id: "flycast", folder: "Flycast", source: "flyinghead/flycast", hint: None, exes: &["flycast.exe"], portable: Portable::None },
    Entry { id: "snes9x", folder: "Snes9x", source: "snes9xgit/snes9x", hint: None, exes: &["snes9x-x64.exe", "snes9x.exe"], portable: Portable::None },
    Entry { id: "mesen", folder: "Mesen", source: "SourMesen/Mesen2", hint: None, exes: &["Mesen.exe"], portable: Portable::None },
    // Lichii: "x86_64.zip" escolhe o pacote padrão (não o "-pgo", otimizado para CPUs recentes)
    Entry { id: "lichii", folder: "Lichii", source: "zortos293/Lichii", hint: Some("x86_64.zip"), exes: &["lichii.exe"], portable: Portable::Dir("user") },
    Entry { id: "rmg", folder: "RMG", source: "Rosalie241/RMG", hint: Some("portable"), exes: &["RMG.exe"], portable: Portable::None },
    Entry { id: "ares", folder: "ares", source: "ares-emulator/ares", hint: Some("x64"), exes: &["ares.exe"], portable: Portable::None },
];

fn entry(id: &str) -> AppResult<&'static Entry> {
    CATALOG.iter().find(|e| e.id == id).ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))
}

fn source(e: &Entry) -> AppResult<Source> {
    Ok(Source { hint: e.hint, ..emu_update::parse_source(e.source)? })
}

/// Item do catálogo que corresponde a um emulador cadastrado (pela fonte reconhecida ou pelo executável).
fn matches(e: &Entry, name: &str, exe: &str) -> bool {
    let file = Path::new(exe).file_name().and_then(|n| n.to_str()).unwrap_or("");
    e.exes.iter().any(|x| x.eq_ignore_ascii_case(file)) || emu_update::preset(name, exe).is_some_and(|(s, _)| s == e.source)
}

pub fn install_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let db = app.state::<Db>();
    let custom: Option<String> = db.conn().ok().and_then(|c| c.query_row("SELECT value FROM app_settings WHERE key=?1", [SETTING_DIR], |r| r.get(0)).ok());
    // a pasta escolhida é um caminho deste PC: num backup restaurado em outro computador (sem a pasta),
    // volta para a padrão em vez de falhar
    match custom.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && Path::new(s).is_dir()) {
        Some(d) => Ok(PathBuf::from(d)),
        None => Ok(app.path().app_local_data_dir().map_err(|e| AppError::Other(e.to_string()))?.join("Emulators")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: &'static str,
    /// Nome da pasta criada dentro da pasta de instalação escolhida.
    pub folder: &'static str,
    /// Emulador cadastrado que corresponde a este item (o cartão do catálogo some).
    pub registered: Option<i64>,
    pub page: String,
    pub busy: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInfo {
    pub install_dir: String,
    pub items: Vec<CatalogItem>,
}

#[tauri::command]
pub fn emulator_catalog(app: AppHandle) -> AppResult<CatalogInfo> {
    let list = {
        let db = app.state::<Db>();
        let conn = db.conn()?;
        emulators::load_all(&conn)?
    };
    let busy = app.state::<EmuUpdates>().catalog_busy.lock().map(|b| b.clone()).unwrap_or_default();
    let items = CATALOG
        .iter()
        .map(|e| CatalogItem {
            id: e.id,
            folder: e.folder,
            registered: list.iter().find(|x| matches(e, &x.name, &x.exe_path)).map(|x| x.id),
            page: source(e).map(|s| s.page).unwrap_or_default(),
            busy: busy.contains(e.id),
        })
        .collect();
    Ok(CatalogInfo { install_dir: install_dir(&app)?.to_string_lossy().into_owned(), items })
}

fn progress(app: &AppHandle, id: &str, phase: &str, pct: Option<u8>) {
    let _ = app.emit("emulator-catalog-progress", serde_json::json!({ "id": id, "phase": phase, "pct": pct }));
}

struct Busy<'a>(&'a AppHandle, &'static str);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        if let Ok(mut b) = self.0.state::<EmuUpdates>().catalog_busy.lock() {
            b.remove(self.1);
        }
    }
}

/// Baixa a última versão do emulador, instala em `<base_dir>/<pasta do emulador>` e cria o cadastro (`input`
/// traz as plataformas, formatos e argumentos da sugestão; o executável é preenchido aqui). Sem `base_dir`,
/// usa a pasta padrão; com `remember`, a escolhida passa a ser a padrão.
#[tauri::command]
pub async fn emulator_catalog_install(
    app: AppHandle,
    id: String,
    mut input: EmulatorInput,
    base_dir: Option<String>,
    remember: Option<bool>,
    replace: Option<bool>,
) -> AppResult<Emulator> {
    let e = entry(&id)?;
    {
        let st = app.state::<EmuUpdates>();
        let mut b = st.catalog_busy.lock().map_err(|_| AppError::Other(String::from("estado")))?;
        if !b.insert(e.id.to_string()) {
            return Err(AppError::Validation("Este emulador já está sendo instalado.".into()));
        }
    }
    let _busy = Busy(&app, e.id);
    progress(&app, e.id, "check", None);
    let latest = emu_update::latest_cached(&app, &source(e)?, false).await.map_err(AppError::Other)?;
    let asset = latest.asset.clone().ok_or_else(|| AppError::Validation("A versão publicada não tem pacote portátil para Windows.".into()))?;
    let base = match base_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => {
            let p = PathBuf::from(d);
            if !p.is_dir() {
                return Err(AppError::Validation("Pasta não encontrada.".into()));
            }
            if remember.unwrap_or(false) {
                let db = app.state::<Db>();
                db.conn()?.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", params![SETTING_DIR, d])?;
            }
            p
        }
        None => install_dir(&app)?,
    };
    let target = base.join(e.folder);
    // pasta de mesmo nome já existe: com "Substituir", vai para a Lixeira e a instalação começa do zero;
    // sem ele, instala por cima (configurações, BIOS e saves são mantidos)
    if replace.unwrap_or(false) && target.exists() {
        check_replace(&target, None)?;
        if e.exes.iter().any(|x| exe_open(&target.join(x))) {
            return Err(AppError::Validation("Feche o emulador antes de atualizar.".into()));
        }
        to_recycle_bin(&target)?;
    }
    fs::create_dir_all(&target).map_err(|_| AppError::Validation("Não foi possível criar a pasta de emuladores. Escolha outra pasta.".into()))?;
    if !emu_update::dir_writable(&target) {
        return Err(AppError::Validation("Não foi possível criar a pasta de emuladores. Escolha outra pasta.".into()));
    }
    // reinstalar por cima de um emulador aberto falharia no meio
    if e.exes.iter().any(|x| exe_open(&target.join(x))) {
        return Err(AppError::Validation("Feche o emulador antes de atualizar.".into()));
    }

    let work = emu_update::work_dir(&app, &format!("catalog-{}", e.id))?;
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let ext = if asset.name.to_lowercase().ends_with(".7z") { "7z" } else { "zip" };
    let pkg = work.join(format!("package.{ext}"));
    let app2 = app.clone();
    let result = async {
        emu_update::download(&asset, &pkg, &move |pct| progress(&app2, e.id, "download", Some(pct))).await?;
        progress(&app, e.id, "install", None);
        let (pkg, work, target) = (pkg.clone(), work.clone(), target.clone());
        tauri::async_runtime::spawn_blocking(move || -> AppResult<String> {
            let files = work.join("files");
            emu_update::extract(&pkg, &files)?;
            let (root, exe) = emu_update::find_root_any(&files, e.exes)
                .ok_or_else(|| AppError::Validation(format!("O pacote baixado não contém {}.", e.exes[0])))?;
            emu_update::apply(&root, &target, &work.join("backup"))?;
            match e.portable {
                Portable::File(f) if !target.join(f).exists() => fs::write(target.join(f), b"")?,
                Portable::Dir(d) => fs::create_dir_all(target.join(d))?,
                _ => {}
            }
            Ok(exe)
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
    }
    .await;
    let _ = fs::remove_dir_all(&work);
    let exe = result?;

    input.id = None;
    input.exe_path = target.join(&exe).to_string_lossy().into_owned();
    let db = app.state::<Db>();
    let conn = db.conn()?;
    let saved = emulators::save(&conn, input)?;
    conn.execute("UPDATE emulators SET update_version=?1, update_key=?2 WHERE id=?3", params![latest.version, asset.key, saved.id])?;
    progress(&app, e.id, "done", None);
    Ok(saved)
}

fn exe_open(exe: &Path) -> bool {
    exe.is_file() && fs::OpenOptions::new().write(true).open(exe).is_err_and(|err| err.raw_os_error() == Some(32))
}

// ---------------------------------------------------------------- pasta do emulador (abrir / mover)

fn emulator_exe(app: &AppHandle, id: i64) -> AppResult<PathBuf> {
    let db = app.state::<Db>();
    let conn = db.conn()?;
    let exe: String = conn
        .query_row("SELECT exe_path FROM emulators WHERE id=?1", [id], |r| r.get(0))
        .map_err(|_| AppError::NotFound("Emulador não encontrado.".into()))?;
    Ok(PathBuf::from(exe))
}

/// "Abrir pasta de instalação": Explorador na pasta do emulador, com o executável selecionado.
#[tauri::command]
pub fn emulator_reveal(app: AppHandle, id: i64) -> AppResult<()> {
    let exe = emulator_exe(&app, id)?;
    if !exe.is_file() {
        return Err(AppError::NotFound("Executável do emulador não encontrado.".into()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let s = exe.to_string_lossy().replace('/', "\\");
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{s}\""))
            .spawn()
            .map_err(|e| AppError::Other(format!("Não foi possível abrir o Explorador de Arquivos: {e}")))?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub dir: String,
    pub name: String,
    pub files: u64,
    pub bytes: u64,
    /// Motivo para não mover (pasta do sistema/pessoal, protegida, emulador aberto); `None` = pode mover.
    pub blocked: Option<String>,
}

/// Pastas que nunca são movidas inteiras: se o executável está solto numa delas, mover "a pasta do emulador"
/// levaria junto tudo o que há nela (Downloads, Área de Trabalho, Arquivos de Programas…).
fn personal_or_system(dir: &Path) -> bool {
    if dir.parent().is_none() {
        return true; // raiz da unidade
    }
    let mut list: Vec<PathBuf> = Vec::new();
    for v in ["USERPROFILE", "ProgramFiles", "ProgramFiles(x86)", "ProgramData", "LOCALAPPDATA", "APPDATA", "SystemRoot", "PUBLIC"] {
        if let Some(p) = std::env::var_os(v) {
            list.push(PathBuf::from(p));
        }
    }
    if let Some(h) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        for sub in ["Desktop", "Downloads", "Documents", "Pictures", "Videos", "Music", "OneDrive", "Games"] {
            list.push(h.join(sub));
        }
    }
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let d = norm(dir);
    list.iter().any(|p| norm(p) == d)
}

fn dir_stats(dir: &Path, files: &mut u64, bytes: &mut u64) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            dir_stats(&e.path(), files, bytes);
        } else {
            *files += 1;
            *bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
}

fn folder_info(exe: &Path) -> AppResult<FolderInfo> {
    let dir = exe.parent().filter(|_| exe.is_file()).ok_or_else(|| AppError::NotFound("Executável do emulador não encontrado.".into()))?;
    let (mut files, mut bytes) = (0, 0);
    let blocked: Option<String> = if personal_or_system(dir) {
        Some("O executável está numa pasta pessoal ou do sistema (ex.: Downloads, Área de Trabalho): mover levaria tudo o que há nela. Coloque o emulador numa pasta só dele.".into())
    } else if !emu_update::dir_writable(dir) || dir.parent().is_some_and(|p| !emu_update::dir_writable(p)) {
        Some("O emulador está numa pasta protegida (instalado pelo instalador): mova pelo próprio instalador.".into())
    } else if exe_open(exe) {
        Some("Feche o emulador antes de mover a pasta.".into())
    } else {
        dir_stats(dir, &mut files, &mut bytes);
        None
    };
    Ok(FolderInfo {
        dir: dir.to_string_lossy().into_owned(),
        name: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        files,
        bytes,
        blocked,
    })
}

/// Tamanho e se a pasta do emulador pode ser movida (mostrado antes de confirmar).
#[tauri::command]
pub async fn emulator_folder_info(app: AppHandle, id: i64) -> AppResult<FolderInfo> {
    let exe = emulator_exe(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || folder_info(&exe)).await.map_err(|e| AppError::Other(e.to_string()))?
}

/// Manda uma pasta para a Lixeira (recuperável), sem perguntas do Windows. Usado quando o usuário escolhe
/// substituir uma pasta de mesmo nome no destino.
#[cfg(windows)]
fn to_recycle_bin(p: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    // lista terminada por dois zeros
    let from: Vec<u16> = p.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        ..Default::default()
    };
    let r = unsafe { SHFileOperationW(&mut op) };
    if r != 0 || op.fAnyOperationsAborted.as_bool() || p.exists() {
        return Err(AppError::Other(format!("Não foi possível mandar a pasta existente para a Lixeira (código {r}). Feche programas que estejam usando arquivos dela.")));
    }
    Ok(())
}

#[cfg(not(windows))]
fn to_recycle_bin(p: &Path) -> AppResult<()> {
    fs::remove_dir_all(p).map_err(AppError::from)
}

/// A pasta de mesmo nome no destino pode ser substituída? Nunca uma pasta pessoal/do sistema nem uma que
/// contenha a pasta de origem (apagar o destino apagaria a origem junto).
fn check_replace(target: &Path, source: Option<&Path>) -> AppResult<()> {
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if personal_or_system(target) {
        return Err(AppError::Validation("Essa pasta não pode ser substituída (pasta pessoal ou do sistema).".into()));
    }
    if source.is_some_and(|s| canon(s).starts_with(canon(target))) {
        return Err(AppError::Validation("A pasta do destino contém a pasta do emulador: escolha outro destino.".into()));
    }
    Ok(())
}

/// Copia a árvore inteira (mudança de unidade), avisando o progresso por bytes.
fn copy_tree(src: &Path, dst: &Path, done: &mut u64, on: &mut dyn FnMut(u64)) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let (from, to) = (e.path(), dst.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&from, &to, done, on)?;
        } else {
            *done += fs::copy(&from, &to)?;
            on(*done);
        }
    }
    Ok(())
}

/// Move a pasta para dentro de `dest_parent` (mesmo nome). Na mesma unidade é instantâneo; entre unidades,
/// copia tudo e só então apaga a original (se a cópia falhar, a original fica intacta).
fn move_dir(dir: &Path, dest_parent: &Path, replace: bool, on: &mut dyn FnMut(u64)) -> AppResult<PathBuf> {
    if !dest_parent.is_dir() {
        return Err(AppError::Validation("Pasta não encontrada.".into()));
    }
    let name = dir.file_name().ok_or_else(|| AppError::Validation("Pasta inválida.".into()))?;
    let target = dest_parent.join(name);
    let canon = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canon(dest_parent).starts_with(canon(dir)) {
        return Err(AppError::Validation("Escolha uma pasta fora da pasta do emulador.".into()));
    }
    if canon(dir.parent().unwrap_or(dir)) == canon(dest_parent) {
        return Err(AppError::Validation("O emulador já está nessa pasta.".into()));
    }
    if target.exists() {
        if !replace {
            return Err(AppError::Validation(format!("Já existe uma pasta \"{}\" no destino.", name.to_string_lossy())));
        }
        check_replace(&target, Some(dir))?;
        to_recycle_bin(&target)?;
    }
    match fs::rename(dir, &target) {
        Ok(()) => Ok(target),
        // ERROR_NOT_SAME_DEVICE: outra unidade
        Err(e) if e.raw_os_error() == Some(17) => {
            let mut done = 0;
            if let Err(err) = copy_tree(dir, &target, &mut done, on) {
                let _ = fs::remove_dir_all(&target);
                return Err(AppError::Other(format!("Não foi possível copiar a pasta: {err}. Nada foi alterado.")));
            }
            let _ = fs::remove_dir_all(dir);
            Ok(target)
        }
        Err(e) => Err(AppError::Other(format!("Não foi possível mover a pasta: {e}"))),
    }
}

/// "Mover pasta de instalação": move a pasta do emulador e atualiza o cadastro (de todos os emuladores
/// cujo executável está dentro dela). `replace`: se já houver uma pasta de mesmo nome no destino, ela vai
/// para a Lixeira (o usuário confirmou); sem ele, a mudança é recusada.
#[tauri::command]
pub async fn emulator_move_folder(app: AppHandle, id: i64, dest: String, replace: Option<bool>) -> AppResult<Emulator> {
    let replace = replace.unwrap_or(false);
    let exe = emulator_exe(&app, id)?;
    let info = {
        let exe = exe.clone();
        tauri::async_runtime::spawn_blocking(move || folder_info(&exe)).await.map_err(|e| AppError::Other(e.to_string()))??
    };
    if let Some(why) = info.blocked {
        return Err(AppError::Validation(why));
    }
    let dir = PathBuf::from(&info.dir);
    let (app2, total) = (app.clone(), info.bytes.max(1));
    let (dir2, dest2) = (dir.clone(), PathBuf::from(dest.trim()));
    let target = tauri::async_runtime::spawn_blocking(move || {
        let mut last = 255u8;
        move_dir(&dir2, &dest2, replace, &mut |done| {
            let pct = (done * 100 / total).min(100) as u8;
            if pct != last {
                last = pct;
                let _ = app2.emit("emulator-move-progress", serde_json::json!({ "id": id, "pct": pct }));
            }
        })
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    let db = app.state::<Db>();
    let conn = db.conn()?;
    for e in emulators::load_all(&conn)? {
        if let Ok(rel) = Path::new(&e.exe_path).strip_prefix(&dir) {
            conn.execute("UPDATE emulators SET exe_path=?1 WHERE id=?2", params![target.join(rel).to_string_lossy(), e.id])?;
        }
    }
    emulators::load_all(&conn)?.into_iter().find(|e| e.id == id).ok_or_else(|| AppError::NotFound("Emulador não encontrado.".into()))
}

// ---------------------------------------------------------------- busca no PC

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundInstall {
    pub id: &'static str,
    pub exe_path: String,
}

/// Pastas que nunca têm emulador (e são enormes).
const SKIP_DIRS: &[&str] = &[
    "windows", "$recycle.bin", "system volume information", "programdata", "appdata", "node_modules", ".git", "$windows.~bt",
    "$windows.~ws", "windowsapps", "steamapps", "msocache", "recovery", "perflogs", "emu_updates",
];
/// Limite de pastas visitadas na varredura (a busca precisa terminar em poucos segundos).
const MAX_DIRS: usize = 40_000;

fn by_exe_name(file: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.exes.iter().any(|x| x.eq_ignore_ascii_case(file)))
}

fn note(found: &mut HashMap<&'static str, PathBuf>, exe: &Path) {
    let Some(file) = exe.file_name().and_then(|n| n.to_str()) else { return };
    if let Some(e) = by_exe_name(file) {
        if exe.is_file() {
            found.entry(e.id).or_insert_with(|| exe.to_path_buf());
        }
    }
}

/// Procura os executáveis do catálogo dentro de `dir` (ou nele mesmo), até `depth` níveis.
fn scan(dir: &Path, depth: usize, found: &mut HashMap<&'static str, PathBuf>, budget: &mut usize) {
    if *budget == 0 || found.len() == CATALOG.len() {
        return;
    }
    *budget -= 1;
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    for ent in rd.flatten() {
        let Ok(ft) = ent.file_type() else { continue };
        let name = ent.file_name().to_string_lossy().to_lowercase();
        if ft.is_dir() {
            if depth > 0 && !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                subdirs.push(ent.path());
            }
        } else if name.ends_with(".exe") {
            note(found, &ent.path());
        }
    }
    for d in subdirs {
        scan(&d, depth - 1, found, budget);
    }
}

/// Atalhos (.lnk) que apontam para um executável do catálogo.
fn scan_shortcuts(dir: &Path, depth: usize, found: &mut HashMap<&'static str, PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            if depth > 0 {
                scan_shortcuts(&p, depth - 1, found);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            if let Some(t) = crate::lnk::read(&p) {
                note(found, &t.path);
            }
        }
    }
}

/// Pastas de programas instalados (InstallLocation / ícone) do registro do Windows.
#[cfg(windows)]
fn registry_dirs() -> Vec<PathBuf> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::*;
    const UNINSTALL: [&str; 2] = [r"Software\Microsoft\Windows\CurrentVersion\Uninstall", r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"];
    fn value(key: HKEY, sub: &str, name: &str) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let (s, n) = (HSTRING::from(sub), HSTRING::from(name));
        let r = unsafe { RegGetValueW(key, PCWSTR(s.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) };
        if r.is_err() {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        let v = String::from_utf16_lossy(&buf[..len]).trim_matches(|c: char| c == '\0' || c == '"' || c.is_whitespace()).to_string();
        (!v.is_empty()).then_some(v)
    }
    let mut out = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for path in UNINSTALL {
            let mut key = HKEY::default();
            let p = HSTRING::from(path);
            if unsafe { RegOpenKeyExW(root, PCWSTR(p.as_ptr()), Some(0), KEY_READ, &mut key) }.is_err() {
                continue;
            }
            let mut name = [0u16; 256];
            for idx in 0.. {
                let mut cch = name.len() as u32;
                if unsafe { RegEnumKeyExW(key, idx, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut cch, None, None, None, None) }.is_err() {
                    break;
                }
                let sub = String::from_utf16_lossy(&name[..cch as usize]);
                if let Some(loc) = value(key, &sub, "InstallLocation") {
                    out.push(PathBuf::from(loc));
                }
                if let Some(icon) = value(key, &sub, "DisplayIcon") {
                    // "C:\...\pcsx2-qt.exe,0"
                    let icon = icon.split(',').next().unwrap_or("").trim_matches('"');
                    if let Some(dir) = Path::new(icon).parent() {
                        out.push(dir.to_path_buf());
                    }
                }
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_dirs() -> Vec<PathBuf> {
    Vec::new()
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_dir())
}

fn detect(own_dir: Option<PathBuf>) -> Vec<FoundInstall> {
    let mut found: HashMap<&'static str, PathBuf> = HashMap::new();
    // 1) programas instalados e atalhos: rápidos e certeiros
    for d in registry_dirs() {
        scan(&d, 1, &mut found, &mut 50);
    }
    let home = env_dir("USERPROFILE");
    let mut lnk_dirs: Vec<PathBuf> = Vec::new();
    if let Some(a) = env_dir("APPDATA") {
        lnk_dirs.push(a.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(a) = env_dir("ProgramData") {
        lnk_dirs.push(a.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(h) = &home {
        lnk_dirs.push(h.join("Desktop"));
    }
    if let Some(p) = env_dir("PUBLIC") {
        lnk_dirs.push(p.join("Desktop"));
    }
    for d in &lnk_dirs {
        scan_shortcuts(d, 3, &mut found);
    }
    // 2) varredura das pastas onde emuladores portáteis costumam ficar
    let mut budget = MAX_DIRS;
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    if let Some(d) = own_dir {
        roots.push((d, 2));
    }
    for v in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(d) = env_dir(v) {
            roots.push((d, 2));
        }
    }
    if let Some(d) = env_dir("LOCALAPPDATA") {
        roots.push((d.join("Programs"), 2));
    }
    if let Some(h) = &home {
        for sub in ["Desktop", "Downloads", "Documents", "Games", "Emulators", "Emuladores"] {
            roots.push((h.join(sub), 3));
        }
        roots.push((h.clone(), 1));
    }
    for letter in b'A'..=b'Z' {
        let d = PathBuf::from(format!("{}:\\", letter as char));
        if d.is_dir() {
            roots.push((d, 3));
        }
    }
    for (d, depth) in roots {
        scan(&d, depth, &mut found, &mut budget);
    }
    let mut out: Vec<FoundInstall> = found.into_iter().map(|(id, p)| FoundInstall { id, exe_path: p.to_string_lossy().into_owned() }).collect();
    out.sort_by_key(|f| CATALOG.iter().position(|e| e.id == f.id));
    out
}

/// Busca automática de emuladores já instalados no PC (somente leitura).
#[tauri::command]
pub async fn emulator_catalog_detect(app: AppHandle) -> AppResult<Vec<FoundInstall>> {
    let own = install_dir(&app).ok();
    tauri::async_runtime::spawn_blocking(move || detect(own)).await.map_err(|e| AppError::Other(e.to_string()))
}

/// Pasta onde o app instala os emuladores (vazio = padrão do app).
#[tauri::command]
pub fn emulator_catalog_set_dir(db: tauri::State<Db>, dir: String) -> AppResult<()> {
    let dir = dir.trim();
    if !dir.is_empty() && !Path::new(dir).is_dir() {
        return Err(AppError::Validation("Pasta não encontrada.".into()));
    }
    let conn = db.conn()?;
    conn.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", params![SETTING_DIR, dir])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent_with_update_presets() {
        for e in CATALOG {
            assert!(source(e).is_ok(), "{}", e.id);
            // o executável instalado pelo catálogo é reconhecido pela atualização (mesma fonte)
            let exe = format!("C:/Emu/{}/{}", e.folder, e.exes[0]);
            assert_eq!(emu_update::preset(e.folder, &exe).map(|p| p.0), Some(e.source), "{}", e.id);
            assert!(matches(e, "qualquer", &exe));
        }
        assert!(matches(entry("pcsx2").unwrap(), "PCSX2", "D:/x/pcsx2-qtx64-avx2.exe"));
        assert!(!matches(entry("pcsx2").unwrap(), "RPCS3", "D:/x/rpcs3.exe"));
    }

    #[test]
    fn moves_emulator_folder_and_refuses_bad_targets() {
        let base = std::env::temp_dir().join(format!("nx-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (from, to) = (base.join("a/PCSX2"), base.join("b"));
        fs::create_dir_all(from.join("inis")).unwrap();
        fs::create_dir_all(&to).unwrap();
        fs::write(from.join("pcsx2-qt.exe"), "x").unwrap();
        fs::write(from.join("inis/PCSX2.ini"), "cfg").unwrap();
        assert!(move_dir(&from, &from.join("inis"), false, &mut |_| {}).is_err(), "dentro dela mesma");
        assert!(move_dir(&from, &base.join("a"), false, &mut |_| {}).is_err(), "mesma pasta");
        let t = move_dir(&from, &to, false, &mut |_| {}).unwrap();
        assert_eq!(t, to.join("PCSX2"));
        assert!(!from.exists() && fs::read_to_string(t.join("inis/PCSX2.ini")).unwrap() == "cfg");
        fs::create_dir_all(&from).unwrap();
        assert!(move_dir(&from, &to, false, &mut |_| {}).is_err(), "destino já tem a pasta (sem substituir)");
        // substituir: nunca uma pasta que contém a origem nem uma pasta pessoal/do sistema
        assert!(check_replace(&base.join("a"), Some(&from)).is_err());
        assert!(check_replace(&to.join("PCSX2"), Some(&from)).is_ok());
        assert!(check_replace(Path::new("C:\\"), None).is_err());
        let (mut done, mut n) = (0, 0);
        copy_tree(&t, &base.join("c"), &mut done, &mut |_| n += 1).unwrap();
        assert_eq!((n, done), (2, 4));
        assert!(personal_or_system(Path::new("C:\\")));
        if let Some(h) = std::env::var_os("USERPROFILE") {
            assert!(personal_or_system(&PathBuf::from(h).join("Downloads")));
        }
        assert!(!personal_or_system(&base.join("b")));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn scan_finds_portable_folders_and_skips_system_dirs() {
        let base = std::env::temp_dir().join(format!("nx-catalog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("Emus/PCSX2 2.8")).unwrap();
        fs::create_dir_all(base.join("Emus/rpcs3")).unwrap();
        fs::create_dir_all(base.join("node_modules/x")).unwrap();
        fs::write(base.join("Emus/PCSX2 2.8/pcsx2-qt.exe"), "").unwrap();
        fs::write(base.join("Emus/rpcs3/RPCS3.EXE"), "").unwrap();
        fs::write(base.join("node_modules/x/xemu.exe"), "").unwrap();
        let mut found = HashMap::new();
        scan(&base, 3, &mut found, &mut 1000);
        assert!(found["pcsx2"].ends_with("pcsx2-qt.exe"));
        assert!(found.contains_key("rpcs3"));
        assert!(!found.contains_key("xemu"));
        let _ = fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod e2e {
    use super::*;

    /// Baixa e extrai o pacote de cada item do catálogo e confere o executável (rede, ~300 MB):
    /// `cargo test emu_catalog::e2e -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn every_package_has_the_expected_exe() {
        let client = reqwest::Client::builder().user_agent("NEXSTAGE-test").build().unwrap();
        let only = std::env::var("NX_CATALOG").ok();
        for e in CATALOG.iter().filter(|e| only.as_deref().is_none_or(|o| o.split(',').any(|x| x == e.id))) {
            let l = emu_update::fetch_latest(&source(e).unwrap()).await.unwrap();
            let a = l.asset.clone().expect(e.id);
            let (pick, url) = (a.name.clone(), a.url.clone());
            let dir = std::env::temp_dir().join(format!("nx-cat-{}", e.id));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let pkg = dir.join(&pick);
            fs::write(&pkg, client.get(&url).send().await.unwrap().bytes().await.unwrap()).unwrap();
            emu_update::extract(&pkg, &dir.join("files")).unwrap();
            let found = emu_update::find_root_any(&dir.join("files"), e.exes);
            println!("{}: {pick} -> {:?}", e.id, found.as_ref().map(|f| &f.1));
            assert!(found.is_some(), "{}", e.id);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod e2e_detect {
    /// Busca real neste PC (somente leitura): `cargo test emu_catalog::e2e_detect -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn detect_on_this_pc() {
        let t = std::time::Instant::now();
        let found = super::detect(None);
        println!("{:?} em {:?}", found, t.elapsed());
    }
}

#[cfg(test)]
mod e2e_recycle {
    /// Manda uma pasta temporária para a Lixeira de verdade: `cargo test emu_catalog::e2e_recycle -- --ignored`.
    #[test]
    #[ignore]
    fn sends_folder_to_recycle_bin() {
        let d = std::env::temp_dir().join("nx-recycle-test");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a.txt"), "x").unwrap();
        super::to_recycle_bin(&d).unwrap();
        assert!(!d.exists());
    }
}
