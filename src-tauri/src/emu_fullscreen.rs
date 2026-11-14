//! Opção "Iniciar jogos em tela cheia" do cadastro de emuladores.
//!
//! Cada emulador abre em tela cheia de um jeito: a maioria aceita um argumento de linha de comando, mas o
//! RPCS3 não tem argumento para isso e só obedece à própria configuração (`config.yml`). Aqui o emulador é
//! reconhecido pelo nome do executável (ou pelo nome cadastrado) e a opção é aplicada no método certo.
//! A opção do emulador vale para todos os jogos associados: os argumentos de tela cheia/janela que estiverem
//! escritos à mão (no emulador ou no jogo) são removidos e substituídos pelos da opção.

use std::path::Path;

/// Como o emulador entra em tela cheia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pcsx2,
    DuckStation,
    Rpcs3,
    Ppsspp,
    Vita3k,
    Ryujinx,
    /// yuzu e derivados (suyu, sudachi, citron, eden, torzu): `-f`.
    Yuzu,
    Lichii,
    Xemu,
    Xenia,
}

/// Reconhece o emulador. O executável vem primeiro: o nome cadastrado pode ser genérico
/// (ex.: "Emulador de Switch (Ryujinx ou compatível)" apontando para um yuzu).
pub fn detect(name: &str, exe: &str) -> Option<Kind> {
    let stem = Path::new(exe).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    by_text(&stem).or_else(|| by_text(&name.to_lowercase()))
}

fn by_text(s: &str) -> Option<Kind> {
    const TABLE: &[(&str, Kind)] = &[
        ("pcsx2", Kind::Pcsx2),
        ("duckstation", Kind::DuckStation),
        ("rpcs3", Kind::Rpcs3),
        ("ppsspp", Kind::Ppsspp),
        ("vita3k", Kind::Vita3k),
        ("ryujinx", Kind::Ryujinx),
        ("ryubing", Kind::Ryujinx),
        ("yuzu", Kind::Yuzu),
        ("suyu", Kind::Yuzu),
        ("sudachi", Kind::Yuzu),
        ("citron", Kind::Yuzu),
        ("torzu", Kind::Yuzu),
        ("eden", Kind::Yuzu),
        ("lichii", Kind::Lichii),
        ("xemu", Kind::Xemu),
        ("xenia", Kind::Xenia),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, v)| *v)
}

impl Kind {
    /// Argumento(s) que abrem em tela cheia (vazio = não é por argumento).
    fn on(self) -> &'static [&'static str] {
        match self {
            Kind::Pcsx2 | Kind::DuckStation => &["-fullscreen"],
            Kind::Ppsspp => &["--fullscreen"],
            Kind::Vita3k => &["-F"],
            Kind::Ryujinx => &["--fullscreen"],
            Kind::Yuzu | Kind::Lichii => &["-f"],
            Kind::Xemu => &["-full-screen"],
            Kind::Xenia => &["--fullscreen=true"],
            Kind::Rpcs3 => &[],
        }
    }
    /// Argumento(s) que forçam janela (só onde o emulador tem um; nos outros, basta não pedir tela cheia).
    fn off(self) -> &'static [&'static str] {
        match self {
            Kind::Pcsx2 | Kind::DuckStation => &["-nofullscreen"],
            Kind::Ppsspp => &["--windowed"],
            Kind::Xenia => &["--fullscreen=false"],
            _ => &[],
        }
    }
    /// Argumentos de tela cheia/janela deste emulador que podem estar escritos à mão (são substituídos).
    fn is_mode_arg(self, a: &str) -> bool {
        let l = a.to_lowercase();
        // "-fullscreen"/"--fullscreen" escritos à mão (às vezes copiados de outro emulador, que este não entende)
        if l == "-fullscreen" || l == "--fullscreen" {
            return true;
        }
        match self {
            Kind::Pcsx2 | Kind::DuckStation => l == "-fullscreen" || l == "-nofullscreen",
            Kind::Ppsspp => l == "--fullscreen" || l == "--windowed",
            Kind::Vita3k => a == "-F" || l == "--fullscreen",
            Kind::Ryujinx => l == "--fullscreen" || l == "-f",
            Kind::Yuzu | Kind::Lichii => l == "-f" || l == "--fullscreen",
            Kind::Xemu => l == "-full-screen",
            Kind::Xenia => l == "--fullscreen" || l.starts_with("--fullscreen="),
            Kind::Rpcs3 => false,
        }
    }
}

/// O NEXSTAGE sabe aplicar a opção a este emulador?
pub fn supported(name: &str, exe: &str) -> bool {
    detect(name, exe).is_some()
}

/// Remove dos argumentos (texto do cadastro) os de tela cheia/janela do emulador: a caixa passa a mandar.
pub fn strip_text(kind: Kind, args: &str) -> String {
    // Separação simples por espaço, mantendo trechos entre aspas intactos.
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for c in args.chars() {
        if c == '"' { q = !q; }
        if c.is_whitespace() && !q {
            if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out.retain(|a| !kind.is_mode_arg(a));
    out.join(" ")
}

/// Aplica a opção aos argumentos já separados: tira os de modo escritos à mão e põe os da opção no início
/// (alguns emuladores, como Lichii/yuzu e PCSX2 com `--`, param de ler opções depois do arquivo do jogo).
pub fn apply_args(kind: Kind, fullscreen: bool, mut args: Vec<String>) -> Vec<String> {
    args.retain(|a| !kind.is_mode_arg(a));
    let add = if fullscreen { kind.on() } else { kind.off() };
    let mut out: Vec<String> = add.iter().map(|s| s.to_string()).collect();
    out.append(&mut args);
    out
}

/// Ajustes fora dos argumentos, feitos logo antes de iniciar (RPCS3: "Start games in fullscreen mode" no
/// `config.yml` da pasta do emulador e nas configurações personalizadas por jogo, que sobrepõem a global).
/// Falhas não impedem o jogo de abrir.
pub fn apply_config(kind: Kind, fullscreen: bool, emu_exe: &Path) {
    if kind != Kind::Rpcs3 {
        return;
    }
    let Some(dir) = emu_exe.parent() else { return };
    // Versões novas usam `config/config.yml`; antigas, `config.yml` ao lado do executável.
    let new = dir.join("config").join("config.yml");
    let old = dir.join("config.yml");
    let main = if !new.is_file() && old.is_file() { old } else { new };
    let _ = write_rpcs3(&main, fullscreen, true);
    {
        if let Ok(rd) = std::fs::read_dir(dir.join("config").join("custom_configs")) {
            for e in rd.flatten() {
                let p = e.path();
                let is_cfg = p.extension().is_some_and(|x| x == "yml")
                    && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("config_"));
                if is_cfg {
                    let _ = write_rpcs3(&p, fullscreen, false); // só altera se a chave já existir
                }
            }
        }
    }
}

const RPCS3_KEY: &str = "Start games in fullscreen mode:";

/// Tela cheia já ligada na configuração do próprio emulador (RPCS3)? Usado na migração para a caixa nascer
/// com o estado atual, e não desligar o que o usuário já tinha configurado no emulador.
pub fn config_fullscreen(kind: Kind, emu_exe: &Path) -> Option<bool> {
    if kind != Kind::Rpcs3 {
        return None;
    }
    let dir = emu_exe.parent()?;
    let new = dir.join("config").join("config.yml");
    let path = if new.is_file() { new } else { dir.join("config.yml") };
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with(RPCS3_KEY))?;
    Some(line.trim_start()[RPCS3_KEY.len()..].trim() == "true")
}

/// Grava a chave no YAML do RPCS3, preservando o resto do arquivo. `create`: cria arquivo/seção/chave se faltar
/// (o RPCS3 completa o restante com os padrões ao abrir).
fn write_rpcs3(path: &Path, on: bool, create: bool) -> std::io::Result<()> {
    let val = if on { "true" } else { "false" };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) if create => {
            if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
            return std::fs::write(path, format!("Miscellaneous:\n  {RPCS3_KEY} {val}\n"));
        }
        Err(e) => return Err(e),
    };
    let out = set_rpcs3_key(&text, val, create);
    match out {
        Some(o) if o != text => std::fs::write(path, o),
        _ => Ok(()),
    }
}

/// Texto do YAML com a chave definida; `None` se ela não existe e `create` é falso.
fn set_rpcs3_key(text: &str, val: &str, create: bool) -> Option<String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    if let Some(l) = lines.iter_mut().find(|l| l.trim_start().starts_with(RPCS3_KEY)) {
        let indent = &l[..l.len() - l.trim_start().len()];
        *l = format!("{indent}{RPCS3_KEY} {val}");
    } else if !create {
        return None;
    } else if let Some(i) = lines.iter().position(|l| l.trim_end() == "Miscellaneous:") {
        lines.insert(i + 1, format!("  {RPCS3_KEY} {val}"));
    } else {
        lines.push("Miscellaneous:".into());
        lines.push(format!("  {RPCS3_KEY} {val}"));
    }
    let mut s = lines.join(nl);
    s.push_str(nl);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn detects_by_exe_then_name() {
        assert_eq!(detect("Emulador de Switch (Ryujinx ou compatível)", r"C:\emu\yuzu.exe"), Some(Kind::Yuzu));
        assert_eq!(detect("Emulador de Switch (Ryujinx ou compatível)", r"C:\emu\app.exe"), Some(Kind::Ryujinx));
        assert_eq!(detect("PS2", r"C:\PCSX2\pcsx2-qt.exe"), Some(Kind::Pcsx2));
        assert_eq!(detect("X360", r"C:\x\xenia_canary.exe"), Some(Kind::Xenia));
        assert_eq!(detect("Meu emu", r"C:\x\foo.exe"), None);
    }

    #[test]
    fn replaces_manual_mode_args_and_keeps_order() {
        let a = v(&["-batch", "-fullscreen", "--", "C:\\g.iso"]);
        assert_eq!(apply_args(Kind::Pcsx2, true, a.clone()), v(&["-fullscreen", "-batch", "--", "C:\\g.iso"]));
        assert_eq!(apply_args(Kind::Pcsx2, false, a), v(&["-nofullscreen", "-batch", "--", "C:\\g.iso"]));
        assert_eq!(apply_args(Kind::Lichii, true, v(&["-f", "-g", "x.nsp"])), v(&["-f", "-g", "x.nsp"]));
        assert_eq!(apply_args(Kind::Lichii, false, v(&["-f", "-g", "x.nsp"])), v(&["-g", "x.nsp"]));
        assert_eq!(apply_args(Kind::Rpcs3, true, v(&["--no-gui", "x"])), v(&["--no-gui", "x"]));
        assert_eq!(apply_args(Kind::Xenia, false, v(&["--fullscreen", "x"])), v(&["--fullscreen=false", "x"]));
        // argumento de outro emulador (PCSX2) num Vita3K: trocado pelo do Vita3K
        assert_eq!(apply_args(Kind::Vita3k, true, v(&["-fullscreen", "--", "x"])), v(&["-F", "--", "x"]));
    }

    #[test]
    fn strips_text_keeping_quotes() {
        assert_eq!(strip_text(Kind::Pcsx2, r#"-batch -fullscreen -- "{rom}""#), r#"-batch -- "{rom}""#);
        assert_eq!(strip_text(Kind::Xemu, r#"-full-screen -dvd_path "C:\a b\{rom}""#), r#"-dvd_path "C:\a b\{rom}""#);
    }

    #[test]
    fn rpcs3_yaml_key() {
        let y = "Core:\n  PPU Decoder: Recompiler (LLVM)\nMiscellaneous:\n  Automatically start games after boot: true\n  Start games in fullscreen mode: false\n";
        let o = set_rpcs3_key(y, "true", true).unwrap();
        assert!(o.contains("  Start games in fullscreen mode: true\n"));
        assert!(o.contains("PPU Decoder: Recompiler (LLVM)"));
        let o = set_rpcs3_key("Core:\n  A: 1\n", "true", true).unwrap();
        assert!(o.ends_with("Miscellaneous:\n  Start games in fullscreen mode: true\n"));
        assert_eq!(set_rpcs3_key("Core:\n  A: 1\n", "true", false), None);
        let o = set_rpcs3_key("Miscellaneous:\r\n  X: 1\r\n", "false", true).unwrap();
        assert_eq!(o, "Miscellaneous:\r\n  Start games in fullscreen mode: false\r\n  X: 1\r\n");
    }
}
