//! Hash de identificação do RetroAchievements (mesmo algoritmo da biblioteca oficial `rcheevos`,
//! `rc_hash.c`), calculado SOMENTE LENDO o arquivo do jogo (nada é montado, executado ou alterado).
//!
//! - PlayStation: nome do executável principal (SYSTEM.CNF → BOOT, ou PSX.EXE) + conteúdo dele;
//!   o tamanho vem do cabeçalho "PS-X EXE" (+ 2048 do próprio cabeçalho).
//! - PlayStation 2: nome do executável (SYSTEM.CNF → BOOT2) + conteúdo (tamanho do diretório).
//! - PSP: PSP_GAME\PARAM.SFO + PSP_GAME\SYSDIR\EBOOT.BIN (sem nomes).
//! - NES: arquivo inteiro sem o cabeçalho iNES de 16 bytes; SNES: sem o cabeçalho de copiadora de 512 bytes;
//!   Game Boy, Game Boy Color, Game Boy Advance e Mega Drive: arquivo inteiro.
//! - Nintendo 64: arquivo inteiro convertido para a ordem big-endian (.z64) — .v64 troca os bytes de cada
//!   par, .n64 inverte cada palavra de 4 bytes.
//!
//! A leitura do ISO 9660 reaproveita `crate::detect` (a mesma usada para identificar a plataforma na
//! importação), tanto para .iso (setores de 2048) quanto para .bin/.cue brutos (2352).

use super::md5::Md5;
use crate::detect::{layout, list_dir, read_at, sector, u32le, Layout};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

pub const PS1: i64 = 12;
pub const PS2: i64 = 21;
pub const PSP: i64 = 41;
pub const NES: i64 = 7;
pub const SNES: i64 = 3;
pub const GBA: i64 = 5;
pub const MEGA_DRIVE: i64 = 1;
pub const N64: i64 = 2;
pub const GB: i64 = 4;
pub const GBC: i64 = 6;

/// Plataforma oficial do NEXSTAGE → ID do console no RetroAchievements (`None` = sem suporte no RA).
pub fn console_for_platform(platform: &str) -> Option<i64> {
    match crate::commands::games::canonical_platform(platform)? {
        "Playstation 1" => Some(PS1),
        "Playstation 2" => Some(PS2),
        "PSP" => Some(PSP),
        "Nintendinho" => Some(NES),
        "Super Nintendo" => Some(SNES),
        "Game Boy Advance" => Some(GBA),
        "Game Boy" => Some(GB),
        "Game Boy Color" => Some(GBC),
        "Mega Drive" => Some(MEGA_DRIVE),
        "Nintendo 64" => Some(N64),
        _ => None,
    }
}

/// Arquivo de dados de um .cue (primeira linha FILE); outros formatos são usados como estão.
fn data_file(path: &Path) -> Result<PathBuf, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext != "cue" {
        return Ok(path.to_path_buf());
    }
    let txt = std::fs::read_to_string(path).map_err(|_| "Não foi possível ler o arquivo .cue.".to_string())?;
    let first = crate::detect::cue_files(&txt).into_iter().next().ok_or("O .cue não referencia nenhum arquivo de dados.")?;
    Ok(path.parent().unwrap_or(Path::new("")).join(first))
}

/// Calcula o hash do jogo para o console informado.
pub fn hash_file(console: i64, path: &Path) -> Result<String, String> {
    let path = data_file(path)?;
    let f = File::open(&path).map_err(|_| "Arquivo do jogo não encontrado ou sem permissão de leitura.".to_string())?;
    let mut r = BufReader::new(f);
    match console {
        PS1 => hash_psx(&mut r),
        PS2 => hash_ps2(&mut r),
        PSP => hash_psp(&mut r),
        NES | SNES | GBA | GB | GBC | MEGA_DRIVE => hash_cart(console, &mut r),
        N64 => hash_n64(&mut r),
        _ => Err("Console sem suporte a hash no NEXSTAGE.".into()),
    }
}

// ---------------------------------------------------------------- discos (ISO 9660)

struct Disc<'a, R: Read + Seek> {
    r: &'a mut R,
    l: Layout,
}

impl<'a, R: Read + Seek> Disc<'a, R> {
    fn open(r: &'a mut R) -> Result<Self, String> {
        let l = layout(r).ok_or("Imagem de disco ilegível.")?;
        let pvd = sector(r, &l, 16).ok_or("Imagem sem sistema de arquivos ISO 9660.")?;
        if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
            return Err("Imagem sem sistema de arquivos ISO 9660.".into());
        }
        Ok(Self { r, l })
    }

    /// Setor inicial e tamanho de um arquivo pelo caminho ("DIR\\ARQ.EXT"), sem diferenciar maiúsculas.
    fn find(&mut self, path: &str) -> Option<(u32, u32)> {
        let pvd = sector(self.r, &self.l, 16)?;
        let (mut lba, mut size) = (u32le(&pvd, 158), u32le(&pvd, 166));
        let parts: Vec<String> = path.split('\\').filter(|p| !p.is_empty()).map(|p| p.to_uppercase()).collect();
        for (i, part) in parts.iter().enumerate() {
            let entries = list_dir(self.r, &self.l, lba as u64, size as usize);
            let e = entries.into_iter().find(|e| &e.0 == part)?;
            if i + 1 == parts.len() {
                return Some((e.1, e.2));
            }
            lba = e.1;
            size = e.2;
        }
        None
    }

    fn read_sector(&mut self, lba: u32) -> Option<Vec<u8>> {
        sector(self.r, &self.l, lba as u64)
    }

    /// `rc_hash_cd_file`: acrescenta o nome (se houver) e `size` bytes a partir do setor.
    fn hash_file(&mut self, md5: &mut Md5, lba: u32, name: Option<&str>, size: u32) {
        if let Some(n) = name {
            md5.update(n.as_bytes());
        }
        let mut left = size as usize;
        let mut cur = lba;
        while left > 0 {
            let Some(buf) = self.read_sector(cur) else { break };
            let take = left.min(buf.len());
            md5.update(&buf[..take]);
            left -= take;
            cur += 1;
        }
    }
}

/// `rc_hash_find_playstation_executable`: lê SYSTEM.CNF e devolve (nome, setor, tamanho).
fn find_ps_exe<R: Read + Seek>(d: &mut Disc<R>, key: &str, prefix: &str) -> Option<(String, Option<(u32, u32)>)> {
    let (lba, size) = d.find("SYSTEM.CNF")?;
    let buf = d.read_sector(lba)?;
    let len = (size as usize).min(2047);
    let raw = &buf[..len];
    let raw = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
    let text = String::from_utf8_lossy(raw);
    let name = parse_boot_line(&text, key, prefix)?;
    let found = d.find(&name);
    Some((name, found))
}

/// Interpreta a linha `BOOT = cdrom:\SLUS_000.67;1` como no rcheevos (só no início de cada linha).
pub fn parse_boot_line(text: &str, key: &str, prefix: &str) -> Option<String> {
    for line in text.split('\n') {
        let Some(rest) = line.strip_prefix(key) else { continue };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let rest = rest.strip_prefix(prefix).unwrap_or(rest);
        let rest = rest.trim_start_matches('\\');
        let name: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != ';').collect();
        return if name.is_empty() { None } else { Some(name.chars().take(63).collect()) };
    }
    None
}

fn hash_psx<R: Read + Seek>(r: &mut R) -> Result<String, String> {
    let mut d = Disc::open(r)?;
    let mut exe = find_ps_exe(&mut d, "BOOT", "cdrom:").and_then(|(n, f)| f.map(|f| (n, f)));
    if exe.is_none() {
        exe = d.find("PSX.EXE").map(|f| ("PSX.EXE".to_string(), f));
    }
    let (name, (lba, mut size)) = exe.ok_or("Executável principal do PlayStation não encontrado no disco.")?;
    let head = d.read_sector(lba).ok_or("Não foi possível ler o executável do disco.")?;
    if &head[..7] == b"PS-X EX" {
        // o cabeçalho informa o tamanho do código sem ele mesmo (2048 bytes)
        size = u32le(&head, 28).saturating_add(2048);
    }
    let mut md5 = Md5::new();
    d.hash_file(&mut md5, lba, Some(&name), size);
    Ok(md5.hex())
}

fn hash_ps2<R: Read + Seek>(r: &mut R) -> Result<String, String> {
    let mut d = Disc::open(r)?;
    let (name, found) = find_ps_exe(&mut d, "BOOT2", "cdrom0:").ok_or("SYSTEM.CNF do PlayStation 2 não encontrado no disco.")?;
    let (lba, size) = found.ok_or("Executável principal do PlayStation 2 não encontrado no disco.")?;
    let mut md5 = Md5::new();
    d.hash_file(&mut md5, lba, Some(&name), size);
    Ok(md5.hex())
}

fn hash_psp<R: Read + Seek>(r: &mut R) -> Result<String, String> {
    let mut d = Disc::open(r)?;
    let (sfo, sfo_size) = d.find("PSP_GAME\\PARAM.SFO").ok_or("PSP_GAME\\PARAM.SFO não encontrado no disco.")?;
    let (eboot, eboot_size) = d.find("PSP_GAME\\SYSDIR\\EBOOT.BIN").ok_or("PSP_GAME\\SYSDIR\\EBOOT.BIN não encontrado no disco.")?;
    let mut md5 = Md5::new();
    d.hash_file(&mut md5, sfo, None, sfo_size);
    d.hash_file(&mut md5, eboot, None, eboot_size);
    Ok(md5.hex())
}

// ---------------------------------------------------------------- cartuchos

/// Nintendo 64: reordena para big-endian (.z64) pelo início do cabeçalho e calcula o md5.
fn hash_n64<R: Read + Seek>(r: &mut R) -> Result<String, String> {
    let size = r.seek(std::io::SeekFrom::End(0)).map_err(|_| "Arquivo ilegível.")?;
    if size < 0x40 || size > 128 * 1024 * 1024 {
        return Err("Tamanho de ROM inesperado para o console.".into());
    }
    r.seek(std::io::SeekFrom::Start(0)).map_err(|_| "Arquivo ilegível.")?;
    let mut data = Vec::with_capacity(size as usize);
    r.read_to_end(&mut data).map_err(|_| "Erro ao ler a ROM.")?;
    n64_to_big_endian(&mut data);
    let mut md5 = Md5::new();
    md5.update(&data);
    Ok(md5.hex())
}

/// .z64 (80 37 12 40) fica como está; .v64 (37 80 40 12) troca bytes em pares; .n64 (40 12 37 80) inverte palavras.
fn n64_to_big_endian(data: &mut [u8]) {
    match data.get(..4) {
        Some([0x37, 0x80, ..]) => data.chunks_exact_mut(2).for_each(|c| c.swap(0, 1)),
        Some([0x40, 0x12, ..]) => data.chunks_exact_mut(4).for_each(|c| c.reverse()),
        _ => {}
    }
}

fn hash_cart<R: Read + Seek>(console: i64, r: &mut R) -> Result<String, String> {
    let size = r.seek(std::io::SeekFrom::End(0)).map_err(|_| "Arquivo ilegível.")?;
    if size == 0 || size > 64 * 1024 * 1024 {
        return Err("Tamanho de ROM inesperado para o console.".into());
    }
    let skip = match console {
        NES => match read_at(r, 0, 4) {
            Some(h) if h == b"NES\x1a" => 16,
            _ => 0,
        },
        SNES if size % 8192 == 512 => 512,
        _ => 0,
    };
    r.seek(std::io::SeekFrom::Start(skip)).map_err(|_| "Arquivo ilegível.")?;
    let mut md5 = Md5::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf).map_err(|_| "Erro ao ler a ROM.")?;
        if n == 0 {
            break;
        }
        md5.update(&buf[..n]);
    }
    Ok(md5.hex())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// ISO 9660 mínimo: entradas do diretório raiz (nome, conteúdo) e subdiretórios simples.
    struct IsoBuilder {
        d: Vec<u8>,
        next: usize,
    }
    impl IsoBuilder {
        fn new() -> Self {
            let mut d = vec![0u8; 200 * 2048];
            let p = 16 * 2048;
            d[p] = 1;
            d[p + 1..p + 6].copy_from_slice(b"CD001");
            Self { d, next: 30 }
        }
        fn record(&mut self, dir_lba: usize, slot: &mut usize, name: &str, lba: usize, size: usize) {
            let pos = dir_lba * 2048 + *slot;
            let len = 33 + name.len() + (name.len() % 2 == 0) as usize;
            self.d[pos] = len as u8;
            self.d[pos + 2..pos + 6].copy_from_slice(&(lba as u32).to_le_bytes());
            self.d[pos + 10..pos + 14].copy_from_slice(&(size as u32).to_le_bytes());
            self.d[pos + 32] = name.len() as u8;
            self.d[pos + 33..pos + 33 + name.len()].copy_from_slice(name.as_bytes());
            *slot += len;
        }
        fn file(&mut self, content: &[u8]) -> usize {
            let lba = self.next;
            self.d[lba * 2048..lba * 2048 + content.len()].copy_from_slice(content);
            self.next += content.len().div_ceil(2048).max(1);
            lba
        }
        fn set_root(&mut self, lba: usize) {
            let p = 16 * 2048;
            self.d[p + 158..p + 162].copy_from_slice(&(lba as u32).to_le_bytes());
            self.d[p + 166..p + 170].copy_from_slice(&2048u32.to_le_bytes());
        }
    }

    #[test]
    fn parses_boot_lines_like_rcheevos() {
        let cnf = "BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.00\r\n";
        assert_eq!(parse_boot_line(cnf, "BOOT2", "cdrom0:").as_deref(), Some("SLUS_200.62"));
        // "BOOT" não pode casar com a linha BOOT2 (depois da chave precisa vir o "=")
        assert_eq!(parse_boot_line(cnf, "BOOT", "cdrom:"), None);
        let ps1 = "BOOT = cdrom:\\SLUS_000.67;1\nTCB = 4\n";
        assert_eq!(parse_boot_line(ps1, "BOOT", "cdrom:").as_deref(), Some("SLUS_000.67"));
        let sub = "BOOT=cdrom:\\GAME\\MAIN.EXE;1";
        assert_eq!(parse_boot_line(sub, "BOOT", "cdrom:").as_deref(), Some("GAME\\MAIN.EXE"));
    }

    #[test]
    fn ps2_hash_is_name_plus_executable() {
        let mut b = IsoBuilder::new();
        let elf: Vec<u8> = b"\x7fELF".iter().copied().chain((0..3000u32).map(|i| (i % 200) as u8)).collect();
        let cnf = b"BOOT2 = cdrom0:\\SLUS_123.45;1\nVER = 1.00\n";
        let cnf_lba = b.file(cnf);
        let elf_lba = b.file(&elf);
        let root = 20;
        let mut slot = 0;
        b.record(root, &mut slot, "SYSTEM.CNF;1", cnf_lba, cnf.len());
        b.record(root, &mut slot, "SLUS_123.45;1", elf_lba, elf.len());
        b.set_root(root);
        let got = hash_ps2(&mut Cursor::new(b.d)).unwrap();
        let mut expect = Md5::new();
        expect.update(b"SLUS_123.45");
        expect.update(&elf);
        assert_eq!(got, expect.hex());
    }

    #[test]
    fn psx_hash_uses_exe_header_size() {
        let mut b = IsoBuilder::new();
        let mut exe = vec![0u8; 2048 + 4096];
        exe[..8].copy_from_slice(b"PS-X EXE");
        exe[28..32].copy_from_slice(&2048u32.to_le_bytes()); // só 2048 bytes de código contam
        for (i, x) in exe.iter_mut().enumerate().skip(2048) {
            *x = (i % 97) as u8;
        }
        let cnf = b"BOOT = cdrom:\\SCUS_944.55;1\nTCB = 4\n";
        let cnf_lba = b.file(cnf);
        let exe_lba = b.file(&exe);
        let root = 20;
        let mut slot = 0;
        b.record(root, &mut slot, "SYSTEM.CNF;1", cnf_lba, cnf.len());
        b.record(root, &mut slot, "SCUS_944.55;1", exe_lba, exe.len());
        b.set_root(root);
        let got = hash_psx(&mut Cursor::new(b.d)).unwrap();
        let mut expect = Md5::new();
        expect.update(b"SCUS_944.55");
        expect.update(&exe[..4096]);
        assert_eq!(got, expect.hex());
    }

    #[test]
    fn psp_hash_walks_subdirectories() {
        let mut b = IsoBuilder::new();
        let sfo = b"\0PSF param data".to_vec();
        let eboot: Vec<u8> = (0..5000u32).map(|i| (i % 13) as u8).collect();
        let sfo_lba = b.file(&sfo);
        let eboot_lba = b.file(&eboot);
        let (root, game, sysdir) = (20, 21, 22);
        let mut s = 0;
        b.record(root, &mut s, "PSP_GAME", game, 2048);
        let mut s = 0;
        b.record(game, &mut s, "PARAM.SFO;1", sfo_lba, sfo.len());
        b.record(game, &mut s, "SYSDIR", sysdir, 2048);
        let mut s = 0;
        b.record(sysdir, &mut s, "EBOOT.BIN;1", eboot_lba, eboot.len());
        b.set_root(root);
        let got = hash_psp(&mut Cursor::new(b.d)).unwrap();
        let mut expect = Md5::new();
        expect.update(&sfo);
        expect.update(&eboot);
        assert_eq!(got, expect.hex());
    }

    #[test]
    fn cartridge_headers_are_skipped() {
        let mut nes = b"NES\x1a".to_vec();
        nes.extend_from_slice(&[0u8; 12]);
        nes.extend_from_slice(b"PRGDATA");
        assert_eq!(hash_cart(NES, &mut Cursor::new(nes)).unwrap(), super::super::md5::hex_of(b"PRGDATA"));
        let mut snes = vec![0xAAu8; 512];
        snes.extend(std::iter::repeat(7u8).take(8192));
        assert_eq!(hash_cart(SNES, &mut Cursor::new(snes)).unwrap(), super::super::md5::hex_of(&[7u8; 8192]));
        assert!(hash_psx(&mut Cursor::new(vec![0u8; 100])).is_err());
    }

    /// Teste manual com arquivos reais (somente leitura):
    /// RA_HASH_FILES="D:\a.iso;D:\b.cue" cargo test live_hash -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_hash_files() {
        let list = std::env::var("RA_HASH_FILES").unwrap_or_default();
        for p in list.split(';').filter(|s| !s.is_empty()) {
            let path = Path::new(p);
            let data = data_file(path).unwrap();
            let found = crate::detect::detect_disc(&mut BufReader::new(File::open(&data).unwrap()));
            let console = found.platform.and_then(console_for_platform);
            let t = std::time::Instant::now();
            let h = console.map(|c| hash_file(c, path));
            println!("{p}\n  plataforma: {:?} | console RA: {console:?} | hash: {h:?} | {:?}", found.platform, t.elapsed());
        }
    }

    #[test]
    fn platform_mapping() {
        assert_eq!(console_for_platform("Playstation 2"), Some(PS2));
        assert_eq!(console_for_platform("ps1"), Some(PS1));
        assert_eq!(console_for_platform("PSP"), Some(PSP));
        assert_eq!(console_for_platform("Playstation 3"), None);
        assert_eq!(console_for_platform("PC"), None);
    }
}

#[cfg(test)]
mod n64_tests {
    use super::*;

    #[test]
    fn n64_byte_orders_hash_the_same() {
        let z64: Vec<u8> = [0x80u8, 0x37, 0x12, 0x40, 1, 2, 3, 4, 5, 6, 7, 8].iter().copied().cycle().take(0x80).collect();
        let v64: Vec<u8> = z64.chunks(2).flat_map(|c| [c[1], c[0]]).collect();
        let n64: Vec<u8> = z64.chunks(4).flat_map(|c| [c[3], c[2], c[1], c[0]]).collect();
        let h = |d: Vec<u8>| hash_n64(&mut std::io::Cursor::new(d)).unwrap();
        let expected = h(z64.clone());
        assert_eq!(h(v64), expected);
        assert_eq!(h(n64), expected);
        assert_eq!(console_for_platform("N64"), Some(N64));
        assert_eq!(console_for_platform("Sega Genesis"), Some(MEGA_DRIVE));
    }
}
