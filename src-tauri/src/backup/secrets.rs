//! Credenciais no backup: registro ÚNICO de tudo o que é segredo no banco do NEXSTAGE.
//!
//! No computador, esses valores ficam cifrados pela DPAPI (`vault`), que só abre no mesmo usuário do
//! mesmo Windows. Para transportá-los:
//! - backup COM senha: os segredos são decifrados aqui, vão para `data/secrets.json` (que é cifrado com
//!   a senha, como todo o backup) e, na restauração, são cifrados de novo pela DPAPI do computador de destino;
//! - backup SEM senha: os segredos não entram no backup; o usuário informa as credenciais de novo no destino.
//! Em nenhum caso o banco copiado leva os blobs da DPAPI de origem (inúteis em outro computador).
//!
//! Ao criar uma integração nova com segredo, adicione-a aqui.

use crate::error::AppResult;
use crate::sources::Source;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Chaves de `app_settings` que guardam chaves de API (pesquisa de imagens), cifradas por `apikeys`.
const PLAIN_SETTINGS: [&str; 2] = crate::apikeys::KEYS;

#[derive(Serialize, Deserialize, Default)]
pub struct Bundle {
    pub version: u32,
    pub items: Vec<Item>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    /// Segredo de uma conta de plataforma (chave de API do RetroAchievements/Steam, refresh token do Xbox).
    #[serde(rename_all = "camelCase")]
    Account { profile_id: i64, platform: String, value: String },
    /// Credenciais de uma fonte de dados (IGDB, Nexus Mods...).
    Source { id: String, value: String },
    /// Legado (proteção de perfis removida): e-mail de recuperação. Backups antigos podem trazer; é ignorado.
    #[serde(rename_all = "camelCase")]
    ProfileEmail { profile_id: i64, value: String },
    /// Legado (proteção de perfis removida): senha SMTP dos códigos. Backups antigos podem trazer; é ignorado.
    MailPassword { value: String },
    /// Chave de API em `app_settings`.
    Setting { key: String, value: String },
}

fn account_unprotect(platform: &str, blob: &[u8]) -> Option<String> {
    match platform {
        crate::retro::store::PLATFORM => crate::retro::secret::unprotect(blob),
        crate::steam::store::PLATFORM => crate::steam::sync::unprotect_key(blob),
        crate::xbox::store::PLATFORM => crate::xbox::unprotect_token(blob),
        _ => None,
    }
}
fn account_protect(platform: &str, v: &str) -> Option<Vec<u8>> {
    match platform {
        crate::retro::store::PLATFORM => crate::retro::secret::protect(v),
        crate::steam::store::PLATFORM => crate::steam::sync::protect_key(v),
        crate::xbox::store::PLATFORM => crate::xbox::protect_token(v),
        _ => None,
    }
}

/// Lê e decifra todos os segredos do banco (cópia do backup). Devolve também quantos existem mas não
/// puderam ser lidos (ex.: vieram de outro computador e nunca foram informados de novo aqui).
pub fn extract(conn: &Connection) -> AppResult<(Bundle, usize)> {
    let mut items = Vec::new();
    let mut unreadable = 0;
    {
        let mut st = conn.prepare("SELECT profile_id, platform, secret FROM platform_accounts WHERE secret IS NOT NULL")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
        for row in rows {
            let (profile_id, platform, blob) = row?;
            match account_unprotect(&platform, &blob) {
                Some(value) => items.push(Item::Account { profile_id, platform, value }),
                None => unreadable += 1,
            }
        }
    }
    {
        let mut st = conn.prepare("SELECT id, secret FROM data_sources WHERE secret IS NOT NULL")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
        for row in rows {
            let (id, blob) = row?;
            match Source::parse(&id).ok().and_then(|s| crate::vault::unprotect(s.entropy(), &blob)) {
                Some(value) => items.push(Item::Source { id, value }),
                None => unreadable += 1,
            }
        }
    }
    let setting = |k: &str| -> AppResult<Option<String>> {
        let mut st = conn.prepare("SELECT value FROM app_settings WHERE key=?1")?;
        let mut rows = st.query([k])?;
        Ok(match rows.next()? { Some(r) => Some(r.get(0)?), None => None })
    };
    for k in PLAIN_SETTINGS {
        if let Some(stored) = setting(k)?.filter(|v| !v.trim().is_empty()) {
            match crate::apikeys::open(k, &stored) {
                Some(value) => items.push(Item::Setting { key: k.to_string(), value }),
                None => unreadable += 1,
            }
        }
    }
    Ok((Bundle { version: 1, items }, unreadable))
}

/// Quantos segredos existem no banco (para avisar o usuário que um backup sem senha não os leva).
pub fn count(conn: &Connection) -> AppResult<usize> {
    let q = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0));
    let n = q("SELECT COUNT(*) FROM platform_accounts WHERE secret IS NOT NULL")?
        + q("SELECT COUNT(*) FROM data_sources WHERE secret IS NOT NULL")?
        + q("SELECT COUNT(*) FROM app_settings WHERE key IN ('sgdb_key','rawg_key') AND trim(value)<>''")?;
    Ok(n as usize)
}

/// Remove todos os segredos do banco (cópia do backup ou banco restaurado antes de receber os
/// segredos do destino).
pub fn strip(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "UPDATE platform_accounts SET secret=NULL WHERE secret IS NOT NULL;
         UPDATE data_sources SET secret=NULL, status=NULL WHERE secret IS NOT NULL;
         DELETE FROM app_settings WHERE key IN ('sgdb_key','rawg_key');",
    )?;
    Ok(())
}

/// Grava os segredos no banco restaurado, cifrando-os com a DPAPI DESTE computador.
/// Devolve (aplicados, falharam).
pub fn apply(conn: &Connection, bundle: &Bundle) -> AppResult<(usize, usize)> {
    let (mut ok, mut fail) = (0, 0);
    for it in &bundle.items {
        let done = match it {
            // proteção de perfis removida: nada a restaurar (não conta como falha)
            Item::ProfileEmail { .. } | Item::MailPassword { .. } => continue,
            Item::Account { profile_id, platform, value } => match account_protect(platform, value) {
                Some(b) => conn.execute("UPDATE platform_accounts SET secret=?3 WHERE profile_id=?1 AND platform=?2", params![profile_id, platform, b])? > 0,
                None => false,
            },
            Item::Source { id, value } => match Source::parse(id).ok().and_then(|s| crate::vault::protect(s.entropy(), value)) {
                Some(b) => {
                    conn.execute("INSERT OR IGNORE INTO data_sources(id) VALUES(?1)", [id])?;
                    conn.execute("UPDATE data_sources SET secret=?2 WHERE id=?1", params![id, b])? > 0
                }
                None => false,
            },
            Item::Setting { key, value } => {
                PLAIN_SETTINGS.contains(&key.as_str()) && crate::apikeys::write(conn, key, value).is_ok()
            }
        };
        if done { ok += 1 } else { fail += 1 }
    }
    Ok((ok, fail))
}

/// Coerência depois de restaurar: fonte sem credencial não fica marcada como conectada.
pub fn finalize(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "UPDATE data_sources SET status=NULL WHERE secret IS NULL AND id<>'hltb';",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let p = std::env::temp_dir().join(format!("nexstage-test-{}-secrets.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", p.display()));
        }
        let db = crate::db::Db::open(&p).unwrap();
        drop(db);
        Connection::open(&p).unwrap()
    }

    #[test]
    fn extract_strip_apply_roundtrip() {
        let c = db();
        c.execute_batch("INSERT INTO profiles(id,username,display_name) VALUES(1,'a','A');").unwrap();
        let ra = crate::retro::secret::protect("RAKEY123").unwrap();
        crate::accounts::save(&c, 1, "retroachievements", "Jogador", None, Some(&ra), "{}").unwrap();
        c.execute_batch("INSERT INTO app_settings(key,value) VALUES('sgdb_key','SGDB');").unwrap();
        assert_eq!(count(&c).unwrap(), 2);

        let (bundle, unreadable) = extract(&c).unwrap();
        assert_eq!(unreadable, 0);
        assert_eq!(bundle.items.len(), 2);
        strip(&c).unwrap();
        assert_eq!(count(&c).unwrap(), 0);

        let json = serde_json::to_vec(&bundle).unwrap();
        let mut back: Bundle = serde_json::from_slice(&json).unwrap();
        // backup antigo com itens da proteção de perfis: aceito e ignorado
        back.items.push(Item::ProfileEmail { profile_id: 1, value: "x@y.com".into() });
        back.items.push(Item::MailPassword { value: "smtp".into() });
        assert_eq!(apply(&c, &back).unwrap(), (2, 0));
        let acc = crate::accounts::get(&c, 1, "retroachievements").unwrap().unwrap();
        assert_eq!(crate::retro::secret::unprotect(&acc.secret.unwrap()).as_deref(), Some("RAKEY123"));
    }
}
