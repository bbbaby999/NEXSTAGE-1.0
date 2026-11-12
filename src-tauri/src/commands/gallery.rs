//! Galeria de capturas de tela. Os arquivos importados são COPIADOS para
//! `<dados do app>/gallery/<perfil>/<sha256>.<ext>` (o original nunca é alterado nem apagado).
//! A mesma imagem (mesmo conteúdo) não é importada duas vezes. Excluir remove só a cópia.

use super::games::ensure_owned;
use crate::db::Db;
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use std::process::Command;
use tauri::{AppHandle, Manager, State};

pub(crate) fn gallery_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?
        .join("gallery"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: u32,
    pub duplicates: u32,
    pub failed: Vec<String>,
}

pub(crate) fn import_one(conn: &Connection, dir: &Path, profile_id: i64, game_id: Option<i64>, p: &str) -> AppResult<bool> {
    let src = Path::new(p);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp", "bmp", "gif"].contains(&ext.as_str()) {
        return Err(AppError::Validation("formato não suportado".into()));
    }
    let meta = fs::metadata(src)?;
    if meta.len() > 60_000_000 {
        return Err(AppError::Validation("arquivo maior que 60 MB".into()));
    }
    let bytes = fs::read(src)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM gallery_items WHERE profile_id=?1 AND hash=?2",
        params![profile_id, hash],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Ok(false);
    }
    let dest = dir.join(format!("{hash}.{ext}"));
    fs::write(&dest, &bytes)?;
    let secs = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let name = src.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let r = conn.execute(
        "INSERT INTO gallery_items(profile_id,game_id,path,hash,original_name,taken_at)
         VALUES(?1,?2,?3,?4,?5,CASE WHEN ?6 IS NULL THEN datetime('now') ELSE datetime(?6,'unixepoch') END)",
        params![profile_id, game_id, dest.to_string_lossy().to_string(), hash, name, secs],
    );
    if let Err(e) = r {
        let _ = fs::remove_file(&dest);
        return Err(e.into());
    }
    Ok(true)
}

#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn import_screenshots(
    app: AppHandle,
    db: State<Db>,
    profile_id: i64,
    game_id: Option<i64>,
    paths: Vec<String>,
) -> AppResult<ImportResult> {
    let dir = gallery_dir(&app)?.join(profile_id.to_string());
    fs::create_dir_all(&dir)?;
    let conn = db.conn()?;
    if let Some(g) = game_id {
        ensure_owned(&conn, profile_id, g)?;
    }
    let mut res = ImportResult { imported: 0, duplicates: 0, failed: vec![] };
    for p in paths {
        match import_one(&conn, &dir, profile_id, game_id, &p) {
            Ok(true) => res.imported += 1,
            Ok(false) => res.duplicates += 1,
            Err(e) => {
                let name = Path::new(&p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(p.clone());
                res.failed.push(format!("{name}: {e}"));
            }
        }
    }
    Ok(res)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItem {
    pub id: i64,
    pub game_id: Option<i64>,
    pub game_title: Option<String>,
    pub path: String,
    pub favorite: bool,
    pub taken_at: Option<String>,
    pub added_at: String,
    pub exists: bool,
    pub original_name: String,
}

#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn list_gallery(db: State<Db>, profile_id: i64) -> AppResult<Vec<GalleryItem>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT i.id,i.game_id,g.title,i.path,i.favorite,i.taken_at,i.added_at,i.original_name
         FROM gallery_items i LEFT JOIN games g ON g.id = i.game_id
         WHERE i.profile_id=?1 ORDER BY COALESCE(i.taken_at,i.added_at) DESC, i.id DESC",
    )?;
    let rows = stmt
        .query_map([profile_id], |r| {
            let path: String = r.get(3)?;
            Ok(GalleryItem {
                id: r.get(0)?,
                game_id: r.get(1)?,
                game_title: r.get(2)?,
                exists: Path::new(&path).is_file(),
                path,
                favorite: r.get::<_, i64>(4)? != 0,
                taken_at: r.get(5)?,
                added_at: r.get(6)?,
                original_name: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[tauri::command]
pub fn set_gallery_favorite(db: State<Db>, profile_id: i64, id: i64, favorite: bool) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "UPDATE gallery_items SET favorite=?1 WHERE id=?2 AND profile_id=?3",
        params![favorite as i64, id, profile_id],
    )?;
    Ok(())
}

/// Remove a cópia gerenciada (somente se estiver dentro da pasta do NEXSTAGE) e o registro.
#[tauri::command]
pub fn delete_gallery_item(app: AppHandle, db: State<Db>, profile_id: i64, id: i64) -> AppResult<()> {
    let base = gallery_dir(&app)?;
    let conn = db.conn()?;
    let path: String = conn
        .query_row(
            "SELECT path FROM gallery_items WHERE id=?1 AND profile_id=?2",
            params![id, profile_id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound("Captura não encontrada.".into()))?;
    remove_managed(&base, &path);
    conn.execute("DELETE FROM gallery_items WHERE id=?1 AND profile_id=?2", params![id, profile_id])?;
    Ok(())
}

fn remove_managed(base: &Path, path: &str) {
    // Só apaga se o arquivo estiver DENTRO da pasta gerenciada pelo NEXSTAGE.
    if let (Ok(b), Ok(p)) = (base.canonicalize(), Path::new(path).canonicalize()) {
        if p.starts_with(&b) {
            let _ = fs::remove_file(&p);
        }
    }
}

/// Exclui várias capturas do perfil: remove só as cópias gerenciadas e os registros.
/// Os arquivos originais (fora da pasta do NEXSTAGE) nunca são tocados. Retorna quantas foram removidas.
#[tauri::command(async)] // fora da thread da interface (E/S de disco ou consulta grande)
pub fn delete_gallery_items(app: AppHandle, db: State<Db>, profile_id: i64, ids: Vec<i64>) -> AppResult<u32> {
    let base = gallery_dir(&app)?;
    let conn = db.conn()?;
    let mut n = 0;
    for id in ids {
        let path: Option<String> = conn
            .query_row(
                "SELECT path FROM gallery_items WHERE id=?1 AND profile_id=?2",
                params![id, profile_id],
                |r| r.get(0),
            )
            .ok();
        let Some(path) = path else { continue };
        remove_managed(&base, &path);
        conn.execute("DELETE FROM gallery_items WHERE id=?1 AND profile_id=?2", params![id, profile_id])?;
        n += 1;
    }
    Ok(n)
}

/// Abre o Explorador de Arquivos na pasta da captura, com o arquivo selecionado.
/// Usa o caminho gravado no banco (cópia gerenciada), nunca um caminho informado pela interface.
#[tauri::command]
pub fn reveal_capture(db: State<Db>, profile_id: i64, id: i64) -> AppResult<()> {
    let path: String = {
        let conn = db.conn()?;
        conn.query_row(
            "SELECT path FROM gallery_items WHERE id=?1 AND profile_id=?2",
            params![id, profile_id],
            |r| r.get(0),
        )
        .map_err(|_| AppError::NotFound("Captura não encontrada.".into()))?
    };
    if !Path::new(&path).is_file() {
        return Err(AppError::NotFound(
            "O arquivo desta captura não existe mais no disco. Você pode excluir este registro.".into(),
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let p = path.replace('/', "\\");
        Command::new("explorer")
            .raw_arg(format!("/select,\"{p}\""))
            .spawn()
            .map_err(|e| AppError::Other(format!("Não foi possível abrir o Explorador de Arquivos: {e}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("true");
        Err(AppError::Other("Disponível apenas no Windows.".into()))
    }
}
