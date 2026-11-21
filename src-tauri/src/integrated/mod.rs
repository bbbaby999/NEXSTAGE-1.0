//! Jogos oficialmente integrados ao NexStage (catálogo, presença na biblioteca, local de instalação).
//!
//! - Cada jogo integrado aparece AUTOMATICAMENTE uma vez na biblioteca de cada perfil (`source = 'nexstage'`,
//!   `external_id = <game_id>`). Se o usuário remover, ele não volta sozinho (`profile_settings`).
//! - Todos os perfis usam a MESMA linha em `games` (o jogo instalado é um só no PC); tempo de jogo, favoritos e
//!   histórico continuam por perfil em `profile_games` / `play_sessions`.
//! - O executável é sempre definido pelo NexStage (instalação gerenciada — Fase 5 — ou "Localizar instalação");
//!   o usuário não edita o caminho nem os argumentos.
//! - O Game Bridge (`bridge`) não depende deste módulo para as regras do protocolo; só consulta `is_official`.

pub mod install;
pub mod manifest;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Dados fixos de um jogo integrado oficial.
pub struct Official {
    pub id: &'static str,
    pub name: &'static str,
    /// Executável e arquivo de dados que precisam estar juntos na pasta do jogo.
    pub exe: &'static str,
    pub data_file: &'static str,
    pub cover: &'static [u8],
    pub background: &'static [u8],
    /// Manifesto assinado da versão mais recente (repositório público de distribuição) e a assinatura.
    pub manifest_url: &'static str,
    pub signature_url: &'static str,
    /// Chave pública Ed25519 (base64, 32 bytes) que assina os manifestos deste jogo.
    pub public_key: &'static str,
    /// Descrição oficial (página especial), pt-BR e en.
    pub description: [&'static str; 2],
    /// Página pública com todas as versões (notas completas e download manual).
    pub releases_url: &'static str,
}

pub const OFFICIAL: &[Official] = &[Official {
    id: "tomb-survivors",
    name: "Tomb Survivors",
    exe: "TombSurvivors.exe",
    data_file: "TombSurvivors.pck",
    cover: include_bytes!("../../assets/integrated/tomb-survivors-cover.jpg"),
    background: include_bytes!("../../assets/integrated/tomb-survivors-background.jpg"),
    manifest_url: "https://github.com/bbbaby999/tomb-survivors-releases/releases/latest/download/manifest.json",
    signature_url: "https://github.com/bbbaby999/tomb-survivors-releases/releases/latest/download/manifest.json.sig",
    public_key: "PJrQZ38cygRWZlG5WguvZDPsmzNK8BYqPRqThL2jN8w=",
    description: [
        "Roguelite de sobrevivência numa tumba egípcia. Uma exploradora enfrenta hordas de múmias, aranhas e escaravelhos por 10 minutos, escolhendo cartas de habilidade a cada nível, até despertar o Sacerdote-Guardião. Armas, relíquias, armadilhas e um tesouro que cresce entre as partidas.",
        "A survival roguelite set in an Egyptian tomb. An explorer faces hordes of mummies, spiders and scarabs for 10 minutes, picking ability cards at every level, until the Guardian Priest awakens. Weapons, relics, traps and a treasure that grows between runs.",
    ],
    releases_url: "https://github.com/bbbaby999/tomb-survivors-releases/releases",
}];

pub fn official(id: &str) -> Option<&'static Official> {
    OFFICIAL.iter().find(|g| g.id == id)
}

pub fn is_official(id: &str) -> bool {
    official(id).is_some()
}

fn added_key(id: &str) -> String {
    format!("integrated_added:{id}")
}

/// Linha compartilhada em `games` do jogo integrado (cria na primeira vez, com capa e arte de fundo).
fn library_row(conn: &Connection, g: &Official) -> AppResult<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM games WHERE source='nexstage' AND external_id=?1 ORDER BY id LIMIT 1", [g.id], |r| r.get::<_, i64>(0))
        .optional()?
    {
        return Ok(id);
    }
    let cover = crate::images::store_bytes(g.cover, "image/jpeg").ok();
    let background = crate::images::store_bytes(g.background, "image/jpeg").ok();
    let exe = installed_exe(conn, g).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    conn.execute(
        "INSERT INTO games(title,platform,executable_path,launch_args,cover,background,source,external_id) VALUES(?1,'PC',?2,'',?3,?4,'nexstage',?5)",
        params![g.name, exe, cover, background, g.id],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute("UPDATE integrated_games SET library_game_id=?1 WHERE game_id=?2", params![id, g.id])?;
    Ok(id)
}

/// Garante o cadastro dos jogos oficiais (`integrated_games`) — idempotente.
pub fn seed(conn: &Connection) -> AppResult<()> {
    for g in OFFICIAL {
        conn.execute("INSERT OR IGNORE INTO integrated_games(game_id,name) VALUES(?1,?2)", params![g.id, g.name])?;
    }
    Ok(())
}

/// Coloca os jogos integrados na biblioteca do perfil (uma vez por perfil; respeita a remoção pelo usuário).
pub fn ensure_library(conn: &Connection, profile_id: i64) -> AppResult<()> {
    seed(conn)?;
    for g in OFFICIAL {
        let done: bool = conn
            .query_row("SELECT 1 FROM profile_settings WHERE profile_id=?1 AND key=?2", params![profile_id, added_key(g.id)], |_| Ok(()))
            .optional()?
            .is_some();
        if done {
            continue;
        }
        let game_id = library_row(conn, g)?;
        conn.execute("INSERT OR IGNORE INTO profile_games(profile_id,game_id) VALUES(?1,?2)", params![profile_id, game_id])?;
        conn.execute("INSERT OR REPLACE INTO profile_settings(profile_id,key,value) VALUES(?1,?2,'1')", params![profile_id, added_key(g.id)])?;
    }
    Ok(())
}

/// A linha da biblioteca é de um jogo integrado oficial?
pub fn library_source_is_integrated(conn: &Connection, game_id: i64) -> bool {
    conn.query_row("SELECT source, external_id FROM games WHERE id=?1", [game_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
        .map(|(s, e)| s == "nexstage" && e.as_deref().is_some_and(is_official))
        .unwrap_or(false)
}

/// Executável da instalação registrada, se a pasta ainda tem o jogo completo.
pub fn installed_exe(conn: &Connection, g: &Official) -> Option<PathBuf> {
    let dir: Option<String> = conn.query_row("SELECT install_dir FROM integrated_games WHERE game_id=?1", [g.id], |r| r.get(0)).ok().flatten();
    let dir = PathBuf::from(dir?);
    let exe = dir.join(g.exe);
    (exe.is_file() && dir.join(g.data_file).is_file()).then_some(exe)
}

/// Atualiza o executável na linha da biblioteca (todos os perfis enxergam o mesmo jogo instalado).
fn sync_library_exe(conn: &Connection, g: &Official) -> AppResult<()> {
    let exe = installed_exe(conn, g).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    conn.execute("UPDATE games SET executable_path=?1, launch_args='' WHERE source='nexstage' AND external_id=?2", params![exe, g.id])?;
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedStatus {
    pub game_id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub install_dir: Option<String>,
    /// `true` = instalado pelo NexStage (atualizações gerenciadas); `false` = pasta localizada pelo usuário.
    pub managed: bool,
    pub build: Option<i64>,
    /// Versão anterior guardada (para "Voltar versão").
    pub previous_version: Option<String>,
    /// Última versão publicada (depois de "Procurar atualização").
    pub latest: Option<Latest>,
    pub update_available: bool,
    /// Download/instalação em andamento ou pausado.
    pub job: Option<JobState>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub version: String,
    pub build: i64,
    pub size: u64,
    pub notes: serde_json::Value,
    pub released_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    /// "manifest" | "downloading" | "verifying" | "installing" | "paused"
    pub phase: String,
    pub version: String,
    pub done: u64,
    pub total: u64,
}

/// Tarefas de instalação em andamento e a última versão conhecida de cada jogo (memória da sessão).
#[derive(Default)]
pub struct Installs {
    jobs: Mutex<HashMap<String, (Arc<install::Control>, JobState)>>,
    latest: Mutex<HashMap<String, Latest>>,
}

/// Pasta gerenciada do jogo: `%LOCALAPPDATA%\NEXSTAGE\games\<game_id>`.
pub fn layout(app: &AppHandle, g: &Official) -> AppResult<install::Layout> {
    let base = app.path().local_data_dir().map_err(|e| AppError::Other(e.to_string()))?;
    Ok(install::Layout::new(base.join("NEXSTAGE").join("games").join(g.id)))
}

fn status_full(app: &AppHandle, conn: &Connection, g: &Official) -> AppResult<IntegratedStatus> {
    let mut s = status(conn, g)?;
    let lay = layout(app, g)?;
    if s.managed {
        s.previous_version = lay.previous();
    }
    let st = app.state::<Installs>();
    s.latest = st.latest.lock().ok().and_then(|m| m.get(g.id).cloned());
    s.update_available = match (&s.latest, s.build) {
        (Some(l), Some(b)) if s.managed && s.installed => l.build > b,
        (Some(_), _) => !s.installed,
        _ => false,
    };
    s.job = st.jobs.lock().ok().and_then(|m| m.get(g.id).map(|(_, j)| j.clone()));
    Ok(s)
}

fn status(conn: &Connection, g: &Official) -> AppResult<IntegratedStatus> {
    let (version, dir, build): (Option<String>, Option<String>, Option<i64>) = conn
        .query_row("SELECT installed_version, install_dir, installed_build FROM integrated_games WHERE game_id=?1", [g.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?
        .unwrap_or((None, None, None));
    let installed = installed_exe(conn, g).is_some();
    let managed = dir.as_deref().is_some_and(|d| crate::integrated::is_managed_dir(d));
    Ok(IntegratedStatus { game_id: g.id.into(), name: g.name.into(), installed, version, install_dir: dir, managed, build, previous_version: None, latest: None, update_available: false, job: None })
}

/// Pasta gerenciada pelo instalador do NexStage (Fase 5): `%LOCALAPPDATA%\NEXSTAGE\games\...`.
pub fn is_managed_dir(dir: &str) -> bool {
    let d = dir.replace('/', "\\").to_lowercase();
    d.contains("\\nexstage\\games\\")
}

fn get(id: &str) -> AppResult<&'static Official> {
    official(id).ok_or_else(|| AppError::NotFound("Jogo integrado não encontrado.".into()))
}

/// Versão escrita pelo jogo exportado (arquivo `version.txt` ao lado do executável), quando existir.
fn read_version(dir: &Path) -> Option<String> {
    let v = std::fs::read_to_string(dir.join("version.txt")).ok()?;
    let v = v.trim().to_string();
    (!v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))).then_some(v)
}

#[tauri::command]
pub fn integrated_status(app: AppHandle, db: State<Db>, game_id: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    seed(&conn)?;
    status_full(&app, &conn, g)
}

/// Conquistas do jogo (catálogo do manifesto assinado) com o estado do perfil.
#[tauri::command]
pub fn integrated_achievements(db: State<Db>, profile_id: i64, game_id: String) -> AppResult<Vec<crate::bridge::achievements::AchievementView>> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    crate::bridge::achievements::list(&conn, profile_id, g.id)
}

/// Informações fixas do jogo oficial para a página especial.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedInfo {
    pub game_id: String,
    pub name: String,
    pub description: HashMap<String, String>,
    pub releases_url: String,
}

#[tauri::command]
pub fn integrated_info(game_id: String) -> AppResult<IntegratedInfo> {
    let g = get(&game_id)?;
    let description = HashMap::from([("pt-BR".to_string(), g.description[0].to_string()), ("en".to_string(), g.description[1].to_string())]);
    Ok(IntegratedInfo { game_id: g.id.into(), name: g.name.into(), description, releases_url: g.releases_url.into() })
}

/// Slots do jogo usados neste perfil, com os números principais.
#[tauri::command]
pub fn integrated_slots(db: State<Db>, profile_id: i64, game_id: String) -> AppResult<Vec<crate::bridge::statistics::SlotView>> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    crate::bridge::statistics::slots(&conn, profile_id, g.id)
}

/// Estatísticas do jogo no perfil (soma dos slots do perfil; definições do manifesto assinado).
#[tauri::command]
pub fn integrated_stats(db: State<Db>, profile_id: i64, game_id: String) -> AppResult<crate::bridge::statistics::ProfileStats> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    crate::bridge::statistics::profile(&conn, profile_id, g.id)
}

/// "Localizar instalação": usa uma cópia do jogo já existente no PC (ex.: o zip baixado manualmente).
/// Só aceita o executável oficial com o arquivo de dados ao lado; nada é copiado nem alterado.
#[tauri::command]
pub fn integrated_locate(app: AppHandle, db: State<Db>, game_id: String, path: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    let exe = PathBuf::from(path.trim());
    let name_ok = exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(g.exe));
    if !name_ok || !exe.is_file() {
        return Err(AppError::Validation(format!("Escolha o arquivo {} do jogo.", g.exe)));
    }
    let dir = exe.parent().ok_or_else(|| AppError::Validation("Pasta do jogo inválida.".into()))?.to_path_buf();
    if !dir.join(g.data_file).is_file() {
        return Err(AppError::Validation(format!("O arquivo {} precisa estar na mesma pasta do executável.", g.data_file)));
    }
    let conn = db.conn()?;
    seed(&conn)?;
    conn.execute(
        "UPDATE integrated_games SET install_dir=?1, installed_version=?2 WHERE game_id=?3",
        params![dir.to_string_lossy(), read_version(&dir), g.id],
    )?;
    sync_library_exe(&conn, g)?;
    status_full(&app, &conn, g)
}

/// Esquece a instalação localizada (não apaga nenhum arquivo). Instalação gerenciada: use Desinstalar (Fase 5).
#[tauri::command]
pub fn integrated_unlink(app: AppHandle, db: State<Db>, game_id: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    if status(&conn, g)?.managed {
        return Err(AppError::Validation("Instalação feita pelo NexStage: use Desinstalar.".into()));
    }
    conn.execute("UPDATE integrated_games SET install_dir=NULL, installed_version=NULL, installed_build=NULL WHERE game_id=?1", [g.id])?;
    sync_library_exe(&conn, g)?;
    status_full(&app, &conn, g)
}

// ------------------------------------------------------------ distribuição (Fase 5)

fn emit_status(app: &AppHandle, g: &Official) {
    if let Ok(conn) = app.state::<Db>().conn() {
        if let Ok(s) = status_full(app, &conn, g) {
            let _ = app.emit("integrated-status", s);
        }
    }
}

fn app_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Baixa e verifica o manifesto mais recente (assinatura e regras). Devolve o manifesto e os bytes exatos.
async fn latest_manifest(app: &AppHandle, g: &Official) -> AppResult<(manifest::Manifest, Vec<u8>)> {
    let net = manifest::NetRules::default();
    let client = install::client(net.clone());
    let raw = install::fetch_bytes(&client, &net, g.manifest_url, 1 << 20).await?;
    let sig = install::fetch_bytes(&client, &net, g.signature_url, 4096).await?;
    let m = manifest::verify_and_parse(&raw, &String::from_utf8_lossy(&sig), g.public_key, g.id, &app_version(app), &net)?;
    // catálogo de conquistas: só daqui (manifesto com assinatura conferida)
    if let Ok(mut conn) = app.state::<Db>().conn() {
        if let Err(e) = crate::bridge::achievements::save_catalog(&mut conn, g.id, &m.catalog) {
            crate::diaglog::write("aviso", "integrated", &format!("catálogo de {}: {e}", g.id));
        }
        if let Err(e) = crate::bridge::statistics::save_catalog(&mut conn, g.id, &m.catalog) {
            crate::diaglog::write("aviso", "integrated", &format!("estatísticas de {}: {e}", g.id));
        }
    }
    let latest = Latest { version: m.version.clone(), build: m.build, size: m.total_size(), notes: m.notes.clone(), released_at: m.released_at.clone() };
    if let Ok(mut l) = app.state::<Installs>().latest.lock() {
        l.insert(g.id.to_string(), latest);
    }
    Ok((m, raw))
}

/// Ao abrir o app (depois de um tempo, sem atrasar a abertura): procura versão nova dos jogos integrados
/// instalados e traz o catálogo de conquistas. Avisa uma vez por versão (`integrated_update_notified:<id>`).
pub fn spawn_startup_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        for g in OFFICIAL {
            if latest_manifest(&app, g).await.is_err() {
                continue; // sem internet: tenta na próxima abertura
            }
            let db = app.state::<Db>();
            let Ok(conn) = db.conn() else { continue };
            let Ok(st) = status_full(&app, &conn, g) else { continue };
            let Some(latest) = st.latest.as_ref().filter(|_| st.installed && st.update_available) else { continue };
            let key = format!("integrated_update_notified:{}", g.id);
            let seen: Option<String> = conn.query_row("SELECT value FROM app_settings WHERE key=?1", [&key], |r| r.get(0)).optional().ok().flatten();
            if seen.as_deref() == Some(latest.version.as_str()) {
                continue;
            }
            let _ = conn.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", params![key, latest.version]);
            let _ = app.emit("integrated-update-available", serde_json::json!({ "gameId": g.id, "name": g.name, "version": latest.version }));
            let _ = app.emit("integrated-status", &st);
        }
    });
}

/// "Procurar atualização": consulta o manifesto publicado (precisa de internet; o jogo funciona sem).
#[tauri::command]
pub async fn integrated_check_update(app: AppHandle, game_id: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    latest_manifest(&app, g).await?;
    let db = app.state::<Db>();
    let conn = db.conn()?;
    status_full(&app, &conn, g)
}

fn game_running(app: &AppHandle, g: &Official) -> bool {
    let lib: Option<i64> = app.state::<Db>().conn().ok().and_then(|c| {
        c.query_row("SELECT id FROM games WHERE source='nexstage' AND external_id=?1", [g.id], |r| r.get(0)).optional().ok().flatten()
    });
    lib.is_some_and(|id| app.state::<crate::overlay::Runtime>().is_game_running(id))
}

fn set_job(app: &AppHandle, g: &Official, job: Option<(Arc<install::Control>, JobState)>) {
    if let Ok(mut m) = app.state::<Installs>().jobs.lock() {
        match job {
            Some(j) => { m.insert(g.id.to_string(), j); }
            None => { m.remove(g.id); }
        }
    }
}

/// Instala ou atualiza para a versão mais recente (também retoma um download pausado). Roda em segundo plano;
/// o progresso chega pelo evento `integrated-progress` e o fim por `integrated-status` / `integrated-error`.
#[tauri::command]
pub async fn integrated_install(app: AppHandle, game_id: String) -> AppResult<()> {
    let g = get(&game_id)?;
    if game_running(&app, g) {
        return Err(AppError::Validation("Feche o jogo antes de instalar ou atualizar.".into()));
    }
    {
        let st = app.state::<Installs>();
        let jobs = st.jobs.lock().map_err(|_| AppError::Other("Estado indisponível.".into()))?;
        if jobs.get(g.id).is_some_and(|(_, j)| j.phase != "paused") {
            return Err(AppError::Validation("A instalação já está em andamento.".into()));
        }
    }
    let ctl = Arc::new(install::Control::default());
    set_job(&app, g, Some((ctl.clone(), JobState { phase: "manifest".into(), version: String::new(), done: 0, total: 0 })));
    emit_status(&app, g);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let r = run_install(&app2, g, ctl).await;
        match r {
            Ok(install::Outcome::Paused) => {}
            Ok(_) => set_job(&app2, g, None),
            Err(e) => {
                set_job(&app2, g, None);
                crate::diaglog::write("erro", "integrated", &format!("instalação de {}: {e}", g.id));
                let _ = app2.emit("integrated-error", serde_json::json!({ "gameId": g.id, "message": e.to_string() }));
            }
        }
        emit_status(&app2, g);
    });
    Ok(())
}

async fn run_install(app: &AppHandle, g: &'static Official, ctl: Arc<install::Control>) -> AppResult<install::Outcome> {
    let (m, raw) = latest_manifest(app, g).await?;
    let lay = layout(app, g)?;
    let net = manifest::NetRules::default();
    let client = install::client(net.clone());
    let total = m.total_size();
    let version = m.version.clone();
    set_job(app, g, Some((ctl.clone(), JobState { phase: "downloading".into(), version: version.clone(), done: 0, total })));
    let app_p = app.clone();
    let ctl_p = ctl.clone();
    let on = move |p: install::Progress| {
        set_job(&app_p, g, Some((ctl_p.clone(), JobState { phase: p.phase.into(), version: version.clone(), done: p.done, total: p.total })));
        let _ = app_p.emit("integrated-progress", serde_json::json!({ "gameId": g.id, "phase": p.phase, "file": p.file, "done": p.done, "total": p.total }));
    };
    let outcome = install::install(&lay, &m, &raw, &client, &net, &ctl, &on).await?;
    match &outcome {
        install::Outcome::Installed { .. } => {
            let db = app.state::<Db>();
            let conn = db.conn()?;
            seed(&conn)?;
            conn.execute(
                "UPDATE integrated_games SET install_dir=?1, installed_version=?2, installed_build=?3, manifest=?4, manifest_checked_at=datetime('now') WHERE game_id=?5",
                params![lay.version_dir(&m.version).to_string_lossy(), m.version, m.build, String::from_utf8_lossy(&raw), g.id],
            )?;
            sync_library_exe(&conn, g)?;
            crate::diaglog::write("info", "integrated", &format!("{} {} instalado", g.id, m.version));
            let _ = app.emit("integrated-installed", serde_json::json!({ "gameId": g.id, "name": g.name, "version": m.version }));
        }
        install::Outcome::Paused => {
            set_job(app, g, Some((ctl.clone(), JobState { phase: "paused".into(), version: m.version.clone(), done: 0, total })));
        }
        install::Outcome::Canceled => {}
    }
    Ok(outcome)
}

#[tauri::command]
pub fn integrated_pause(app: AppHandle, game_id: String) -> AppResult<()> {
    let g = get(&game_id)?;
    if let Some((ctl, _)) = app.state::<Installs>().jobs.lock().ok().and_then(|m| m.get(g.id).cloned()) {
        ctl.pause.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub fn integrated_cancel(app: AppHandle, game_id: String) -> AppResult<()> {
    let g = get(&game_id)?;
    let job = app.state::<Installs>().jobs.lock().ok().and_then(|m| m.get(g.id).cloned());
    match job {
        Some((ctl, j)) if j.phase == "paused" => {
            // pausado: não há tarefa rodando, então o parcial é apagado aqui
            let _ = std::fs::remove_dir_all(layout(&app, g)?.base.join("staging"));
            set_job(&app, g, None);
            emit_status(&app, g);
            let _ = ctl;
        }
        Some((ctl, _)) => ctl.cancel.store(true, Ordering::SeqCst),
        None => {}
    }
    Ok(())
}

/// Confere o SHA-256 de todos os arquivos da versão instalada. Devolve os arquivos com problema.
#[tauri::command]
pub async fn integrated_verify(app: AppHandle, game_id: String) -> AppResult<Vec<String>> {
    let g = get(&game_id)?;
    let lay = layout(&app, g)?;
    tauri::async_runtime::spawn_blocking(move || install::verify(&lay)).await.map_err(|e| AppError::Other(e.to_string()))?
}

#[tauri::command]
pub fn integrated_rollback(app: AppHandle, db: State<Db>, game_id: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    if game_running(&app, g) {
        return Err(AppError::Validation("Feche o jogo antes de trocar de versão.".into()));
    }
    let lay = layout(&app, g)?;
    let v = install::rollback(&lay)?;
    let m = lay.manifest_of(&v);
    let conn = db.conn()?;
    conn.execute(
        "UPDATE integrated_games SET install_dir=?1, installed_version=?2, installed_build=?3 WHERE game_id=?4",
        params![lay.version_dir(&v).to_string_lossy(), v, m.map(|m| m.build), g.id],
    )?;
    sync_library_exe(&conn, g)?;
    status_full(&app, &conn, g)
}

/// Apaga os arquivos do jogo instalado pelo NexStage. Saves, conquistas e tempo de jogo continuam.
#[tauri::command]
pub fn integrated_uninstall(app: AppHandle, db: State<Db>, game_id: String) -> AppResult<IntegratedStatus> {
    let g = get(&game_id)?;
    if game_running(&app, g) {
        return Err(AppError::Validation("Feche o jogo antes de desinstalar.".into()));
    }
    install::uninstall(&layout(&app, g)?)?;
    let conn = db.conn()?;
    conn.execute("UPDATE integrated_games SET install_dir=NULL, installed_version=NULL, installed_build=NULL, manifest=NULL WHERE game_id=?1", [g.id])?;
    sync_library_exe(&conn, g)?;
    status_full(&app, &conn, g)
}

/// Põe o jogo integrado de volta na biblioteca do perfil (depois de removido pelo usuário).
#[tauri::command]
pub fn integrated_add_to_library(db: State<Db>, profile_id: i64, game_id: String) -> AppResult<i64> {
    let g = get(&game_id)?;
    let conn = db.conn()?;
    seed(&conn)?;
    let id = library_row(&conn, g)?;
    conn.execute("INSERT OR IGNORE INTO profile_games(profile_id,game_id) VALUES(?1,?2)", params![profile_id, id])?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db(name: &str) -> (std::path::PathBuf, crate::db::Db) {
        let p = std::env::temp_dir().join(format!("nexstage-test-{}-integrated-{name}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        let d = crate::db::Db::open(&p).unwrap();
        (p, d)
    }

    #[test]
    fn appears_once_per_profile_and_respects_removal() {
        let (_p, d) = db("library");
        let c = d.conn().unwrap();
        c.execute_batch("INSERT INTO profiles(id,username,display_name) VALUES(1,'a','A'),(2,'b','B');").unwrap();
        ensure_library(&c, 1).unwrap();
        ensure_library(&c, 1).unwrap();
        ensure_library(&c, 2).unwrap();
        let rows: i64 = c.query_row("SELECT COUNT(*) FROM games WHERE source='nexstage'", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1, "uma linha compartilhada");
        let links: i64 = c.query_row("SELECT COUNT(*) FROM profile_games pg JOIN games g ON g.id=pg.game_id WHERE g.source='nexstage'", [], |r| r.get(0)).unwrap();
        assert_eq!(links, 2, "um vínculo por perfil");
        // o perfil 1 remove: não volta sozinho
        c.execute("DELETE FROM profile_games WHERE profile_id=1", []).unwrap();
        ensure_library(&c, 1).unwrap();
        let back: i64 = c.query_row("SELECT COUNT(*) FROM profile_games WHERE profile_id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(back, 0);
    }

    /// Ponta a ponta com o release REAL publicado (precisa de internet; ~200 MB):
    ///   cargo test --lib integrated::tests::e2e_real_release -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn e2e_real_release() {
        let g = official("tomb-survivors").unwrap();
        let net = manifest::NetRules::default();
        let c = install::client(net.clone());
        let raw = install::fetch_bytes(&c, &net, g.manifest_url, 1 << 20).await.unwrap();
        let sig = install::fetch_bytes(&c, &net, g.signature_url, 4096).await.unwrap();
        let m = manifest::verify_and_parse(&raw, &String::from_utf8_lossy(&sig), g.public_key, g.id, "1.3.1", &net).unwrap();
        println!("manifesto ok: {} build {} ({} arquivos, {} MB)", m.version, m.build, m.files.len(), m.total_size() / 1048576);
        let dir = std::env::temp_dir().join(format!("nexstage-e2e-release-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lay = install::Layout::new(dir);
        let r = install::install(&lay, &m, &raw, &c, &net, &install::Control::default(), &|_p| {}).await.unwrap();
        assert!(matches!(r, install::Outcome::Installed { .. }));
        assert!(install::verify(&lay).unwrap().is_empty(), "SHA-256 de todos os arquivos confere");
        assert!(lay.version_dir(&m.version).join(g.exe).is_file() && lay.version_dir(&m.version).join(g.data_file).is_file());
        install::uninstall(&lay).unwrap();
    }

    #[test]
    fn located_install_requires_exe_and_data_file() {
        let (_p, d) = db("locate");
        let dir = std::env::temp_dir().join(format!("nexstage-int-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("TombSurvivors.exe"), b"x").unwrap();
        let c = d.conn().unwrap();
        seed(&c).unwrap();
        let g = official("tomb-survivors").unwrap();
        c.execute("UPDATE integrated_games SET install_dir=?1 WHERE game_id='tomb-survivors'", [dir.to_string_lossy()]).unwrap();
        assert!(installed_exe(&c, g).is_none(), "sem o .pck não está instalado");
        std::fs::write(dir.join("TombSurvivors.pck"), b"x").unwrap();
        std::fs::write(dir.join("version.txt"), b"0.2.0-alpha\n").unwrap();
        assert!(installed_exe(&c, g).is_some());
        assert_eq!(read_version(&dir).as_deref(), Some("0.2.0-alpha"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
