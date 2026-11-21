//! Manifesto de distribuição de um jogo integrado (formato em `docs/bridge/NEXSTAGE_GAME_BRIDGE.md` §10).
//!
//! Só é aceito depois de: assinatura Ed25519 válida (chave pública embutida no NexStage) → formato →
//! regras (jogo certo, versão mínima do NexStage, protocolo, URLs só HTTPS em hosts permitidos, caminhos
//! seguros, executável na lista de arquivos). O manifesto é só dados: nunca contém nem executa comandos.

use crate::error::{AppError, AppResult};
use base64::Engine;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Hosts de onde o NexStage aceita baixar (GitHub Releases e os servidores para onde ele redireciona).
pub const ALLOWED_HOSTS: &[&str] = &["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com", "github-releases.githubusercontent.com"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: i64,
    pub game_id: String,
    pub name: String,
    pub version: String,
    pub build: i64,
    #[serde(default)]
    pub released_at: String,
    pub min_nexstage: String,
    pub protocol: Vec<i64>,
    #[serde(default)]
    pub requirements: Value,
    pub executable: String,
    pub files: Vec<FileEntry>,
    #[serde(default)]
    pub assets: Value,
    #[serde(default)]
    pub notes: Value,
    #[serde(default)]
    pub catalog: Value,
}

impl Manifest {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Regras de rede: em produção só HTTPS nos hosts permitidos; os testes usam um servidor local.
#[derive(Clone)]
pub struct NetRules {
    pub hosts: Vec<String>,
    pub allow_http_loopback: bool,
}

impl Default for NetRules {
    fn default() -> Self {
        Self { hosts: ALLOWED_HOSTS.iter().map(|h| h.to_string()).collect(), allow_http_loopback: false }
    }
}

impl NetRules {
    pub fn url_ok(&self, url: &str) -> bool {
        let Some((scheme, rest)) = url.split_once("://") else { return false };
        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
        let (host_only, _port) = host.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).unwrap_or((host, ""));
        if host_only.contains('@') {
            return false;
        }
        match scheme {
            "https" => self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host_only)),
            "http" => self.allow_http_loopback && host_only == "127.0.0.1",
            _ => false,
        }
    }
}

/// Caminho relativo seguro dentro da pasta do jogo (sem `..`, absoluto, unidade, barras invertidas…).
pub fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 200
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
        && p.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
}

/// Compara versões "1.3.1" numericamente (sufixos como "-alpha" são ignorados na comparação).
pub fn version_at_least(have: &str, need: &str) -> bool {
    let nums = |s: &str| -> Vec<u64> { s.split(['-', '+']).next().unwrap_or("").split('.').map(|x| x.parse().unwrap_or(0)).collect() };
    let (a, b) = (nums(have), nums(need));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

fn bad(msg: &str) -> AppError {
    AppError::Validation(format!("Manifesto de distribuição recusado: {msg}."))
}

/// Confere a assinatura destacada (base64) sobre os bytes exatos do manifesto.
pub fn verify_signature(bytes: &[u8], sig_b64: &str, pubkey_b64: &str) -> AppResult<()> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let sig = b64.decode(sig_b64.trim()).map_err(|_| bad("assinatura ilegível"))?;
    let key = b64.decode(pubkey_b64.trim()).map_err(|_| bad("chave pública inválida"))?;
    UnparsedPublicKey::new(&ED25519, key).verify(bytes, &sig).map_err(|_| bad("assinatura inválida"))
}

/// Assinatura → formato → regras. Devolve o manifesto pronto para instalar.
pub fn verify_and_parse(bytes: &[u8], sig_b64: &str, pubkey_b64: &str, game_id: &str, app_version: &str, net: &NetRules) -> AppResult<Manifest> {
    verify_signature(bytes, sig_b64, pubkey_b64)?;
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| bad("formato inválido"))?;
    if m.schema != 1 {
        return Err(bad("versão de formato não suportada (atualize o NexStage)"));
    }
    if m.game_id != game_id {
        return Err(bad("é de outro jogo"));
    }
    if !valid_version(&m.version) || m.build <= 0 {
        return Err(bad("versão inválida"));
    }
    if !version_at_least(app_version, &m.min_nexstage) {
        return Err(AppError::Validation(format!("Esta versão do jogo precisa do NexStage {} ou mais recente. Atualize o NexStage.", m.min_nexstage)));
    }
    if !m.protocol.contains(&crate::bridge::protocol::PROTOCOL) {
        return Err(bad("protocolo de comunicação incompatível"));
    }
    if m.files.is_empty() || m.files.len() > 64 {
        return Err(bad("lista de arquivos inválida"));
    }
    let mut seen = std::collections::HashSet::new();
    for f in &m.files {
        if !safe_path(&f.path) || !seen.insert(f.path.to_lowercase()) {
            return Err(bad("caminho de arquivo inseguro ou repetido"));
        }
        if f.size == 0 || f.size > 8 * 1024 * 1024 * 1024 {
            return Err(bad("tamanho de arquivo inválido"));
        }
        if f.sha256.len() != 64 || !f.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad("hash SHA-256 inválido"));
        }
        if !net.url_ok(&f.url) {
            return Err(bad("endereço de download não permitido"));
        }
    }
    if !m.files.iter().any(|f| f.path == m.executable) {
        return Err(bad("o executável não está na lista de arquivos"));
    }
    Ok(m)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    pub fn keypair() -> (Ed25519KeyPair, String) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let pk = base64::engine::general_purpose::STANDARD.encode(kp.public_key().as_ref());
        (kp, pk)
    }

    pub fn sign(kp: &Ed25519KeyPair, bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(kp.sign(bytes).as_ref())
    }

    fn sample(url: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": 1, "game_id": "tomb-survivors", "name": "Tomb Survivors", "version": "0.2.0-alpha", "build": 7,
            "min_nexstage": "1.3.1", "protocol": [1], "executable": "TombSurvivors.exe",
            "files": [{"path": "TombSurvivors.exe", "size": 3, "sha256": "a".repeat(64), "url": url}]
        })
    }

    #[test]
    fn accepts_signed_and_rejects_tampered() {
        let (kp, pk) = keypair();
        let bytes = serde_json::to_vec(&sample("https://github.com/x/y/releases/download/v1/TombSurvivors.exe")).unwrap();
        let sig = sign(&kp, &bytes);
        let net = NetRules::default();
        assert!(verify_and_parse(&bytes, &sig, &pk, "tomb-survivors", "1.3.1", &net).is_ok());
        let mut tampered = bytes.clone();
        tampered[10] ^= 1;
        assert!(verify_and_parse(&tampered, &sig, &pk, "tomb-survivors", "1.3.1", &net).is_err(), "manifesto alterado");
        let (_, other) = keypair();
        assert!(verify_and_parse(&bytes, &sig, &other, "tomb-survivors", "1.3.1", &net).is_err(), "outra chave");
        assert!(verify_and_parse(&bytes, &sig, &pk, "outro-jogo", "1.3.1", &net).is_err(), "outro jogo");
        assert!(verify_and_parse(&bytes, &sig, &pk, "tomb-survivors", "1.2.0", &net).is_err(), "NexStage antigo");
    }

    #[test]
    fn rejects_bad_hosts_and_paths() {
        let (kp, pk) = keypair();
        let net = NetRules::default();
        for url in ["http://github.com/a", "https://evil.com/a", "https://github.com.evil.com/a", "https://user@github.com/a", "file:///C:/x"] {
            let bytes = serde_json::to_vec(&sample(url)).unwrap();
            assert!(verify_and_parse(&bytes, &sign(&kp, &bytes), &pk, "tomb-survivors", "1.3.1", &net).is_err(), "{url}");
        }
        for p in ["../x.exe", "/x.exe", "C:x.exe", "a\\b.exe", "a/../b", "a//b", ""] {
            assert!(!safe_path(p), "{p}");
        }
        assert!(safe_path("TombSurvivors.pck") && safe_path("data/sub-1/file_2.bin"));
        let mut v = sample("https://github.com/a");
        v["executable"] = serde_json::json!("outro.exe");
        let bytes = serde_json::to_vec(&v).unwrap();
        assert!(verify_and_parse(&bytes, &sign(&kp, &bytes), &pk, "tomb-survivors", "1.3.1", &net).is_err(), "executável fora da lista");
    }

    #[test]
    fn versions() {
        assert!(version_at_least("1.3.1", "1.3.1"));
        assert!(version_at_least("1.4.0", "1.3.9"));
        assert!(!version_at_least("1.3.0", "1.3.1"));
        assert!(version_at_least("2.0", "1.9.9"));
    }
}
