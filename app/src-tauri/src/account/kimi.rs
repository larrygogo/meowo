//! Kimi Code 账号 + 用量（best-effort 容错）实现。
//!
//! **账号**：读 kimi_share_dir()/credentials/kimi-code.json 的 access_token，
//!   尝试 decode_jwt_payload 取 email claim（只读、不打印 token）；无 email →
//!   login_label="已登录 · managed:kimi-code"。凭据文件不存在 → None。
//!
//! **用量**：GET {base_url}/usages，8s 超时，不刷新 token，不写回凭据。
//!   任何非 2xx / 网络错 / 解析失败 → 安静降级 None，不崩溃、不影响其它 provider。
//!   容错解析 parse_kimi_usage：支持多种推断 schema，字段漂移 used↔remaining、
//!   resetAt↔reset_at↔reset_in/ttl。

use serde_json::Value;
use std::time::Duration;

use super::{Account, ProviderAccount, ProviderUsage, UsageKind, UsageLane};

const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_BASE_URL: &str = "https://api.kimi.com/coding/v1";
const USER_AGENT: &str = concat!("cc-kanban/", env!("CARGO_PKG_VERSION"));

// ═══ 路径工具 ═══

fn kimi_credentials_path() -> Option<std::path::PathBuf> {
    Some(cc_reporter::kimi::kimi_share_dir()?.join("credentials").join("kimi-code.json"))
}

fn read_kimi_credentials() -> Option<Value> {
    super::read_json(&kimi_credentials_path()?)
}

fn read_device_id() -> Option<String> {
    let path = cc_reporter::kimi::kimi_share_dir()?.join("device_id");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ═══ 配置读取 ═══

/// 读 base_url：env KIMI_CODE_BASE_URL > kimi_share_dir/config.toml > 缺省。
fn kimi_base_url() -> String {
    // env 覆盖优先
    if let Ok(url) = std::env::var("KIMI_CODE_BASE_URL") {
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.is_empty() {
            return url;
        }
    }
    // config.toml 简单解析（best-effort）
    if let Some(url) = read_config_base_url() {
        return url;
    }
    DEFAULT_BASE_URL.to_string()
}

/// 从 kimi_share_dir()/config.toml 简单逐行解析 [providers."managed:kimi-code"].base_url。
/// 不引入 toml 依赖，best-effort，失败返回 None。
fn read_config_base_url() -> Option<String> {
    let path = cc_reporter::kimi::kimi_share_dir()?.join("config.toml");
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.contains("providers") && trimmed.contains("managed:kimi-code");
            continue;
        }
        if !in_section {
            continue;
        }
        // 跳过注释行
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("base_url") {
            if let Some(after_eq) = rest.trim_start().strip_prefix('=') {
                let url = after_eq.trim().trim_matches('"').trim_end_matches('/').to_string();
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
    }
    None
}

// ═══ 用量解析（纯函数） ═══

/// 复用 codex 模块的 unix 秒 → ISO 8601 实现。
fn unix_to_iso8601(ts: i64) -> String {
    super::codex::unix_to_iso8601(ts)
}

/// 解析 resetAt 字段族，兼容多种形态（容错）。
/// - resetAt / reset_at：字符串→原样；数字→unix→ISO。
/// - reset_in / ttl：秒数偏移，now+secs→ISO。
fn parse_resets_at(v: &Value) -> Option<String> {
    for key in &["resetAt", "reset_at"] {
        if let Some(val) = v.get(key) {
            if let Some(s) = val.as_str() {
                return Some(s.to_string());
            }
            // 数字（可能带小数）→ unix 秒
            if let Some(ts) = val.as_i64().or_else(|| val.as_f64().map(|f| f as i64)) {
                return Some(unix_to_iso8601(ts));
            }
        }
    }
    // reset_in / ttl：从现在起的秒数偏移（兼容浮点秒，与 resetAt 数字分支一致）
    for key in &["reset_in", "ttl"] {
        if let Some(secs) = v.get(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            return Some(unix_to_iso8601(now + secs));
        }
    }
    None
}

/// 从含 used/remaining/limit 的对象提取 (used, limit)。
/// 字段漂移容错：优先 used；无 used 时从 remaining 反推（used = limit - remaining）。
fn extract_used_limit(v: &Value) -> Option<(f64, f64)> {
    let limit = v.get("limit").and_then(|x| x.as_f64())?;
    let used = if let Some(u) = v.get("used").and_then(|x| x.as_f64()) {
        u
    } else if let Some(r) = v.get("remaining").and_then(|x| x.as_f64()) {
        limit - r
    } else {
        return None;
    };
    Some((used, limit))
}

/// window {duration, timeUnit} → UsageKind（统一换算成小时比较，容错返回 Other）。
fn window_to_kind(duration: f64, time_unit: &str) -> UsageKind {
    let hours = match time_unit.to_ascii_uppercase().as_str() {
        "MINUTE" => duration / 60.0,
        "HOUR" => duration,
        "DAY" => duration * 24.0,
        _ => return UsageKind::Other,
    };
    // ~5h → FiveHour，~168h（7d）→ SevenDay，其余 → Other
    if (hours - 5.0).abs() < 1.0 {
        UsageKind::FiveHour
    } else if (hours - 168.0).abs() < 1.0 {
        UsageKind::SevenDay
    } else {
        UsageKind::Other
    }
}

/// 解析顶层 usage 对象 → Weekly lane。
fn parse_usage_object(usage: &Value) -> Option<UsageLane> {
    let (used, limit) = extract_used_limit(usage)?;
    let used_pct = if limit > 0.0 { Some(used / limit * 100.0) } else { None };
    let resets_at = parse_resets_at(usage);
    Some(UsageLane {
        kind: UsageKind::Weekly,
        used_pct,
        used: Some(used),
        limit: Some(limit),
        unit: Some("tokens".to_string()),
        resets_at,
    })
}

/// 解析 limits[] 单项 → lane（按 window 派生类型）。
fn parse_limit_item(item: &Value) -> Option<UsageLane> {
    let detail = item.get("detail")?;
    let window = item.get("window")?;
    let time_unit = window.get("timeUnit").and_then(|v| v.as_str()).unwrap_or("");
    let duration = window.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let kind = window_to_kind(duration, time_unit);

    let (used, limit) = extract_used_limit(detail)?;
    let used_pct = if limit > 0.0 { Some(used / limit * 100.0) } else { None };
    let resets_at = parse_resets_at(detail);

    Some(UsageLane {
        kind,
        used_pct,
        used: Some(used),
        limit: Some(limit),
        unit: Some("tokens".to_string()),
        resets_at,
    })
}

/// 从 /usages 响应解析 ProviderUsage（纯函数，容错，解析不出任何 lane → None）。
///
/// 支持三种推断 schema（容错，同时存在时叠加）：
/// - **Schema A** `{usage:{name,used,limit,resetAt}}` → Weekly lane
/// - **Schema B** `{limits:[{detail:{used,limit},window:{duration,timeUnit}}]}` → 按 window 派生 lane
/// - **Schema C** `{data:{available_balance:n}}` → Balance lane（unit="usd"，used_pct=None）
///
/// 字段漂移容错：
/// - `used ↔ remaining`（remaining 时 used=limit-remaining）
/// - `resetAt ↔ reset_at`（字符串→原样；数字→unix→ISO）
/// - `reset_in / ttl`（秒数→now+secs→ISO）
pub fn parse_kimi_usage(v: &Value) -> Option<ProviderUsage> {
    let mut lanes: Vec<UsageLane> = Vec::new();

    // Schema A: 顶层 usage 对象 → Weekly
    if let Some(usage) = v.get("usage") {
        if let Some(lane) = parse_usage_object(usage) {
            lanes.push(lane);
        }
    }

    // Schema B: limits 数组 → 按 window 派生
    if let Some(arr) = v.get("limits").and_then(|a| a.as_array()) {
        for item in arr {
            if let Some(lane) = parse_limit_item(item) {
                lanes.push(lane);
            }
        }
    }

    // Schema C: open-platform 余额
    if let Some(balance) = v.pointer("/data/available_balance").and_then(|b| b.as_f64()) {
        lanes.push(UsageLane {
            kind: UsageKind::Balance,
            used_pct: None,
            used: Some(balance),
            limit: None,
            unit: Some("usd".to_string()),
            resets_at: None,
        });
    }

    if lanes.is_empty() { None } else { Some(ProviderUsage { lanes, note: None }) }
}

// ═══ 联网取用量 ═══

/// 联网拉 GET {base}/usages（不刷新 token，不写回凭据）。
/// 任何非 2xx / 网络错 / 解析失败 → None（安静降级）。
fn fetch_kimi_usage_live() -> Option<ProviderUsage> {
    let creds = read_kimi_credentials()?;
    let access_token = creds.get("access_token").and_then(|v| v.as_str())?.to_string();
    let base = kimi_base_url();
    let url = format!("{base}/usages");

    let device_id = read_device_id();

    let req = ureq::get(&url)
        .timeout(HTTP_TIMEOUT)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .set("User-Agent", USER_AGENT)
        .set("X-Msh-Platform", "kimi_code_cli");

    // best-effort 附设备头（读不到就不带）
    let req = match device_id.as_deref() {
        Some(did) => req.set("X-Msh-Device-Id", did),
        None => req,
    };

    // 任何非 2xx / 网络错 → None（ureq 2.x 在 4xx/5xx 时返回 Err）
    let resp = req.call().ok()?;
    let v: Value = resp.into_json().ok()?;
    parse_kimi_usage(&v)
}

// ═══ ProviderAccount impl ═══

pub struct KimiProviderAccount;

impl ProviderAccount for KimiProviderAccount {
    fn key(&self) -> cc_store::ProviderKey {
        cc_store::ProviderKey::Kimi
    }

    /// best-effort 读账号：解 JWT email claim 或降级「已登录」标签。凭据不存在 → None。
    fn account(&self) -> Option<Account> {
        let creds = read_kimi_credentials()?;
        let access_token = creds.get("access_token").and_then(|v| v.as_str())?;
        // 只读 JWT claim、不打印 token 原文
        let payload = super::codex::decode_jwt_payload(access_token);
        let email = payload.as_ref().and_then(|p| {
            p.get("email")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && s.contains('@'))
                .map(|s| s.to_string())
        });

        if let Some(email_str) = email {
            // 拿到 email → 正常账号
            Some(Account {
                email: Some(email_str),
                display_name: None,
                organization: None,
                plan: None,
                login_label: None,
            })
        } else {
            // 解不出 email → 降级「已登录」标签
            Some(Account {
                email: None,
                display_name: None,
                organization: None,
                plan: None,
                login_label: Some("已登录 · managed:kimi-code".to_string()),
            })
        }
    }

    /// force=false：仅读缓存；force=true：60s 限频后联网（失败降级 None）。
    fn usage(&self, force: bool) -> Option<ProviderUsage> {
        let key = self.key();
        if !force {
            return super::read_cached_usage(key);
        }
        if super::cache_is_fresh(key, 60_000) {
            if let Some(cached) = super::read_cached_usage(key) {
                return Some(cached);
            }
        }
        // 联网拉取，成功写缓存
        fetch_kimi_usage_live().inspect(|pu| {
            super::write_cached_usage(key, pu);
        })
    }

    /// 凭据文件存在即支持（不保证能成功，失败会降级 None）。
    fn usage_supported(&self) -> bool {
        kimi_credentials_path().is_some_and(|p| p.exists())
    }
}

// ═══ Tests ═══

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── parse_kimi_usage · Schema A（顶层 usage 对象）──

    #[test]
    fn schema_a_weekly_basic() {
        let v = json!({
            "usage": {"name": "kimi_tokens", "used": 500, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("schema A 应解析成功");
        assert_eq!(pu.lanes.len(), 1);
        let lane = &pu.lanes[0];
        assert_eq!(lane.kind, UsageKind::Weekly);
        assert!((lane.used_pct.unwrap() - 50.0).abs() < 0.01, "used_pct 应为 50%");
        assert_eq!(lane.used, Some(500.0));
        assert_eq!(lane.limit, Some(1000.0));
        assert_eq!(lane.unit.as_deref(), Some("tokens"));
        assert_eq!(lane.resets_at.as_deref(), Some("2026-06-30T12:00:00Z"));
    }

    #[test]
    fn schema_a_no_used_only_remaining() {
        // 无 used 字段，只有 remaining → used = limit - remaining（漂移容错）
        let v = json!({
            "usage": {"remaining": 700, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("schema A remaining 漂移应解析成功");
        let lane = &pu.lanes[0];
        assert_eq!(lane.used, Some(300.0));
        assert!((lane.used_pct.unwrap() - 30.0).abs() < 0.01, "used_pct 应为 30%");
    }

    #[test]
    fn schema_a_used_takes_priority_over_remaining() {
        // used 和 remaining 同时存在 → used 优先
        let v = json!({
            "usage": {"used": 100, "remaining": 700, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].used, Some(100.0));
        assert!((pu.lanes[0].used_pct.unwrap() - 10.0).abs() < 0.01, "used_pct 应为 10%");
    }

    #[test]
    fn schema_a_reset_at_unix_seconds() {
        // resetAt 为 unix 秒（数字）→ 转 ISO
        let v = json!({
            "usage": {"used": 100, "limit": 1000, "resetAt": 1782820800i64}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(
            pu.lanes[0].resets_at.as_deref().unwrap_or("").contains("2026-06-30"),
            "1782820800 应解析为 2026-06-30"
        );
    }

    #[test]
    fn schema_a_reset_at_alias() {
        // reset_at（下划线别名）
        let v = json!({
            "usage": {"used": 100, "limit": 1000, "reset_at": "2026-07-01T00:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-07-01T00:00:00Z"));
    }

    #[test]
    fn schema_a_reset_in_seconds_offset() {
        // reset_in（秒数偏移）→ now+secs→ISO（只检查 Some，不固定值）
        let v = json!({
            "usage": {"used": 100, "limit": 1000, "reset_in": 3600}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some(), "reset_in 应产生 Some ISO 字符串");
    }

    #[test]
    fn schema_a_ttl_seconds_offset() {
        // ttl（秒数偏移别名）
        let v = json!({
            "usage": {"used": 50, "limit": 500, "ttl": 7200}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some(), "ttl 应产生 Some ISO 字符串");
    }

    #[test]
    fn schema_a_zero_limit_no_pct() {
        // limit=0 → 不计算百分比（避免除零）
        let v = json!({"usage": {"used": 0, "limit": 0, "resetAt": "2026-06-30T00:00:00Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].used_pct.is_none(), "limit=0 时 used_pct 应为 None");
    }

    // ── parse_kimi_usage · Schema B（limits 数组）──

    #[test]
    fn schema_b_five_hour_hour_unit() {
        // HOUR 5 → FiveHour
        let v = json!({
            "limits": [{"detail": {"used": 200, "limit": 400}, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("schema B HOUR 5 应解析成功");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
        assert!((pu.lanes[0].used_pct.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_five_hour_minute_unit() {
        // MINUTE 300 = 5h → FiveHour
        let v = json!({
            "limits": [{"detail": {"used": 50, "limit": 100}, "window": {"duration": 300, "timeUnit": "MINUTE"}}]
        });
        let pu = parse_kimi_usage(&v).expect("schema B MINUTE 300 应解析成功");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
    }

    #[test]
    fn schema_b_seven_day_day_unit() {
        // DAY 7 → SevenDay
        let v = json!({
            "limits": [{"detail": {"used": 1000, "limit": 10000}, "window": {"duration": 7, "timeUnit": "DAY"}}]
        });
        let pu = parse_kimi_usage(&v).expect("schema B DAY 7 应解析成功");
        assert_eq!(pu.lanes[0].kind, UsageKind::SevenDay);
        assert!((pu.lanes[0].used_pct.unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_seven_day_hour_unit() {
        // HOUR 168 = 7d → SevenDay
        let v = json!({
            "limits": [{"detail": {"used": 500, "limit": 5000}, "window": {"duration": 168, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("schema B HOUR 168 应解析成功");
        assert_eq!(pu.lanes[0].kind, UsageKind::SevenDay);
    }

    #[test]
    fn schema_b_remaining_drift_in_detail() {
        // detail 中 remaining 漂移
        let v = json!({
            "limits": [{"detail": {"remaining": 300, "limit": 400}, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("schema B remaining 漂移应解析成功");
        // used = limit - remaining = 400 - 300 = 100
        assert_eq!(pu.lanes[0].used, Some(100.0));
        assert!((pu.lanes[0].used_pct.unwrap() - 25.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_multiple_limits() {
        // 多条 limit → 多条 lane
        let v = json!({
            "limits": [
                {"detail": {"used": 50, "limit": 100}, "window": {"duration": 5, "timeUnit": "HOUR"}},
                {"detail": {"used": 200, "limit": 1000}, "window": {"duration": 7, "timeUnit": "DAY"}}
            ]
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes.len(), 2);
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
        assert_eq!(pu.lanes[1].kind, UsageKind::SevenDay);
    }

    // ── parse_kimi_usage · Schema C（open-platform 余额）──

    #[test]
    fn schema_c_balance() {
        let v = json!({"data": {"available_balance": 5.42}});
        let pu = parse_kimi_usage(&v).expect("schema C 应解析成功");
        assert_eq!(pu.lanes.len(), 1);
        let lane = &pu.lanes[0];
        assert_eq!(lane.kind, UsageKind::Balance);
        assert_eq!(lane.used_pct, None);
        assert_eq!(lane.used, Some(5.42));
        assert_eq!(lane.unit.as_deref(), Some("usd"));
        assert!(lane.resets_at.is_none());
    }

    // ── 混合 / 畸形 ──

    #[test]
    fn mixed_all_schemas() {
        // 三种 schema 同时存在 → 叠加所有 lane
        let v = json!({
            "usage": {"used": 100, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"},
            "limits": [{"detail": {"used": 50, "limit": 100}, "window": {"duration": 5, "timeUnit": "HOUR"}}],
            "data": {"available_balance": 3.0}
        });
        let pu = parse_kimi_usage(&v).expect("混合 schema 应解析成功");
        assert_eq!(pu.lanes.len(), 3);
    }

    #[test]
    fn empty_object_returns_none() {
        assert!(parse_kimi_usage(&json!({})).is_none(), "空对象应返回 None");
    }

    #[test]
    fn malformed_no_limit_returns_none() {
        // usage 缺 limit → extract_used_limit 失败 → schema A 跳过 → None
        let v = json!({"usage": {"used": 100}});
        assert!(parse_kimi_usage(&v).is_none(), "无 limit 应返回 None");
    }

    #[test]
    fn malformed_no_used_no_remaining_returns_none() {
        // 无 used 也无 remaining
        let v = json!({"usage": {"limit": 1000}});
        assert!(parse_kimi_usage(&v).is_none(), "无 used/remaining 应返回 None");
    }

    #[test]
    fn malformed_limits_no_detail_skipped() {
        // limits 项缺 detail → 该项跳过；数组整体为空 → None
        let v = json!({
            "limits": [{"window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        assert!(parse_kimi_usage(&v).is_none(), "缺 detail 应跳过并返回 None");
    }

    // ── window_to_kind 内部分支全覆盖 ──

    #[test]
    fn window_to_kind_all_branches() {
        assert_eq!(window_to_kind(5.0, "HOUR"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(300.0, "MINUTE"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(7.0, "DAY"), UsageKind::SevenDay);
        assert_eq!(window_to_kind(168.0, "HOUR"), UsageKind::SevenDay);
        assert_eq!(window_to_kind(1.0, "DAY"), UsageKind::Other);
        assert_eq!(window_to_kind(5.0, "UNKNOWN"), UsageKind::Other);
        // 大小写不敏感（to_ascii_uppercase）
        assert_eq!(window_to_kind(5.0, "hour"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(7.0, "day"), UsageKind::SevenDay);
    }
}
