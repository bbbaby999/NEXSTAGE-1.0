//! Central de fontes de dados (Configurações → Fontes de dados).
//!
//! Cada fonte é independente: ativação, credenciais (cofre do Windows via `crate::vault`, nunca em
//! texto puro), teste de conexão, cache próprio (`source_cache`), controle de requisições
//! (`crate::net::Gate`), tratamento de erros e data da última sincronização.
//! - `igdb`: dados e imagens dos jogos (API oficial; OAuth da Twitch).
//! - `hltb`: HowLongToBeat — sem acesso autorizado disponível: provedor preparado e DESATIVADO.
//! - `nexus`: mods (API oficial v1 + v2 GraphQL; chave pessoal).
//! - `matching`: identificação segura de jogos (compartilhada com o Metacritic).
//! Nenhuma fonte é chamada na inicialização; falhas nunca bloqueiam a biblioteca.

pub mod hltb;
pub mod igdb;
pub mod matching;
pub mod nexus;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::retro::util::{fmt_epoch, now_epoch};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS data_sources (
    id          TEXT PRIMARY KEY,           -- 'igdb' | 'hltb' | 'nexus'
    enabled     INTEGER NOT NULL DEFAULT 0,
    config      TEXT NOT NULL DEFAULT '{}', -- dados NÃO secretos (ex.: nome do usuário no Nexus)
    secret      BLOB,                       -- credenciais cifradas pela DPAPI
    status      TEXT,                       -- 'ok' | 'error' | NULL (nunca testado)
    last_sync   TEXT,
    last_error  TEXT
);
-- Cache das respostas das fontes (chave livre por fonte), com data para expirar.
CREATE TABLE IF NOT EXISTS source_cache (
    source      TEXT NOT NULL,
    key         TEXT NOT NULL,
    data        TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    PRIMARY KEY (source, key)
);
-- Jogo da biblioteca ↔ jogo no Nexus Mods (só depois de confirmado pelo usuário).
CREATE TABLE IF NOT EXISTS nexus_links (
    game_id         INTEGER PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
    domain          TEXT NOT NULL,
    nexus_game_id   INTEGER NOT NULL,
    name            TEXT NOT NULL
);
-- Mods favoritos do usuário dentro do NEXSTAGE (nada é baixado).
CREATE TABLE IF NOT EXISTS nexus_favorites (
    profile_id  INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    domain      TEXT NOT NULL,
    mod_id      INTEGER NOT NULL,
    data        TEXT NOT NULL,              -- instantâneo do mod (nome, imagem, resumo...)
    added_at    TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (profile_id, domain, mod_id)
);
"#;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    Igdb,
    Hltb,
    Nexus,
}

impl Source {
    pub fn id(self) -> &'static str {
        match self {
            Source::Igdb => "igdb",
            Source::Hltb => "hltb",
            Source::Nexus => "nexus",
        }
    }
    pub fn parse(s: &str) -> AppResult<Self> {
        Ok(match s {
            "igdb" => Source::Igdb,
            "hltb" => Source::Hltb,
            "nexus" => Source::Nexus,
            _ => return Err(AppError::Validation("Fonte de dados desconhecida.".into())),
        })
    }
    /// Entropia própria no cofre: o segredo de uma fonte não abre com a de outra.
    pub(crate) fn entropy(self) -> &'static [u8] {
        match self {
            Source::Igdb => b"NEXSTAGE/Source/IGDB/v1",
            Source::Hltb => b"NEXSTAGE/Source/HLTB/v1",
            Source::Nexus => b"NEXSTAGE/Source/Nexus/v1",
        }
    }
    pub const ALL: [Source; 3] = [Source::Igdb, Source::Hltb, Source::Nexus];
}

pub struct Row {
    pub enabled: bool,
    pub config: Value,
    pub secret: Option<Vec<u8>>,
    pub status: Option<String>,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
}

pub fn row(conn: &Connection, s: Source) -> AppResult<Row> {
    Ok(conn
        .query_row("SELECT enabled, config, secret, status, last_sync, last_error FROM data_sources WHERE id=?1", [s.id()], |r| {
            Ok(Row {
                enabled: r.get::<_, i64>(0)? != 0,
                config: serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or(Value::Null),
                secret: r.get(2)?,
                status: r.get(3)?,
                last_sync: r.get(4)?,
                last_error: r.get(5)?,
            })
        })
        .optional()?
        .unwrap_or(Row { enabled: false, config: Value::Null, secret: None, status: None, last_sync: None, last_error: None }))
}

fn ensure(conn: &Connection, s: Source) -> AppResult<()> {
    conn.execute("INSERT OR IGNORE INTO data_sources(id) VALUES(?1)", [s.id()])?;
    Ok(())
}

/// Segredo decifrado (só no backend). `None` = sem credenciais ou não legível neste Windows.
pub fn secret(conn: &Connection, s: Source) -> AppResult<Option<String>> {
    Ok(row(conn, s)?.secret.as_deref().and_then(|b| crate::vault::unprotect(s.entropy(), b)))
}

pub fn save_secret(conn: &Connection, s: Source, value: Option<&str>, config: Option<&Value>) -> AppResult<()> {
    ensure(conn, s)?;
    let blob = match value {
        Some(v) => Some(crate::vault::protect(s.entropy(), v).ok_or_else(|| AppError::Other("Não foi possível proteger a credencial neste computador.".into()))?),
        None => None,
    };
    conn.execute(
        "UPDATE data_sources SET secret=?2, config=COALESCE(?3, config), status=NULL, last_error=NULL WHERE id=?1",
        params![s.id(), blob, config.map(|c| c.to_string())],
    )?;
    Ok(())
}

pub fn set_enabled(conn: &Connection, s: Source, on: bool) -> AppResult<()> {
    ensure(conn, s)?;
    conn.execute("UPDATE data_sources SET enabled=?2 WHERE id=?1", params![s.id(), on as i64])?;
    Ok(())
}

/// Registra o resultado de um uso/teste (status, erro e última sincronização bem-sucedida).
pub fn record(conn: &Connection, s: Source, result: Result<(), &str>) -> AppResult<()> {
    ensure(conn, s)?;
    match result {
        Ok(()) => conn.execute(
            "UPDATE data_sources SET status='ok', last_error=NULL, last_sync=?2 WHERE id=?1",
            params![s.id(), fmt_epoch(now_epoch())],
        )?,
        Err(e) => conn.execute("UPDATE data_sources SET status='error', last_error=?2 WHERE id=?1", params![s.id(), e])?,
    };
    Ok(())
}

/// Fonte pronta para uso: ativada e com credenciais legíveis.
pub fn active(conn: &Connection, s: Source) -> AppResult<Option<String>> {
    let r = row(conn, s)?;
    if !r.enabled {
        return Ok(None);
    }
    Ok(r.secret.as_deref().and_then(|b| crate::vault::unprotect(s.entropy(), b)))
}

// ---------------------------------------------------------------- cache genérico

pub fn cache_get(conn: &Connection, s: Source, key: &str, max_age_secs: i64) -> AppResult<Option<Value>> {
    let row: Option<(String, i64)> = conn
        .query_row("SELECT data, fetched_at FROM source_cache WHERE source=?1 AND key=?2", params![s.id(), key], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(row.filter(|(_, at)| now_epoch() - at <= max_age_secs).and_then(|(d, _)| serde_json::from_str(&d).ok()))
}

pub fn cache_put(conn: &Connection, s: Source, key: &str, data: &Value) -> AppResult<()> {
    conn.execute(
        "INSERT INTO source_cache(source, key, data, fetched_at) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(source, key) DO UPDATE SET data=excluded.data, fetched_at=excluded.fetched_at",
        params![s.id(), key, data.to_string(), now_epoch()],
    )?;
    Ok(())
}

pub fn cache_clear(conn: &Connection, s: Source) -> AppResult<()> {
    conn.execute("DELETE FROM source_cache WHERE source=?1", [s.id()])?;
    Ok(())
}

// ---------------------------------------------------------------- comandos da central

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: String,
    pub enabled: bool,
    /// Há credenciais salvas e legíveis neste Windows.
    pub configured: bool,
    /// Há acesso autorizado disponível para esta fonte (HowLongToBeat: não).
    pub authorized: bool,
    pub status: Option<String>,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    /// Dados públicos da configuração (ex.: Client ID mascarado, usuário do Nexus).
    pub info: Value,
    pub cache_entries: i64,
}

fn view(conn: &Connection, s: Source) -> AppResult<SourceView> {
    let r = row(conn, s)?;
    let configured = r.secret.as_deref().and_then(|b| crate::vault::unprotect(s.entropy(), b)).is_some();
    let cache_entries: i64 = conn.query_row("SELECT COUNT(*) FROM source_cache WHERE source=?1", [s.id()], |x| x.get(0))?;
    Ok(SourceView {
        id: s.id().into(),
        enabled: r.enabled && (s != Source::Hltb || hltb::AUTHORIZED),
        configured,
        authorized: s != Source::Hltb || hltb::AUTHORIZED,
        status: r.status,
        last_sync: r.last_sync,
        last_error: r.last_error,
        info: if r.config.is_null() { Value::Object(Default::default()) } else { r.config },
        cache_entries,
    })
}

#[tauri::command]
pub fn sources_list(db: State<Db>) -> AppResult<Vec<SourceView>> {
    let conn = db.conn()?;
    // HowLongToBeat: integração removida das Configurações (sem API pública autorizada; o provedor nunca
    // pôde ser ativado). Os tempos para zerar vêm do IGDB ou do preenchimento manual.
    Source::ALL.iter().filter(|s| **s != Source::Hltb).map(|s| view(&conn, *s)).collect()
}

#[tauri::command]
pub fn source_set_enabled(db: State<Db>, id: String, enabled: bool) -> AppResult<SourceView> {
    let s = Source::parse(&id)?;
    if s == Source::Hltb && enabled && !hltb::AUTHORIZED {
        return Err(AppError::Validation(hltb::UNAVAILABLE.into()));
    }
    let conn = db.conn()?;
    if enabled && secret(&conn, s)?.is_none() {
        return Err(AppError::Validation("Configure e teste as credenciais antes de ativar esta fonte.".into()));
    }
    set_enabled(&conn, s, enabled)?;
    view(&conn, s)
}

/// Salva as credenciais (testando antes) e ativa a fonte. `fields`: IGDB {clientId, clientSecret};
/// Nexus {apiKey}. Nada volta para a interface além do resultado público do teste.
#[tauri::command]
pub async fn source_connect(db: State<'_, Db>, id: String, fields: serde_json::Map<String, Value>) -> AppResult<SourceView> {
    let s = Source::parse(&id)?;
    let get = |k: &str| fields.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
    let (secret_value, info) = match s {
        Source::Igdb => {
            let (cid, sec) = (get("clientId"), get("clientSecret"));
            let (Some(cid), Some(sec)) = (cid, sec) else {
                return Err(AppError::Validation("Informe o Client ID e o Client Secret do aplicativo criado no console de desenvolvedores da Twitch.".into()));
            };
            igdb::validate_format(&cid, &sec)?;
            let creds = igdb::Creds { client_id: cid.clone(), client_secret: sec.clone() };
            igdb::test(&creds).await?;
            let masked = format!("{}…{}", &cid[..4.min(cid.len())], &cid[cid.len().saturating_sub(4)..]);
            (serde_json::to_string(&creds).unwrap_or_default(), serde_json::json!({ "clientId": masked }))
        }
        Source::Nexus => {
            let Some(key) = get("apiKey") else { return Err(AppError::Validation("Informe a sua chave pessoal da API do Nexus Mods.".into())) };
            nexus::validate_format(&key)?;
            let user = nexus::validate(&key).await?;
            (key, user)
        }
        Source::Hltb => return Err(AppError::Validation(hltb::UNAVAILABLE.into())),
    };
    let conn = db.conn()?;
    save_secret(&conn, s, Some(&secret_value), Some(&info))?;
    set_enabled(&conn, s, true)?;
    record(&conn, s, Ok(()))?;
    view(&conn, s)
}

/// Testa a conexão com as credenciais salvas.
#[tauri::command]
pub async fn source_test(db: State<'_, Db>, id: String) -> AppResult<SourceView> {
    let s = Source::parse(&id)?;
    let sec = { secret(&*db.conn()?, s)? }.ok_or_else(|| AppError::Validation("Nenhuma credencial salva para esta fonte (ou ela não pode ser lida neste Windows).".into()))?;
    let res: AppResult<Option<Value>> = match s {
        Source::Igdb => match serde_json::from_str::<igdb::Creds>(&sec) {
            Ok(c) => igdb::test(&c).await.map(|_| None),
            Err(_) => Err(AppError::Validation("Credenciais do IGDB inválidas: salve novamente.".into())),
        },
        Source::Nexus => nexus::validate(&sec).await.map(Some),
        Source::Hltb => Err(AppError::Validation(hltb::UNAVAILABLE.into())),
    };
    let conn = db.conn()?;
    match res {
        Ok(info) => {
            if let Some(i) = info {
                conn.execute("UPDATE data_sources SET config=?2 WHERE id=?1", params![s.id(), i.to_string()])?;
            }
            record(&conn, s, Ok(()))?;
            view(&conn, s)
        }
        Err(e) => {
            record(&conn, s, Err(&e.to_string()))?;
            Err(e)
        }
    }
}

/// Remove as credenciais e o cache da fonte (dados já gravados nos jogos são preservados).
#[tauri::command]
pub fn source_disconnect(app: AppHandle, db: State<Db>, id: String) -> AppResult<SourceView> {
    let s = Source::parse(&id)?;
    let conn = db.conn()?;
    save_secret(&conn, s, None, Some(&Value::Object(Default::default())))?;
    set_enabled(&conn, s, false)?;
    cache_clear(&conn, s)?;
    if s == Source::Igdb {
        igdb::forget_token();
    }
    let _ = app;
    view(&conn, s)
}

#[tauri::command]
pub fn source_clear_cache(db: State<Db>, id: String) -> AppResult<SourceView> {
    let s = Source::parse(&id)?;
    let conn = db.conn()?;
    cache_clear(&conn, s)?;
    view(&conn, s)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE profiles(id INTEGER PRIMARY KEY); CREATE TABLE games(id INTEGER PRIMARY KEY, title TEXT, platform TEXT);
             INSERT INTO profiles(id) VALUES(1);",
        )
        .unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c
    }

    #[test]
    fn secrets_are_encrypted_isolated_and_cache_expires() {
        let c = mem();
        save_secret(&c, Source::Nexus, Some("CHAVE-NEXUS-123"), None).unwrap();
        let blob: Vec<u8> = c.query_row("SELECT secret FROM data_sources WHERE id='nexus'", [], |r| r.get(0)).unwrap();
        #[cfg(windows)]
        assert!(!blob.windows(5).any(|w| w == b"CHAVE"), "a chave nunca fica legível no banco");
        assert_eq!(secret(&c, Source::Nexus).unwrap().as_deref(), Some("CHAVE-NEXUS-123"));
        assert_eq!(active(&c, Source::Nexus).unwrap(), None, "desativada: não é usada");
        set_enabled(&c, Source::Nexus, true).unwrap();
        assert!(active(&c, Source::Nexus).unwrap().is_some());
        assert_eq!(secret(&c, Source::Igdb).unwrap(), None, "fontes não compartilham segredos");

        cache_put(&c, Source::Nexus, "k", &serde_json::json!({"a": 1})).unwrap();
        assert!(cache_get(&c, Source::Nexus, "k", 60).unwrap().is_some());
        c.execute("UPDATE source_cache SET fetched_at=fetched_at-120", []).unwrap();
        assert!(cache_get(&c, Source::Nexus, "k", 60).unwrap().is_none(), "cache expirado não é usado");

        record(&c, Source::Nexus, Err("falhou")).unwrap();
        let v = view(&c, Source::Nexus).unwrap();
        assert_eq!((v.status.as_deref(), v.last_error.as_deref()), (Some("error"), Some("falhou")));
        let h = view(&c, Source::Hltb).unwrap();
        assert!(!h.authorized && !h.enabled, "HowLongToBeat sem acesso autorizado fica desativado");
    }
}
