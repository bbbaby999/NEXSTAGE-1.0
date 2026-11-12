//! Identificação de plataforma pelo CONTEÚDO dos arquivos (somente leitura; nada é executado,
//! montado ou modificado). Cada função recebe um leitor genérico para poder ser testada em memória.

use std::io::{Read, Seek, SeekFrom};

/// Resultado: `platform` = nome oficial do NEXSTAGE (veja `PLATFORMS`) ou `None` quando não dá para afirmar.
pub struct Found {
    pub platform: Option<&'static str>,
    pub note: String,
}
fn found(p: &'static str, note: &str) -> Found { Found { platform: Some(p), note: note.into() } }
fn unk(note: &str) -> Found { Found { platform: None, note: note.into() } }

pub(crate) fn read_at<R: Read + Seek>(r: &mut R, pos: u64, len: usize) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut b = vec![0u8; len];
    r.read_exact(&mut b).ok()?;
    Some(b)
}
pub(crate) fn u32le(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }
fn u32be(b: &[u8], o: usize) -> u32 { u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

/// Tamanho do setor e deslocamento dos dados: ISO comum (2048) ou BIN bruto de 2352 bytes (MODE1/MODE2).
pub(crate) struct Layout { pub sector: u64, pub off: u64 }

pub(crate) fn layout<R: Read + Seek>(r: &mut R) -> Option<Layout> {
    let h = read_at(r, 0, 24)?;
    if h[0] == 0 && h[1..11].iter().all(|&x| x == 0xFF) && h[11] == 0 {
        Some(Layout { sector: 2352, off: if h[15] == 2 { 24 } else { 16 } })
    } else {
        Some(Layout { sector: 2048, off: 0 })
    }
}
pub(crate) fn sector<R: Read + Seek>(r: &mut R, l: &Layout, lba: u64) -> Option<Vec<u8>> {
    read_at(r, lba * l.sector + l.off, 2048)
}

/// Entradas (nome sem ";1", extent, tamanho) de um diretório ISO 9660.
pub(crate) fn list_dir<R: Read + Seek>(r: &mut R, l: &Layout, lba: u64, size: usize) -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    let n = ((size + 2047) / 2048).clamp(1, 64) as u64;
    for i in 0..n {
        let Some(buf) = sector(r, l, lba + i) else { break };
        let mut pos = 0usize;
        while pos + 34 <= 2048 {
            let len = buf[pos] as usize;
            if len == 0 || pos + len > 2048 { break; }
            let nl = buf[pos + 32] as usize;
            if pos + 33 + nl <= 2048 && !(nl == 1 && buf[pos + 33] <= 1) {
                let name = String::from_utf8_lossy(&buf[pos + 33..pos + 33 + nl]).to_uppercase();
                let name = name.split(';').next().unwrap_or("").to_string();
                out.push((name, u32le(&buf, pos + 2), u32le(&buf, pos + 10)));
            }
            pos += len;
        }
    }
    out
}

/// Identifica o console de uma imagem de disco (.iso, ou .bin/.cue de 2352 bytes).
pub fn detect_disc<R: Read + Seek>(r: &mut R) -> Found {
    let Some(l) = layout(r) else { return unk("Arquivo pequeno demais ou ilegível.") };
    if let Some(s) = sector(r, &l, 32) {
        if &s[..20] == b"MICROSOFT*XBOX*MEDIA" { return found("XBOX", "Disco do Xbox (XISO)"); }
    }
    if l.sector == 2048 {
        if let Some(b) = read_at(r, 0x0FD9_0000, 20) {
            if &b[..] == b"MICROSOFT*XBOX*MEDIA" { return found("Xbox 360", "Disco do Xbox 360"); }
        }
    }
    let Some(pvd) = sector(r, &l, 16) else { return unk("Imagem sem sistema de arquivos reconhecível.") };
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        if let Some(h) = read_at(r, 0, 0x20) {
            if u32be(&h, 0x18) == 0x5D1C_9EA3 || u32be(&h, 0x1C) == 0xC233_9F3D {
                return unk("Nintendo Wii/GameCube: plataforma não suportada pelo NEXSTAGE.");
            }
        }
        return unk("Sem sistema de arquivos ISO 9660: plataforma não identificada.");
    }
    let root = list_dir(r, &l, u32le(&pvd, 158) as u64, u32le(&pvd, 166) as usize);
    let has = |n: &str| root.iter().any(|e| e.0 == n);
    if has("PSP_GAME") { return found("PSP", "Disco de PSP (pasta PSP_GAME)"); }
    if has("PS3_GAME") || has("PS3_DISC.SFB") { return found("Playstation 3", "Disco de PlayStation 3"); }
    if let Some(e) = root.iter().find(|e| e.0 == "SYSTEM.CNF") {
        let txt = sector(r, &l, e.1 as u64)
            .map(|s| String::from_utf8_lossy(&s[..(e.2 as usize).min(2048)]).to_uppercase())
            .unwrap_or_default();
        if txt.contains("BOOT2") { return found("Playstation 2", "Disco de PlayStation 2 (SYSTEM.CNF: BOOT2)"); }
        if txt.contains("BOOT") { return found("Playstation 1", "Disco de PlayStation 1 (SYSTEM.CNF: BOOT)"); }
    }
    unk("ISO sem marcadores de console conhecidos (disco de dados ou plataforma não suportada).")
}

/// Informações do executável do Windows lidas do cabeçalho PE.
pub struct Pe { pub gui: bool, pub arch: &'static str }

pub fn detect_pe<R: Read + Seek>(r: &mut R) -> Result<Pe, &'static str> {
    let h = read_at(r, 0, 64).ok_or("arquivo pequeno demais")?;
    if &h[..2] != b"MZ" { return Err("não é um executável do Windows (sem cabeçalho MZ)"); }
    let pe = u32le(&h, 0x3C) as u64;
    let hdr = read_at(r, pe, 24 + 70).ok_or("cabeçalho PE incompleto (arquivo corrompido)")?;
    if &hdr[..4] != b"PE\0\0" { return Err("assinatura PE inválida (arquivo corrompido)"); }
    let machine = u16::from_le_bytes([hdr[4], hdr[5]]);
    let characteristics = u16::from_le_bytes([hdr[22], hdr[23]]);
    if characteristics & 0x2000 != 0 { return Err("é uma biblioteca (DLL), não um programa"); }
    let subsystem = u16::from_le_bytes([hdr[24 + 68], hdr[24 + 69]]);
    let arch = match machine { 0x014C => "x86", 0x8664 => "x64", 0xAA64 => "ARM64", _ => "outra" };
    Ok(Pe { gui: subsystem == 2, arch })
}

pub fn detect_nsp<R: Read + Seek>(r: &mut R) -> Found {
    match read_at(r, 0, 4) {
        Some(h) if &h[..] == b"PFS0" => found("Nintendo Switch", "Pacote NSP do Nintendo Switch (PFS0)"),
        _ => unk("Cabeçalho NSP inválido (arquivo corrompido ou não é um NSP)."),
    }
}

/// .nup (Nintendo Switch 2): sem cabeçalho documentado para conferir, então a plataforma vem da
/// extensão; só se descarta arquivo vazio.
pub fn detect_nup(path: &std::path::Path) -> std::io::Result<Found> {
    Ok(if std::fs::metadata(path)?.len() == 0 {
        unk("Arquivo .nup vazio.")
    } else {
        found("Nintendo Switch 2", "Arquivo NUP do Nintendo Switch 2 (identificado pela extensão)")
    })
}

pub fn detect_pkg<R: Read + Seek>(r: &mut R) -> Found {
    let Some(h) = read_at(r, 0, 8) else { return unk("Arquivo pequeno demais.") };
    if h[..4] == [0x7F, b'P', b'K', b'G'] {
        return match u16::from_be_bytes([h[6], h[7]]) {
            1 => found("Playstation 3", "Pacote PKG do PlayStation 3"),
            2 => unk("PKG de PSP/Vita (tipo 2): não é um pacote de PlayStation 3."),
            _ => unk("PKG de tipo desconhecido."),
        };
    }
    if h[..4] == [0x7F, b'C', b'N', b'T'] { return found("Playstation 4", "Pacote PKG do PlayStation 4"); }
    unk("Cabeçalho PKG inválido (arquivo corrompido ou formato desconhecido).")
}

pub fn detect_vpk<R: Read + Seek>(r: &mut R) -> Found {
    let mut buf = vec![0u8; 256 * 1024];
    let n = r.seek(SeekFrom::Start(0)).ok().and_then(|_| r.read(&mut buf).ok()).unwrap_or(0);
    let b = &buf[..n];
    if n < 4 || &b[..4] != b"PK\x03\x04" { return unk("Não é um pacote VPK/ZIP válido (arquivo corrompido?)."); }
    let has = |needle: &[u8]| b.windows(needle.len()).any(|w| w == needle);
    if has(b"sce_sys") || has(b"eboot.bin") { found("Ps Vita", "Pacote VPK do PlayStation Vita") } else { unk("ZIP sem os arquivos de um jogo de PS Vita (sce_sys / eboot.bin).") }
}

/// Nomes de arquivos referenciados por um .cue (linhas FILE).
pub fn cue_files(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.get(..4).map(|s| s.eq_ignore_ascii_case("FILE")).unwrap_or(false) {
            if let Some(a) = t.find('"') {
                if let Some(b) = t[a + 1..].find('"') { out.push(t[a + 1..a + 1 + b].to_string()); continue; }
            }
            if let Some(n) = t.split_whitespace().nth(1) { out.push(n.to_string()); }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Monta um ISO 9660 mínimo em memória com os nomes (e conteúdo opcional) no diretório raiz.
    fn iso(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
        let mut d = vec![0u8; 40 * 2048];
        let p = 16 * 2048;
        d[p] = 1;
        d[p + 1..p + 6].copy_from_slice(b"CD001");
        d[p + 156] = 34;
        d[p + 158..p + 162].copy_from_slice(&20u32.to_le_bytes());
        d[p + 166..p + 170].copy_from_slice(&2048u32.to_le_bytes());
        let mut pos = 20 * 2048;
        for (i, (name, content)) in entries.iter().enumerate() {
            let nm = format!("{name};1");
            let len = 33 + nm.len() + (nm.len() % 2 == 0) as usize;
            d[pos] = len as u8;
            d[pos + 2..pos + 6].copy_from_slice(&((22 + i) as u32).to_le_bytes());
            let size = content.map(|c| c.len()).unwrap_or(0) as u32;
            d[pos + 10..pos + 14].copy_from_slice(&size.to_le_bytes());
            d[pos + 32] = nm.len() as u8;
            d[pos + 33..pos + 33 + nm.len()].copy_from_slice(nm.as_bytes());
            if let Some(c) = content {
                let o = (22 + i) * 2048;
                d[o..o + c.len()].copy_from_slice(c.as_bytes());
            }
            pos += len;
        }
        d
    }

    #[test]
    fn identifies_consoles_from_inside_the_iso() {
        let ps2 = iso(&[("SYSTEM.CNF", Some("BOOT2 = cdrom0:\\SLUS_200.62;1\nVER = 1.00"))]);
        assert_eq!(detect_disc(&mut Cursor::new(ps2)).platform, Some("Playstation 2"));
        let ps1 = iso(&[("SYSTEM.CNF", Some("BOOT = cdrom:\\SLUS_000.67;1\nTCB = 4"))]);
        assert_eq!(detect_disc(&mut Cursor::new(ps1)).platform, Some("Playstation 1"));
        let psp = iso(&[("PSP_GAME", None)]);
        assert_eq!(detect_disc(&mut Cursor::new(psp)).platform, Some("PSP"));
        let ps3 = iso(&[("PS3_GAME", None)]);
        assert_eq!(detect_disc(&mut Cursor::new(ps3)).platform, Some("Playstation 3"));
        let data = iso(&[("SETUP.EXE", None)]);
        assert_eq!(detect_disc(&mut Cursor::new(data)).platform, None);
        assert_eq!(detect_disc(&mut Cursor::new(vec![0u8; 100])).platform, None);
    }

    #[test]
    fn pe_nsp_pkg_vpk_and_cue() {
        let mut pe = vec![0u8; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&128u32.to_le_bytes());
        pe[128..132].copy_from_slice(b"PE\0\0");
        pe[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[128 + 24 + 68..128 + 24 + 70].copy_from_slice(&2u16.to_le_bytes());
        let info = detect_pe(&mut Cursor::new(pe.clone())).unwrap();
        assert!(info.gui && info.arch == "x64");
        pe[128 + 22..128 + 24].copy_from_slice(&0x2000u16.to_le_bytes()); // DLL
        assert!(detect_pe(&mut Cursor::new(pe)).is_err());
        assert!(detect_pe(&mut Cursor::new(b"nao sou exe".to_vec())).is_err());

        assert_eq!(detect_nsp(&mut Cursor::new(b"PFS0....".to_vec())).platform, Some("Nintendo Switch"));
        assert_eq!(detect_nsp(&mut Cursor::new(b"XXXX".to_vec())).platform, None);

        let mut p3 = vec![0u8; 16]; p3[..4].copy_from_slice(&[0x7F, b'P', b'K', b'G']); p3[7] = 1;
        assert_eq!(detect_pkg(&mut Cursor::new(p3.clone())).platform, Some("Playstation 3"));
        p3[7] = 2;
        assert_eq!(detect_pkg(&mut Cursor::new(p3)).platform, None);

        let mut v = b"PK\x03\x04".to_vec(); v.extend_from_slice(b"....sce_sys/param.sfo");
        assert_eq!(detect_vpk(&mut Cursor::new(v)).platform, Some("Ps Vita"));
        assert_eq!(detect_vpk(&mut Cursor::new(b"PK\x03\x04 outro zip".to_vec())).platform, None);

        let cue = "FILE \"Jogo (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\nFILE Faixa2.bin BINARY\n";
        assert_eq!(cue_files(cue), vec!["Jogo (Track 1).bin", "Faixa2.bin"]);
    }
}
