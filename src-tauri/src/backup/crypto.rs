//! Proteção por senha dos backups.
//!
//! - Chave: Argon2id (64 MiB, 3 passadas) sobre a senha + salt aleatório guardado no manifesto.
//! - Arquivos: AES-256-GCM em blocos de 1 MiB (processa backups grandes sem carregar tudo na memória).
//!   Formato: `NXBK1\0` + prefixo aleatório de 8 bytes; cada bloco = texto cifrado + etiqueta de 16 bytes.
//!   Nonce = prefixo + contador do bloco; o AAD marca o último bloco (um arquivo truncado não é aceito).
//! - A senha nunca é gravada, registrada em log ou devolvida em mensagens de erro.

use crate::error::{AppError, AppResult};
use argon2::{Algorithm, Argon2, Params, Version};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use std::io::{Read, Write};

const MAGIC: &[u8; 6] = b"NXBK1\0";
const HEADER: usize = 14;
const TAG: usize = 16;
pub const CHUNK: usize = 1 << 20;
/// Texto conhecido cifrado no manifesto: confirma a senha antes de ler qualquer arquivo.
const CHECK: &[u8] = b"NEXSTAGE-BACKUP-PASSWORD-OK";

pub const KDF_M: u32 = 65_536;
pub const KDF_T: u32 = 3;
pub const KDF_P: u32 = 1;

pub struct Key(LessSafeKey);

pub fn random<const N: usize>() -> AppResult<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|_| AppError::Other("Gerador de números aleatórios indisponível.".into()))?;
    Ok(b)
}

pub fn validate_password(p: &str) -> AppResult<()> {
    if p.chars().count() < 8 {
        return Err(AppError::Validation("A senha do backup deve ter pelo menos 8 caracteres.".into()));
    }
    if p.len() > 256 {
        return Err(AppError::Validation("Senha longa demais.".into()));
    }
    Ok(())
}

/// Deriva a chave (os parâmetros vêm do manifesto, para abrir backups feitos com outros valores).
pub fn derive(password: &str, salt: &[u8], m: u32, t: u32, p: u32) -> AppResult<Key> {
    // Limites: um manifesto adulterado não pode pedir memória/tempo absurdos.
    if !(8_192..=1_048_576).contains(&m) || !(1..=10).contains(&t) || !(1..=4).contains(&p) || salt.len() < 16 {
        return Err(AppError::Validation("Parâmetros de proteção do backup inválidos.".into()));
    }
    let params = Params::new(m, t, p, Some(32)).map_err(|_| AppError::Validation("Parâmetros de proteção do backup inválidos.".into()))?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|_| AppError::Other("Falha ao derivar a chave do backup.".into()))?;
    let key = UnboundKey::new(&AES_256_GCM, &out).map_err(|_| AppError::Other("Falha ao preparar a chave do backup.".into()));
    out.iter_mut().for_each(|b| *b = 0);
    Ok(Key(LessSafeKey::new(key?)))
}

fn nonce(prefix: &[u8; 8], i: u32) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(prefix);
    n[8..].copy_from_slice(&i.to_be_bytes());
    Nonce::assume_unique_for_key(n)
}

/// Lê até encher `buf` ou acabar o arquivo. Devolve quantos bytes leu.
fn fill(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Cifra `r` → `w`. `tick(bytes)` é chamado a cada bloco (progresso/cancelamento: devolver erro interrompe).
pub fn encrypt(key: &Key, r: &mut impl Read, w: &mut impl Write, tick: &mut dyn FnMut(u64) -> AppResult<()>) -> AppResult<()> {
    let prefix = random::<8>()?;
    w.write_all(MAGIC)?;
    w.write_all(&prefix)?;
    let mut cur = vec![0u8; CHUNK];
    let mut next = vec![0u8; CHUNK];
    let mut n = fill(r, &mut cur)?;
    let mut i: u32 = 0;
    loop {
        // Bloco cheio: espia o próximo para saber se este é o último.
        let m = if n == CHUNK { fill(r, &mut next)? } else { 0 };
        let last = m == 0;
        let mut buf = cur[..n].to_vec();
        key.0
            .seal_in_place_append_tag(nonce(&prefix, i), Aad::from([last as u8]), &mut buf)
            .map_err(|_| AppError::Other("Falha ao cifrar o backup.".into()))?;
        w.write_all(&buf)?;
        tick(n as u64)?;
        if last {
            break;
        }
        std::mem::swap(&mut cur, &mut next);
        n = m;
        i = i.checked_add(1).ok_or_else(|| AppError::Other("Arquivo grande demais.".into()))?;
    }
    Ok(())
}

/// Decifra `r` (com `len` bytes no total) → `w`. Qualquer alteração ou corte no arquivo é recusado.
pub fn decrypt(key: &Key, r: &mut impl Read, len: u64, w: &mut impl Write, tick: &mut dyn FnMut(u64) -> AppResult<()>) -> AppResult<()> {
    let bad = || AppError::Validation("Arquivo do backup danificado ou senha incorreta.".into());
    if len < (HEADER + TAG) as u64 {
        return Err(bad());
    }
    let mut head = [0u8; HEADER];
    r.read_exact(&mut head)?;
    if &head[..6] != MAGIC {
        return Err(bad());
    }
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&head[6..]);
    let body = len - HEADER as u64;
    let block = (CHUNK + TAG) as u64;
    let count = body.div_ceil(block).max(1);
    let mut buf = vec![0u8; CHUNK + TAG];
    for i in 0..count {
        let size = (body - i * block).min(block) as usize;
        if size < TAG {
            return Err(bad());
        }
        r.read_exact(&mut buf[..size])?;
        let last = i + 1 == count;
        let plain = key.0
            .open_in_place(nonce(&prefix, i as u32), Aad::from([last as u8]), &mut buf[..size])
            .map_err(|_| bad())?;
        w.write_all(plain)?;
        tick(plain.len() as u64)?;
    }
    Ok(())
}

pub fn encrypt_bytes(key: &Key, data: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len() + HEADER + TAG);
    encrypt(key, &mut &data[..], &mut out, &mut |_| Ok(()))?;
    Ok(out)
}

pub fn decrypt_bytes(key: &Key, data: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    decrypt(key, &mut &data[..], data.len() as u64, &mut out, &mut |_| Ok(()))?;
    Ok(out)
}

pub fn make_check(key: &Key) -> AppResult<Vec<u8>> {
    encrypt_bytes(key, CHECK)
}

pub fn verify_check(key: &Key, blob: &[u8]) -> bool {
    decrypt_bytes(key, blob).map(|v| v == CHECK).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> Key {
        derive("senha-de-teste", &[7u8; 16], 8_192, 1, 1).unwrap()
    }

    #[test]
    fn roundtrip_sizes_including_exact_chunks() {
        let k = key();
        for size in [0, 1, 100, CHUNK - 1, CHUNK, CHUNK + 1, 2 * CHUNK] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let enc = encrypt_bytes(&k, &data).unwrap();
            assert!(size < 32 || !enc.windows(32).any(|w| w == &data[..32]), "não pode conter o texto puro");
            assert_eq!(decrypt_bytes(&k, &enc).unwrap(), data, "tamanho {size}");
        }
    }

    #[test]
    fn rejects_wrong_password_tamper_and_truncation() {
        let k = key();
        let data = vec![9u8; CHUNK + 500];
        let enc = encrypt_bytes(&k, &data).unwrap();
        let other = derive("outra-senha", &[7u8; 16], 8_192, 1, 1).unwrap();
        assert!(decrypt_bytes(&other, &enc).is_err());
        let mut bad = enc.clone();
        bad[HEADER + 10] ^= 1;
        assert!(decrypt_bytes(&k, &bad).is_err());
        // corta o último bloco inteiro: o anterior não está marcado como último
        assert!(decrypt_bytes(&k, &enc[..HEADER + CHUNK + TAG]).is_err());
        assert!(verify_check(&k, &make_check(&k).unwrap()));
        assert!(!verify_check(&other, &make_check(&k).unwrap()));
    }
}
