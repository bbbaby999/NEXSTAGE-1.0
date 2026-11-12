//! Backup e restauração do NEXSTAGE (serviço central: manual, automático e de segurança usam o mesmo código).
//!
//! O que entra (auditoria dos locais de armazenamento):
//! - `data/database.db`: TODO o banco SQLite — perfis (com avatar e banner), proteção dos perfis (hashes),
//!   configurações globais e por perfil, biblioteca, plataformas, favoritos, coleções, histórico e tempo de
//!   jogo, emuladores e associações, pastas de jogos, informações dos jogos, conquistas (locais, Steam e
//!   RetroAchievements), contas e vínculos das integrações, mods favoritos, controles e atalhos.
//!   Cópia consistente via `VACUUM INTO` com o banco travado (nada é gravado durante a cópia).
//! - `data/secrets.json`: credenciais (só em backups com senha; ver `secrets`).
//! - `images/`: capas e fundos dos jogos e capas dos emuladores.
//! - `gallery/<perfil>/`: capturas da Galeria de cada perfil (favoritos e jogo associado ficam no banco).
//! - `captures/` (opcional): a pasta Imagens\NEXSTAGE\Capturas com os arquivos originais.
//!
//! O que NÃO entra: jogos, emuladores, BIOS, instaladores (só caminhos e associações, no banco); dados
//! deste computador (posição da janela, entrada de inicialização do Windows — reaplicada a partir da
//! configuração restaurada) e as próprias preferências de backup automático (pasta, senha, data).
//!
//! Estrutura: pasta `NEXSTAGE_Backup` com `nexstage-backup.json` (identificação, versões, resumo e
//! SHA-256 de cada arquivo). Com senha, todos os arquivos são cifrados (`crypto`).

pub mod auto;
pub mod crypto;
pub mod secrets;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub const FORMAT: &str = "nexstage-backup";
/// Versão do formato da pasta/manifesto. Aumente ao mudar a estrutura e trate as anteriores em `upgrade`.
pub const FORMAT_VERSION: u32 = 1;
pub const MANIFEST: &str = "nexstage-backup.json";
pub const FOLDER: &str = "NEXSTAGE_Backup";
pub const CANCELED: &str = "Operação cancelada.";
const DB_REL: &str = "data/database.db";
const SECRETS_REL: &str = "data/secrets.json";
const SAFETY_DIR: &str = "safety-backups";
const SAFETY_KEEP: usize = 2;
const IMAGE_EXT: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Manual,
    Auto,
    /// Cópia local feita antes de uma restauração (fica na pasta de dados do app).
    Safety,
}
impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Manual => "manual",
            Kind::Auto => "auto",
            Kind::Safety => "safety",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub profiles: i64,
    /// Nomes de exibição (só em backups sem senha; com senha, o manifesto não identifica ninguém).
    #[serde(default)]
    pub profile_names: Vec<String>,
    pub games: i64,
    pub library_entries: i64,
    pub favorites: i64,
    pub sessions: i64,
    pub playtime_seconds: i64,
    pub collections: i64,
    pub emulators: i64,
    pub gallery: i64,
    pub images: i64,
    pub captures: i64,
    pub achievements: i64,
    pub accounts: i64,
    pub mod_favorites: i64,
    pub settings: i64,
    pub total_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Encryption {
    pub alg: String,
    pub kdf: String,
    pub m: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
    pub check: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    pub kind: String,
    pub schema_version: i64,
    pub encryption: Option<Encryption>,
    /// "included" (com senha) | "excluded" (sem senha) | "local" (backup de segurança deste computador).
    pub secrets: String,
    pub summary: Summary,
    pub files: Vec<FileEntry>,
}

/* ---------------------------------------------------------------------------------------------
 * Controle da operação: uma por vez, progresso e cancelamento
 * ------------------------------------------------------------------------------------------- */

static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
static CANCELABLE: AtomicBool = AtomicBool::new(false);

pub struct Busy;
impl Busy {
    pub fn take() -> AppResult<Self> {
        BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| AppError::Validation("Já existe um backup ou uma restauração em andamento.".into()))?;
        CANCEL.store(false, Ordering::SeqCst);
        CANCELABLE.store(true, Ordering::SeqCst);
        Ok(Busy)
    }
}
impl Drop for Busy {
    fn drop(&mut self) {
        CANCELABLE.store(false, Ordering::SeqCst);
        BUSY.store(false, Ordering::SeqCst);
    }
}
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}
/// Pede o cancelamento (só tem efeito enquanto a etapa atual permite).
pub fn cancel() -> bool {
    if CANCELABLE.load(Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}
pub fn is_canceled(e: &AppError) -> bool {
    matches!(e, AppError::Other(s) if s == CANCELED)
}

/// Progresso enviado à interface pelo evento `backup-progress`.
pub struct Op {
    emit: Box<dyn Fn(serde_json::Value) + Send>,
    op: &'static str,
    kind: String,
    phase: &'static str,
    alias: Option<&'static str>,
    done: u64,
    total: u64,
    last: Instant,
}

impl Op {
    pub fn new(app: &AppHandle, op: &'static str, kind: &str) -> Self {
        let a = app.clone();
        Self::with_emitter(Box::new(move |v| { let _ = a.emit("backup-progress", v); }), op, kind)
    }
    pub fn with_emitter(emit: Box<dyn Fn(serde_json::Value) + Send>, op: &'static str, kind: &str) -> Self {
        Op { emit, op, kind: kind.to_string(), phase: "start", alias: None, done: 0, total: 0, last: Instant::now() }
    }
    pub fn phase(&mut self, p: &'static str, total: u64) {
        self.phase = p;
        self.done = 0;
        self.total = total;
        self.emit();
    }
    pub fn check(&self) -> AppResult<()> {
        if CANCEL.load(Ordering::SeqCst) && CANCELABLE.load(Ordering::SeqCst) {
            return Err(AppError::Other(CANCELED.into()));
        }
        Ok(())
    }
    pub fn add(&mut self, n: u64) -> AppResult<()> {
        self.done += n;
        self.check()?;
        if self.last.elapsed() >= Duration::from_millis(120) {
            self.emit();
        }
        Ok(())
    }
    pub fn set_cancelable(&mut self, v: bool) {
        CANCELABLE.store(v, Ordering::SeqCst);
        self.emit();
    }
    fn emit(&mut self) {
        self.last = Instant::now();
        (self.emit)(
            serde_json::json!({
                "op": self.op, "kind": self.kind, "phase": self.alias.unwrap_or(self.phase),
                "done": self.done, "total": self.total, "cancelable": CANCELABLE.load(Ordering::SeqCst),
            }),
        );
    }
}

/* ---------------------------------------------------------------------------------------------
 * Utilidades
 * ------------------------------------------------------------------------------------------- */

pub fn app_data(app: &AppHandle) -> AppResult<PathBuf> {
    app.path().app_data_dir().map_err(|e| AppError::Other(e.to_string()))
}

/// O que o serviço de backup precisa do app (separado do Tauri: testável sem janela).
pub struct Ctx<'a> {
    pub data: PathBuf,
    pub captures: Option<PathBuf>,
    pub version: String,
    pub db: &'a Db,
}
impl<'a> Ctx<'a> {
    pub fn new(app: &'a AppHandle) -> AppResult<Self> {
        Ok(Ctx {
            data: app_data(app)?,
            captures: crate::screenshot::captures_dir(app).ok(),
            version: app.package_info().version.to_string(),
            db: app.state::<Db>().inner(),
        })
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn rand_tag() -> String {
    crypto::random::<6>().map(|b| hex(&b)).unwrap_or_else(|_| format!("{}", std::process::id()))
}

/// Pasta removida ao sair do escopo (a menos que `keep()` seja chamado).
struct TempDir(PathBuf, bool);
impl TempDir {
    fn new(p: PathBuf) -> AppResult<Self> {
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p)?;
        Ok(TempDir(p, false))
    }
    fn keep(&mut self) {
        self.1 = true;
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

struct HashRead<R> {
    r: R,
    h: Sha256,
}
impl<R: Read> Read for HashRead<R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = self.r.read(b)?;
        self.h.update(&b[..n]);
        Ok(n)
    }
}
struct HashWrite<W> {
    w: W,
    h: Sha256,
    n: u64,
}
impl<W: Write> Write for HashWrite<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let n = self.w.write(b)?;
        self.h.update(&b[..n]);
        self.n += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

fn copy(r: &mut impl Read, w: &mut impl Write, tick: &mut dyn FnMut(u64) -> AppResult<()>) -> AppResult<()> {
    let mut buf = vec![0u8; crypto::CHUNK];
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        w.write_all(&buf[..n])?;
        tick(n as u64)?;
    }
    Ok(())
}

/// Caminho relativo seguro vindo de um manifesto (nunca sai da pasta do backup ou da pasta de destino).
fn safe_rel(p: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = p.split('/').collect();
    if parts.len() < 2 || !matches!(parts[0], "data" | "images" | "gallery" | "captures") {
        return None;
    }
    let mut out = PathBuf::new();
    for c in parts {
        let bad = c.is_empty() || c == "." || c == ".." || c.len() > 200
            || c.chars().any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'));
        if bad {
            return None;
        }
        out.push(c);
    }
    Some(out)
}

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| IMAGE_EXT.contains(&e.to_lowercase().as_str())).unwrap_or(false)
}

/// "1.2.3" → (1,2,3); sufixos como "-beta" são ignorados.
fn version_tuple(v: &str) -> (u64, u64, u64) {
    let mut it = v.split(|c: char| c == '.' || c == '-' || c == '+').map(|s| s.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn local_now(conn: &Connection, fmt: &str) -> String {
    conn.query_row("SELECT strftime(?1,'now','localtime')", [fmt], |r| r.get(0)).unwrap_or_default()
}

fn user_version(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn summarize(conn: &Connection, with_names: bool) -> Summary {
    let n = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let names = if with_names {
        conn.prepare("SELECT display_name FROM profiles ORDER BY id")
            .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Summary {
        profiles: n("SELECT COUNT(*) FROM profiles"),
        profile_names: names,
        games: n("SELECT COUNT(*) FROM games"),
        library_entries: n("SELECT COUNT(*) FROM profile_games"),
        favorites: n("SELECT COUNT(*) FROM profile_games WHERE favorite=1"),
        sessions: n("SELECT COUNT(*) FROM play_sessions"),
        playtime_seconds: n("SELECT COALESCE(SUM(playtime_seconds),0) FROM profile_games"),
        collections: n("SELECT COUNT(*) FROM collections"),
        emulators: n("SELECT COUNT(*) FROM emulators"),
        gallery: n("SELECT COUNT(*) FROM gallery_items"),
        images: 0,
        captures: 0,
        achievements: n("SELECT COUNT(*) FROM profile_achievements") + n("SELECT COUNT(*) FROM ra_unlocks"),
        accounts: n("SELECT COUNT(*) FROM platform_accounts") + n("SELECT COUNT(*) FROM data_sources WHERE enabled=1"),
        mod_favorites: n("SELECT COUNT(*) FROM nexus_favorites"),
        settings: n("SELECT COUNT(*) FROM app_settings") + n("SELECT COUNT(*) FROM profile_settings"),
        total_bytes: 0,
    }
}

pub fn read_manifest(root: &Path) -> AppResult<Manifest> {
    let p = root.join(MANIFEST);
    let meta = fs::metadata(&p).map_err(|_| AppError::Validation("A pasta selecionada não é um backup do NEXSTAGE (arquivo de identificação ausente).".into()))?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err(AppError::Validation("Arquivo de identificação do backup inválido.".into()));
    }
    let raw = fs::read(&p)?;
    let v: serde_json::Value = serde_json::from_slice(&raw).map_err(|_| AppError::Validation("Arquivo de identificação do backup danificado.".into()))?;
    if v.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err(AppError::Validation("A pasta selecionada não é um backup do NEXSTAGE.".into()));
    }
    let v = upgrade(v)?;
    serde_json::from_value(v).map_err(|_| AppError::Validation("Arquivo de identificação do backup danificado ou de formato desconhecido.".into()))
}

/// Migração do manifesto de formatos anteriores para o atual (hoje só existe a versão 1).
fn upgrade(v: serde_json::Value) -> AppResult<serde_json::Value> {
    let ver = v.get("formatVersion").and_then(|x| x.as_u64()).unwrap_or(0);
    if ver == 0 {
        return Err(AppError::Validation("Backup sem versão de formato: não é possível garantir a compatibilidade.".into()));
    }
    // Formatos mais novos são recusados em `check` (com mensagem clara), não aqui.
    Ok(v)
}

/// Aceita a própria pasta do backup, o arquivo de identificação ou a pasta que contém `NEXSTAGE_Backup`.
pub fn locate(path: &Path) -> AppResult<PathBuf> {
    if path.is_file() && path.file_name().map(|n| n == MANIFEST).unwrap_or(false) {
        return Ok(path.parent().unwrap_or(path).to_path_buf());
    }
    if path.join(MANIFEST).is_file() {
        return Ok(path.to_path_buf());
    }
    if path.join(FOLDER).join(MANIFEST).is_file() {
        return Ok(path.join(FOLDER));
    }
    Err(AppError::Validation("A pasta selecionada não é um backup do NEXSTAGE. Escolha a pasta “NEXSTAGE_Backup”.".into()))
}

pub fn is_backup_dir(p: &Path) -> bool {
    read_manifest(p).is_ok()
}

/* ---------------------------------------------------------------------------------------------
 * Criação
 * ------------------------------------------------------------------------------------------- */

pub struct CreateOpts {
    pub parent: PathBuf,
    pub name: String,
    pub password: Option<String>,
    pub include_captures: bool,
    pub kind: Kind,
    /// Substitui um backup do NEXSTAGE que já exista com o mesmo nome (o antigo só é apagado no fim).
    pub replace: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateReport {
    pub path: String,
    pub bytes: u64,
    pub files: usize,
    pub encrypted: bool,
    pub secrets: String,
    /// Credenciais que ficaram de fora (backup sem senha, ou não legíveis neste Windows).
    pub secrets_skipped: usize,
    /// Capturas registradas na Galeria cujo arquivo não existe mais no disco.
    pub missing: usize,
}

enum Data {
    File(PathBuf),
    Bytes(Vec<u8>),
}
struct Src {
    rel: String,
    data: Data,
    kind: &'static str,
    size: u64,
}

pub fn create(cx: &Ctx, o: &CreateOpts, op: &mut Op) -> AppResult<CreateReport> {
    let data = cx.data.clone();
    let final_dir = o.parent.join(&o.name);
    if final_dir.exists() && (!o.replace || !is_backup_dir(&final_dir)) {
        return Err(AppError::Validation(if is_backup_dir(&final_dir) {
            "Já existe um backup do NEXSTAGE nesse local.".into()
        } else {
            "Já existe uma pasta “NEXSTAGE_Backup” nesse local que não é um backup do NEXSTAGE. Escolha outro local.".into()
        }));
    }
    if let Some(p) = &o.password {
        crypto::validate_password(p)?;
    }
    let local = o.kind == Kind::Safety;

    // 1) Cópia consistente do banco (travado durante a cópia; WAL incluído).
    op.phase("snapshot", 0);
    if !local {
        let _ = fs::remove_dir_all(data.join("backup-work"));
    }
    let work = TempDir::new(data.join("backup-work").join(rand_tag()))?;
    let snap = work.0.join("database.db");
    {
        let conn = cx.db.conn()?;
        conn.execute("VACUUM INTO ?1", [snap.to_string_lossy().to_string()])?;
    }
    op.check()?;

    // 2) Ajustes na cópia: credenciais e dados deste computador.
    let (bundle, secrets_mode, secrets_skipped, schema_version, mut summary, gallery_rows, created_at) = {
        let c = Connection::open(&snap)?;
        let schema_version = user_version(&c)?;
        let summary = summarize(&c, o.password.is_none());
        let rows: Vec<(i64, String)> = c
            .prepare("SELECT profile_id, path FROM gallery_items")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let created_at = local_now(&c, "%Y-%m-%dT%H:%M:%S");
        let (bundle, mode, skipped) = if local {
            (None, "local", 0)
        } else {
            // apagar de verdade (sem restos nas páginas livres do arquivo)
            c.execute_batch("PRAGMA secure_delete=ON;")?;
            let total = secrets::count(&c)?;
            let r = if o.password.is_some() {
                let (b, unreadable) = secrets::extract(&c)?;
                (Some(b), "included", unreadable)
            } else {
                (None, "excluded", total)
            };
            secrets::strip(&c)?;
            c.execute("DELETE FROM app_settings WHERE key LIKE 'backup\\_%' ESCAPE '\\'", [])?;
            c.execute_batch("VACUUM;")?;
            r
        };
        (bundle, mode, skipped, schema_version, summary, rows, created_at)
    };

    // 3) Lista de arquivos.
    let mut srcs: Vec<Src> = Vec::new();
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    srcs.push(Src { rel: DB_REL.into(), size: size(&snap), data: Data::File(snap.clone()), kind: "database" });
    if let Some(b) = &bundle {
        let bytes = serde_json::to_vec(b).map_err(|e| AppError::Other(e.to_string()))?;
        srcs.push(Src { rel: SECRETS_REL.into(), size: bytes.len() as u64, data: Data::Bytes(bytes), kind: "secrets" });
    }
    if let Ok(rd) = fs::read_dir(data.join("images")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if crate::images::valid_name(&name) && e.path().is_file() {
                srcs.push(Src { rel: format!("images/{name}"), size: size(&e.path()), data: Data::File(e.path()), kind: "image" });
                summary.images += 1;
            }
        }
    }
    let gdir = data.join("gallery");
    let mut seen = HashSet::new();
    let mut missing = 0;
    for (pid, p) in &gallery_rows {
        let Some(name) = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()) else { missing += 1; continue };
        let rel = format!("gallery/{pid}/{name}");
        if safe_rel(&rel).is_none() || !seen.insert(rel.clone()) {
            continue;
        }
        let file = [gdir.join(pid.to_string()).join(&name), PathBuf::from(p)].into_iter().find(|f| f.is_file());
        match file {
            Some(f) => srcs.push(Src { rel, size: size(&f), data: Data::File(f), kind: "gallery" }),
            None => missing += 1,
        }
    }
    if o.include_captures && !local {
        if let Ok(rd) = cx.captures.as_ref().ok_or(()).and_then(|d| fs::read_dir(d).map_err(|_| ())) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let rel = format!("captures/{name}");
                if p.is_file() && is_image(&p) && safe_rel(&rel).is_some() {
                    srcs.push(Src { rel, size: size(&p), data: Data::File(p), kind: "capture" });
                    summary.captures += 1;
                }
            }
        }
    }
    let total: u64 = srcs.iter().map(|s| s.size).sum();
    summary.total_bytes = total;

    // 4) Grava numa pasta temporária ao lado do destino; só vira `NEXSTAGE_Backup` quando tudo deu certo.
    fs::create_dir_all(&o.parent)?;
    let mut tmp = TempDir::new(o.parent.join(format!(".{}.partial-{}", o.name, rand_tag())))?;
    let (key, encryption) = match &o.password {
        Some(p) => {
            let salt = crypto::random::<16>()?;
            let key = crypto::derive(p, &salt, crypto::KDF_M, crypto::KDF_T, crypto::KDF_P)?;
            let check = crypto::make_check(&key)?;
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD;
            let enc = Encryption {
                alg: "AES-256-GCM".into(), kdf: "argon2id".into(), m: crypto::KDF_M, t: crypto::KDF_T, p: crypto::KDF_P,
                salt: b64.encode(salt), check: b64.encode(check),
            };
            (Some(key), Some(enc))
        }
        None => (None, None),
    };
    op.phase("files", total);
    let mut files = Vec::with_capacity(srcs.len());
    for s in &srcs {
        op.check()?;
        let rel = safe_rel(&s.rel).ok_or_else(|| AppError::Other("Nome de arquivo inválido.".into()))?;
        let dest = tmp.0.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        // Backup de segurança: capas e capturas não mudam de conteúdo (nome = hash), então um link
        // físico basta (instantâneo e sem ocupar espaço).
        if local && matches!(s.kind, "image" | "gallery") {
            if let Data::File(src) = &s.data {
                let stem = src.file_stem().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                if stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()) && fs::hard_link(src, &dest).is_ok() {
                    files.push(FileEntry { path: s.rel.clone(), size: s.size, sha256: stem, kind: s.kind.into() });
                    op.add(s.size)?;
                    continue;
                }
            }
        }
        let mut w = HashWrite { w: BufWriter::new(fs::File::create(&dest)?), h: Sha256::new(), n: 0 };
        let mut tick = |n: u64| op.add(n);
        match &s.data {
            Data::File(p) => {
                let mut r = fs::File::open(p)?;
                match &key {
                    Some(k) => crypto::encrypt(k, &mut r, &mut w, &mut tick)?,
                    None => copy(&mut r, &mut w, &mut tick)?,
                }
            }
            Data::Bytes(b) => match &key {
                Some(k) => crypto::encrypt(k, &mut &b[..], &mut w, &mut tick)?,
                None => copy(&mut &b[..], &mut w, &mut tick)?,
            },
        }
        w.flush()?;
        files.push(FileEntry { path: s.rel.clone(), size: w.n, sha256: hex(&w.h.clone().finalize()), kind: s.kind.into() });
    }

    let manifest = Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        app_version: cx.version.clone(),
        created_at,
        kind: o.kind.id().into(),
        schema_version,
        encryption,
        secrets: secrets_mode.into(),
        summary,
        files,
    };
    let bytes: u64 = manifest.files.iter().map(|f| f.size).sum();
    let count = manifest.files.len();
    fs::write(tmp.0.join(MANIFEST), serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::Other(e.to_string()))?)?;
    op.set_cancelable(false);

    // 5) Troca: o backup antigo (se houver) só é apagado depois que o novo está no lugar.
    if final_dir.exists() {
        let old = o.parent.join(format!(".{}.old-{}", o.name, rand_tag()));
        fs::rename(&final_dir, &old)?;
        if let Err(e) = fs::rename(&tmp.0, &final_dir) {
            let _ = fs::rename(&old, &final_dir);
            return Err(e.into());
        }
        let _ = fs::remove_dir_all(&old);
    } else {
        fs::rename(&tmp.0, &final_dir)?;
    }
    tmp.keep();
    Ok(CreateReport {
        path: final_dir.to_string_lossy().into_owned(),
        bytes,
        files: count,
        encrypted: o.password.is_some(),
        secrets: secrets_mode.into(),
        secrets_skipped,
        missing,
    })
}

/* ---------------------------------------------------------------------------------------------
 * Inspeção, compatibilidade e verificação
 * ------------------------------------------------------------------------------------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspect {
    pub path: String,
    pub app_version: String,
    pub created_at: String,
    pub kind: String,
    pub format_version: u32,
    pub encrypted: bool,
    pub needs_password: bool,
    pub secrets: String,
    pub summary: Summary,
    pub files: usize,
    pub total_bytes: u64,
    /// Motivo pelo qual o backup não pode ser restaurado nesta versão (nada é alterado).
    pub problem: Option<String>,
    pub warnings: Vec<String>,
}

/// Lê o manifesto, confere compatibilidade e (se houver) a senha. Não altera nada.
pub fn check(cx: &Ctx, root: &Path, password: Option<&str>) -> AppResult<(Manifest, Inspect, Option<crypto::Key>)> {
    let m = read_manifest(root)?;
    let mut problem = None;
    let mut warnings = Vec::new();
    let current = cx.version.clone();
    let current_schema = {
        let conn = cx.db.conn()?;
        user_version(&conn)?
    };
    if m.format_version > FORMAT_VERSION {
        problem = Some(format!("Este backup usa um formato mais novo (versão {}). Atualize o NEXSTAGE para restaurá-lo.", m.format_version));
    } else if version_tuple(&m.app_version) > version_tuple(&current) {
        problem = Some(format!("Este backup foi criado no NEXSTAGE {}, mais novo que a versão instalada ({current}). Atualize o NEXSTAGE para restaurá-lo.", m.app_version));
    } else if m.schema_version > current_schema {
        problem = Some("O banco de dados deste backup é de uma versão mais nova do NEXSTAGE. Atualize o NEXSTAGE para restaurá-lo.".into());
    } else if m.files.iter().filter(|f| f.kind == "database" && f.path == DB_REL).count() != 1 {
        problem = Some("O backup não contém o banco de dados do NEXSTAGE.".into());
    } else if let Some(f) = m.files.iter().find(|f| safe_rel(&f.path).is_none()) {
        problem = Some(format!("O backup contém um caminho de arquivo inválido ({}).", f.path));
    }
    if version_tuple(&m.app_version) < version_tuple(&current) && problem.is_none() {
        warnings.push(format!("Backup criado no NEXSTAGE {}: os dados serão atualizados para a versão instalada.", m.app_version));
    }
    match m.secrets.as_str() {
        "excluded" => warnings.push("Este backup não contém chaves de API nem credenciais (foi criado sem senha). Depois de restaurar, informe-as novamente nas configurações.".into()),
        "local" => warnings.push("Backup de segurança deste computador: as credenciais só funcionam no mesmo usuário do Windows em que ele foi criado.".into()),
        _ => {}
    }
    let mut key = None;
    let mut needs_password = false;
    if let Some(e) = &m.encryption {
        if e.alg != "AES-256-GCM" || e.kdf != "argon2id" {
            problem.get_or_insert_with(|| "Proteção do backup desconhecida para esta versão do NEXSTAGE.".into());
        } else {
            match password.filter(|p| !p.is_empty()) {
                None => needs_password = true,
                Some(p) => {
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD;
                    let salt = b64.decode(&e.salt).map_err(|_| AppError::Validation("Backup danificado.".into()))?;
                    let chk = b64.decode(&e.check).map_err(|_| AppError::Validation("Backup danificado.".into()))?;
                    let k = crypto::derive(p, &salt, e.m, e.t, e.p)?;
                    if !crypto::verify_check(&k, &chk) {
                        return Err(AppError::Validation("Senha incorreta.".into()));
                    }
                    key = Some(k);
                }
            }
        }
    }
    let total_bytes = m.files.iter().map(|f| f.size).sum();
    let info = Inspect {
        path: root.to_string_lossy().into_owned(),
        app_version: m.app_version.clone(),
        created_at: m.created_at.clone(),
        kind: m.kind.clone(),
        format_version: m.format_version,
        encrypted: m.encryption.is_some(),
        needs_password,
        secrets: m.secrets.clone(),
        summary: m.summary.clone(),
        files: m.files.len(),
        total_bytes,
        problem,
        warnings,
    };
    Ok((m, info, key))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub ok: usize,
    /// Arquivos de mídia ausentes ou danificados (a restauração continua sem eles, se o usuário aceitar).
    pub invalid: Vec<String>,
}

fn critical(kind: &str) -> bool {
    matches!(kind, "database" | "secrets")
}

/// Confere a presença, o tamanho e o SHA-256 de cada arquivo. Banco ou credenciais danificados = erro.
pub fn verify(root: &Path, m: &Manifest, op: &mut Op) -> AppResult<VerifyReport> {
    let total = m.files.iter().map(|f| f.size).sum();
    op.phase("verify", total);
    let mut ok = 0;
    let mut invalid = Vec::new();
    for f in &m.files {
        op.check()?;
        let good = (|| -> AppResult<bool> {
            let Some(rel) = safe_rel(&f.path) else { return Ok(false) };
            let p = root.join(rel);
            let Ok(meta) = fs::metadata(&p) else { return Ok(false) };
            if !meta.is_file() || meta.len() != f.size {
                return Ok(false);
            }
            let mut r = HashRead { r: fs::File::open(&p)?, h: Sha256::new() };
            copy(&mut r, &mut std::io::sink(), &mut |n| op.add(n))?;
            Ok(hex(&r.h.finalize()) == f.sha256.to_lowercase())
        })();
        match good {
            Ok(true) => ok += 1,
            Err(e) if is_canceled(&e) => return Err(e),
            _ if critical(&f.kind) => {
                return Err(AppError::Validation(
                    "O banco de dados ou as credenciais do backup estão ausentes ou danificados. A restauração não é possível e nenhum dado foi alterado.".into(),
                ))
            }
            _ => invalid.push(f.path.clone()),
        }
    }
    Ok(VerifyReport { ok, invalid })
}

/* ---------------------------------------------------------------------------------------------
 * Restauração
 * ------------------------------------------------------------------------------------------- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub safety_path: String,
    pub restored_files: usize,
    pub skipped: Vec<String>,
    pub secrets: String,
    pub secrets_applied: usize,
    pub secrets_failed: usize,
    pub warnings: Vec<String>,
}

/// Restaura: prepara e valida tudo numa pasta temporária, cria o backup de segurança e só então
/// substitui os dados. A troca do banco é atômica (API de backup do SQLite numa única transação);
/// se ela falhar, os arquivos novos são removidos e nada muda.
pub fn restore(cx: &Ctx, root: &Path, m: &Manifest, key: Option<&crypto::Key>, op: &mut Op) -> AppResult<RestoreReport> {
    let data = cx.data.clone();
    let mut warnings = Vec::new();

    // Preferências de backup automático DESTE computador (pasta, senha, data): continuam valendo.
    let local_backup_settings: Vec<(String, String)> = {
        let conn = cx.db.conn()?;
        let mut st = conn.prepare("SELECT key, value FROM app_settings WHERE key LIKE 'backup\\_%' ESCAPE '\\'")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };

    // 1) Copia (e decifra) para a área de preparação, conferindo o SHA-256 de cada arquivo.
    let stage = TempDir::new(data.join("restore-staging"))?;
    let total = m.files.iter().map(|f| f.size).sum();
    op.phase("stage", total);
    let mut skipped = Vec::new();
    let mut secrets_json: Option<Vec<u8>> = None;
    let mut restored = 0;
    for f in &m.files {
        op.check()?;
        let rel = safe_rel(&f.path).ok_or_else(|| AppError::Validation("Caminho inválido no backup.".into()))?;
        let src = root.join(&rel);
        let dest = stage.0.join(&rel);
        let res = (|| -> AppResult<Option<Vec<u8>>> {
            let len = fs::metadata(&src)?.len();
            let mut r = HashRead { r: fs::File::open(&src)?, h: Sha256::new() };
            let mut tick = |n: u64| op.add(n);
            let mem = f.kind == "secrets"; // credenciais nunca vão em texto puro para o disco
            let mut buf: Vec<u8> = Vec::new();
            {
                let mut w: Box<dyn Write + '_> = if mem {
                    Box::new(&mut buf)
                } else {
                    if let Some(p) = dest.parent() {
                        fs::create_dir_all(p)?;
                    }
                    Box::new(BufWriter::new(fs::File::create(&dest)?))
                };
                match key {
                    Some(k) => crypto::decrypt(k, &mut r, len, &mut w, &mut tick)?,
                    None => copy(&mut r, &mut w, &mut tick)?,
                }
                w.flush()?;
            }
            if hex(&r.h.finalize()) != f.sha256.to_lowercase() {
                return Err(AppError::Validation("arquivo danificado".into()));
            }
            Ok(mem.then_some(buf))
        })();
        match res {
            Ok(Some(b)) => secrets_json = Some(b),
            Ok(None) => restored += 1,
            Err(e) if is_canceled(&e) => return Err(e),
            Err(e) if critical(&f.kind) => {
                return Err(AppError::Validation(format!(
                    "Não foi possível ler o banco de dados ou as credenciais do backup ({e}). Nenhum dado foi alterado."
                )))
            }
            Err(_) => {
                let _ = fs::remove_file(&dest);
                skipped.push(f.path.clone());
            }
        }
    }

    // 2) Valida e prepara o banco restaurado (migra versões anteriores, credenciais e caminhos deste PC).
    op.phase("validate", 0);
    let sdb = stage.0.join(DB_REL);
    {
        let c = Connection::open(&sdb)?;
        let ok: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ok != "ok" {
            return Err(AppError::Validation("O banco de dados do backup está danificado. Nenhum dado foi alterado.".into()));
        }
        let tables: i64 = c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('profiles','games','profile_games','app_settings')",
            [],
            |r| r.get(0),
        )?;
        if tables != 4 {
            return Err(AppError::Validation("O backup não contém um banco de dados válido do NEXSTAGE. Nenhum dado foi alterado.".into()));
        }
        let current = {
            let conn = cx.db.conn()?;
            user_version(&conn)?
        };
        if user_version(&c)? > current {
            return Err(AppError::Validation("O banco de dados deste backup é de uma versão mais nova do NEXSTAGE. Nenhum dado foi alterado.".into()));
        }
    }
    drop(Db::open(&sdb).map_err(|e| AppError::Validation(format!("Não foi possível atualizar o banco do backup para esta versão ({e}). Nenhum dado foi alterado.")))?);
    let (mut sec_ok, mut sec_fail) = (0, 0);
    let gdir = data.join("gallery");
    {
        let c = Connection::open(&sdb)?;
        match m.secrets.as_str() {
            "local" => {}
            "included" => {
                secrets::strip(&c)?;
                match secrets_json.as_deref().map(serde_json::from_slice::<secrets::Bundle>) {
                    Some(Ok(b)) => {
                        let (a, f) = secrets::apply(&c, &b)?;
                        sec_ok = a;
                        sec_fail = f;
                    }
                    _ => warnings.push("As credenciais do backup não puderam ser lidas: informe-as novamente nas configurações.".into()),
                }
            }
            _ => secrets::strip(&c)?,
        }
        secrets::finalize(&c)?;
        if sec_fail > 0 {
            warnings.push(format!("{sec_fail} credencial(is) não puderam ser protegidas neste computador: informe-as novamente nas configurações."));
        }
        c.execute("DELETE FROM app_settings WHERE key LIKE 'backup\\_%' ESCAPE '\\'", [])?;
        for (k, v) in &local_backup_settings {
            c.execute("INSERT OR REPLACE INTO app_settings(key,value) VALUES(?1,?2)", [k, v])?;
        }
        // Galeria: o banco guarda o caminho completo do arquivo, que muda de um computador para outro.
        let rows: Vec<(i64, i64, String)> = c
            .prepare("SELECT id, profile_id, path FROM gallery_items")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (id, pid, p) in rows {
            if let Some(name) = Path::new(&p).file_name() {
                let np = gdir.join(pid.to_string()).join(name);
                c.execute("UPDATE gallery_items SET path=?1 WHERE id=?2", rusqlite::params![np.to_string_lossy().to_string(), id])?;
            }
        }
        // arquivo único (sem -wal) para a troca
        let _: String = c.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    }
    op.check()?;

    // 3) Backup de segurança dos dados atuais (nada foi alterado até aqui).
    op.alias = Some("safety");
    let safety = {
        let dir = data.join(SAFETY_DIR);
        let stamp = {
            let conn = cx.db.conn()?;
            local_now(&conn, "%Y-%m-%d_%H-%M-%S")
        };
        let base = format!("{FOLDER}_antes-da-restauracao_{stamp}");
        let mut name = base.clone();
        let mut n = 2;
        while dir.join(&name).exists() {
            name = format!("{base}_{n}");
            n += 1;
        }
        let opts = CreateOpts { parent: dir.clone(), name, password: None, include_captures: false, kind: Kind::Safety, replace: false };
        let r = create(cx, &opts, op).map_err(|e| {
            if is_canceled(&e) { e } else { AppError::Other(format!("Não foi possível criar o backup de segurança dos dados atuais ({e}). Nenhum dado foi alterado.")) }
        })?;
        prune_safety(&dir);
        r
    };
    op.alias = None;
    CANCELABLE.store(true, Ordering::SeqCst);
    op.check()?;

    // 4) Aplicação (sem cancelamento a partir daqui).
    op.set_cancelable(false);
    op.phase("apply", 0);
    let mut placed: Vec<PathBuf> = Vec::new();
    let place = |from: &Path, to: &Path, placed: &mut Vec<PathBuf>| -> AppResult<()> {
        if to.exists() {
            return Ok(()); // mesmo nome = mesmo conteúdo (hash) ou captura já existente: mantém
        }
        if let Some(p) = to.parent() {
            fs::create_dir_all(p)?;
        }
        if fs::rename(from, to).is_err() {
            fs::copy(from, to)?;
        }
        placed.push(to.to_path_buf());
        Ok(())
    };
    let staged_files = |sub: &str| -> Vec<(PathBuf, PathBuf)> {
        let mut v = Vec::new();
        let base = stage.0.join(sub);
        fn walk(dir: &Path, base: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p, base, out);
                    } else if let Ok(rel) = p.strip_prefix(base) {
                        out.push((p.clone(), rel.to_path_buf()));
                    }
                }
            }
        }
        walk(&base, &base, &mut v);
        v
    };
    let apply_files = (|| -> AppResult<()> {
        for (from, rel) in staged_files("images") {
            place(&from, &data.join("images").join(rel), &mut placed)?;
        }
        for (from, rel) in staged_files("gallery") {
            place(&from, &gdir.join(rel), &mut placed)?;
        }
        let caps = staged_files("captures");
        if !caps.is_empty() {
            let cdir = cx.captures.clone().ok_or_else(|| AppError::Other("Pasta de capturas indisponível.".into()))?;
            for (from, rel) in caps {
                place(&from, &cdir.join(rel), &mut placed)?;
            }
        }
        Ok(())
    })();
    let swapped = apply_files.and_then(|_| swap_db(cx.db, &sdb));
    if let Err(e) = swapped {
        for p in &placed {
            let _ = fs::remove_file(p);
        }
        return Err(AppError::Other(format!(
            "A restauração não pôde ser concluída ({e}). Os dados anteriores foram mantidos; se algo parecer errado, carregue o backup de segurança em: {}",
            safety.path
        )));
    }

    // 5) Pós-troca (melhor esforço): limpa capturas que não pertencem aos dados restaurados (existem no
    // backup de segurança). Proteção dos perfis e inicialização com o Windows: ver `commands::backup`.
    if let Err(e) = clean_gallery(cx.db, &gdir) {
        warnings.push(format!("Algumas capturas antigas não puderam ser removidas: {e}"));
    }
    if !skipped.is_empty() {
        warnings.push(format!("{} arquivo(s) de mídia danificados ou ausentes no backup não foram restaurados.", skipped.len()));
    }
    Ok(RestoreReport {
        safety_path: safety.path,
        restored_files: restored,
        skipped,
        secrets: m.secrets.clone(),
        secrets_applied: sec_ok,
        secrets_failed: sec_fail,
        warnings,
    })
}

/// Substitui o conteúdo do banco em uso pelo banco preparado, numa única transação do SQLite.
fn swap_db(db: &Db, staged: &Path) -> AppResult<()> {
    let src = Connection::open(staged)?;
    let mut live = db.conn()?;
    // No modo WAL o SQLite não aceita trocar o tamanho de página do destino: usa o diário clássico durante a troca.
    let _: String = live.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    let res = (|| -> AppResult<()> {
        let b = rusqlite::backup::Backup::new(&src, &mut live)?;
        match b.step(-1)? {
            rusqlite::backup::StepResult::Done => Ok(()),
            _ => Err(AppError::Other("o banco de dados está ocupado".into())),
        }
    })();
    let _ = live.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0));
    let _ = live.execute_batch("PRAGMA foreign_keys = ON;");
    res
}

/// Remove da Galeria os arquivos que nenhum registro usa (ex.: perfis que não existem no backup).
fn clean_gallery(db: &Db, gdir: &Path) -> AppResult<()> {
    let used: HashMap<String, HashSet<String>> = {
        let conn = db.conn()?;
        let mut st = conn.prepare("SELECT profile_id, path FROM gallery_items")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut m: HashMap<String, HashSet<String>> = HashMap::new();
        for r in rows.flatten() {
            if let Some(n) = Path::new(&r.1).file_name() {
                m.entry(r.0.to_string()).or_default().insert(n.to_string_lossy().into_owned());
            }
        }
        m
    };
    let Ok(rd) = fs::read_dir(gdir) else { return Ok(()) };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let pid = e.file_name().to_string_lossy().into_owned();
        match used.get(&pid) {
            None => fs::remove_dir_all(&p)?,
            Some(names) => {
                for f in fs::read_dir(&p)?.flatten() {
                    if !names.contains(&f.file_name().to_string_lossy().into_owned()) {
                        fs::remove_file(f.path())?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Mantém só os backups de segurança mais recentes (cópias internas do NEXSTAGE, não do usuário).
fn prune_safety(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().map(|n| n.to_string_lossy().starts_with(FOLDER)).unwrap_or(false))
        .collect();
    v.sort();
    while v.len() > SAFETY_KEEP {
        let _ = fs::remove_dir_all(v.remove(0));
    }
    // sobras de execuções interrompidas
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with('.') {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_blocks_traversal_and_unknown_roots() {
        assert!(safe_rel("data/database.db").is_some());
        assert!(safe_rel("gallery/3/abc.png").is_some());
        assert!(safe_rel("captures/Jogo_20260101.png").is_some());
        for bad in ["../x", "data/../../x", "data", "other/x", "images/a\\b", "images/C:x", "/images/x", "images//x", "gallery/./x"] {
            assert!(safe_rel(bad).is_none(), "{bad}");
        }
    }

    /// Pasta de dados de um "computador" de teste, com banco, uma capa e uma captura.
    fn machine(tag: &str, profile: &str, auto_dir: &str) -> (PathBuf, Db) {
        let d = std::env::temp_dir().join(format!("nexstage-bk-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("images")).unwrap();
        fs::create_dir_all(d.join("gallery").join("1")).unwrap();
        let db = Db::open(&d.join("nexstage.db")).unwrap();
        let img = format!("{}.png", hex(&Sha256::digest(profile.as_bytes())));
        fs::write(d.join("images").join(&img), profile.as_bytes()).unwrap();
        let shot_bytes = format!("shot-{profile}");
        let shot = format!("{}.png", hex(&Sha256::digest(shot_bytes.as_bytes())));
        fs::write(d.join("gallery").join("1").join(&shot), shot_bytes.as_bytes()).unwrap();
        {
            let c = db.conn().unwrap();
            c.execute("INSERT INTO profiles(id,username,display_name) VALUES(1,?1,?1)", [profile]).unwrap();
            c.execute("INSERT INTO games(id,title,executable_path,cover) VALUES(1,'Jogo','C:/jogo.exe',?1)", [format!("nximg:{img}")]).unwrap();
            c.execute_batch("INSERT INTO profile_games(profile_id,game_id,playtime_seconds) VALUES(1,1,3600);").unwrap();
            let p = d.join("gallery").join("1").join(&shot);
            c.execute("INSERT INTO gallery_items(profile_id,game_id,path,hash) VALUES(1,1,?1,'h')", [p.to_string_lossy().to_string()]).unwrap();
            let key = crate::retro::secret::protect(&format!("KEY-{profile}")).unwrap();
            crate::accounts::save(&c, 1, "retroachievements", "Jogador", None, Some(&key), "{}").unwrap();
            c.execute("INSERT INTO app_settings(key,value) VALUES('backup_auto_dir',?1),('sgdb_key','SGDB-SECRETO')", [auto_dir]).unwrap();
        }
        (d, db)
    }

    fn cx<'a>(d: &Path, db: &'a Db) -> Ctx<'a> {
        Ctx { data: d.to_path_buf(), captures: None, version: "1.0.0".into(), db }
    }
    fn op() -> Op {
        Op::with_emitter(Box::new(|_| {}), "test", "test")
    }
    fn one<T: rusqlite::types::FromSql>(db: &Db, sql: &str) -> T {
        db.conn().unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn create_verify_restore_between_machines() {
        let (da, dba) = machine("a", "ana", "A:/backups");
        let (db_dir, dbb) = machine("b", "bruno", "B:/backups");
        let out = da.join("saida");
        let opts = CreateOpts { parent: out.clone(), name: FOLDER.into(), password: Some("senha-forte-1".into()), include_captures: false, kind: Kind::Manual, replace: false };
        let rep = create(&cx(&da, &dba), &opts, &mut op()).unwrap();
        assert_eq!(rep.secrets, "included");
        let root = PathBuf::from(&rep.path);

        // Nada sensível legível no backup cifrado; manifesto sem nomes de perfis.
        let raw = fs::read(root.join(DB_REL)).unwrap();
        assert!(!raw.windows(12).any(|w| w == b"SGDB-SECRETO"));
        let m = read_manifest(&root).unwrap();
        assert!(m.summary.profile_names.is_empty());
        assert!(m.files.iter().any(|f| f.kind == "gallery") && m.files.iter().any(|f| f.kind == "image"));

        // Senha errada / ausente.
        assert!(check(&cx(&db_dir, &dbb), &root, Some("errada-123")).is_err());
        assert!(check(&cx(&db_dir, &dbb), &root, None).unwrap().1.needs_password);
        assert!(verify(&root, &m, &mut op()).unwrap().invalid.is_empty());

        // Restaura no computador B.
        let (m, info, key) = check(&cx(&db_dir, &dbb), &root, Some("senha-forte-1")).unwrap();
        assert!(info.problem.is_none());
        let r = restore(&cx(&db_dir, &dbb), &root, &m, key.as_ref(), &mut op()).unwrap();
        assert_eq!(r.secrets_failed, 0);
        assert!(r.secrets_applied >= 2);
        let name: String = one(&dbb, "SELECT display_name FROM profiles");
        assert_eq!(name, "ana");
        assert_eq!(one::<i64>(&dbb, "SELECT playtime_seconds FROM profile_games"), 3600);
        // caminho da captura reescrito para a pasta de B, com o arquivo presente
        let gp: String = one(&dbb, "SELECT path FROM gallery_items");
        assert!(gp.starts_with(&*db_dir.join("gallery").to_string_lossy()), "{gp}");
        assert!(Path::new(&gp).is_file());
        // a captura antiga de B (perfil que não existe mais) foi removida da pasta da galeria
        assert_eq!(fs::read_dir(db_dir.join("gallery").join("1")).unwrap().count(), 1);
        // capa restaurada
        let cover: String = one(&dbb, "SELECT cover FROM games");
        assert!(db_dir.join("images").join(cover.trim_start_matches("nximg:")).is_file());
        // credenciais recifradas neste computador; preferências de backup de B preservadas
        let acc = crate::accounts::get(&dbb.conn().unwrap(), 1, "retroachievements").unwrap().unwrap();
        assert_eq!(crate::retro::secret::unprotect(&acc.secret.unwrap()).as_deref(), Some("KEY-ana"));
        assert_eq!(one::<String>(&dbb, "SELECT value FROM app_settings WHERE key='backup_auto_dir'"), "B:/backups");
        assert_eq!(crate::apikeys::read(&dbb.conn().unwrap(), "sgdb_key").as_deref(), Some("SGDB-SECRETO"));
        assert!(!one::<String>(&dbb, "SELECT value FROM app_settings WHERE key='sgdb_key'").contains("SGDB-SECRETO"), "recifrada no destino");
        // backup de segurança com os dados anteriores de B
        let safety = PathBuf::from(&r.safety_path);
        let sm = read_manifest(&safety).unwrap();
        assert_eq!(sm.kind, "safety");
        assert!(verify(&safety, &sm, &mut op()).unwrap().invalid.is_empty());
        let c = Connection::open(safety.join(DB_REL)).unwrap();
        let old: String = c.query_row("SELECT display_name FROM profiles", [], |r| r.get(0)).unwrap();
        assert_eq!(old, "bruno");
        drop(c);

        // Sem senha: credenciais ficam de fora e não sobram no destino.
        let opts2 = CreateOpts { parent: da.join("saida2"), password: None, ..opts };
        let rep2 = create(&cx(&da, &dba), &opts2, &mut op()).unwrap();
        assert_eq!(rep2.secrets, "excluded");
        assert!(rep2.secrets_skipped >= 2);
        let root2 = PathBuf::from(&rep2.path);
        let raw2 = fs::read(root2.join(DB_REL)).unwrap();
        assert!(!raw2.windows(12).any(|w| w == b"SGDB-SECRETO"), "chave em texto puro não pode ir para backup sem senha");
        let (m2, _, k2) = check(&cx(&db_dir, &dbb), &root2, None).unwrap();
        restore(&cx(&db_dir, &dbb), &root2, &m2, k2.as_ref(), &mut op()).unwrap();
        let acc = crate::accounts::get(&dbb.conn().unwrap(), 1, "retroachievements").unwrap().unwrap();
        assert!(acc.secret.is_none());

        // Banco do backup adulterado: verificação recusa sem tocar nos dados.
        let mut bad = fs::read(root2.join(DB_REL)).unwrap();
        let mid = bad.len() / 2;
        bad[mid] ^= 0xff;
        fs::write(root2.join(DB_REL), bad).unwrap();
        assert!(verify(&root2, &m2, &mut op()).is_err());
        assert!(restore(&cx(&db_dir, &dbb), &root2, &m2, None, &mut op()).is_err());
        assert_eq!(one::<String>(&dbb, "SELECT display_name FROM profiles"), "ana");

        drop((dba, dbb));
        let _ = fs::remove_dir_all(&da);
        let _ = fs::remove_dir_all(&db_dir);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_tuple("1.10.0") > version_tuple("1.9.3"));
        assert_eq!(version_tuple("2.0.0-beta"), (2, 0, 0));
    }
}
