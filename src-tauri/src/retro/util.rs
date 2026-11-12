//! Utilidades para validar respostas da API (números às vezes chegam como texto) e para datas.
//! Todas as datas gravadas no cache ficam em UTC no formato do SQLite: "AAAA-MM-DD HH:MM:SS".

use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Inteiro de um campo que pode vir como número ou texto ("10"). Ausente/inválido → 0.
pub fn int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(0),
        Value::Bool(b) => *b as i64,
        _ => 0,
    }
}

/// Texto de um campo (números viram texto); ausente → "".
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn opt_text(v: &Value) -> Option<String> {
    let s = text(v);
    (!s.is_empty()).then_some(s)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "2024-03-10 18:22:11", "2024-03-10T18:22:11+00:00", "2024-03-10T18:22:11.000000Z" → epoch (UTC).
/// Sem fuso explícito, a data é tratada como UTC (padrão da API do RetroAchievements).
pub fn parse_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (mut h, mut mi, mut se) = (0, 0, 0);
    let mut rest = "";
    if s.len() >= 19 {
        h = num(11, 13)?;
        mi = num(14, 16)?;
        se = num(17, 19)?;
        rest = &s[19..];
    }
    // fração de segundos
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let mut offset = 0i64;
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let digits: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 4 {
            let oh: i64 = digits[0..2].parse().ok()?;
            let om: i64 = digits[2..4].parse().ok()?;
            offset = (oh * 3600 + om * 60) * if sign == '-' { -1 } else { 1 };
        }
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se - offset)
}

pub fn fmt_epoch(e: i64) -> String {
    let (y, m, d) = civil_from_days(e.div_euclid(86400));
    let r = e.rem_euclid(86400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", r / 3600, (r % 3600) / 60, r % 60)
}

/// Normaliza qualquer data da API para o formato do cache; inválida/ausente → `None`.
pub fn norm_date(v: &Value) -> Option<String> {
    parse_epoch(&text(v)).map(fmt_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dates_roundtrip_and_offsets() {
        let e = parse_epoch("2024-03-10 18:22:11").unwrap();
        assert_eq!(fmt_epoch(e), "2024-03-10 18:22:11");
        assert_eq!(parse_epoch("2024-03-10T18:22:11+00:00"), Some(e));
        assert_eq!(parse_epoch("2024-03-10T15:22:11-03:00"), Some(e));
        assert_eq!(parse_epoch("2024-03-10T18:22:11.000000Z"), Some(e));
        assert_eq!(parse_epoch("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_epoch(""), None);
        assert_eq!(parse_epoch("lixo"), None);
        assert_eq!(norm_date(&json!(null)), None);
    }

    #[test]
    fn lenient_numbers() {
        assert_eq!(int(&json!("10")), 10);
        assert_eq!(int(&json!(5)), 5);
        assert_eq!(int(&json!(null)), 0);
        assert_eq!(int(&json!("x")), 0);
        assert_eq!(text(&json!(12)), "12");
    }
}
