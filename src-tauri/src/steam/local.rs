//! Cliente Steam instalado neste PC — SOMENTE LEITURA de arquivos públicos de configuração:
//! - pasta da Steam (registro do Windows: HKCU\Software\Valve\Steam → SteamPath);
//! - bibliotecas (`steamapps/libraryfolders.vdf`) e jogos instalados (`appmanifest_<appid>.acf`);
//! - protocolo `steam://` registrado (HKCR\steam\Shell\Open\Command);
//! - jogo em execução (HKCU\Software\Valve\Steam → RunningAppID) e a DATA de modificação dos arquivos
//!   `appcache/stats/UserGameStats_<conta>_<appid>.bin` (o conteúdo não é lido): gatilho para conferir
//!   conquistas novas na Web API (ver `watch`).
//! Nada é escrito, nenhum processo é aberto e nenhuma credencial do cliente (login, cookies,
//! `config.vdf`, `loginusers.vdf`, ssfn) é lida.

use super::vdf::{self, Vdf};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod reg {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, cb: *mut u32) -> i32;
    }
    pub const HKCR: isize = 0x8000_0000u32 as i32 as isize;
    pub const HKCU: isize = 0x8000_0001u32 as i32 as isize;
    pub const HKLM: isize = 0x8000_0002u32 as i32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const RRF_RT_REG_DWORD: u32 = 0x10;

    /// Valor DWORD do registro.
    pub fn dword(root: isize, sub: &str, name: &str) -> Option<u32> {
        let (sub_w, name_w) = (wide(sub), wide(name));
        let mut v: u32 = 0;
        let mut cb: u32 = 4;
        let r = unsafe { RegGetValueW(root, sub_w.as_ptr(), name_w.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as *mut c_void, &mut cb) };
        (r == 0).then_some(v)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Valor texto do registro (`name = None` = valor padrão da chave).
    pub fn string(root: isize, sub: &str, name: Option<&str>) -> Option<String> {
        let sub_w = wide(sub);
        let name_w = name.map(wide);
        let name_p = name_w.as_ref().map(|n| n.as_ptr()).unwrap_or(std::ptr::null());
        let mut cb: u32 = 0;
        let r = unsafe { RegGetValueW(root, sub_w.as_ptr(), name_p, RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut cb) };
        if r != 0 || cb == 0 {
            return None;
        }
        let mut buf = vec![0u16; (cb as usize).div_ceil(2) + 1];
        let mut cb2 = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(root, sub_w.as_ptr(), name_p, RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut cb2) };
        if r != 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// AppID do jogo da Steam em execução agora (`None` = nenhum), seja qual for o jeito como foi aberto.
pub fn running_appid() -> Option<i64> {
    #[cfg(windows)]
    {
        reg::dword(reg::HKCU, r"Software\Valve\Steam", "RunningAppID").filter(|v| *v > 0).map(i64::from)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Data de modificação de cada `UserGameStats_<conta>_<appid>.bin` da conta (AppID → data).
/// O cliente Steam regrava o arquivo do jogo quando as estatísticas/conquistas mudam.
pub fn stats_mtimes(account_id: u32) -> HashMap<i64, std::time::SystemTime> {
    let Some(dir) = steam_root().map(|r| r.join("appcache").join("stats")) else { return HashMap::new() };
    let prefix = format!("UserGameStats_{account_id}_");
    let Ok(rd) = std::fs::read_dir(dir) else { return HashMap::new() };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let appid = name.strip_prefix(&prefix)?.strip_suffix(".bin")?.parse::<i64>().ok()?;
            Some((appid, e.metadata().ok()?.modified().ok()?))
        })
        .collect()
}

/// Pasta de instalação da Steam (`None` = cliente não encontrado).
pub fn steam_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let cands = [
            reg::string(reg::HKCU, r"Software\Valve\Steam", Some("SteamPath")),
            reg::string(reg::HKLM, r"SOFTWARE\WOW6432Node\Valve\Steam", Some("InstallPath")),
            reg::string(reg::HKLM, r"SOFTWARE\Valve\Steam", Some("InstallPath")),
        ];
        for c in cands.into_iter().flatten() {
            let p = PathBuf::from(c.replace('/', "\\"));
            if p.join("steamapps").is_dir() {
                return Some(p);
            }
        }
    }
    let def = PathBuf::from(r"C:\Program Files (x86)\Steam");
    def.join("steamapps").is_dir().then_some(def)
}

/// O protocolo `steam://` está registrado no Windows (necessário para instalar/iniciar pelo cliente).
pub fn protocol_available() -> bool {
    #[cfg(windows)]
    {
        reg::string(reg::HKCR, r"steam\Shell\Open\Command", None).map(|c| !c.trim().is_empty()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Bibliotecas de jogos da Steam (pasta raiz + as do libraryfolders.vdf).
pub fn library_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    if let Ok(txt) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        let v = vdf::parse(&txt);
        if let Some(lf) = v.get("libraryfolders") {
            for (k, e) in lf.entries() {
                if !k.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let path = match e {
                    Vdf::Str(s) => Some(s.as_str()),
                    Vdf::Obj(_) => e.str("path"),
                };
                if let Some(p) = path {
                    let pb = PathBuf::from(p);
                    if !out.iter().any(|o| o.to_string_lossy().eq_ignore_ascii_case(&pb.to_string_lossy())) {
                        out.push(pb);
                    }
                }
            }
        }
    }
    out.into_iter().filter(|p| p.join("steamapps").is_dir()).collect()
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LocalApp {
    pub appid: i64,
    pub name: String,
    /// "installed" | "updating" | "installing"
    pub state: String,
    pub installed: bool,
    pub size_on_disk: i64,
    pub last_updated: i64,
}

/// Interpreta um appmanifest. StateFlags: 4 = totalmente instalado; 2/1024/... = atualização pendente.
pub fn parse_manifest(text: &str) -> Option<LocalApp> {
    let v = vdf::parse(text);
    let st = v.get("AppState")?;
    let appid: i64 = st.str("appid")?.parse().ok()?;
    let flags: i64 = st.str("StateFlags").and_then(|s| s.parse().ok()).unwrap_or(0);
    let installed = flags & 4 != 0;
    let state = if installed && flags & !4 == 0 {
        "installed"
    } else if installed {
        "updating"
    } else {
        "installing"
    };
    Some(LocalApp {
        appid,
        name: st.str("name").unwrap_or("").to_string(),
        state: state.into(),
        installed,
        size_on_disk: st.str("SizeOnDisk").and_then(|s| s.parse().ok()).unwrap_or(0),
        last_updated: st.str("LastUpdated").and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

/// Todos os apps com manifesto nas bibliotecas locais (instalados ou em instalação).
pub fn local_apps() -> HashMap<i64, LocalApp> {
    let mut out = HashMap::new();
    let Some(root) = steam_root() else { return out };
    for lib in library_dirs(&root) {
        let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(e.path()).ok().as_deref().and_then(parse_manifest) {
                // ferramentas da Steam (redistribuíveis, Proton, SDKs) ficam de fora
                if app.appid == 228980 || app.name.to_lowercase().contains("steamworks common") {
                    continue;
                }
                out.insert(app.appid, app);
            }
        }
    }
    out
}

pub fn app(appid: i64) -> Option<LocalApp> {
    local_apps().remove(&appid)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientStatus {
    pub found: bool,
    pub path: Option<String>,
    pub protocol: bool,
    pub libraries: usize,
    pub installed: usize,
}

pub fn client_status() -> ClientStatus {
    let root = steam_root();
    let libs = root.as_ref().map(|r| library_dirs(r).len()).unwrap_or(0);
    let installed = local_apps().values().filter(|a| a.installed).count();
    ClientStatus { found: root.is_some(), path: root.map(|r| r.to_string_lossy().into_owned()), protocol: protocol_available(), libraries: libs, installed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_states() {
        let m = |f: &str| format!("\"AppState\" {{ \"appid\" \"570\" \"name\" \"Dota 2\" \"StateFlags\" \"{f}\" \"SizeOnDisk\" \"123\" }}");
        let a = parse_manifest(&m("4")).unwrap();
        assert!(a.installed && a.state == "installed" && a.appid == 570 && a.size_on_disk == 123);
        assert_eq!(parse_manifest(&m("6")).unwrap().state, "updating");
        let b = parse_manifest(&m("1026")).unwrap();
        assert!(!b.installed && b.state == "installing");
        assert!(parse_manifest("lixo").is_none());
    }

    /// Mostra o que foi detectado neste PC: cargo test steam_local_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn steam_local_live() {
        let s = client_status();
        println!("steam: {:?} protocolo: {} bibliotecas: {} instalados: {}", s.path, s.protocol, s.libraries, s.installed);
        let mut apps: Vec<_> = local_apps().into_values().collect();
        apps.sort_by_key(|a| a.appid);
        for a in apps.iter().take(15) {
            println!("  {} {} [{}]", a.appid, a.name, a.state);
        }
    }
}
