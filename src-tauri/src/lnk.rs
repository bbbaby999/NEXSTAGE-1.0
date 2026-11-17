//! Leitura nativa de atalhos do Windows (.lnk, formato "Shell Link" [MS-SHLLINK]).
//!
//! Só o necessário para achar o destino de um atalho comum: o caminho local (LinkInfo). Rápido o bastante para listar uma pasta cheia de atalhos —
//! `msstore::resolve_lnk` (via Shell/PowerShell) continua para os casos especiais (jogos da Microsoft
//! Store / Xbox, atalhos só com lista de IDs).

use std::path::{Path, PathBuf};

pub struct LnkTarget {
    pub path: PathBuf,
}

const HEADER_SIZE: usize = 0x4C;
const HAS_ID_LIST: u32 = 0x1;
const HAS_LINK_INFO: u32 = 0x2;
const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;

fn u16_at(b: &[u8], i: usize) -> Option<u16> { Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?)) }
fn u32_at(b: &[u8], i: usize) -> Option<u32> { Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?)) }

/// Texto terminado em zero: ANSI (Latin-1 aproximado, suficiente para o Windows em português) ou UTF-16.
fn cstr(b: &[u8], i: usize) -> Option<String> {
    let s = b.get(i..)?;
    let end = s.iter().position(|&c| c == 0)?;
    Some(s[..end].iter().map(|&c| c as char).collect())
}
fn wstr(b: &[u8], i: usize) -> Option<String> {
    let mut v = Vec::new();
    let mut j = i;
    loop {
        let c = u16_at(b, j)?;
        if c == 0 {
            break;
        }
        v.push(c);
        j += 2;
    }
    Some(String::from_utf16_lossy(&v))
}

/// Interpreta os bytes de um .lnk.
pub fn parse(b: &[u8]) -> Option<LnkTarget> {
    if b.len() < HEADER_SIZE || u32_at(b, 0)? != 0x4C {
        return None;
    }
    let flags = u32_at(b, 0x14)?;
    let mut i = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        i += 2 + u16_at(b, i)? as usize;
    }
    if flags & HAS_LINK_INFO == 0 {
        return None;
    }
    let li = i;
    let header = u32_at(b, li + 4)? as usize;
    let li_flags = u32_at(b, li + 8)?;
    if li_flags & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None; // atalho de rede/sem caminho local
    }
    let base_off = u32_at(b, li + 16)? as usize;
    let suffix_off = u32_at(b, li + 24)? as usize;
    // versões Unicode (cabeçalho >= 0x24) têm prioridade: acentos e nomes fora do ANSI
    let (base, suffix) = if header >= 0x24 {
        let bu = u32_at(b, li + 28)? as usize;
        let su = u32_at(b, li + 32)? as usize;
        (wstr(b, li + bu)?, if su > 0 { wstr(b, li + su).unwrap_or_default() } else { String::new() })
    } else {
        (cstr(b, li + base_off)?, cstr(b, li + suffix_off).unwrap_or_default())
    };
    let full = if suffix.is_empty() {
        base
    } else if base.ends_with('\\') {
        format!("{base}{suffix}")
    } else {
        format!("{base}\\{suffix}")
    };
    if full.trim().is_empty() {
        return None;
    }
    Some(LnkTarget { path: PathBuf::from(full) })
}

/// Lê um atalho do disco (no máximo 64 KB; atalhos comuns têm 1–4 KB).
pub fn read(path: &Path) -> Option<LnkTarget> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 64 * 1024 {
        return None;
    }
    parse(&std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monta um .lnk mínimo: cabeçalho + LinkInfo com caminho local Unicode.
    fn build(path: &str) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_SIZE];
        b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        b[0x14..0x18].copy_from_slice(&HAS_LINK_INFO.to_le_bytes());
        let ansi: Vec<u8> = path.bytes().chain([0]).collect();
        let uni: Vec<u8> = path.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect();
        let header = 0x24u32;
        let base_off = header;
        let suffix_off = base_off + ansi.len() as u32;
        let base_u = suffix_off + 1;
        let suffix_u = base_u + uni.len() as u32;
        let size = suffix_u + 2;
        let mut li = Vec::new();
        for v in [size, header, VOLUME_ID_AND_LOCAL_BASE_PATH, 0, base_off, 0, suffix_off, base_u, suffix_u] {
            li.extend_from_slice(&v.to_le_bytes());
        }
        li.extend_from_slice(&ansi);
        li.push(0); // sufixo ANSI vazio
        li.extend_from_slice(&uni);
        li.extend_from_slice(&[0, 0]); // sufixo Unicode vazio
        b.extend_from_slice(&li);
        b
    }

    #[test]
    fn reads_folder_shortcut_with_accents() {
        let t = parse(&build("C:\\Users\\Usuário\\Nova pasta")).unwrap();
        assert_eq!(t.path, PathBuf::from("C:\\Users\\Usuário\\Nova pasta"));
    }

    #[test]
    fn reads_file_shortcut_and_rejects_garbage() {
        let t = parse(&build("D:\\Jogos\\jogo.exe")).unwrap();
        assert_eq!(t.path, PathBuf::from("D:\\Jogos\\jogo.exe"));
        assert!(parse(b"not a shortcut").is_none());
    }
}
