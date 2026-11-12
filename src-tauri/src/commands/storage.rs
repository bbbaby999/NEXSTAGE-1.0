//! Unidades de armazenamento do Windows. Só valores medidos: espaço total/livre vêm do sistema;
//! a divisão por categoria inclui apenas o que o NEXSTAGE consegue medir com exatidão (seus
//! próprios dados e a pasta de capturas). O restante é "Outros / não identificado".

use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::{fs, path::Path};
use sysinfo::{DiskKind, Disks};
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub name: String,
    pub mount: String,
    pub kind: String,
    pub file_system: String,
    pub removable: bool,
    pub accessible: bool,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub nexstage_bytes: u64,
    pub screenshots_bytes: u64,
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else { return 0 };
    let mut total = 0;
    for e in rd.flatten() {
        let path = e.path();
        let Ok(m) = path.symlink_metadata() else { continue };
        if m.is_dir() {
            total += dir_size(&path);
        } else if m.is_file() {
            total += m.len();
        }
    }
    total
}

#[tauri::command]
pub async fn get_storage(app: AppHandle) -> AppResult<Vec<Drive>> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    tauri::async_runtime::spawn_blocking(move || {
        let disks = Disks::new_with_refreshed_list();
        let gallery = dir_size(&data.join("gallery"));
        let nexstage = dir_size(&data).saturating_sub(gallery);
        let data_s = data.to_string_lossy().to_lowercase();

        // Unidade que guarda os dados do NEXSTAGE: ponto de montagem mais longo que prefixa o caminho.
        let owner = disks
            .list()
            .iter()
            .map(|d| d.mount_point().to_string_lossy().to_lowercase())
            .filter(|m| data_s.starts_with(m.as_str()))
            .max_by_key(|m| m.len());

        disks
            .list()
            .iter()
            .map(|d| {
                let mount = d.mount_point().to_string_lossy().to_string();
                let is_owner = owner.as_deref() == Some(mount.to_lowercase().as_str());
                let total = d.total_space();
                let free = d.available_space();
                Drive {
                    name: d.name().to_string_lossy().to_string(),
                    mount,
                    kind: match d.kind() {
                        DiskKind::SSD => "SSD".into(),
                        DiskKind::HDD => "HDD".into(),
                        _ => String::new(),
                    },
                    file_system: d.file_system().to_string_lossy().to_string(),
                    removable: d.is_removable(),
                    accessible: total > 0,
                    total_bytes: total,
                    free_bytes: free,
                    used_bytes: total.saturating_sub(free),
                    nexstage_bytes: if is_owner { nexstage } else { 0 },
                    screenshots_bytes: if is_owner { gallery } else { 0 },
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))
}
