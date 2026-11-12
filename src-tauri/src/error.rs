//! Erro unificado do backend. É serializado como texto simples para o frontend,
//! que o exibe diretamente ao usuário (mensagens em português).

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Dados inválidos enviados pelo usuário.
    #[error("{0}")]
    Validation(String),
    /// Registro ou arquivo inexistente.
    #[error("{0}")]
    NotFound(String),
    #[error("Erro no banco de dados: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("Erro de arquivo: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
