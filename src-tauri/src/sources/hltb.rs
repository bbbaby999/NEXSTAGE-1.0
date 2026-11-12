//! HowLongToBeat — provedor PREPARADO e DESATIVADO.
//!
//! Situação verificada: o HowLongToBeat não oferece API pública nem programa de acesso para
//! aplicativos. O único caminho técnico é o endpoint interno do site, protegido por tokens
//! anti-automação — usá-lo seria contornar essa proteção, o que o NEXSTAGE não faz. A versão
//! anterior fazia exatamente isso e foi removida.
//!
//! O que continua valendo:
//! - tempos já obtidos antes ficam guardados, identificados como "HowLongToBeat (obtido anteriormente)";
//! - tempos digitados à mão;
//! - "tempo para zerar" do IGDB (API oficial, `game_time_to_beats`), sempre identificado como IGDB.
//!
//! Para ativar no futuro: quando houver acesso autorizado (API/parceria), implementar `lookup`
//! com ele, trocar `AUTHORIZED` para `true` e registrar credenciais pela central (`sources`).

/// Não há acesso autorizado ao HowLongToBeat.
pub const AUTHORIZED: bool = false;

pub const UNAVAILABLE: &str = "O HowLongToBeat não oferece API pública ou acesso autorizado para aplicativos. O provedor está preparado, mas desativado: use os tempos do IGDB ou preencha manualmente.";

#[cfg(test)]
mod tests {
    #[test]
    fn stays_disabled_without_authorized_access() {
        assert!(!super::AUTHORIZED);
    }
}
