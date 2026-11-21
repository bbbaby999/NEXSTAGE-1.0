//! Instalação, atualização, verificação, volta de versão e desinstalação de um jogo integrado.
//! Independente do Tauri (testável com um servidor HTTP local).
//!
//! Pasta do jogo: `%LOCALAPPDATA%\NEXSTAGE\games\<game_id>\`
//!   current                 versão ativa (texto; gravado com arquivo temporário + renomear)
//!   versions\<versão>\      arquivos do jogo + manifest.json (o manifesto verificado daquela versão)
//!   staging\<versão>\       download em andamento (retomável); apagado ao concluir, cancelar ou falhar
//!
//! Nunca deixa uma instalação pela metade: tudo é baixado e conferido (SHA-256) em `staging`, só então a pasta
//! vira `versions\<versão>` (renomeação no mesmo disco) e `current` aponta para ela. A versão anterior fica
//! guardada para voltar; as mais antigas são apagadas.

use super::manifest::{FileEntry, Manifest, NetRules};
use crate::error::{AppError, AppResult};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct Layout {
    pub base: PathBuf,
}

impl Layout {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
    fn versions(&self) -> PathBuf {
        self.base.join("versions")
    }
    pub fn version_dir(&self, v: &str) -> PathBuf {
        self.versions().join(v)
    }
    fn staging(&self, v: &str) -> PathBuf {
        self.base.join("staging").join(v)
    }
    pub fn current(&self) -> Option<String> {
        let v = fs::read_to_string(self.base.join("current")).ok()?;
        let v = v.trim().to_string();
        (!v.is_empty() && self.version_dir(&v).is_dir()).then_some(v)
    }
    fn set_current(&self, v: &str) -> AppResult<()> {
        let tmp = self.base.join("current.tmp");
        fs::write(&tmp, v)?;
        fs::rename(&tmp, self.base.join("current"))?;
        Ok(())
    }
    /// Manifesto verificado de uma versão instalada.
    pub fn manifest_of(&self, v: &str) -> Option<Manifest> {
        serde_json::from_slice(&fs::read(self.version_dir(v).join("manifest.json")).ok()?).ok()
    }
    /// Versão guardada para voltar (a outra pasta em `versions`, além da ativa).
    pub fn previous(&self) -> Option<String> {
        let cur = self.current();
        let mut list: Vec<(std::time::SystemTime, String)> = fs::read_dir(self.versions())
            .ok()?
            .flatten()
            .filter(|e| e.path().join("manifest.json").is_file())
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.file_name().to_string_lossy().into_owned())))
            .filter(|(_, n)| Some(n) != cur.as_ref())
            .collect();
        list.sort();
        list.pop().map(|(_, n)| n)
    }
    /// Há um download interrompido (pausado/fechado) para esta versão?
    #[cfg(test)]
    pub fn has_partial(&self, v: &str) -> bool {
        self.staging(v).is_dir()
    }
}

#[derive(Default)]
pub struct Control {
    pub pause: AtomicBool,
    pub cancel: AtomicBool,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// "downloading" | "verifying" | "installing"
    pub phase: &'static str,
    pub file: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Installed { previous: Option<String> },
    Paused,
    Canceled,
}

/// Cliente HTTP: só segue redirecionamentos para endereços permitidos.
pub fn client(net: NetRules) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("NexStage/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(move |a| {
            if a.previous().len() > 8 {
                a.error("muitos redirecionamentos")
            } else if net.url_ok(a.url().as_str()) {
                a.follow()
            } else {
                a.error("redirecionamento para endereço não permitido")
            }
        }))
        .build()
        .unwrap_or_default()
}

pub fn sha256_file(p: &Path) -> AppResult<String> {
    let mut f = fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn matches(p: &Path, f: &FileEntry) -> bool {
    fs::metadata(p).map(|m| m.len() == f.size).unwrap_or(false) && sha256_file(p).map(|h| h.eq_ignore_ascii_case(&f.sha256)).unwrap_or(false)
}

/// Baixa um texto/bytes pequeno (manifesto e assinatura).
pub async fn fetch_bytes(client: &reqwest::Client, net: &NetRules, url: &str, max: usize) -> AppResult<Vec<u8>> {
    if !net.url_ok(url) {
        return Err(AppError::Validation("Endereço de download não permitido.".into()));
    }
    let resp = client.get(url).send().await.map_err(|e| AppError::Other(format!("Sem conexão com o servidor de downloads: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Other(format!("O servidor de downloads respondeu {}.", resp.status())));
    }
    let b = resp.bytes().await.map_err(|e| AppError::Other(format!("Falha no download: {e}")))?;
    if b.len() > max {
        return Err(AppError::Validation("Arquivo de distribuição grande demais.".into()));
    }
    Ok(b.to_vec())
}

enum Dl {
    Done,
    Paused,
    Canceled,
}

async fn download(client: &reqwest::Client, net: &NetRules, f: &FileEntry, dest: &Path, ctl: &Control, done: &mut u64, total: u64, on: &(dyn Fn(Progress) + Send + Sync)) -> AppResult<Dl> {
    if !net.url_ok(&f.url) {
        return Err(AppError::Validation("Endereço de download não permitido.".into()));
    }
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > f.size {
        let _ = fs::remove_file(&part);
        have = 0;
    }
    let mut req = client.get(&f.url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = req.send().await.map_err(|e| AppError::Other(format!("Falha no download de {}: {e}", f.path)))?;
    let status = resp.status();
    let append = status == reqwest::StatusCode::PARTIAL_CONTENT && have > 0;
    if !status.is_success() {
        return Err(AppError::Other(format!("O servidor de downloads respondeu {status} para {}.", f.path)));
    }
    if !append {
        have = 0; // servidor não aceitou retomar: recomeça o arquivo
    }
    let mut out = fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&part)?;
    *done += have;
    let mut since = 0u64;
    loop {
        if ctl.cancel.load(Ordering::SeqCst) {
            return Ok(Dl::Canceled);
        }
        if ctl.pause.load(Ordering::SeqCst) {
            out.flush()?;
            return Ok(Dl::Paused);
        }
        let chunk = tokio::time::timeout(Duration::from_secs(60), resp.chunk())
            .await
            .map_err(|_| AppError::Other("O download parou de responder. Tente de novo (ele continua de onde parou).".into()))?
            .map_err(|e| AppError::Other(format!("Falha no download de {}: {e}", f.path)))?;
        let Some(chunk) = chunk else { break };
        have += chunk.len() as u64;
        if have > f.size {
            let _ = fs::remove_file(&part);
            return Err(AppError::Other(format!("{} veio maior que o esperado.", f.path)));
        }
        out.write_all(&chunk)?;
        *done += chunk.len() as u64;
        since += chunk.len() as u64;
        if since >= 512 * 1024 {
            since = 0;
            on(Progress { phase: "downloading", file: f.path.clone(), done: *done, total });
        }
    }
    out.flush()?;
    drop(out);
    fs::rename(&part, dest)?;
    Ok(Dl::Done)
}

/// Falha de rede que vale tentar de novo (o arquivo parcial é mantido e o download continua dele).
fn is_transient(e: &AppError) -> bool {
    matches!(e, AppError::Other(m) if m.starts_with("Falha no download de ") || m.starts_with("O download parou de responder"))
}

fn fail_clean(staging: &Path, msg: String) -> AppError {
    let _ = fs::remove_dir_all(staging);
    AppError::Other(msg)
}

/// Instala (ou atualiza para) a versão do manifesto já verificado. `raw` = bytes do manifesto (guardado junto).
pub async fn install(layout: &Layout, m: &Manifest, raw: &[u8], client: &reqwest::Client, net: &NetRules, ctl: &Control, on: &(dyn Fn(Progress) + Send + Sync)) -> AppResult<Outcome> {
    let total = m.total_size();
    let staging = layout.staging(&m.version);
    fs::create_dir_all(&staging)?;
    let cur = layout.current();
    // reparo da mesma versão: a versão anterior guardada continua guardada
    let keep_prev = if cur.as_deref() == Some(m.version.as_str()) { layout.previous() } else { cur.clone() };
    let cur_manifest = cur.as_deref().and_then(|v| layout.manifest_of(v));
    let mut done = 0u64;
    for f in &m.files {
        let dest = staging.join(&f.path);
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        // já baixado numa tentativa anterior
        if dest.is_file() && matches(&dest, f) {
            done += f.size;
            continue;
        }
        // atualização incremental: arquivo igual na versão instalada → cópia local
        let same = cur_manifest.as_ref().and_then(|cm| cm.files.iter().find(|x| x.path == f.path && x.sha256.eq_ignore_ascii_case(&f.sha256)));
        if let (Some(_), Some(v)) = (same, cur.as_deref()) {
            let src = layout.version_dir(v).join(&f.path);
            // a cópia só vale se conferir (o arquivo instalado pode ter sido alterado ou danificado)
            if src.is_file() && fs::copy(&src, &dest).is_ok() && matches(&dest, f) {
                done += f.size;
                on(Progress { phase: "downloading", file: f.path.clone(), done, total });
                continue;
            }
            let _ = fs::remove_file(&dest);
        }
        // queda de conexão no meio do arquivo: tenta de novo sozinho (continua do .part), até 3 vezes
        let base = done;
        let mut attempt = 0;
        let r = loop {
            match download(client, net, f, &dest, ctl, &mut done, total, on).await {
                Err(e) if attempt < 3 && is_transient(&e) => {
                    attempt += 1;
                    done = base;
                    crate::diaglog::write("aviso", "integrated", &format!("{}: {e} — nova tentativa {attempt}/3", f.path));
                    tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
                }
                other => break other?,
            }
        };
        match r {
            Dl::Done => {}
            Dl::Paused => return Ok(Outcome::Paused),
            Dl::Canceled => {
                let _ = fs::remove_dir_all(&staging);
                return Ok(Outcome::Canceled);
            }
        }
    }
    // tudo conferido antes de mexer na instalação
    let mut checked = 0u64;
    for f in &m.files {
        on(Progress { phase: "verifying", file: f.path.clone(), done: checked, total });
        if !matches(&staging.join(&f.path), f) {
            return Err(fail_clean(&staging, format!("O arquivo {} chegou corrompido. Nada foi alterado; tente de novo.", f.path)));
        }
        checked += f.size;
    }
    fs::write(staging.join("manifest.json"), raw)?;
    on(Progress { phase: "installing", file: String::new(), done: total, total });
    fs::create_dir_all(layout.versions())?;
    let target = layout.version_dir(&m.version);
    if target.exists() {
        // reinstalação/reparo da mesma versão: a pasta antiga sai do caminho antes da troca
        let old = layout.versions().join(format!("{}.old-{}", m.version, std::process::id()));
        fs::rename(&target, &old).map_err(|e| fail_clean(&staging, format!("Não foi possível substituir a versão instalada (jogo aberto?): {e}")))?;
        let _ = fs::remove_dir_all(&old);
    }
    fs::rename(&staging, &target).map_err(|e| fail_clean(&staging, format!("Não foi possível concluir a instalação: {e}")))?;
    let _ = fs::remove_dir(layout.base.join("staging"));
    layout.set_current(&m.version)?;
    let previous = keep_prev;
    // guarda só a ativa e a anterior
    if let Ok(rd) = fs::read_dir(layout.versions()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n != m.version && Some(&n) != previous.as_ref() {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(Outcome::Installed { previous })
}

/// Arquivos da versão ativa que não conferem com o manifesto dela (vazio = tudo certo).
pub fn verify(layout: &Layout) -> AppResult<Vec<String>> {
    let v = layout.current().ok_or_else(|| AppError::NotFound("O jogo não está instalado pelo NexStage.".into()))?;
    let m = layout.manifest_of(&v).ok_or_else(|| AppError::Other("Manifesto da versão instalada não encontrado.".into()))?;
    let dir = layout.version_dir(&v);
    Ok(m.files.iter().filter(|f| !matches(&dir.join(&f.path), f)).map(|f| f.path.clone()).collect())
}

/// Volta para a versão anterior guardada. Devolve a versão que ficou ativa.
pub fn rollback(layout: &Layout) -> AppResult<String> {
    let prev = layout.previous().ok_or_else(|| AppError::NotFound("Não há versão anterior guardada.".into()))?;
    layout.set_current(&prev)?;
    Ok(prev)
}

/// Apaga a pasta do jogo (os saves ficam em outro lugar e não são tocados).
pub fn uninstall(layout: &Layout) -> AppResult<()> {
    if layout.base.exists() {
        fs::remove_dir_all(&layout.base).map_err(|e| AppError::Other(format!("Não foi possível apagar a pasta do jogo (ele está aberto?): {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::manifest::tests::{keypair, sign};
    use super::super::manifest::verify_and_parse;
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Servidor HTTP mínimo (GET + Range) para os testes; conta quantas vezes cada arquivo foi pedido.
    struct Server {
        port: u16,
        files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        hits: Arc<Mutex<HashMap<String, usize>>>,
    }

    async fn server() -> Server {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let files: Arc<Mutex<HashMap<String, Vec<u8>>>> = Default::default();
        let hits: Arc<Mutex<HashMap<String, usize>>> = Default::default();
        let (f2, h2) = (files.clone(), hits.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let (f3, h3) = (f2.clone(), h2.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *h3.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let body = f3.lock().unwrap().get(&path).cloned();
                    let Some(body) = body else {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
                        return;
                    };
                    let start = req.lines().find_map(|l| l.to_lowercase().strip_prefix("range: bytes=").map(|r| r.trim_end_matches('-').parse::<usize>().unwrap_or(0))).unwrap_or(0);
                    let part = &body[start.min(body.len())..];
                    // "/cut/…": a primeira resposta cai no meio (queda de conexão simulada)
                    if path.starts_with("/cut/") && start == 0 && *h3.lock().unwrap().get(&path).unwrap_or(&0) == 1 {
                        let head = format!("HTTP/1.1 200 OK
content-length: {}
connection: close

", part.len());
                        let _ = s.write_all(head.as_bytes()).await;
                        let _ = s.write_all(&part[..part.len() / 2]).await;
                        let _ = s.shutdown().await;
                        return;
                    }
                    let head = if start > 0 {
                        format!("HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {}-{}/{}\r\nconnection: close\r\n\r\n", part.len(), start, body.len() - 1, body.len())
                    } else {
                        format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", part.len())
                    };
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(part).await;
                    // fecha com FIN e espera o cliente encerrar: fechar direto pode virar RST no Windows e cortar o fim do corpo
                    let _ = s.shutdown().await;
                    let mut sink = [0u8; 512];
                    while matches!(s.read(&mut sink).await, Ok(n) if n > 0) {}
                });
            }
        });
        Server { port, files, hits }
    }

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    struct Rel {
        manifest: Manifest,
        raw: Vec<u8>,
    }

    fn release(srv: &Server, version: &str, build: i64, exe: &[u8], pck: &[u8], kp: &ring::signature::Ed25519KeyPair, pk: &str) -> Rel {
        let mut files = vec![];
        for (name, data) in [("TombSurvivors.exe", exe), ("TombSurvivors.pck", pck)] {
            let url_path = format!("/{version}/{name}");
            srv.files.lock().unwrap().insert(url_path.clone(), data.to_vec());
            files.push(serde_json::json!({"path": name, "size": data.len(), "sha256": sha(data), "url": format!("http://127.0.0.1:{}{url_path}", srv.port)}));
        }
        let v = serde_json::json!({"schema":1,"game_id":"tomb-survivors","name":"Tomb Survivors","version":version,"build":build,"min_nexstage":"1.0.0","protocol":[1],"executable":"TombSurvivors.exe","files":files});
        let raw = serde_json::to_vec(&v).unwrap();
        let manifest = verify_and_parse(&raw, &sign(kp, &raw), pk, "tomb-survivors", "1.3.1", &net()).unwrap();
        Rel { manifest, raw }
    }

    fn net() -> NetRules {
        NetRules { hosts: vec![], allow_http_loopback: true }
    }

    fn base(name: &str) -> Layout {
        let p = std::env::temp_dir().join(format!("nexstage-inst-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        Layout::new(p)
    }

    fn noop() -> impl Fn(Progress) + Send + Sync {
        |_p| {}
    }

    #[tokio::test]
    async fn install_update_incremental_verify_rollback_uninstall() {
        let srv = server().await;
        let (kp, pk) = keypair();
        let lay = base("full");
        let c = client(net());
        let ctl = Control::default();
        // instalação limpa
        let v1 = release(&srv, "0.1.0", 1, b"EXE-1", &vec![7u8; 300_000], &kp, &pk);
        let r = install(&lay, &v1.manifest, &v1.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        assert_eq!(r, Outcome::Installed { previous: None });
        assert_eq!(lay.current().as_deref(), Some("0.1.0"));
        assert!(verify(&lay).unwrap().is_empty());
        // atualização: só o .exe mudou → o .pck é copiado da versão instalada, sem download
        let v2 = release(&srv, "0.2.0", 2, b"EXE-2", &vec![7u8; 300_000], &kp, &pk);
        let r = install(&lay, &v2.manifest, &v2.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        assert_eq!(r, Outcome::Installed { previous: Some("0.1.0".into()) });
        assert_eq!(srv.hits.lock().unwrap().get("/0.2.0/TombSurvivors.pck"), None, "arquivo igual não foi baixado");
        assert_eq!(fs::read(lay.version_dir("0.2.0").join("TombSurvivors.exe")).unwrap(), b"EXE-2");
        // verificação detecta arquivo adulterado
        fs::write(lay.version_dir("0.2.0").join("TombSurvivors.exe"), b"HACK!").unwrap();
        assert_eq!(verify(&lay).unwrap(), vec!["TombSurvivors.exe".to_string()]);
        // reparo = reinstalar a mesma versão (só o arquivo ruim é baixado de novo)
        install(&lay, &v2.manifest, &v2.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        assert!(verify(&lay).unwrap().is_empty());
        // volta para a versão anterior
        assert_eq!(lay.previous().as_deref(), Some("0.1.0"));
        assert_eq!(rollback(&lay).unwrap(), "0.1.0");
        assert_eq!(lay.current().as_deref(), Some("0.1.0"));
        assert_eq!(fs::read(lay.version_dir("0.1.0").join("TombSurvivors.exe")).unwrap(), b"EXE-1");
        // desinstalar
        uninstall(&lay).unwrap();
        assert!(!lay.base.exists());
    }

    #[tokio::test]
    async fn corrupted_download_changes_nothing() {
        let srv = server().await;
        let (kp, pk) = keypair();
        let lay = base("corrupt");
        let c = client(net());
        let ctl = Control::default();
        let v1 = release(&srv, "0.1.0", 1, b"EXE-1", b"PCK-1", &kp, &pk);
        install(&lay, &v1.manifest, &v1.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        let v2 = release(&srv, "0.2.0", 2, b"EXE-2", b"PCK-2", &kp, &pk);
        // o servidor entrega outro conteúdo, do mesmo tamanho (corrompido no caminho)
        srv.files.lock().unwrap().insert("/0.2.0/TombSurvivors.pck".into(), b"PCK-X".to_vec());
        let e = install(&lay, &v2.manifest, &v2.raw, &c, &net(), &ctl, &noop()).await.unwrap_err();
        assert!(e.to_string().contains("corrompido"), "{e}");
        assert_eq!(lay.current().as_deref(), Some("0.1.0"), "a versão instalada continua a mesma");
        assert!(!lay.has_partial("0.2.0"), "o download com falha foi apagado");
        assert!(verify(&lay).unwrap().is_empty());
    }

    #[tokio::test]
    async fn pause_resume_and_cancel() {
        let srv = server().await;
        let (kp, pk) = keypair();
        let lay = base("pause");
        let c = client(net());
        let big = vec![3u8; 2_000_000];
        let v1 = release(&srv, "0.1.0", 1, b"EXE-1", &big, &kp, &pk);
        // pausa no primeiro aviso de progresso: fica um download parcial retomável
        let ctl = Arc::new(Control::default());
        let ctl2 = ctl.clone();
        let r = install(&lay, &v1.manifest, &v1.raw, &c, &net(), &ctl, &move |_p| ctl2.pause.store(true, Ordering::SeqCst)).await.unwrap();
        assert_eq!(r, Outcome::Paused);
        assert!(lay.has_partial("0.1.0"));
        assert!(lay.current().is_none(), "nada instalado ainda");
        // retomar: continua do ponto onde parou (Range) e termina
        let ctl = Control::default();
        let r = install(&lay, &v1.manifest, &v1.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        assert_eq!(r, Outcome::Installed { previous: None });
        assert!(verify(&lay).unwrap().is_empty());
        // cancelar uma atualização apaga o parcial e mantém a instalação
        let v2 = release(&srv, "0.2.0", 2, b"EXE-2", &vec![4u8; 2_000_000], &kp, &pk);
        let ctl = Control::default();
        ctl.cancel.store(true, Ordering::SeqCst);
        let r = install(&lay, &v2.manifest, &v2.raw, &c, &net(), &ctl, &noop()).await.unwrap();
        assert_eq!(r, Outcome::Canceled);
        assert!(!lay.has_partial("0.2.0"));
        assert_eq!(lay.current().as_deref(), Some("0.1.0"));
        let _ = uninstall(&lay);
    }

    #[tokio::test]
    async fn dropped_connection_resumes_automatically() {
        let srv = server().await;
        let (kp, pk) = keypair();
        let lay = base("cut");
        let c = client(net());
        let pck = vec![9u8; 400_000];
        srv.files.lock().unwrap().insert("/cut/TombSurvivors.pck".into(), pck.clone());
        let mut v1 = release(&srv, "0.1.0", 1, b"EXE-1", &pck, &kp, &pk);
        for f in v1.manifest.files.iter_mut().filter(|f| f.path == "TombSurvivors.pck") {
            f.url = format!("http://127.0.0.1:{}/cut/TombSurvivors.pck", srv.port);
        }
        let r = install(&lay, &v1.manifest, &v1.raw, &c, &net(), &Control::default(), &noop()).await.unwrap();
        assert_eq!(r, Outcome::Installed { previous: None });
        assert!(verify(&lay).unwrap().is_empty());
        assert_eq!(srv.hits.lock().unwrap()["/cut/TombSurvivors.pck"], 2, "uma queda, uma retomada (Range)");
        let _ = uninstall(&lay);
    }
}
