//! Identificação segura de jogos em fontes externas (Metacritic/RAWG, IGDB, Nexus Mods).
//! Regra única: só escolhe sozinho com folga clara; na dúvida, o usuário confirma.
//! Pontuação = nome (`info::name_score`, que já separa sequências: "God of War" ≠ "God of War II")
//! + existir na plataforma do jogo + ano de lançamento (quando conhecido).

use crate::commands::info::name_score;

/// Candidato mínimo para a pontuação.
pub trait Matchable {
    fn id(&self) -> i64;
    fn name(&self) -> &str;
    fn year(&self) -> Option<i64>;
    fn on_platform(&self) -> bool;
    /// Identificador forte confirmado (ex.: mesmo AppID da Steam): escolha direta.
    fn strong(&self) -> bool {
        false
    }
}

pub fn score<M: Matchable>(term: &str, c: &M, year: Option<i64>) -> f64 {
    let name = name_score(term, c.name());
    if name < 0.5 {
        return 0.0;
    }
    let plat = if c.on_platform() { 0.15 } else { -0.3 };
    let yr = match (year, c.year()) {
        (Some(a), Some(b)) if a == b => 0.1,
        (Some(a), Some(b)) if (a - b).abs() <= 1 => 0.05,
        (Some(_), Some(_)) => -0.1,
        _ => 0.0,
    };
    name + plat + yr
}

/// Escolha automática só com segurança: identificador forte único; ou nome equivalente na plataforma
/// e sem empate; ou pontuação alta com folga grande. Caso contrário, `None` (o usuário escolhe).
pub fn pick_confident<M: Matchable>(term: &str, cands: &[M], year: Option<i64>) -> Option<i64> {
    let strong: Vec<&M> = cands.iter().filter(|c| c.strong()).collect();
    if strong.len() == 1 {
        return Some(strong[0].id());
    }
    let mut s: Vec<(f64, f64, &M)> = cands.iter().map(|c| (score(term, c, year), name_score(term, c.name()), c)).collect();
    s.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let (best, best_name, c) = *s.first()?;
    if !c.on_platform() {
        return None;
    }
    let second = s.get(1).map(|x| x.0).unwrap_or(0.0);
    if best_name >= 1.0 && best - second >= 0.1 {
        return Some(c.id());
    }
    if best_name >= 0.8 && best - second >= 0.25 {
        return Some(c.id());
    }
    None
}

/// Ordena candidatos para exibição (da plataforma do jogo primeiro, depois pelo nome).
pub fn rank_for_display<M: Matchable>(term: &str, v: &mut [M]) {
    v.sort_by(|a, b| {
        b.strong().cmp(&a.strong()).then(b.on_platform().cmp(&a.on_platform())).then(
            name_score(term, b.name()).partial_cmp(&name_score(term, a.name())).unwrap_or(std::cmp::Ordering::Equal),
        )
    });
}

pub fn year_of(s: &str) -> Option<i64> {
    s.get(..4).and_then(|y| y.parse().ok()).filter(|y| (1970..=2100).contains(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct C(i64, &'static str, i64, bool, bool);
    impl Matchable for C {
        fn id(&self) -> i64 { self.0 }
        fn name(&self) -> &str { self.1 }
        fn year(&self) -> Option<i64> { Some(self.2) }
        fn on_platform(&self) -> bool { self.3 }
        fn strong(&self) -> bool { self.4 }
    }

    #[test]
    fn rules() {
        assert_eq!(pick_confident("Ico", &[C(1, "Ico", 2001, true, false), C(2, "Ico HD", 2011, false, false)], None), Some(1));
        assert_eq!(pick_confident("Resident Evil 4", &[C(1, "Resident Evil 4", 2005, true, false), C(2, "Resident Evil 4", 2023, true, false)], None), None);
        assert_eq!(pick_confident("Resident Evil 4", &[C(1, "Resident Evil 4", 2005, true, false), C(2, "Resident Evil 4", 2023, true, false)], Some(2005)), Some(1));
        // identificador forte (AppID da Steam) decide mesmo com nomes parecidos
        assert_eq!(pick_confident("Doom", &[C(1, "DOOM", 2016, true, true), C(2, "Doom", 1993, true, false)], None), Some(1));
        assert_eq!(pick_confident("God of War", &[C(1, "God of War II", 2007, true, false)], None), None);
    }
}
