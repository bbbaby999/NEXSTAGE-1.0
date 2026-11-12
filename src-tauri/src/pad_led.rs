//! Cor da barra de luz (lightbar) dos controles PlayStation: DualSense / DualSense Edge e DualShock 4,
//! por USB e Bluetooth. A cor acompanha a cor de destaque do perfil (enviada pela interface).
//!
//! Relatórios de saída (mesmo formato do driver `hid-playstation` do Linux):
//!  - DualSense USB: id 0x02, 48 bytes. Bluetooth: id 0x31, 78 bytes, com sequência e CRC32.
//!    Antes da primeira cor é preciso "liberar" a barra de luz (o firmware mantém a própria animação
//!    azul até receber `LIGHTBAR_SETUP = LIGHT_OUT`).
//!  - DualShock 4 USB: id 0x05, 32 bytes. Bluetooth: id 0x11, 78 bytes, com CRC32.
//!    Depois do primeiro relatório por Bluetooth, o DS4 passa a enviar a entrada completa (id 0x11),
//!    que `input::parse_hid` também entende.

/// CRC32 (IEEE, refletido) usado pelos relatórios Bluetooth: semente 0xA2 (cabeçalho HID de saída)
/// seguida dos bytes do relatório.
pub fn bt_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in std::iter::once(&0xA2u8).chain(data.iter()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn seal_bt(mut buf: Vec<u8>) -> Vec<u8> {
    buf.resize(78, 0);
    let crc = bt_crc32(&buf[..74]);
    buf[74..78].copy_from_slice(&crc.to_le_bytes());
    buf
}

/// Bloco comum do DualSense (47 bytes, depois do id/cabeçalho).
fn ds_common(flag1: u8, flag2: u8, setup: u8, rgb: (u8, u8, u8)) -> [u8; 47] {
    let mut c = [0u8; 47];
    c[1] = flag1; // valid_flag1: BIT(2) = controle da barra de luz
    c[38] = flag2; // valid_flag2: BIT(1) = configuração da barra de luz
    c[41] = setup; // lightbar_setup: BIT(1) = LIGHT_OUT (encerra a animação do firmware)
    c[44] = rgb.0;
    c[45] = rgb.1;
    c[46] = rgb.2;
    c
}

fn ds_wrap(common: [u8; 47], bt: bool, seq: u8) -> Vec<u8> {
    if bt {
        let mut buf = vec![0x31, (seq & 0x0F) << 4, 0x10];
        buf.extend_from_slice(&common);
        seal_bt(buf)
    } else {
        let mut buf = vec![0x02];
        buf.extend_from_slice(&common);
        buf
    }
}

/// DualSense: libera a barra de luz (enviar uma vez por conexão, antes da primeira cor).
pub fn dualsense_release(bt: bool, seq: u8) -> Vec<u8> {
    ds_wrap(ds_common(0, 0x02, 0x02, (0, 0, 0)), bt, seq)
}

/// DualSense: define a cor da barra de luz.
pub fn dualsense_color(bt: bool, seq: u8, rgb: (u8, u8, u8)) -> Vec<u8> {
    ds_wrap(ds_common(0x04, 0, 0, rgb), bt, seq)
}

/// DualShock 4: define a cor da barra de luz (só o bit de LED: vibração e piscar não são alterados).
pub fn ds4_color(bt: bool, rgb: (u8, u8, u8)) -> Vec<u8> {
    if bt {
        let mut buf = vec![0u8; 13];
        buf[0] = 0x11;
        buf[1] = 0xC0; // HID + CRC32
        buf[3] = 0x02; // valid_flag0: LED
        buf[8] = rgb.0;
        buf[9] = rgb.1;
        buf[10] = rgb.2;
        seal_bt(buf)
    } else {
        let mut buf = vec![0u8; 32];
        buf[0] = 0x05;
        buf[1] = 0x02;
        buf[6] = rgb.0;
        buf[7] = rgb.1;
        buf[8] = rgb.2;
        buf
    }
}

/// DualSense: vibração (modo compatível: motor esquerdo forte, direito fraco). Só os bits de vibração
/// são marcados, então a barra de luz e os demais ajustes do controle não mudam.
pub fn dualsense_rumble(bt: bool, seq: u8, left: u8, right: u8) -> Vec<u8> {
    let mut c = [0u8; 47];
    c[0] = 0x03; // valid_flag0: COMPATIBLE_VIBRATION | HAPTICS_SELECT
    c[2] = right;
    c[3] = left;
    ds_wrap(c, bt, seq)
}

/// DualShock 4: vibração (só o bit de motores; a cor da barra de luz não é tocada).
pub fn ds4_rumble(bt: bool, left: u8, right: u8) -> Vec<u8> {
    if bt {
        let mut buf = vec![0u8; 13];
        buf[0] = 0x11;
        buf[1] = 0xC0;
        buf[3] = 0x01; // valid_flag0: motores
        buf[6] = right;
        buf[7] = left;
        seal_bt(buf)
    } else {
        let mut buf = vec![0u8; 32];
        buf[0] = 0x05;
        buf[1] = 0x01;
        buf[4] = right;
        buf[5] = left;
        buf
    }
}

/// "#RRGGBB" → (r, g, b).
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_known_vector() {
        // CRC32 IEEE de "123456789" é 0xCBF43926; aqui a semente 0xA2 vem antes, então testamos a
        // própria função contra o cálculo direto sem semente para garantir o algoritmo.
        fn plain(d: &[u8]) -> u32 {
            let mut c: u32 = 0xFFFF_FFFF;
            for &b in d { c ^= b as u32; for _ in 0..8 { c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }; } }
            !c
        }
        assert_eq!(plain(b"123456789"), 0xCBF4_3926);
        let mut with_seed = vec![0xA2];
        with_seed.extend_from_slice(b"abc");
        assert_eq!(bt_crc32(b"abc"), plain(&with_seed));
    }

    #[test]
    fn dualsense_layouts() {
        let usb = dualsense_color(false, 0, (1, 2, 3));
        assert_eq!((usb.len(), usb[0], usb[2], usb[45], usb[46], usb[47]), (48, 0x02, 0x04, 1, 2, 3));
        let rel = dualsense_release(false, 0);
        assert_eq!((rel[39], rel[42]), (0x02, 0x02));
        let bt = dualsense_color(true, 3, (9, 8, 7));
        assert_eq!((bt.len(), bt[0], bt[1], bt[2], bt[4], bt[47], bt[48], bt[49]), (78, 0x31, 0x30, 0x10, 0x04, 9, 8, 7));
        assert_eq!(u32::from_le_bytes(bt[74..78].try_into().unwrap()), bt_crc32(&bt[..74]));
    }

    #[test]
    fn ds4_layouts() {
        let usb = ds4_color(false, (10, 20, 30));
        assert_eq!((usb.len(), usb[0], usb[1], usb[6], usb[7], usb[8]), (32, 0x05, 0x02, 10, 20, 30));
        let bt = ds4_color(true, (10, 20, 30));
        assert_eq!((bt.len(), bt[0], bt[1], bt[3], bt[8], bt[9], bt[10]), (78, 0x11, 0xC0, 0x02, 10, 20, 30));
    }

    #[test]
    fn rumble_layouts_touch_only_motors() {
        let ds = dualsense_rumble(false, 0, 200, 100);
        assert_eq!((ds.len(), ds[1], ds[2], ds[3], ds[4]), (48, 0x03, 0, 100, 200));
        assert_eq!(ds[39], 0, "não mexe na configuração da barra de luz");
        let usb = ds4_rumble(false, 200, 100);
        assert_eq!((usb[1], usb[4], usb[5], usb[6]), (0x01, 100, 200, 0));
        let bt = ds4_rumble(true, 200, 100);
        assert_eq!((bt.len(), bt[3], bt[6], bt[7]), (78, 0x01, 100, 200));
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex("#3fa82c"), Some((0x3f, 0xa8, 0x2c)));
        assert_eq!(parse_hex("zzz"), None);
    }
}
