//! Contas de plataformas externas por perfil (tabela `platform_accounts`, uma por plataforma).
//! Usado pelo RetroAchievements e pela Steam. O segredo (chave de API, quando existe) fica cifrado
//! pelo `vault`; os demais campos são dados públicos do perfil e o estado da sincronização.

use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};

pub struct AccountRow {
    pub username: String,
    pub external_id: Option<String>,
    pub secret: Option<Vec<u8>>,
    pub data: Option<String>,
    pub synced_at: Option<String>,
    pub sync_error: Option<String>,
}

pub fn get(conn: &Connection, profile_id: i64, platform: &str) -> AppResult<Option<AccountRow>> {
    Ok(conn
        .query_row(
            "SELECT account_name, external_id, secret, data, synced_at, sync_error FROM platform_accounts WHERE profile_id=?1 AND platform=?2",
            params![profile_id, platform],
            |r| Ok(AccountRow { username: r.get(0)?, external_id: r.get(1)?, secret: r.get(2)?, data: r.get(3)?, synced_at: r.get(4)?, sync_error: r.get(5)? }),
        )
        .optional()?)
}

/// Cria/atualiza a conta. `secret = None` mantém o segredo já salvo. A data da última sincronização
/// é zerada se a identidade (ID externo ou, sem ele, o nome) mudou: outra conta começa do zero.
pub fn save(conn: &Connection, profile_id: i64, platform: &str, name: &str, external_id: Option<&str>, secret: Option<&[u8]>, data: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO platform_accounts(profile_id, platform, account_name, external_id, secret, data, sync_error)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, NULL)
         ON CONFLICT(profile_id, platform) DO UPDATE SET account_name=excluded.account_name, external_id=excluded.external_id,
           secret=COALESCE(excluded.secret, platform_accounts.secret), data=excluded.data, sync_error=NULL,
           synced_at=CASE WHEN COALESCE(platform_accounts.external_id, lower(platform_accounts.account_name))
                             = COALESCE(excluded.external_id, lower(excluded.account_name)) THEN platform_accounts.synced_at ELSE NULL END",
        params![profile_id, platform, name, external_id, secret, data],
    )?;
    Ok(())
}

/// Troca ou remove (`None`) só o segredo.
pub fn set_secret(conn: &Connection, profile_id: i64, platform: &str, secret: Option<&[u8]>) -> AppResult<()> {
    conn.execute("UPDATE platform_accounts SET secret=?3 WHERE profile_id=?1 AND platform=?2", params![profile_id, platform, secret])?;
    Ok(())
}

pub fn set_data(conn: &Connection, profile_id: i64, platform: &str, name: Option<&str>, data: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE platform_accounts SET data=?3, account_name=COALESCE(?4, account_name) WHERE profile_id=?1 AND platform=?2",
        params![profile_id, platform, data, name],
    )?;
    Ok(())
}

pub fn set_sync_result(conn: &Connection, profile_id: i64, platform: &str, ok_at: Option<&str>, error: Option<&str>) -> AppResult<()> {
    match ok_at {
        Some(at) => conn.execute(
            "UPDATE platform_accounts SET synced_at=?3, sync_error=NULL WHERE profile_id=?1 AND platform=?2",
            params![profile_id, platform, at],
        )?,
        None => conn.execute("UPDATE platform_accounts SET sync_error=?3 WHERE profile_id=?1 AND platform=?2", params![profile_id, platform, error])?,
    };
    Ok(())
}

pub fn delete(conn: &Connection, profile_id: i64, platform: &str) -> AppResult<()> {
    conn.execute("DELETE FROM platform_accounts WHERE profile_id=?1 AND platform=?2", params![profile_id, platform])?;
    Ok(())
}
