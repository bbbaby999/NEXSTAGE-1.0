//! Feedback e relatórios (Configurações → Atualizações).
//!
//! O app envia o relato para um serviço intermediário do desenvolvedor (Cloudflare Worker, código em
//! `feedback-worker/`), que cria a Issue no GitHub. O token do GitHub fica SÓ no serviço: o app
//! distribuído não contém credencial nenhuma, e quem envia não precisa de conta no GitHub.
//! O endereço do serviço não é segredo e vem de `src-tauri/feedback.json` na compilação (build.rs).
//!
//! Anexos só existem se o usuário marcar a opção; ele vê o conteúdo exato antes de enviar, e o texto
//! passa pelo filtro de dados pessoais (`diaglog::sanitize`) ao gerar e de novo ao enviar.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const ENDPOINT: &str = env!("NX_FEEDBACK_URL");
const LOG_MAX: usize = 48 * 1024;
const TEXT_MAX: usize = 8000;
const CATEGORIES: [&str; 4] = ["improvement", "feature", "bug", "general"];
const ATTACHMENT_NAMES: [&str; 2] = ["diagnostico.txt", "nexstage.log"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub description: String,
    pub content: String,
    pub bytes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInfo {
    pub configured: bool,
    pub version: String,
    pub log_dir: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentAttachment {
    pub name: String,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackInput {
    pub category: String,
    pub title: String,
    pub comments: String,
    pub problem: String,
    /// Data e hora locais do envio (texto já formatado pela interface, com fuso).
    pub sent_at: String,
    pub language: String,
    pub attachments: Vec<SentAttachment>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackResult {
    /// Número do relato (Issue), quando o serviço informa.
    pub id: Option<u64>,
}

fn data_dir(app: &AppHandle) -> AppResult<std::path::PathBuf> {
    app.path().app_data_dir().map_err(|e| AppError::Other(e.to_string()))
}

#[tauri::command]
pub fn feedback_info(app: AppHandle) -> AppResult<FeedbackInfo> {
    let d = crate::diaglog::dir(&data_dir(&app)?);
    Ok(FeedbackInfo { configured: !ENDPOINT.trim().is_empty(), version: env!("NX_VERSION").into(), log_dir: crate::diaglog::sanitize(&d.to_string_lossy()) })
}

/// Monta os anexos para o usuário revisar (nada é enviado aqui).
#[tauri::command]
pub async fn feedback_attachments(app: AppHandle) -> AppResult<Vec<Attachment>> {
    let data = data_dir(&app)?;
    let controllers = crate::commands::controllers::get_controllers().await;
    let diag = diagnostics(&app, &controllers);
    let mut out = vec![att(
        "diagnostico.txt",
        "Versão do NEXSTAGE, versão do Windows, idioma, quantidade de jogos/perfis/emuladores, integrações ativas (só sim/não), modelos de controle conectados e situação do backup automático.",
        diag,
    )];
    if let Some(log) = crate::diaglog::read_recent(&data, LOG_MAX) {
        out.push(att("nexstage.log", "Registro de erros do NEXSTAGE (últimos 48 KB): abertura do app, erros exibidos na tela e falhas internas.", log));
    }
    Ok(out)
}

fn att(name: &str, description: &str, content: String) -> Attachment {
    let content = crate::diaglog::sanitize(&content);
    Attachment { name: name.into(), description: description.into(), bytes: content.len(), content }
}

fn diagnostics(app: &AppHandle, controllers: &[crate::commands::controllers::Controller]) -> String {
    let mut s = String::new();
    let mut line = |k: &str, v: String| s.push_str(&format!("{k}: {v}\n"));
    line("NEXSTAGE", env!("NX_VERSION").into());
    line("Sistema", windows_version());
    line("Arquitetura", std::env::consts::ARCH.into());
    let db = app.state::<Db>();
    if let Ok(c) = db.conn() {
        let n = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(-1);
        let set = |k: &str| c.query_row("SELECT value FROM app_settings WHERE key=?1", [k], |r| r.get::<_, String>(0)).ok();
        line("Idioma do app", set("language").unwrap_or_else(|| "padrão".into()));
        line("Perfis", n("SELECT COUNT(*) FROM profiles").to_string());
        line("Jogos", n("SELECT COUNT(*) FROM games").to_string());
        line("Emuladores", n("SELECT COUNT(*) FROM emulators").to_string());
        line("Coleções", n("SELECT COUNT(*) FROM collections").to_string());
        line("Capturas na Galeria", n("SELECT COUNT(*) FROM gallery_items").to_string());
        let yes = |b: bool| if b { "sim" } else { "não" }.to_string();
        line("Steam conectada", yes(n("SELECT COUNT(*) FROM platform_accounts WHERE platform='steam'") > 0));
        line("RetroAchievements conectado", yes(n("SELECT COUNT(*) FROM platform_accounts WHERE platform='retroachievements'") > 0));
        line("IGDB configurado", yes(n("SELECT COUNT(*) FROM data_sources WHERE id='igdb' AND secret IS NOT NULL") > 0));
        line("Nexus Mods configurado", yes(n("SELECT COUNT(*) FROM data_sources WHERE id='nexus' AND secret IS NOT NULL") > 0));
        line("SteamGridDB configurado", yes(crate::apikeys::read(&c, "sgdb_key").is_some()));
        line("RAWG configurado", yes(crate::apikeys::read(&c, "rawg_key").is_some()));
        line("Modo de abertura", set("startup_mode").unwrap_or_else(|| "none".into()));
        line("Backup automático", set(crate::backup::auto::K_MODE).unwrap_or_else(|| "off".into()));
        if let Some(e) = set(crate::backup::auto::K_ERROR).filter(|e| !e.is_empty()) {
            line("Último erro do backup automático", e);
        }
    }
    if controllers.is_empty() {
        line("Controles conectados", "nenhum".into());
    } else {
        // só modelo e conexão: nunca o identificador (MAC/serial)
        for c in controllers {
            let link = if c.wired { "cabo" } else { "sem fio" };
            line("Controle", format!("{} ({}, {link})", c.name, c.kind));
        }
    }
    s
}

#[cfg(windows)]
fn windows_version() -> String {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    let key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let text = |name: PCWSTR| -> Option<String> {
        let mut buf = [0u16; 128];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) };
        if r.is_err() {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    };
    let mut ubr = 0u32;
    let mut size = 4u32;
    let has_ubr = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key, w!("UBR"), RRF_RT_REG_DWORD, None, Some(&mut ubr as *mut u32 as *mut _), Some(&mut size)) }.is_ok();
    let build = text(w!("CurrentBuildNumber")).unwrap_or_default();
    let mut product = text(w!("ProductName")).unwrap_or_else(|| "Windows".into());
    // o registro continua dizendo "Windows 10" no Windows 11; o build 22000+ é Windows 11
    if build.parse::<u32>().map(|b| b >= 22000).unwrap_or(false) {
        product = product.replace("Windows 10", "Windows 11");
    }
    let display = text(w!("DisplayVersion")).map(|d| format!(" {d}")).unwrap_or_default();
    let ubr = if has_ubr { format!(".{ubr}") } else { String::new() };
    format!("{product}{display} (build {build}{ubr})")
}

#[cfg(not(windows))]
fn windows_version() -> String {
    std::env::consts::OS.into()
}

fn clean(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(max).collect()
}

/// Envia o relato. Roda fora da thread da interface; o app continua usável durante o envio.
#[tauri::command]
pub async fn feedback_send(input: FeedbackInput) -> AppResult<FeedbackResult> {
    if ENDPOINT.trim().is_empty() {
        return Err(AppError::Other("O envio de feedback ainda não foi configurado nesta versão do NEXSTAGE.".into()));
    }
    if !CATEGORIES.contains(&input.category.as_str()) {
        return Err(AppError::Other("Categoria inválida.".into()));
    }
    let title = clean(&input.title, 120);
    let (comments, problem) = (clean(&input.comments, TEXT_MAX), clean(&input.problem, TEXT_MAX));
    if title.chars().count() < 3 {
        return Err(AppError::Other("Escreva um título com pelo menos 3 letras.".into()));
    }
    if comments.is_empty() && problem.is_empty() {
        return Err(AppError::Other("Escreva um comentário ou a descrição do problema.".into()));
    }
    // só os anexos conhecidos, de novo filtrados e limitados
    let attachments: Vec<serde_json::Value> = input
        .attachments
        .iter()
        .filter(|a| ATTACHMENT_NAMES.contains(&a.name.as_str()))
        .map(|a| serde_json::json!({ "name": a.name, "content": clean(&crate::diaglog::sanitize(&a.content), LOG_MAX + 4096) }))
        .collect();
    let body = serde_json::json!({
        "category": input.category,
        "title": title,
        "comments": comments,
        "problem": problem,
        "version": env!("NX_VERSION"),
        "sentAt": clean(&input.sent_at, 64),
        "language": clean(&input.language, 12),
        "attachments": attachments,
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("NEXSTAGE/", env!("NX_VERSION")))
        .build()
        .map_err(|_| AppError::Other("Não foi possível preparar o envio.".into()))?;
    let res = client
        .post(ENDPOINT.trim())
        .header("X-Nexstage-Client", "feedback-1")
        .json(&body)
        .send()
        .await
        .map_err(|_| AppError::Other("Sem conexão com o serviço de feedback. Verifique a internet e tente de novo.".into()))?;
    let status = res.status();
    if status.as_u16() == 429 {
        return Err(AppError::Other("Muitos envios em pouco tempo. Tente de novo em alguns minutos.".into()));
    }
    if !status.is_success() {
        crate::diaglog::write("ERRO", "feedback", &format!("serviço respondeu {}", status.as_u16()));
        return Err(AppError::Other(format!("O serviço de feedback não aceitou o envio (código {}). Tente de novo mais tarde.", status.as_u16())));
    }
    let id = res.json::<serde_json::Value>().await.ok().and_then(|v| v.get("id").and_then(|i| i.as_u64()));
    Ok(FeedbackResult { id })
}
