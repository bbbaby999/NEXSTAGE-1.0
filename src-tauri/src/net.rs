//! Controle de ritmo das requisições a serviços externos (RetroAchievements, Steam).
//! Cada serviço tem o seu `Gate`: intervalo mínimo entre chamadas e bloqueio temporário
//! quando o serviço responde HTTP 429 (respeitando o Retry-After).

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Gate {
    gap_ms: u64,
    state: Mutex<(Option<Instant>, Option<Instant>)>, // (última chamada reservada, bloqueado até)
}

impl Gate {
    pub const fn new(gap_ms: u64) -> Self {
        Self { gap_ms, state: Mutex::new((None, None)) }
    }

    /// Segundos restantes de bloqueio por limite de requisições (`None` = liberado).
    pub fn blocked_for(&self) -> Option<u64> {
        let g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        g.1.and_then(|t| t.checked_duration_since(Instant::now())).map(|d| d.as_secs().max(1))
    }

    /// Bloqueia novas chamadas por `secs` (5 s a 15 min). Devolve o valor aplicado.
    pub fn block(&self, secs: u64) -> u64 {
        let secs = secs.clamp(5, 900);
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        g.1 = Some(Instant::now() + Duration::from_secs(secs));
        secs
    }

    /// Espera a vez (chamadas concorrentes fazem fila: o horário é reservado antes de dormir).
    pub async fn wait(&self) {
        let wait = {
            let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let start = g.0.map(|l| (l + Duration::from_millis(self.gap_ms)).max(now)).unwrap_or(now);
            g.0 = Some(start);
            start - now
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Lê o Retry-After (segundos) de uma resposta; padrão 60 s.
pub fn retry_after(resp: &reqwest::Response) -> u64 {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_and_release() {
        let g = Gate::new(10);
        assert_eq!(g.blocked_for(), None);
        assert_eq!(g.block(1), 5, "mínimo de 5 s");
        assert!(g.blocked_for().is_some());
    }
}
