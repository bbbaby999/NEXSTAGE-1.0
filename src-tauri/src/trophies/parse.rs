//! Leitura (somente leitura) dos arquivos de troféus gravados pelos emuladores.
//!
//! - Lista de troféus: XML `TROP*.SFM` / `TROPCONF.SFM` (igual no PS3 e no Vita), com nome, descrição,
//!   tipo (P/G/S/B) e se é oculto.
//! - Progresso: `TROPUSR.DAT`, em formatos diferentes:
//!   - Vita3K: formato próprio do emulador (vita3k/np/src/trophy/context.cpp), little-endian, 1648 bytes:
//!     magia 0x12D5819A, bits de desbloqueio u32[4], visibilidade u32[4], grupos, total, platina,
//!     total por grupo u32[16], data de cada troféu u64[128] (segundos Unix) e tipos i32[128].
//!   - RPCS3: formato do PS3, big-endian: cabeçalho (magia 0x818F54AD) com tabelas; a tabela tipo 6
//!     traz, por troféu, o estado (1 = obtido) e a data em microssegundos desde 01/01/0001.

/// Um troféu da lista (o XML não diz se foi obtido).
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub id: u32,
    pub hidden: bool,
    /// "P" platina, "G" ouro, "S" prata, "B" bronze.
    pub grade: String,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conf {
    pub comm_id: String,
    pub title: String,
    pub trophies: Vec<Def>,
}

/// Troféus obtidos: id → data (segundos Unix; `None` quando o arquivo não guarda a data).
pub type Unlocks = std::collections::BTreeMap<u32, Option<i64>>;

fn unescape(s: &str) -> String {
    let s = s.trim();
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Texto do primeiro `<tag>…</tag>` dentro de `s`.
fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some(&s[a..b])
}

/// Valor de um atributo `name="…"` dentro da tag de abertura `head`.
fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let a = head.find(&key)? + key.len();
    let b = head[a..].find('"')? + a;
    Some(&head[a..b])
}

/// Lê um `.SFM` (XML da lista de troféus).
pub fn conf(xml: &str) -> Conf {
    let mut c = Conf {
        comm_id: tag(xml, "npcommid").map(unescape).unwrap_or_default(),
        title: tag(xml, "title-name").map(unescape).unwrap_or_default(),
        trophies: Vec::new(),
    };
    let mut rest = xml;
    while let Some(i) = rest.find("<trophy ") {
        let t = &rest[i..];
        let Some(he) = t.find('>') else { break };
        let head = &t[..he];
        // tag que se fecha sozinha (TROPCONF do Vita: sem nome/descrição)
        let body_end = if head.ends_with('/') { he } else { t.find("</trophy>").unwrap_or(he) };
        let body = &t[he..body_end.max(he)];
        if let Some(id) = attr(head, "id").and_then(|v| v.trim().parse::<u32>().ok()) {
            c.trophies.push(Def {
                id,
                hidden: attr(head, "hidden").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
                grade: attr(head, "ttype").unwrap_or("B").trim().to_uppercase(),
                name: tag(body, "name").map(unescape).unwrap_or_default(),
                detail: tag(body, "detail").map(unescape).unwrap_or_default(),
            });
        }
        rest = &t[he + 1..];
    }
    c
}

fn u32le(b: &[u8], o: usize) -> Option<u32> { b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())) }
fn u64le(b: &[u8], o: usize) -> Option<u64> { b.get(o..o + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap())) }
fn u32be(b: &[u8], o: usize) -> Option<u32> { b.get(o..o + 4).map(|x| u32::from_be_bytes(x.try_into().unwrap())) }
fn u64be(b: &[u8], o: usize) -> Option<u64> { b.get(o..o + 8).map(|x| u64::from_be_bytes(x.try_into().unwrap())) }

const VITA_MAGIC: u32 = 0x12D5_819A;
const VITA_MAX: usize = 128;
const VITA_TS: usize = 4 + 16 + 16 + 4 + 4 + 4 + 16 * 4; // 112

/// Progresso do Vita3K.
pub fn vita_usr(b: &[u8]) -> Option<Unlocks> {
    if u32le(b, 0)? != VITA_MAGIC {
        return None;
    }
    let mut out = Unlocks::new();
    for id in 0..VITA_MAX {
        let word = u32le(b, 4 + (id >> 5) * 4)?;
        if word & (1 << (id & 31)) != 0 {
            let ts = u64le(b, VITA_TS + id * 8).unwrap_or(0);
            out.insert(id as u32, (ts > 0 && ts < i64::MAX as u64).then_some(ts as i64));
        }
    }
    Some(out)
}

const PS3_MAGIC: u32 = 0x818F_54AD;
/// Microssegundos entre 01/01/0001 e 01/01/1970 (CellRtcTick).
const PS3_EPOCH_US: u64 = 62_135_596_800_000_000;

/// Progresso do RPCS3 (formato do PS3).
pub fn ps3_usr(b: &[u8]) -> Option<Unlocks> {
    if u32be(b, 0)? != PS3_MAGIC {
        return None;
    }
    let tables = u32be(b, 8)? as usize;
    let mut out = Unlocks::new();
    for t in 0..tables.min(16) {
        let h = 0x30 + t * 0x20;
        if u32be(b, h)? != 6 {
            continue;
        }
        let size = u32be(b, h + 4)? as usize;
        let count = u32be(b, h + 12)? as usize;
        let off = u64be(b, h + 16)? as usize;
        for i in 0..count.min(1024) {
            let o = off + i * (size + 0x10);
            let Some(state) = u32be(b, o + 0x14) else { break };
            if state != 0 {
                let id = u32be(b, o + 0x10)?;
                let tick = u64be(b, o + 0x20).unwrap_or(0);
                let ts = tick.checked_sub(PS3_EPOCH_US).map(|us| (us / 1_000_000) as i64).filter(|&s| s > 0);
                out.insert(id, ts);
            }
        }
    }
    Some(out)
}

/// Código de idioma dos arquivos `TROP_xx.SFM` (o mesmo no PS3 e no Vita) para o idioma do NEXSTAGE.
pub fn lang_code(lang: &str) -> &'static [u8] {
    match lang {
        "pt-BR" => &[17, 7],
        "en" => &[1, 18],
        "fr" => &[2],
        "es" => &[3],
        "de" => &[4],
        "it" => &[5],
        "ru" => &[8],
        "ko" => &[9],
        "zh-CN" => &[11, 10],
        "ja" => &[0],
        _ => &[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sfm_with_entities_and_self_closing() {
        let xml = r#"<!--sig--><trophyconf version="1.1"><npcommid>NPWR00001_00</npcommid>
 <title-name>Ratchet &amp; Clank</title-name>
 <trophy id="000" hidden="no" ttype="P" pid="-1"><name>Tudo</name><detail>Obtenha &quot;todos&quot;</detail></trophy>
 <trophy id="001" hidden="yes" ttype="G" pid="000">
  <name>Segredo</name>
  <detail>Ache</detail>
 </trophy>
 <trophy id="002" hidden="no" ttype="B" pid="000"/>
</trophyconf>"#;
        let c = conf(xml);
        assert_eq!(c.comm_id, "NPWR00001_00");
        assert_eq!(c.title, "Ratchet & Clank");
        assert_eq!(c.trophies.len(), 3);
        assert_eq!(c.trophies[0], Def { id: 0, hidden: false, grade: "P".into(), name: "Tudo".into(), detail: "Obtenha \"todos\"".into() });
        assert!(c.trophies[1].hidden);
        assert_eq!(c.trophies[1].grade, "G");
        assert_eq!(c.trophies[2].name, "");
    }

    #[test]
    fn reads_vita3k_progress() {
        let mut b = vec![0u8; 1648];
        b[0..4].copy_from_slice(&VITA_MAGIC.to_le_bytes());
        b[4..8].copy_from_slice(&0b1010u32.to_le_bytes()); // troféus 1 e 3
        b[8..12].copy_from_slice(&1u32.to_le_bytes()); // troféu 32
        b[VITA_TS + 8..VITA_TS + 16].copy_from_slice(&1_794_610_703u64.to_le_bytes());
        let u = vita_usr(&b).unwrap();
        assert_eq!(u.keys().copied().collect::<Vec<_>>(), vec![1, 3, 32]);
        assert_eq!(u[&1], Some(1_794_610_703));
        assert_eq!(u[&3], None);
        assert!(vita_usr(&[0u8; 8]).is_none());
    }

    #[test]
    fn reads_rpcs3_progress() {
        // cabeçalho + 1 tabela tipo 6 com 2 troféus (o segundo obtido)
        let mut b = vec![0u8; 0x50 + 2 * 0x70];
        b[0..4].copy_from_slice(&PS3_MAGIC.to_be_bytes());
        b[8..12].copy_from_slice(&1u32.to_be_bytes());
        let h = 0x30;
        b[h..h + 4].copy_from_slice(&6u32.to_be_bytes());
        b[h + 4..h + 8].copy_from_slice(&0x60u32.to_be_bytes());
        b[h + 12..h + 16].copy_from_slice(&2u32.to_be_bytes());
        b[h + 16..h + 24].copy_from_slice(&0x50u64.to_be_bytes());
        let o = 0x50 + 0x70;
        b[o + 0x10..o + 0x14].copy_from_slice(&1u32.to_be_bytes());
        b[o + 0x14..o + 0x18].copy_from_slice(&1u32.to_be_bytes());
        b[o + 0x20..o + 0x28].copy_from_slice(&63_928_140_501_380_791u64.to_be_bytes());
        let u = ps3_usr(&b).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[&1], Some(1_792_543_701)); // 2026-10-21T00:48:21Z
    }
}
