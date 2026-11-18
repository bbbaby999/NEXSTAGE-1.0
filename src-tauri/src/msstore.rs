//! Jogos da Microsoft Store / app Xbox (Game Pass, MSIX/MSIXVC).
//!
//! O .exe desses jogos só roda com a identidade do pacote: abri-lo direto (ou por outro programa) é
//! bloqueado pelo Windows. Eles são cadastrados com o caminho `shell:AppsFolder\<AUMID>` (AUMID =
//! "<família do pacote>!<id do app>", ex.: `Microsoft.ForteBaseGame_8wekyb3d8bbwe!Forzahorizon6`) e
//! iniciados pelo Explorer, como o menu Iniciar faz.
//!
//! Atalho da área de trabalho criado pelo app Xbox: um .lnk sem arquivo de destino, que aponta para
//! `msgamelaunch://shortcutLaunch/?ProductId=...&Exe=<id do app>`; o id do app é procurado nos
//! manifestos dos pacotes instalados para chegar ao AUMID.
//!
//! Pasta do pacote: registro do Windows (HKCU ...\AppModel\Repository\Packages\<nome completo> →
//! PackageRootFolder), sem abrir processos. Em jogos instalados em `XboxGames`, essa pasta é uma junção
//! para `<unidade>:\XboxGames\<jogo>\Content`; o processo do jogo pode aparecer por qualquer dos dois caminhos.

use std::path::{Path, PathBuf};

pub const PREFIX: &str = r"shell:AppsFolder\";

/// AUMID de um caminho `shell:AppsFolder\<AUMID>` (maiúsculas/minúsculas no prefixo não importam).
pub fn aumid(path: &str) -> Option<&str> {
    let p = path.trim();
    let head = p.get(..PREFIX.len())?;
    let a = &p[PREFIX.len()..];
    (head.eq_ignore_ascii_case(PREFIX) && a.contains('!') && !a.contains(['\\', '/'])).then_some(a)
}

pub fn is_app(path: &str) -> bool {
    aumid(path).is_some()
}

/// Nome completo do pacote ("Nome_Versão_Arq_Recurso_Editor") → família ("Nome_Editor").
fn family_of(full: &str) -> Option<String> {
    let parts: Vec<&str> = full.split('_').collect();
    (parts.len() >= 5).then(|| format!("{}_{}", parts[0], parts[parts.len() - 1]))
}

/// Id do app em `msgamelaunch://shortcutLaunch/?ProductId=...&Exe=<id>`.
fn msgamelaunch_app_id(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("msgamelaunch://") {
        return None;
    }
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k.eq_ignore_ascii_case("exe") && !v.is_empty()).then(|| v.to_string())
    })
}

/// O manifesto declara um app com este id? (`<Application Id="...">`, sem diferenciar maiúsculas)
fn manifest_has_app(xml: &str, app_id: &str) -> bool {
    let needle = format!("id=\"{}\"", app_id.to_lowercase());
    let lower = xml.to_lowercase();
    lower.match_indices("<application ").any(|(i, _)| {
        let tag_end = lower[i..].find('>').map(|e| i + e).unwrap_or(lower.len());
        lower[i..tag_end].contains(&needle)
    })
}

#[cfg(windows)]
mod reg {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(hkey: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
        fn RegEnumKeyExW(hkey: isize, idx: u32, name: *mut u16, cch: *mut u32, res: *mut u32, class: *mut u16, cch_class: *mut u32, ft: *mut u64) -> i32;
        fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, cb: *mut u32) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
    }
    const HKCU: isize = 0x8000_0001u32 as i32 as isize;
    const KEY_READ: u32 = 0x20019;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const PACKAGES: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Pacotes instalados para o usuário: (nome completo, PackageRootFolder).
    pub fn packages() -> Vec<(String, String)> {
        let mut key: isize = 0;
        if unsafe { RegOpenKeyExW(HKCU, wide(PACKAGES).as_ptr(), 0, KEY_READ, &mut key) } != 0 {
            return vec![];
        }
        let mut out = Vec::new();
        let mut name = [0u16; 512];
        for idx in 0.. {
            let mut cch = name.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(key, idx, name.as_mut_ptr(), &mut cch, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
            };
            if r != 0 {
                break; // ERROR_NO_MORE_ITEMS (ou falha): fim da lista
            }
            let full = String::from_utf16_lossy(&name[..cch as usize]);
            if let Some(root) = value(key, &full, "PackageRootFolder") {
                out.push((full, root));
            }
        }
        unsafe { RegCloseKey(key) };
        out
    }

    /// Nome de exibição registrado do pacote (pode vir como referência "ms-resource:" não resolvida).
    pub fn display_name(full: &str) -> Option<String> {
        let mut key: isize = 0;
        if unsafe { RegOpenKeyExW(HKCU, wide(PACKAGES).as_ptr(), 0, KEY_READ, &mut key) } != 0 {
            return None;
        }
        let v = value(key, full, "DisplayName");
        unsafe { RegCloseKey(key) };
        v
    }

    fn value(key: isize, sub: &str, name: &str) -> Option<String> {
        let (sub_w, val_w) = (wide(sub), wide(name));
        let mut buf = vec![0u16; 1024];
        let mut cb = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(key, sub_w.as_ptr(), val_w.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut cb) };
        if r != 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end])).filter(|s| !s.is_empty())
    }
}

#[cfg(windows)]
fn packages() -> Vec<(String, String)> {
    reg::packages()
}
#[cfg(not(windows))]
fn packages() -> Vec<(String, String)> {
    vec![]
}

/// Famílias ("Nome_Editor", em minúsculas) de todos os pacotes instalados para este usuário.
pub fn installed_families() -> std::collections::HashSet<String> {
    packages().into_iter().filter_map(|(full, _)| family_of(&full)).map(|f| f.to_lowercase()).collect()
}

/// Pasta do pacote instalado deste AUMID (None = não instalado para este usuário).
pub fn package_root(aumid: &str) -> Option<PathBuf> {
    let family = aumid.split('!').next()?.to_lowercase();
    packages()
        .into_iter()
        .filter(|(full, _)| family_of(full).map(|f| f.to_lowercase() == family).unwrap_or(false))
        .map(|(_, root)| PathBuf::from(root))
        .find(|p| p.is_dir())
}

pub fn installed(path: &str) -> bool {
    aumid(path).and_then(package_root).is_some()
}

/// Pastas em que o processo do jogo pode estar (a do pacote e o destino da junção), em minúsculas com `\` final.
pub fn process_roots(aumid: &str) -> Vec<String> {
    let Some(root) = package_root(aumid) else { return vec![] };
    let mut v = vec![root.clone()];
    if let Ok(real) = std::fs::canonicalize(&root) {
        v.push(real);
    }
    let mut out: Vec<String> = v
        .iter()
        .map(|p| {
            let s = p.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
            if s.ends_with('\\') { s } else { format!("{s}\\") }
        })
        .collect();
    out.dedup();
    out
}

#[cfg(windows)]
fn display_name(full: &str) -> Option<String> {
    reg::display_name(full)
}
#[cfg(not(windows))]
fn display_name(_full: &str) -> Option<String> {
    None
}

/// Valor do primeiro atributo `name="..."` do texto (sem diferenciar maiúsculas no nome).
fn attr(xml: &str, name: &str) -> Option<String> {
    let needle = format!(" {}=\"", name.to_lowercase());
    let i = xml.to_lowercase().find(&needle)? + needle.len();
    let v = &xml[i..];
    Some(v[..v.find('"')?].to_string()).filter(|s| !s.is_empty())
}

/// Posição da primeira tag `<name ...` (seguida de espaço ou quebra de linha, então `<ApplicationList` não conta).
fn find_tag(xml: &str, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    xml.match_indices(&open).map(|(i, _)| i).find(|i| xml[i + open.len()..].starts_with(|c: char| c.is_whitespace()))
}

/// Id do primeiro app declarado no manifesto (`<Application Id="...">`). Pacotes sem app (DLC,
/// conteúdo extra) não têm.
fn first_app_id(manifest: &str) -> Option<String> {
    let i = find_tag(manifest, "Application")?;
    let tag = &manifest[i..i + manifest[i..].find('>')?];
    attr(tag, "Id")
}

/// Jogo da Microsoft Store / Xbox instalado neste PC.
pub struct InstalledGame {
    pub aumid: String,
    /// Família do pacote ("Nome_Editor"), igual ao `pfn` do histórico do Xbox.
    pub family: String,
    pub name: String,
}

/// Nome legível: o registrado no Windows, senão o do `MicrosoftGame.config`, senão o do pacote.
fn game_name(registered: Option<String>, config: &str, family: &str) -> String {
    registered
        .filter(|n| !n.trim().is_empty() && !n.starts_with("ms-resource") && !n.starts_with('@'))
        .or_else(|| attr(config, "DefaultDisplayName"))
        .or_else(|| attr(config, "OverrideDisplayName"))
        .unwrap_or_else(|| family.split('_').next().unwrap_or(family).to_string())
}

/// Jogos (GDK) instalados para este usuário: pacotes com `MicrosoftGame.config` que declaram um
/// executável e um app no manifesto (DLC e conteúdo extra ficam de fora). Só lê arquivos e registro.
pub fn installed_games() -> Vec<InstalledGame> {
    let mut out: Vec<InstalledGame> = Vec::new();
    for (full, root) in packages() {
        let root = Path::new(&root);
        let Ok(config) = std::fs::read_to_string(root.join("MicrosoftGame.config")) else { continue };
        if find_tag(&config, "Executable").is_none() {
            continue;
        }
        let Some(app) = std::fs::read_to_string(root.join("AppxManifest.xml")).ok().and_then(|m| first_app_id(&m)) else { continue };
        let Some(family) = family_of(&full) else { continue };
        if out.iter().any(|g| g.family.eq_ignore_ascii_case(&family)) {
            continue;
        }
        let name = game_name(display_name(&full), &config, &family);
        out.push(InstalledGame { aumid: format!("{family}!{app}"), family, name });
    }
    out.sort_by_key(|g| g.name.to_lowercase());
    out
}

/// AUMID do pacote instalado que declara o app `app_id` (id do `Exe=` do msgamelaunch).
fn aumid_for_app_id(app_id: &str) -> Option<String> {
    packages().into_iter().find_map(|(full, root)| {
        let xml = std::fs::read_to_string(Path::new(&root).join("AppxManifest.xml")).ok()?;
        if !manifest_has_app(&xml, app_id) {
            return None;
        }
        Some(format!("{}!{app_id}", family_of(&full)?))
    })
}

/// Destino de um atalho (.lnk) da área de trabalho / menu Iniciar.
pub struct Shortcut {
    /// Caminho do executável, ou `shell:AppsFolder\<AUMID>` para jogos da Microsoft Store / Xbox.
    pub target: String,
    pub title: String,
}

/// Lê um atalho .lnk: arquivo comum (.exe) ou jogo da Microsoft Store / Xbox. Usa o Shell do Windows
/// (PowerShell sem janela, só leitura do atalho); o caminho vai por variável de ambiente, sem montar comando.
#[cfg(windows)]
pub fn resolve_lnk(lnk: &Path) -> Result<Shortcut, String> {
    use std::os::windows::process::CommandExt;
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$p = $env:NX_LNK
$sh = New-Object -ComObject Shell.Application
$item = $sh.Namespace([IO.Path]::GetDirectoryName($p)).ParseName([IO.Path]::GetFileName($p))
$ws = (New-Object -ComObject WScript.Shell).CreateShortcut($p)
@{ parsing = [string]$item.ExtendedProperty('System.Link.TargetParsingPath'); target = [string]$ws.TargetPath } | ConvertTo-Json -Compress
"#;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("NX_LNK", lnk)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|_| "Não foi possível ler o atalho.".to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| "Não foi possível ler o atalho.".to_string())?;
    let s = |k: &str| v[k].as_str().unwrap_or("").trim().to_string();
    let title = lnk.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    shortcut_from(&s("parsing"), &s("target"), title)
}
#[cfg(not(windows))]
pub fn resolve_lnk(_lnk: &Path) -> Result<Shortcut, String> {
    Err("Atalhos só são suportados no Windows.".into())
}

fn shortcut_from(parsing: &str, target: &str, title: String) -> Result<Shortcut, String> {
    // Jogo do app Xbox (msgamelaunch://...&Exe=<id>) ou item da pasta de apps (shell:AppsFolder\<AUMID>).
    if let Some(app_id) = msgamelaunch_app_id(parsing) {
        let a = aumid_for_app_id(&app_id)
            .ok_or_else(|| "Este atalho é de um jogo do app Xbox que não está instalado neste PC (ou não pôde ser encontrado).".to_string())?;
        return Ok(Shortcut { target: format!("{PREFIX}{a}"), title });
    }
    let as_app = format!("{PREFIX}{parsing}");
    if is_app(&as_app) && !Path::new(parsing).is_absolute() {
        return Ok(Shortcut { target: as_app, title });
    }
    let exe = if !target.is_empty() { target } else { parsing };
    if exe.is_empty() || !Path::new(exe).is_file() {
        return Err("O atalho não aponta para um executável ou jogo instalado neste PC.".into());
    }
    Ok(Shortcut { target: exe.to_string(), title })
}

/// Inicia o app pelo Explorer (como o menu Iniciar). Não devolve o processo do jogo.
/// AUMID completo para iniciar. Jogo adicionado à biblioteca antes de ser instalado (pelo catálogo do Game
/// Pass) fica gravado só com a família (`<família>!`): o id do app é procurado no pacote instalado.
pub fn resolve_aumid(aumid: &str) -> Option<String> {
    let (family, app) = aumid.split_once('!')?;
    if !app.trim().is_empty() {
        return Some(aumid.to_string());
    }
    installed_games().into_iter().find(|g| g.family.eq_ignore_ascii_case(family)).map(|g| g.aumid)
}

pub fn launch(aumid: &str) -> std::io::Result<()> {
    std::process::Command::new("explorer.exe").arg(format!("{PREFIX}{aumid}")).spawn().map(|_| ())
}

/// PIDs dos processos cujo executável está dentro de uma das pastas (`roots` em minúsculas, com `\` final).
pub fn pids_under(roots: &[String]) -> Vec<u32> {
    if roots.is_empty() {
        return vec![];
    }
    process_paths().into_iter().filter(|(_, p)| roots.iter().any(|r| p.starts_with(r.as_str()))).map(|(pid, _)| pid).collect()
}

/// Processos abertos: (PID, caminho do executável em minúsculas). Processos protegidos ficam de fora.
#[cfg(windows)]
pub fn process_paths() -> Vec<(u32, String)> {
    #[repr(C)]
    struct Entry { size: u32, usage: u32, pid: u32, heap: usize, module: u32, threads: u32, parent: u32, pri: i32, flags: u32, exe: [u16; 260] }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snap: isize, e: *mut Entry) -> i32;
        fn Process32NextW(snap: isize, e: *mut Entry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(h: isize, flags: u32, buf: *mut u16, len: *mut u32) -> i32;
        fn CloseHandle(h: isize) -> i32;
    }
    let snap = unsafe { CreateToolhelp32Snapshot(0x2, 0) };
    if snap == -1 {
        return vec![];
    }
    let mut out = Vec::new();
    let mut e = Entry { size: std::mem::size_of::<Entry>() as u32, usage: 0, pid: 0, heap: 0, module: 0, threads: 0, parent: 0, pri: 0, flags: 0, exe: [0; 260] };
    let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
    while ok {
        if e.pid > 4 {
            let h = unsafe { OpenProcess(0x1000, 0, e.pid) }; // PROCESS_QUERY_LIMITED_INFORMATION
            if h != 0 {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) } != 0 {
                    out.push((e.pid, String::from_utf16_lossy(&buf[..len as usize]).to_lowercase()));
                }
                unsafe { CloseHandle(h) };
            }
        }
        ok = unsafe { Process32NextW(snap, &mut e) } != 0;
    }
    unsafe { CloseHandle(snap) };
    out
}
#[cfg(not(windows))]
pub fn process_paths() -> Vec<(u32, String)> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aumid_paths() {
        assert_eq!(aumid(r"shell:AppsFolder\Microsoft.ForteBaseGame_8wekyb3d8bbwe!Forzahorizon6"), Some("Microsoft.ForteBaseGame_8wekyb3d8bbwe!Forzahorizon6"));
        assert_eq!(aumid(r"SHELL:appsfolder\A_b!C"), Some("A_b!C"));
        assert_eq!(aumid(r"C:\Jogos\jogo.exe"), None);
        assert_eq!(aumid(r"shell:AppsFolder\semexclamacao"), None);
        assert_eq!(aumid("ção"), None, "não corta caractere ao meio");
    }

    #[test]
    fn family_from_full_name() {
        assert_eq!(family_of("Microsoft.ForteBaseGame_3.461.691.0_x64__8wekyb3d8bbwe").as_deref(), Some("Microsoft.ForteBaseGame_8wekyb3d8bbwe"));
        assert_eq!(family_of("curto_demais"), None);
    }

    #[test]
    fn msgamelaunch_exe_param() {
        let u = "msgamelaunch://shortcutLaunch/?ProductId=9NR1R1XWLCNB&Exe=Forzahorizon6";
        assert_eq!(msgamelaunch_app_id(u).as_deref(), Some("Forzahorizon6"));
        assert_eq!(msgamelaunch_app_id("https://x/?Exe=a"), None);
    }

    #[test]
    fn manifest_application_id() {
        let xml = r#"<Package><Applications><Application Id="Forzahorizon6" Executable="gamelaunchhelper.exe"><x/></Application></Applications></Package>"#;
        assert!(manifest_has_app(xml, "forzahorizon6"));
        assert!(!manifest_has_app(xml, "Forza"));
        assert!(!manifest_has_app(r#"<Identity Id="Forzahorizon6"/>"#, "Forzahorizon6"));
    }

    /// Diagnóstico manual: `cargo test --lib msstore::tests::probe_desktop -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_desktop() {
        let desk = PathBuf::from(std::env::var("USERPROFILE").unwrap()).join("Desktop");
        for e in std::fs::read_dir(desk).unwrap().flatten().filter(|e| e.path().extension().map(|x| x == "lnk").unwrap_or(false)) {
            match resolve_lnk(&e.path()) {
                Ok(s) => {
                    let roots = aumid(&s.target).map(process_roots).unwrap_or_default();
                    println!("{} -> {} | installed={} | roots={roots:?}", s.title, s.target, installed(&s.target));
                }
                Err(err) => println!("{:?} -> ERRO {err}", e.path()),
            }
        }
    }

    #[test]
    fn game_package_parsing() {
        let manifest = r#"<Package><Identity Name="Microsoft.ForteBaseGame"/><Applications><Application Id="Forzahorizon6" Executable="GameLaunchHelper.exe"><uap:VisualElements DisplayName="x"/></Application></Applications></Package>"#;
        assert_eq!(first_app_id(manifest).as_deref(), Some("Forzahorizon6"));
        assert_eq!(first_app_id(r#"<Package><Identity Name="Dlc"/></Package>"#), None, "DLC não declara app");
        assert_eq!(first_app_id("<Applications>
<Application
  Id=\"Game\">"), Some(String::from("Game")), "atributos na linha seguinte");
        assert!(find_tag("<ExecutableList>
<Executable
 Name=\"x\"/>", "Executable").is_some());
        assert!(find_tag("<ExecutableList></ExecutableList>", "Executable").is_none());
        let config = r#"<Executable Name="forzahorizon6.exe" Id="Forzahorizon6" OverrideDisplayName="Forza Horizon 6"/><ShellVisuals DefaultDisplayName="FH6 Shell"/>"#;
        assert_eq!(game_name(Some(String::from("Forza Horizon 6")), config, "A_b"), "Forza Horizon 6");
        assert_eq!(game_name(Some(String::from("ms-resource:AppName")), config, "A_b"), "FH6 Shell");
        assert_eq!(game_name(None, "", "Pub.Game_abc"), "Pub.Game");
        assert_eq!(attr(r#"<x AppId="no" Id="yes">"#, "Id").as_deref(), Some("yes"), "não confunde AppId com Id");
    }

    /// Diagnóstico manual: `cargo test --lib msstore::tests::probe_installed_games -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_installed_games() {
        for g in installed_games() {
            println!("{} | {} | roots={:?}", g.name, g.aumid, process_roots(&g.aumid));
        }
    }

    #[test]
    fn plain_aumid_shortcut() {
        let s = shortcut_from("Pub.Game_abc!App", "", String::from("x")).unwrap();
        assert_eq!(s.target, r"shell:AppsFolder\Pub.Game_abc!App");
    }
}
