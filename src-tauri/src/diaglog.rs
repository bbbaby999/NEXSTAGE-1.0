//! Registro de erros do NEXSTAGE (para "Feedback e relatórios").
//!
//! Antes desta versão o app não gravava log nenhum: os erros só apareciam como aviso na tela.
//! Agora ficam num arquivo pequeno e local, que NUNCA sai do computador sozinho — só é anexado a um
//! relatório quando o usuário marca "Anexar arquivos de log e diagnóstico" e revisa o conteúdo.
//!
//! Arquivo: `<dados do app>/logs/nexstage.log` (até 256 KB; ao passar disso vira `nexstage.old.log`).
//! O que entra: abertura do app (versão), erros mostrados ao usuário, erros de script da interface e
//! pânicos do backend. Nada de senhas ou chaves: o texto passa por `sanitize` ao gravar e de novo ao enviar.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 256 * 1024;
static FILE: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

pub fn dir(data: &Path) -> PathBuf {
    data.join("logs")
}

/// Liga o registro (chamado uma vez na abertura) e captura pânicos do backend.
pub fn init(data: &Path) {
    let d = dir(data);
    if std::fs::create_dir_all(&d).is_err() {
        return;
    }
    let _ = FILE.set(d.join("nexstage.log"));
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        write("PANIC", "backend", &format!("{msg}{loc}"));
        prev(info);
    }));
    write("INFO", "app", &format!("NEXSTAGE {} iniciado (Windows {})", env!("NX_VERSION"), std::env::consts::ARCH));
}

fn now() -> String {
    // UTC, sem depender de crate de datas: AAAA-MM-DD HH:MM:SS
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // algoritmo civil de Howard Hinnant
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Acrescenta uma linha ao log (silencioso se o registro não estiver ligado ou o disco falhar).
pub fn write(level: &str, source: &str, msg: &str) {
    let Some(path) = FILE.get() else { return };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::metadata(path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(path, path.with_file_name("nexstage.old.log"));
    }
    let mut line: String = sanitize(msg).chars().take(2000).collect();
    line = line.replace(['\r', '\n'], " ⏎ ");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} [{level}] {source}: {line}", now());
    }
}

/// Erro vindo da interface (aviso de erro exibido, erro de script). Limitado para não encher o disco.
#[tauri::command]
pub fn log_client_error(source: String, message: String) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    if COUNT.fetch_add(1, Ordering::Relaxed) > 500 {
        return; // por sessão: um laço de erros não pode crescer o arquivo sem fim
    }
    let src: String = source.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect();
    write("ERRO", if src.is_empty() { "ui" } else { &src }, &message);
}

/// Conteúdo dos logs (mais recente por último), já filtrado, limitado a `max` bytes do fim.
pub fn read_recent(data: &Path, max: usize) -> Option<String> {
    let d = dir(data);
    let mut s = String::new();
    for f in ["nexstage.old.log", "nexstage.log"] {
        if let Ok(t) = std::fs::read_to_string(d.join(f)) {
            s.push_str(&t);
        }
    }
    if s.trim().is_empty() {
        return None;
    }
    if s.len() > max {
        let mut cut = s.len() - max;
        while !s.is_char_boundary(cut) {
            cut += 1;
        }
        s = format!("[…linhas mais antigas omitidas…]\n{}", &s[cut..]);
    }
    Some(sanitize(&s))
}

/// Remove dados pessoais e segredos de um texto antes de gravar ou enviar:
/// nome do usuário do Windows e do computador, caminhos de usuário, e-mails, IDs da Steam (17 dígitos),
/// endereços MAC/IP, `chave=valor` de campos sensíveis e sequências longas que parecem chaves/tokens.
pub fn sanitize(text: &str) -> String {
    let mut s = text.to_string();
    for var in ["USERNAME", "COMPUTERNAME", "USERDOMAIN"] {
        if let Ok(v) = std::env::var(var) {
            if v.len() >= 3 {
                s = replace_ci(&s, &v, "<usuario>");
            }
        }
    }
    s = mask_user_paths(&s);
    let words: Vec<String> = split_keep(&s);
    let mut out = String::with_capacity(s.len());
    let mut redact_next = false;
    for w in words {
        if w.chars().all(|c| !c.is_alphanumeric()) {
            out.push_str(&w);
            continue;
        }
        let lower = w.to_lowercase();
        let token = if redact_next {
            redact_next = false;
            "<removido>".to_string()
        } else if is_secret_label(&lower) {
            redact_next = true;
            w
        } else if w.contains('@') && w.contains('.') {
            "<email>".to_string()
        } else if w.len() == 17 && w.chars().all(|c| c.is_ascii_digit()) && w.starts_with("7656") {
            "<steamid>".to_string()
        } else if is_mac(&w) {
            "<mac>".to_string()
        } else if is_ipv4(&w) {
            "<ip>".to_string()
        } else if looks_like_key(&w) {
            "<removido>".to_string()
        } else {
            w
        };
        out.push_str(&token);
    }
    out
}

fn replace_ci(s: &str, needle: &str, with: &str) -> String {
    let (ls, ln) = (s.to_lowercase(), needle.to_lowercase());
    if ls.len() != s.len() || !ls.contains(&ln) {
        return s.replace(needle, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = ls[i..].find(&ln) {
        out.push_str(&s[i..i + p]);
        out.push_str(with);
        i += p + ln.len();
    }
    out.push_str(&s[i..]);
    out
}

/// `C:\Users\Fulano\...` → `C:\Users\<usuario>\...` (qualquer nome, inclusive de outro usuário).
fn mask_user_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let lower = s.to_lowercase();
    let mut i = 0;
    while let Some(p) = lower[i..].find("users").map(|p| p + i) {
        let sep_after = s[p + 5..].chars().next();
        let sep_before = s[..p].chars().last();
        if matches!(sep_after, Some('\\') | Some('/')) && matches!(sep_before, Some('\\') | Some('/')) {
            out.push_str(&s[i..p + 6]);
            let rest = &s[p + 6..];
            // nomes de usuário podem ter espaço ("Maria Souza"): vai até a próxima barra; sem barra
            // por perto, para no primeiro espaço
            let end = match rest.find(['\\', '/', '"', '\'', '\n', ';', ',']) {
                Some(e) if e <= 64 => e,
                _ => rest.find(' ').unwrap_or(rest.len()),
            };
            out.push_str("<usuario>");
            i = p + 6 + end;
        } else {
            out.push_str(&s[i..p + 5]);
            i = p + 5;
        }
    }
    out.push_str(&s[i..]);
    out
}

fn split_keep(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut cur = String::new();
    let mut word = None;
    for c in s.chars() {
        let w = c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '@' | ':' | '+' | '/' | '=');
        if word != Some(w) && !cur.is_empty() {
            v.push(std::mem::take(&mut cur));
        }
        word = Some(w);
        cur.push(c);
    }
    if !cur.is_empty() {
        v.push(cur);
    }
    // "senha=abc" chega como uma palavra só: separa o rótulo do valor
    v.into_iter()
        .flat_map(|w| match w.find(['=', ':']) {
            Some(p) if p > 0 && p + 1 < w.len() && is_secret_label(&w[..p].to_lowercase()) => vec![w[..=p].to_string(), w[p + 1..].to_string()],
            _ => vec![w],
        })
        .collect()
}

fn is_secret_label(w: &str) -> bool {
    let w = w.trim_end_matches([':', '=']);
    ["senha", "password", "passwd", "pwd", "token", "secret", "segredo", "apikey", "api_key", "key", "chave", "authorization", "bearer", "cookie", "session"]
        .iter()
        .any(|k| w == *k || w.ends_with(&format!("_{k}")) || w.ends_with(&format!("-{k}")))
}

fn is_mac(w: &str) -> bool {
    let parts: Vec<&str> = w.split([':', '-']).collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_ipv4(w: &str) -> bool {
    let parts: Vec<&str> = w.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
}

/// Sequência longa com mistura de letras e números (chaves de API, tokens, hashes).
fn looks_like_key(w: &str) -> bool {
    let core = w.trim_matches(|c: char| !c.is_alphanumeric());
    if core.len() < 24 || core.contains(['/', '\\', '.']) {
        return false;
    }
    let digits = core.chars().filter(|c| c.is_ascii_digit()).count();
    let letters = core.chars().filter(|c| c.is_ascii_alphabetic()).count();
    digits >= 4 && letters >= 4 && core.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '='))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_removes_personal_data_and_secrets() {
        let s = sanitize(
            "Falha em C:\\Users\\Maria Souza\\AppData\\Roaming\\x.db; email maria@exemplo.com; steam 76561198000000001; \
             key=AbCdEf0123456789AbCdEf0123456789 senha: hunter2 MAC 3c:a0:67:12:34:56 IP 192.168.0.10 token ghp_abcdefghij1234567890ABCDEFGH",
        );
        for bad in ["Maria", "maria@exemplo.com", "76561198000000001", "AbCdEf0123456789", "hunter2", "3c:a0:67", "192.168.0.10", "ghp_abcdefghij"] {
            assert!(!s.contains(bad), "vazou {bad}: {s}");
        }
        assert!(s.contains("C:\\Users\\<usuario>\\AppData"), "{s}");
        // texto comum continua legível
        let ok = sanitize("Não foi possível abrir o jogo Elden Ring (versão 1.10) em D:\\Jogos\\Elden Ring\\eldenring.exe");
        assert_eq!(ok, "Não foi possível abrir o jogo Elden Ring (versão 1.10) em D:\\Jogos\\Elden Ring\\eldenring.exe");
    }

    #[test]
    fn utc_timestamp_format() {
        let t = now();
        assert_eq!(t.len(), 23, "{t}");
        assert!(t.ends_with(" UTC") && t.starts_with("20"));
    }
}
