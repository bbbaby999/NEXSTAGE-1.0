//! Navegador de arquivos do NEXSTAGE (substitui o seletor do Windows, com suporte a controle).
//!
//! Só LEITURA: lista unidades, pastas especiais e o conteúdo de pastas; gera miniaturas de imagens e lê
//! uma imagem escolhida. Nada é criado, renomeado, movido ou apagado aqui — o caminho escolhido segue
//! para os comandos de sempre (adicionar jogo, importar capturas, backup...), que fazem as próprias
//! validações.

use crate::error::{AppError, AppResult};
use base64::Engine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Manager};

/// Pastas com mais itens que isso são cortadas (a interface avisa e sugere filtrar/digitar o nome).
const MAX_ENTRIES: usize = 4000;
/// Miniatura: arquivos maiores que isso não são decodificados (mostram só o ícone).
const MAX_THUMB_SOURCE: u64 = 60 * 1024 * 1024;
/// Imagem escolhida (lida inteira para o editor/formulário).
const MAX_IMAGE: u64 = 40 * 1024 * 1024;
const IMAGE_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "avif"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// desktop | documents | downloads | pictures | videos | music | home
    pub id: String,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub mount: String,
    /// Rótulo do volume ("Jogos", "Windows"...), pode vir vazio.
    pub label: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Places {
    pub places: Vec<Place>,
    pub drives: Vec<DriveInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
    pub size: u64,
    /// Última modificação (segundos Unix), 0 se o sistema não informar.
    pub modified: u64,
    /// Extensão em minúsculas, sem ponto ("" para pastas/sem extensão).
    pub ext: String,
    /// Veio de um atalho (.lnk): `path` já é o destino (pasta ou arquivo original).
    pub link: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    /// Pasta acima (None na raiz de uma unidade).
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
    /// Itens que passaram do limite e não foram enviados.
    pub truncated: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInfo {
    pub exists: bool,
    pub dir: bool,
}

#[tauri::command]
pub async fn fs_places(app: AppHandle) -> AppResult<Places> {
    let r = app.path();
    let mut places = Vec::new();
    let mut add = |id: &str, p: Option<PathBuf>| {
        if let Some(p) = p.filter(|p| p.is_dir()) {
            places.push(Place { id: id.into(), path: p.to_string_lossy().into_owned() });
        }
    };
    add("desktop", r.desktop_dir().ok());
    add("documents", r.document_dir().ok());
    add("downloads", r.download_dir().ok());
    add("pictures", r.picture_dir().ok());
    add("videos", r.video_dir().ok());
    add("music", r.audio_dir().ok());
    add("home", r.home_dir().ok());
    let drives = tauri::async_runtime::spawn_blocking(|| {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let mut out: Vec<DriveInfo> = Vec::new();
        for d in disks.list() {
            let mount = d.mount_point().to_string_lossy().into_owned();
            if out.iter().any(|o| o.mount.eq_ignore_ascii_case(&mount)) {
                continue;
            }
            out.push(DriveInfo {
                mount,
                label: d.name().to_string_lossy().trim().to_string(),
                removable: d.is_removable(),
                total_bytes: d.total_space(),
                free_bytes: d.available_space(),
            });
        }
        out.sort_by(|a, b| a.mount.to_lowercase().cmp(&b.mount.to_lowercase()));
        out
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(Places { places, drives })
}

/// Arquivo/pasta oculto ou de sistema (atributos do Windows) ou com nome começando por ".".
fn hidden(name: &str, meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        if meta.file_attributes() & (HIDDEN | SYSTEM) != 0 {
            return true;
        }
    }
    #[cfg(not(windows))]
    let _ = meta;
    name.starts_with('.') || name.eq_ignore_ascii_case("desktop.ini") || name.eq_ignore_ascii_case("thumbs.db")
}

/// Ordem "natural": "Jogo 2" antes de "Jogo 10", sem diferenciar maiúsculas.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) { na.push(c); ai.next(); }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) { nb.push(c); bi.next(); }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn list_dir(path: &Path, exts: &[String], dirs_only: bool, show_hidden: bool) -> AppResult<Listing> {
    let rd = std::fs::read_dir(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => AppError::Validation("Sem permissão para abrir esta pasta.".into()),
        std::io::ErrorKind::NotFound => AppError::NotFound("Pasta não encontrada.".into()),
        _ => AppError::Other(format!("Não foi possível abrir a pasta: {e}")),
    })?;
    let all = exts.is_empty() || exts.iter().any(|e| e == "*");
    let mut entries = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // metadata() segue atalhos simbólicos (junções de pasta aparecem como pastas)
        let Ok(meta) = e.metadata().or_else(|_| e.path().symlink_metadata()) else { continue };
        if !show_hidden && hidden(&name, &meta) {
            continue;
        }
        let mut dir = meta.is_dir();
        let mut ext = if dir { String::new() } else { Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default() };
        let mut path = e.path();
        let (mut name, mut meta, mut link) = (name, meta, false);
        // Atalho (.lnk): para pasta, vira a própria pasta (ex.: "Nova pasta" na Área de trabalho); para
        // arquivo, vale o arquivo original — a não ser que a tela peça atalhos (adicionar jogo resolve o
        // .lnk do jeito dela, inclusive jogos da Microsoft Store / Xbox).
        if ext == "lnk" {
            let wants_lnk = exts.iter().any(|x| x == "lnk");
            if let Some(t) = crate::lnk::read(&path) {
                if let Ok(tm) = std::fs::metadata(&t.path) {
                    if tm.is_dir() || !wants_lnk {
                        name = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name);
                        dir = tm.is_dir();
                        ext = if dir { String::new() } else { t.path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default() };
                        path = t.path;
                        meta = tm;
                        link = true;
                    }
                }
            }
        }
        if !dir && (dirs_only || !(all || exts.iter().any(|x| *x == ext))) {
            continue;
        }
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        entries.push(Entry { path: path.to_string_lossy().into_owned(), name, dir, size: if dir { 0 } else { meta.len() }, modified, ext, link });
    }
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural_cmp(&a.name, &b.name)));
    let truncated = entries.len().saturating_sub(MAX_ENTRIES);
    entries.truncate(MAX_ENTRIES);
    Ok(Listing {
        path: path.to_string_lossy().into_owned(),
        parent: path.parent().map(|p| p.to_string_lossy().into_owned()).filter(|p| !p.is_empty()),
        entries,
        truncated,
    })
}

/// Conteúdo de uma pasta: pastas primeiro, depois os arquivos com as extensões pedidas (`*` = todos).
#[tauri::command]
pub async fn fs_list(path: String, exts: Vec<String>, dirs_only: bool, show_hidden: bool) -> AppResult<Listing> {
    let exts: Vec<String> = exts.into_iter().map(|e| e.trim().trim_start_matches('.').to_lowercase()).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut p = PathBuf::from(path.trim());
        // "D:" sozinho é "a pasta atual de D:" no Windows; a raiz é "D:\"
        if p.as_os_str().len() == 2 && path.trim().ends_with(':') {
            p.push("\\");
        }
        list_dir(&p, &exts, dirs_only, show_hidden)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

#[tauri::command]
pub fn fs_path_info(path: String) -> PathInfo {
    let p = Path::new(path.trim());
    PathInfo { exists: p.exists(), dir: p.is_dir() }
}

fn image_ext(path: &Path) -> AppResult<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if IMAGE_EXTS.contains(&ext.as_str()) { Ok(ext) } else { Err(AppError::Validation("Formato de imagem não suportado.".into())) }
}

/// Miniatura (lado maior = `size`, 64..=512) como data URL: JPEG, ou PNG quando a imagem tem transparência.
#[tauri::command]
pub async fn fs_thumb(path: String, size: u32) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&path);
        image_ext(&p)?;
        if std::fs::metadata(&p)?.len() > MAX_THUMB_SOURCE {
            return Err(AppError::Validation("Imagem grande demais para a miniatura.".into()));
        }
        let img = image::ImageReader::open(&p)?
            .with_guessed_format()?
            .decode()
            .map_err(|e| AppError::Validation(format!("Imagem ilegível: {e}")))?;
        let s = size.clamp(64, 512);
        let th = img.thumbnail(s, s);
        let mut out = std::io::Cursor::new(Vec::new());
        let mime = if th.color().has_alpha() {
            th.write_to(&mut out, image::ImageFormat::Png).map_err(|e| AppError::Other(e.to_string()))?;
            "image/png"
        } else {
            image::DynamicImage::ImageRgb8(th.to_rgb8()).write_to(&mut out, image::ImageFormat::Jpeg).map_err(|e| AppError::Other(e.to_string()))?;
            "image/jpeg"
        };
        Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner())))
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

/// Imagem escolhida, inteira, como data URL (formulários de capa/fundo e o editor de enquadramento).
#[tauri::command]
pub async fn fs_read_image(path: String) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = PathBuf::from(&path);
        let ext = image_ext(&p)?;
        if std::fs::metadata(&p)?.len() > MAX_IMAGE {
            return Err(AppError::Validation("Imagem grande demais (máx. 40 MB).".into()));
        }
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "gif" => "image/gif",
            _ => "image/avif",
        };
        let bytes = std::fs::read(&p)?;
        Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["Jogo 10", "jogo 2", "Jogo 1", "abc", "Jogo 02b"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["abc", "Jogo 1", "jogo 2", "Jogo 02b", "Jogo 10"]);
    }

    #[test]
    fn lists_dirs_first_and_filters_extensions() {
        let d = std::env::temp_dir().join(format!("nexstage-fs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Pasta")).unwrap();
        std::fs::write(d.join("a.png"), b"x").unwrap();
        std::fs::write(d.join("b.txt"), b"x").unwrap();
        let l = list_dir(&d, &[String::from("png")], false, false).unwrap();
        let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Pasta", "a.png"]);
        let only = list_dir(&d, &[], true, false).unwrap();
        assert_eq!(only.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
