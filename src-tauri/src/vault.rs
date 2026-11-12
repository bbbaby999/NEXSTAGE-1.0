//! Cofre local de segredos (chaves de API de integrações), compartilhado por RetroAchievements e Steam.
//!
//! No Windows usa a DPAPI (`CryptProtectData`, escopo do usuário do Windows): o banco guarda só o
//! blob cifrado, que não pode ser decifrado por outra conta do Windows nem em outro computador.
//! Cada integração usa uma "entropia" própria: o blob de uma não abre com a outra.
//! Segredos nunca voltam para a interface depois de salvos e nunca entram em logs ou mensagens.

#[cfg(windows)]
mod dpapi {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct Blob {
        pub cb: u32,
        pub pb: *mut u8,
    }

    #[link(name = "crypt32")]
    extern "system" {
        pub fn CryptProtectData(
            data_in: *const Blob, descr: *const u16, entropy: *const Blob, reserved: *mut c_void,
            prompt: *mut c_void, flags: u32, data_out: *mut Blob,
        ) -> i32;
        pub fn CryptUnprotectData(
            data_in: *const Blob, descr: *mut *mut u16, entropy: *const Blob, reserved: *mut c_void,
            prompt: *mut c_void, flags: u32, data_out: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn LocalFree(h: *mut c_void) -> *mut c_void;
    }
    pub const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;
}

#[cfg(windows)]
fn transform(entropy: &[u8], input: &[u8], protect: bool) -> Option<Vec<u8>> {
    use dpapi::*;
    let inp = Blob { cb: input.len() as u32, pb: input.as_ptr() as *mut u8 };
    let ent = Blob { cb: entropy.len() as u32, pb: entropy.as_ptr() as *mut u8 };
    let mut out = Blob { cb: 0, pb: std::ptr::null_mut() };
    let ok = unsafe {
        if protect {
            CryptProtectData(&inp, std::ptr::null(), &ent, std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&inp, std::ptr::null_mut(), &ent, std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pb.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pb, out.cb as usize).to_vec() };
    unsafe {
        // apaga a cópia temporária da DPAPI antes de liberar
        std::ptr::write_bytes(out.pb, 0, out.cb as usize);
        LocalFree(out.pb as *mut _);
    }
    Some(v)
}

/// Cifra o segredo para gravar no banco.
pub fn protect(entropy: &[u8], key: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        let mut v = b"dpapi1:".to_vec();
        v.extend(transform(entropy, key.as_bytes(), true)?);
        Some(v)
    }
    #[cfg(not(windows))]
    {
        // Fora do Windows (não suportado oficialmente): sem cofre do sistema disponível.
        let mut v = b"plain1:".to_vec();
        v.extend_from_slice(key.as_bytes());
        Some(v)
    }
}

/// Decifra a chave gravada. `None` = blob inválido ou de outro usuário/computador.
pub fn unprotect(entropy: &[u8], blob: &[u8]) -> Option<String> {
    #[cfg(windows)]
    if let Some(rest) = blob.strip_prefix(b"dpapi1:") {
        let mut raw = transform(entropy, rest, false)?;
        let s = String::from_utf8(raw.clone()).ok();
        raw.iter_mut().for_each(|b| *b = 0);
        return s;
    }
    if let Some(rest) = blob.strip_prefix(b"plain1:") {
        return String::from_utf8(rest.to_vec()).ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_never_plain_and_isolated_by_entropy() {
        let key = "AbCdEf0123456789AbCdEf0123456789";
        let blob = protect(b"A", key).unwrap();
        #[cfg(windows)]
        assert!(!blob.windows(key.len()).any(|w| w == key.as_bytes()), "o segredo não pode ficar legível no blob");
        assert_eq!(unprotect(b"A", &blob).as_deref(), Some(key));
        #[cfg(windows)]
        assert_eq!(unprotect(b"B", &blob), None, "blob de uma integração não abre com a entropia de outra");
        assert_eq!(unprotect(b"A", b"lixo"), None);
    }
}
