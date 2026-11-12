//! Chave da API do RetroAchievements no cofre do Windows (ver `crate::vault`).

const ENTROPY: &[u8] = b"NEXSTAGE/RetroAchievements/v1";

pub fn protect(key: &str) -> Option<Vec<u8>> {
    crate::vault::protect(ENTROPY, key)
}

pub fn unprotect(blob: &[u8]) -> Option<String> {
    crate::vault::unprotect(ENTROPY, blob)
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip() {
        let blob = super::protect("CHAVE0123456789").unwrap();
        assert_eq!(super::unprotect(&blob).as_deref(), Some("CHAVE0123456789"));
    }
}
