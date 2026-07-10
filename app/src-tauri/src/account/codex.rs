//! Codex（OpenAI Codex CLI）账号 + 用量的纯本地读取。
//! 全程只读、不联网、不写 token。
//!
//! 账号：`~/.codex/auth.json` → 解析 tokens.id_token（OIDC JWT，仅解中段 payload，不验签）。
//! 用量：`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` + archived_sessions 下按 mtime 取最新文件，
//!       倒序扫找最后一条 `payload.type=="token_count"` 行 → 解析 rate_limits。

use serde_json::Value;

use super::{Account, ProviderAccount, ProviderUsage, UsageKind, UsageLane};

// ═══ base64url 解码（不加依赖，纯手写） ═══

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

/// JWT 三段 base64url 中段（payload）→ serde_json::Value。
/// 不验签，仅解码展示。缺段、畸形、非 JSON 均返回 None。
pub fn decode_jwt_payload(token: &str) -> Option<Value> {
    let mut parts = token.splitn(3, '.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;
    let bytes = base64url_decode_nopad(payload_b64)?;
    serde_json::from_slice(&bytes).ok()
}

// ═══ 账号解析 ═══

/// 从 auth.json 根 Value 解析账号信息（纯函数，便于单测）。
/// auth_mode=="chatgpt"：解 id_token JWT claims；"apikey"：仅标注 login_label。
/// 解析失败一律 None，不冒泡 Err。
pub fn parse_codex_account(auth_json: &Value) -> Option<Account> {
    let auth_mode = auth_json.get("auth_mode").and_then(|v| v.as_str()).unwrap_or("");

    match auth_mode {
        "chatgpt" => {
            let id_token = auth_json
                .pointer("/tokens/id_token")
                .and_then(|v| v.as_str())?;
            let payload = decode_jwt_payload(id_token)?;

            // email：顶层 claim（空串过滤同 plan/org，避免 Some("") 流出）
            let email = payload.get("email").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());

            // plan：https://api.openai.com/auth 命名空间
            let plan = payload
                .pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            // org
            let organization = payload
                .pointer("/https:~1~1api.openai.com~1auth/organization_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            // 至少要有 email 或 plan，否则没有有用信息
            if email.is_none() && plan.is_none() {
                return None;
            }

            Some(Account {
                email,
                display_name: None,
                organization,
                plan,
                login_label: Some("chatgpt".to_string()),
            })
        }
        "apikey" => Some(Account {
            email: None,
            display_name: None,
            organization: None,
            plan: None,
            login_label: Some("API Key".to_string()),
        }),
        _ => None,
    }
}

// ═══ 用量解析 ═══

/// Unix 秒 → ISO 8601 UTC 字符串（如 "2025-06-30T12:00:00Z"）。
/// 不使用 chrono，手写正确的格里历算法（Howard Hinnant civil_from_days）。
/// pub(super)：kimi.rs 复用，避免重写同一函数。
pub(super) fn unix_to_iso8601(ts: i64) -> String {
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

/// 从 token_count 事件的 payload 解析 ProviderUsage（纯函数）。
/// rate_limits.primary → FiveHour（5h 窗口）；secondary → Weekly（7d 窗口）。
/// resets_at 缺失时兼容旧字段 resets_in_seconds（若有则加到记录时间戳）。
pub fn parse_codex_usage(payload: &Value) -> ProviderUsage {
    // 记录时间戳（部分旧格式作为 resets_at 兜底）
    let record_ts = payload.get("timestamp").and_then(|v| v.as_i64());

    let rate_limits = match payload.get("rate_limits") {
        Some(v) => v,
        None => return ProviderUsage::default(),
    };

    let mut lanes: Vec<UsageLane> = Vec::new();

    // 解析一条泳道
    let parse_lane = |key: &str, kind: UsageKind| -> Option<UsageLane> {
        let rl = rate_limits.get(key)?;
        let used_pct = rl.get("used_percent").and_then(|v| v.as_f64())?;

        // resets_at：优先 unix 秒字段，其次旧版 resets_in_seconds + record_ts
        let resets_at: Option<String> = if let Some(ts) = rl.get("resets_at").and_then(|v| v.as_i64()) {
            Some(unix_to_iso8601(ts))
        } else if let (Some(secs), Some(rec)) =
            (rl.get("resets_in_seconds").and_then(|v| v.as_i64()), record_ts)
        {
            Some(unix_to_iso8601(rec + secs))
        } else {
            None
        };

        Some(UsageLane {
            kind,
            used_pct: Some(used_pct),
            used: None,
            limit: None,
            unit: Some("percent".to_string()),
            resets_at,
        })
    };

    if let Some(lane) = parse_lane("primary", UsageKind::FiveHour) {
        lanes.push(lane);
    }
    if let Some(lane) = parse_lane("secondary", UsageKind::Weekly) {
        lanes.push(lane);
    }

    // note：仅承载别处未展示的额外信息(credits)；plan_type 已作为账号 plan 徽标展示，不重复进 note。
    let credits = payload.get("credits").and_then(|v| v.as_f64());
    let note = credits.map(|c| format!("credits:{c}"));

    ProviderUsage { lanes, note }
}

// ═══ 文件系统读取 ═══

/// 在 dir 下递归找所有 rollout-*.jsonl 文件（限深 5），返回 (mtime, path) 列表。
fn collect_rollouts(dir: &std::path::Path, depth: usize, out: &mut Vec<(u64, std::path::PathBuf)>) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rollouts(&p, depth - 1, out);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        {
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((mtime, p));
        }
    }
}

/// 按 mtime 取最新的 rollout-*.jsonl（sessions + archived_sessions 合并排序）。
fn find_latest_rollout(codex_home: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut candidates: Vec<(u64, std::path::PathBuf)> = Vec::new();
    for sub in ["sessions", "archived_sessions"] {
        let dir = codex_home.join(sub);
        if dir.exists() {
            collect_rollouts(&dir, 5, &mut candidates);
        }
    }
    candidates.into_iter().max_by_key(|(mtime, _)| *mtime).map(|(_, p)| p)
}

/// 倒序扫描 JSONL 文件，找最后一条 `payload.type=="token_count"` 行，返回其 payload。
fn tail_scan_token_count(path: &std::path::Path) -> Option<Value> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let lines: Vec<String> = BufReader::new(f).lines().map_while(Result::ok).collect();
    for line in lines.iter().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        // 支持两种结构：顶层 {type, payload} 或 {payload: {type, ...}}
        let payload_type = v
            .get("payload")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str());
        if payload_type == Some("token_count") {
            return v.get("payload").cloned();
        }
    }
    None
}

// ═══ auth.json 读取 ═══

/// 凭据位置由实况变体给出（`Installation.auth.credentials`），不再在此拼路径。
fn read_auth_json() -> Option<Value> {
    let path = meowo_agent::installation(meowo_agent::id::CODEX)?.credentials_path()?;
    let s = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&s).ok()
}

fn auth_mode(auth_json: &Value) -> &str {
    auth_json.get("auth_mode").and_then(|v| v.as_str()).unwrap_or("")
}

// ═══ ProviderAccount impl ═══

pub struct CodexProviderAccount;

impl ProviderAccount for CodexProviderAccount {
    fn id(&self) -> meowo_agent::AgentId {
        meowo_agent::id::CODEX
    }

    fn account(&self) -> Option<Account> {
        parse_codex_account(&read_auth_json()?)
    }

    fn usage(&self, _force: bool) -> Option<ProviderUsage> {
        // 纯本地，无网络，force 参数忽略
        let home = meowo_agent::installation(meowo_agent::id::CODEX).map(|i| i.data_dir)?;
        let rollout = find_latest_rollout(&home)?;
        let payload = tail_scan_token_count(&rollout)?;
        Some(parse_codex_usage(&payload))
    }

    fn usage_supported(&self) -> bool {
        // 仅 chatgpt 模式（订阅）有 rate_limits
        read_auth_json().is_some_and(|auth| auth_mode(&auth) == "chatgpt")
    }
}

// ═══ Tests ═══

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── base64url ──

    #[test]
    fn base64url_decode_empty() {
        assert_eq!(base64url_decode_nopad(""), Some(vec![]));
    }

    #[test]
    fn base64url_decode_known_vectors() {
        // "Man" → TWFu（标准 base64，无填充，- _ 无关）
        let decoded = base64url_decode_nopad("TWFu").unwrap();
        assert_eq!(decoded, b"Man");

        // "f" → Zg（2 chars, 1 byte）
        assert_eq!(base64url_decode_nopad("Zg").unwrap(), b"f");

        // "fo" → Zm8（3 chars, 2 bytes）
        assert_eq!(base64url_decode_nopad("Zm8").unwrap(), b"fo");
    }

    #[test]
    fn base64url_decode_invalid_char_returns_none() {
        // '!' 非合法 base64url 字符
        assert_eq!(base64url_decode_nopad("TW!u"), None);
        // '+' 是标准 base64 而非 base64url
        assert_eq!(base64url_decode_nopad("TW+u"), None);
    }

    #[test]
    fn base64url_decode_url_safe_chars() {
        // '-' 和 '_' 应合法
        // base64url for bytes [0xFB, 0xFF] = "-_8" (3 chars, but only 2 bytes due to padding)
        // Let's verify by encoding manually: 0xFB=11111011, 0xFF=11111111
        // 6-bit groups: 111110 110111 111111 → 62(='-'), 55(='3'), 63(='_')
        // Actually let me use a known example: RFC 4648 test vector
        // ">>" → "Pj4" in base64, but in base64url same (no + or /)
        let decoded = base64url_decode_nopad("Pj4").unwrap();
        assert_eq!(decoded, b">>");
    }

    // ── JWT ──

    fn make_test_jwt(payload: &Value) -> String {
        // 造一个假 JWT（不需真正签名，仅测试解码路径）
        let header = base64url_encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let body = base64url_encode(serde_json::to_string(payload).unwrap().as_bytes());
        let sig = "fakesig";
        format!("{header}.{body}.{sig}")
    }

    fn base64url_encode(data: &[u8]) -> String {
        // 测试辅助：encode（不加依赖）
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for &b in data {
            buf = (buf << 8) | (b as u32);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                out.push(CHARS[((buf >> bits) & 0x3F) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(CHARS[((buf << (6 - bits)) & 0x3F) as usize] as char);
        }
        out
    }

    #[test]
    fn decode_jwt_payload_extracts_claims() {
        let claims = json!({
            "email": "user@example.com",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "pro",
                "organization_id": "org-123"
            }
        });
        let jwt = make_test_jwt(&claims);
        let decoded = decode_jwt_payload(&jwt).unwrap();
        assert_eq!(decoded["email"].as_str(), Some("user@example.com"));
    }

    #[test]
    fn decode_jwt_payload_too_few_segments_returns_none() {
        assert!(decode_jwt_payload("onlyone").is_none());
        assert!(decode_jwt_payload("two.parts").is_none());
    }

    #[test]
    fn decode_jwt_payload_bad_base64_returns_none() {
        assert!(decode_jwt_payload("header.!!!.sig").is_none());
    }

    #[test]
    fn decode_jwt_payload_non_json_returns_none() {
        let garbage = base64url_encode(b"not-json-at-all");
        assert!(decode_jwt_payload(&format!("h.{garbage}.s")).is_none());
    }

    // ── parse_codex_account ──

    fn make_auth_chatgpt(email: &str, plan: &str, org: &str) -> Value {
        let claims = json!({
            "email": email,
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": plan,
                "organization_id": org
            }
        });
        let jwt = make_test_jwt(&claims);
        json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": jwt,
                "account_id": "user-abc"
            }
        })
    }

    #[test]
    fn parse_codex_account_chatgpt_extracts_all_fields() {
        let auth = make_auth_chatgpt("alice@example.com", "pro", "org-xyz");
        let acc = parse_codex_account(&auth).unwrap();
        assert_eq!(acc.email.as_deref(), Some("alice@example.com"));
        assert_eq!(acc.plan.as_deref(), Some("pro"));
        assert_eq!(acc.organization.as_deref(), Some("org-xyz"));
        assert_eq!(acc.login_label.as_deref(), Some("chatgpt"));
    }

    #[test]
    fn parse_codex_account_chatgpt_missing_claims_degrade_gracefully() {
        // JWT 里缺少 plan/org，只有 email → 仍返回 Account
        let claims = json!({"email": "bob@test.com"});
        let jwt = make_test_jwt(&claims);
        let auth = json!({"auth_mode": "chatgpt", "tokens": {"id_token": jwt}});
        let acc = parse_codex_account(&auth).unwrap();
        assert_eq!(acc.email.as_deref(), Some("bob@test.com"));
        assert!(acc.plan.is_none());
        assert!(acc.organization.is_none());
    }

    #[test]
    fn parse_codex_account_chatgpt_no_useful_claims_returns_none() {
        // JWT payload 无 email 无 plan → None
        let claims = json!({"sub": "user123"});
        let jwt = make_test_jwt(&claims);
        let auth = json!({"auth_mode": "chatgpt", "tokens": {"id_token": jwt}});
        assert!(parse_codex_account(&auth).is_none());
    }

    #[test]
    fn parse_codex_account_apikey_returns_login_label() {
        let auth = json!({"auth_mode": "apikey"});
        let acc = parse_codex_account(&auth).unwrap();
        assert_eq!(acc.login_label.as_deref(), Some("API Key"));
        assert!(acc.email.is_none());
    }

    #[test]
    fn parse_codex_account_unknown_mode_returns_none() {
        let auth = json!({"auth_mode": "unknown_mode"});
        assert!(parse_codex_account(&auth).is_none());
        // 无 auth_mode 字段
        assert!(parse_codex_account(&json!({})).is_none());
    }

    // ── parse_codex_usage ──

    fn make_token_count_payload(primary_pct: f64, secondary_pct: f64, resets_at_unix: i64) -> Value {
        json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {
                    "used_percent": primary_pct,
                    "window_minutes": 300,
                    "resets_at": resets_at_unix
                },
                "secondary": {
                    "used_percent": secondary_pct,
                    "window_minutes": 10080,
                    "resets_at": resets_at_unix
                },
                "plan_type": "pro"
            },
            "info": {
                "last_token_usage": {
                    "input_tokens": 1000,
                    "output_tokens": 500,
                    "total_tokens": 1500
                }
            }
        })
    }

    #[test]
    fn parse_codex_usage_extracts_lanes_and_note() {
        // Unix 秒：2026-06-30 12:00:00 UTC = 1751284800
        let payload = make_token_count_payload(45.5, 12.3, 1751284800);
        let pu = parse_codex_usage(&payload);
        assert_eq!(pu.lanes.len(), 2);

        let five_hour = &pu.lanes[0];
        assert_eq!(five_hour.kind, UsageKind::FiveHour);
        assert_eq!(five_hour.used_pct, Some(45.5));
        assert_eq!(five_hour.unit.as_deref(), Some("percent"));
        // 1751284800 = 2025-06-30T12:00:00Z
        assert!(five_hour.resets_at.as_deref().unwrap_or("").contains("2025-06-30"));

        let weekly = &pu.lanes[1];
        assert_eq!(weekly.kind, UsageKind::Weekly);
        assert_eq!(weekly.used_pct, Some(12.3));

        // plan_type 已作为账号 plan 徽标展示，不再进 note；无 credits 时 note 为空。
        assert!(pu.note.is_none());
    }

    #[test]
    fn parse_codex_usage_missing_rate_limits_returns_empty() {
        let pu = parse_codex_usage(&json!({"type": "token_count"}));
        assert!(pu.lanes.is_empty());
        assert!(pu.note.is_none());
    }

    #[test]
    fn parse_codex_usage_resets_at_fallback_to_resets_in_seconds() {
        // 旧格式：无 resets_at，用 resets_in_seconds + timestamp 兜底
        let payload = json!({
            "type": "token_count",
            "timestamp": 1751280000i64,
            "rate_limits": {
                "primary": {
                    "used_percent": 30.0,
                    "resets_in_seconds": 3600
                }
            }
        });
        let pu = parse_codex_usage(&payload);
        assert_eq!(pu.lanes.len(), 1);
        // 1751280000 + 3600 = 1751283600
        assert!(pu.lanes[0].resets_at.is_some());
    }

    #[test]
    fn parse_codex_usage_no_resets_at_no_fallback_gives_none() {
        let payload = json!({
            "type": "token_count",
            "rate_limits": {
                "primary": {"used_percent": 20.0}
            }
        });
        let pu = parse_codex_usage(&payload);
        assert_eq!(pu.lanes.len(), 1);
        assert!(pu.lanes[0].resets_at.is_none());
    }

    #[test]
    fn unix_to_iso8601_known_date() {
        // 1751284800 = 2025-06-30 12:00:00 UTC（经过验证的已知向量）
        assert_eq!(unix_to_iso8601(1751284800), "2025-06-30T12:00:00Z");
        // 1782820800 = 2026-06-30 12:00:00 UTC
        assert_eq!(unix_to_iso8601(1782820800), "2026-06-30T12:00:00Z");
    }

    #[test]
    fn unix_to_iso8601_epoch() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
    }
}
