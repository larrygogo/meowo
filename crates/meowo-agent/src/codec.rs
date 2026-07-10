//! 通用编解码：base64url、JWT payload、Unix 秒 → ISO 8601。
//!
//! 不是任何 agent 的专属知识——codex 与 kimi 都要解 JWT 里的账号 claims、都要把重置时间戳格式化。
//! 此前它们住在 codex 的 account 里、由 kimi 经 `super::codex::` 横向引用；一个插件依赖另一个插件
//! 是错的方向，故提到公共层。

use serde_json::Value;

/// base64url（无填充）→ bytes。字符集：A-Z a-z 0-9 - _。
/// 畸形字符或长度不合法时返回 None。
pub fn base64url_decode_nopad(s: &str) -> Option<Vec<u8>> {
    // base64url 字符 → 6-bit 值
    fn char_val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return Some(vec![]);
    }

    // 计算输出长度：每 4 个输入字符 → 3 字节（末尾可差 1-2 个）
    let out_len = (n * 6) / 8;
    let mut out = Vec::with_capacity(out_len);

    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        let v = char_val(b)?;
        buf = (buf << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    Some(out)
}

/// JWT 三段 base64url 中段（payload）→ `serde_json::Value`。
/// **不验签**，仅解码展示。缺段、畸形、非 JSON 均返回 None。
pub fn decode_jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.splitn(3, '.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let bytes = base64url_decode_nopad(payload_b64)?;
    serde_json::from_slice(&bytes).ok()
}

/// Unix 秒 → ISO 8601 UTC 字符串（如 "2025-06-30T12:00:00Z"）。
/// 不引 chrono，手写正确的格里历算法（Howard Hinnant civil_from_days）。
pub fn unix_to_iso8601(ts: i64) -> String {
    let secs = ts.max(0) as u64;
    let (days_total, rem) = (secs / 86400, secs % 86400);
    let (h, rem2) = (rem / 3600, rem % 3600);
    let (m, s) = (rem2 / 60, rem2 % 60);
    let (year, month, day) = days_to_ymd(days_total);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// 将自 1970-01-01 起的天数转换为 (year, month, day)（格里历）。
/// 实现来自 Howard Hinnant civil_from_days 算法，正确处理所有闰年。
fn days_to_ymd(z: u64) -> (u64, u64, u64) {
    // 平移到以公元 0 年 3 月 1 日为纪元（消除闰年处理中的边界复杂度）
    let z = z as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // 纪元内天数 [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // 纪元内年 [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 年内天数（3月1日起）[0, 365]
    let mp = (5 * doy + 2) / 153; // 月内序号 [0, 11]（3月=0，2月=11）
    let d = doy - (153 * mp + 2) / 5 + 1; // 日 [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // 月 [1, 12]（校正回 Jan=1）
    let y = if m <= 2 { y + 1 } else { y }; // 1/2 月属于下一年（按 3 月起算时）
    (y as u64, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_decodes_without_padding() {
        assert_eq!(base64url_decode_nopad(""), Some(vec![]));
        assert_eq!(base64url_decode_nopad("TWFu").unwrap(), b"Man".to_vec());
        // 畸形字符（标准 base64 的 '+' / '/' 不属于 base64url）。
        assert!(base64url_decode_nopad("a+b").is_none());
        assert!(base64url_decode_nopad("a/b").is_none());
    }

    #[test]
    fn decode_jwt_payload_reads_claims_without_verifying() {
        // {"email":"a@b.com"} 的 base64url，无填充。
        let payload = "eyJlbWFpbCI6ImFAYi5jb20ifQ";
        let token = format!("header.{payload}.signature");
        let v = decode_jwt_payload(&token).expect("应解出 payload");
        assert_eq!(v.get("email").and_then(|x| x.as_str()), Some("a@b.com"));
        // 缺段 / 非 JSON → None。
        assert!(decode_jwt_payload("onlyone").is_none());
        assert!(decode_jwt_payload("h.bm90anNvbg.s").is_none());
    }

    #[test]
    fn unix_to_iso8601_handles_epoch_and_leap_years() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
        // 2024-02-29（闰日）00:00:00 UTC = 1709164800
        assert_eq!(unix_to_iso8601(1_709_164_800), "2024-02-29T00:00:00Z");
        // 2000-03-01（世纪闰年之后）= 951868800
        assert_eq!(unix_to_iso8601(951_868_800), "2000-03-01T00:00:00Z");
        // 负数钳到纪元。
        assert_eq!(unix_to_iso8601(-5), "1970-01-01T00:00:00Z");
    }
}
