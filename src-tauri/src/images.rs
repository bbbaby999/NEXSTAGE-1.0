//! Imagens grandes (capas e fundos dos jogos, capas dos emuladores) ficam como ARQUIVOS em
//! `<app_data>/images/<sha256>.<ext>`, e não mais como data URL dentro do banco.
//!
//! - No banco fica só a referência `nximg:<arquivo>` (relativa: sobrevive se a pasta de dados mudar).
//! - Para a interface, a referência vira o caminho absoluto do arquivo, que o frontend converte em URL
//!   do protocolo de arquivos do Tauri (`convertFileSrc`, escopo `$APPDATA/**`).
//! - Na gravação, aceita: data URL (imagem nova escolhida no formulário), o caminho/URL de uma imagem
//!   que já está na pasta (formulário salvo sem trocar a imagem), `nximg:` ou `https://` (mantidos).
//! - Nome = hash do conteúdo: a mesma imagem em vários jogos ocupa espaço uma vez só.
//! - Compatibilidade: valores antigos (data URL) continuam funcionando e são migrados na inicialização.

use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const PREFIX: &str = "nximg:";
const MAX_BYTES: usize = 15 * 1024 * 1024;
/// Colunas que guardam imagens em arquivo.
const COLUMNS: [(&str, &str); 7] = [
    ("games", "cover"), ("games", "background"), ("games", "logo"), ("emulators", "cover"),
    // página Zerei (jogos acompanhados, independentes da biblioteca)
    ("zerei_entries", "cover"), ("zerei_entries", "logo"), ("zerei_entries", "background"),
];
/// Colunas que podem faltar em bancos de teste/antigos (antes da migração): são puladas na limpeza (sem a
/// coluna, não há imagem nela). As demais continuam obrigatórias: erro interrompe sem apagar nada.
fn optional(table: &str, col: &str) -> bool {
    table == "zerei_entries" || (table, col) == ("games", "logo")
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Define a pasta das imagens (chamado uma vez na inicialização).
pub fn init(app_data: &Path) -> AppResult<()> {
    let dir = app_data.join("images");
    fs::create_dir_all(&dir)?;
    let _ = DIR.set(dir);
    Ok(())
}

fn dir() -> AppResult<&'static PathBuf> {
    DIR.get().ok_or_else(|| AppError::Other("Pasta de imagens não inicializada.".into()))
}

/// Nome de arquivo seguro gerado por nós: hex + extensão conhecida (sem barras, sem "..").
pub(crate) fn valid_name(n: &str) -> bool {
    let Some((stem, ext)) = n.split_once('.') else { return false };
    stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit()) && ext_ok(ext)
}

fn ext_ok(ext: &str) -> bool {
    matches!(ext, "jpg" | "png" | "webp" | "gif" | "bmp" | "avif")
}

fn ext_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        _ => return None,
    })
}

/// Decodifica `%XX` (URL do protocolo de arquivos → caminho).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Se `v` aponta para um arquivo DENTRO da pasta de imagens, devolve o nome do arquivo.
fn name_in_dir(v: &str) -> Option<String> {
    let path = if let Some(rest) = v.strip_prefix("http://asset.localhost/").or_else(|| v.strip_prefix("https://asset.localhost/")).or_else(|| v.strip_prefix("asset://localhost/")) {
        PathBuf::from(percent_decode(rest.split(['?', '#']).next().unwrap_or("")))
    } else {
        PathBuf::from(v)
    };
    let name = path.file_name()?.to_str()?.to_string();
    if !valid_name(&name) {
        return None;
    }
    let base = dir().ok()?.canonicalize().ok()?;
    let real = path.canonicalize().ok()?;
    (real.parent()? == base).then_some(name)
}

/// Grava os bytes (se ainda não existirem) e devolve a referência `nximg:`.
fn write_bytes(bytes: &[u8], ext: &str) -> AppResult<String> {
    let name = format!("{:x}.{ext}", Sha256::digest(bytes));
    let path = dir()?.join(&name);
    if !path.is_file() {
        // grava num temporário e renomeia: um arquivo nunca fica pela metade
        let tmp = dir()?.join(format!("{name}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
    }
    Ok(format!("{PREFIX}{name}"))
}

fn store_data_url(v: &str) -> AppResult<String> {
    let rest = &v["data:".len()..];
    let (meta, b64) = rest.split_once(',').ok_or_else(|| AppError::Validation("Imagem inválida.".into()))?;
    let (mime, enc) = meta.split_once(';').unwrap_or((meta, ""));
    if enc != "base64" {
        return Err(AppError::Validation("Imagem inválida.".into()));
    }
    let ext = ext_for_mime(&mime.to_lowercase()).ok_or_else(|| AppError::Validation("Formato de imagem não suportado.".into()))?;
    if b64.len() > MAX_BYTES / 3 * 4 + 4 {
        return Err(AppError::Validation("Imagem grande demais (máx. 15 MB).".into()));
    }
    let bytes = STANDARD.decode(b64.trim()).map_err(|_| AppError::Validation("Imagem inválida.".into()))?;
    write_bytes(&bytes, ext)
}

/// Normaliza o valor vindo da interface para gravar no banco.
pub fn store(value: Option<String>) -> AppResult<Option<String>> {
    let Some(v) = value.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else { return Ok(None) };
    if v.starts_with("data:") {
        return store_data_url(&v).map(Some);
    }
    if let Some(n) = v.strip_prefix(PREFIX) {
        return if valid_name(n) { Ok(Some(v)) } else { Err(AppError::Validation("Imagem inválida.".into())) };
    }
    if let Some(name) = name_in_dir(&v) {
        return Ok(Some(format!("{PREFIX}{name}")));
    }
    if v.starts_with("https://") {
        return Ok(Some(v)); // endereço da internet: mantido como antes
    }
    Err(AppError::Validation("Imagem inválida ou fora da pasta do NEXSTAGE.".into()))
}

/// Converte o valor do banco para a interface: `nximg:` → caminho absoluto; o resto, sem mudança.
pub fn resolve(value: Option<String>) -> Option<String> {
    match value {
        Some(v) if v.starts_with(PREFIX) => {
            let name = &v[PREFIX.len()..];
            match dir() {
                Ok(d) if valid_name(name) => Some(d.join(name).to_string_lossy().into_owned()),
                _ => None,
            }
        }
        other => other,
    }
}

/// Grava como arquivo uma imagem já em bytes (ex.: capa baixada da Steam).
pub fn store_bytes(bytes: &[u8], mime: &str) -> AppResult<String> {
    let ext = ext_for_mime(mime).ok_or_else(|| AppError::Validation("Formato de imagem não suportado.".into()))?;
    write_bytes(bytes, ext)
}

/// Copia para a pasta de imagens um arquivo de imagem do disco (ex.: captura da Galeria).
pub fn store_file(path: &Path) -> AppResult<String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let ext = if ext == "jpeg" { "jpg".to_string() } else { ext };
    if !ext_ok(&ext) {
        return Err(AppError::Validation("Formato de imagem não suportado.".into()));
    }
    if fs::metadata(path)?.len() as usize > MAX_BYTES {
        return Err(AppError::Validation("Imagem grande demais (máx. 15 MB).".into()));
    }
    write_bytes(&fs::read(path)?, &ext)
}

/// Inicialização: move para arquivos as imagens ainda guardadas como data URL e apaga arquivos que
/// nenhum registro usa mais. Falhas por imagem são ignoradas (o valor antigo continua valendo).
/// Devolve quantas imagens foram migradas.
pub fn migrate_and_clean(conn: &mut Connection) -> AppResult<usize> {
    let mut moved = 0;
    let tx = conn.transaction()?;
    for (table, col) in COLUMNS {
        let rows: Vec<(i64, String)> = {
            let st = tx.prepare(&format!("SELECT id,{col} FROM {table} WHERE {col} LIKE 'data:%'"));
            // só a tabela do Zerei! pode faltar (banco de teste); nas demais, erro interrompe como antes
            let mut st = match st {
                Err(_) if optional(table, col) => continue,
                r => r?,
            };
            let r = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            r
        };
        for (id, v) in rows {
            if let Ok(r) = store_data_url(&v) {
                tx.execute(&format!("UPDATE {table} SET {col}=?1 WHERE id=?2"), params![r, id])?;
                moved += 1;
            }
        }
    }
    // Arquivos em uso (lidos dentro da transação, já com as migrações acima).
    let mut used: HashSet<String> = HashSet::new();
    for (table, col) in COLUMNS {
        // Falha ao ler uma tabela da biblioteca/emuladores interrompe a limpeza (nada é apagado), como antes.
        let mut st = match tx.prepare(&format!("SELECT {col} FROM {table} WHERE {col} LIKE 'nximg:%'")) {
            Err(_) if optional(table, col) => continue,
            r => r?,
        };
        for v in st.query_map([], |r| r.get::<_, String>(0))? {
            if let Some(n) = v?.strip_prefix(PREFIX) {
                used.insert(n.to_string());
            }
        }
    }
    // Imagens guardadas em configurações por perfil (ex.: fundo próprio da Home). Tolerante: bancos de
    // teste sem a tabela seguem normalmente.
    if let Ok(mut st) = tx.prepare("SELECT value FROM profile_settings WHERE key=?1 AND value LIKE 'nximg:%'") {
        for key in crate::commands::settings::HOME_BG_KEYS {
            for v in st.query_map([key], |r| r.get::<_, String>(0))?.flatten() {
                if let Some(n) = v.strip_prefix(PREFIX) {
                    used.insert(n.to_string());
                }
            }
        }
    }
    tx.commit()?;
    if let Ok(rd) = fs::read_dir(dir()?) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            // só arquivos nossos (nome = hash) ou temporários que sobraram de uma gravação interrompida
            if (valid_name(&n) && !used.contains(&n)) || n.ends_with(".tmp") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A pasta de imagens é global e a limpeza apaga arquivos sem uso: os testes rodam um por vez.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn setup() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexstage-img-test-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        init(&d).unwrap();
        d
    }

    // PNG 1x1 válido
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    #[test]
    fn stores_resolves_and_reuses() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        setup();
        let r = store(Some(PNG.into())).unwrap().unwrap();
        assert!(r.starts_with(PREFIX) && r.ends_with(".png"));
        let path = resolve(Some(r.clone())).unwrap();
        assert!(Path::new(&path).is_file());
        // formulário salvo sem trocar a imagem: caminho ou URL do protocolo voltam para a mesma referência
        assert_eq!(store(Some(path.clone())).unwrap().unwrap(), r);
        let url = format!("http://asset.localhost/{}", path.replace(':', "%3A").replace('\\', "%5C"));
        assert_eq!(store(Some(url)).unwrap().unwrap(), r);
        // mesma imagem de novo: mesmo arquivo
        assert_eq!(store(Some(PNG.into())).unwrap().unwrap(), r);
    }

    #[test]
    fn keeps_legacy_and_rejects_outside_files() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        setup();
        assert_eq!(store(None).unwrap(), None);
        assert_eq!(store(Some("  ".into())).unwrap(), None);
        assert_eq!(resolve(Some("data:image/png;base64,AAA".into())).unwrap(), "data:image/png;base64,AAA");
        assert_eq!(store(Some("https://cdn.cloudflare.steamstatic.com/a.jpg".into())).unwrap().unwrap(), "https://cdn.cloudflare.steamstatic.com/a.jpg");
        assert!(store(Some(r"C:\Windows\win.ini".into())).is_err());
        assert!(store(Some("nximg:../../x.png".into())).is_err());
        assert!(store(Some("data:image/svg+xml;base64,PHN2Zz4=".into())).is_err());
    }

    /// Migração sobre uma CÓPIA de um banco real (nunca o original):
    /// `NEXSTAGE_DB_COPY=<copia.db> cargo test --lib images::tests::real_db_copy -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_db_copy() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = PathBuf::from(std::env::var("NEXSTAGE_DB_COPY").expect("defina NEXSTAGE_DB_COPY"));
        let d = path.parent().unwrap().join("img-migration-test");
        fs::create_dir_all(&d).unwrap();
        let _ = DIR.set(d.join("images"));
        init(&d).unwrap();
        let mut c = Connection::open(&path).unwrap();
        let count = |c: &Connection, pat: &str| -> i64 {
            COLUMNS.iter().map(|(t, col)| c.query_row(&format!("SELECT COUNT(*) FROM {t} WHERE {col} LIKE '{pat}'"), [], |r| r.get::<_, i64>(0)).unwrap()).sum()
        };
        let before = count(&c, "data:%");
        let moved = migrate_and_clean(&mut c).unwrap();
        c.execute_batch("VACUUM;").unwrap();
        let (after, refs) = (count(&c, "data:%"), count(&c, "nximg:%"));
        for (t, col) in COLUMNS {
            let mut st = c.prepare(&format!("SELECT {col} FROM {t} WHERE {col} LIKE 'nximg:%'")).unwrap();
            for v in st.query_map([], |r| r.get::<_, String>(0)).unwrap() {
                assert!(Path::new(&resolve(Some(v.unwrap())).unwrap()).is_file(), "toda referência aponta para um arquivo");
            }
        }
        let files = fs::read_dir(dir().unwrap()).unwrap().count();
        println!("data URL antes: {before} | migradas: {moved} | data URL depois: {after} | referências: {refs} | arquivos: {files}");
        assert_eq!(after, 0);
    }

    #[test]
    fn keeps_home_background_from_profile_settings() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        setup();
        let r = store(Some(PNG.into())).unwrap().unwrap();
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE games(id INTEGER PRIMARY KEY, cover TEXT, background TEXT);
             CREATE TABLE emulators(id INTEGER PRIMARY KEY, cover TEXT);
             CREATE TABLE profile_settings(profile_id INTEGER, key TEXT, value TEXT);",
        )
        .unwrap();
        // a captura usada no modo "Escolher da Galeria" é outra imagem (outro arquivo)
        let jpg = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==";
        let r2 = store(Some(jpg.into())).unwrap().unwrap();
        c.execute("INSERT INTO profile_settings VALUES(1,'home_bg_image',?1)", [&r]).unwrap();
        c.execute("INSERT INTO profile_settings VALUES(1,'home_bg_capture',?1)", [&r2]).unwrap();
        migrate_and_clean(&mut c).unwrap();
        assert!(Path::new(&resolve(Some(r)).unwrap()).is_file(), "fundo da Home em uso não é apagado");
        assert!(Path::new(&resolve(Some(r2)).unwrap()).is_file(), "fundo da Galeria em uso não é apagado");
    }

    #[test]
    fn migrates_data_urls_and_cleans_orphans() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = setup();
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE games(id INTEGER PRIMARY KEY, cover TEXT, background TEXT);
             CREATE TABLE emulators(id INTEGER PRIMARY KEY, cover TEXT);",
        )
        .unwrap();
        c.execute("INSERT INTO games(id,cover,background) VALUES(1,?1,NULL)", [PNG]).unwrap();
        c.execute("INSERT INTO emulators(id,cover) VALUES(1,?1)", [PNG]).unwrap();
        let orphan = d.join("images").join(format!("{}.jpg", "a".repeat(64)));
        fs::write(&orphan, b"x").unwrap();
        let foreign = d.join("images").join("leia-me.txt");
        fs::write(&foreign, b"x").unwrap();

        assert_eq!(migrate_and_clean(&mut c).unwrap(), 2);
        let g: String = c.query_row("SELECT cover FROM games", [], |r| r.get(0)).unwrap();
        let e: String = c.query_row("SELECT cover FROM emulators", [], |r| r.get(0)).unwrap();
        assert!(g.starts_with(PREFIX) && g == e);
        assert!(Path::new(&resolve(Some(g)).unwrap()).is_file());
        assert!(!orphan.exists(), "arquivo sem uso é apagado");
        assert!(foreign.exists(), "arquivo que não é nosso nunca é apagado");
    }
}
