//! Kimi Code 账号 + 用量（best-effort 容错）实现。
//!
//! **账号**：读 kimi_share_dir()/credentials/kimi-code.json 的 access_token，
//!   尝试 decode_jwt_payload 取 email claim（只读、不打印 token）；无 email →
//!   login_label="已登录"。凭据文件不存在 → None。
//!
//! **用量**：GET {base_url}/usages，8s 超时，**按需刷新 token**（过期才刷 + mutex 串行 + 原子写回）。
//!   kimi access_token 寿命仅约 15 分钟，不刷新几乎每次都 401；刷新写回保持与 kimi 完全同格式
//!   （TokenInfoWire snake_case；expires_at = now_secs + expires_in），仅并发刷新窄窗有冲突风险。
//!   任何非 2xx / 网络错 / 解析失败 → 安静降级 None，不崩溃、不影响其它 provider。
//!   容错解析 parse_kimi_usage：支持多种推断 schema，字段漂移 used↔remaining、
//!   resetAt/reset_at/resetTime/reset_time↔reset_in/resetIn/ttl/window(秒偏移)。

use serde_json::Value;
use std::time::Duration;

use super::{Account, ProviderAccount, ProviderUsage, UsageKind, UsageLane};

const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_BASE_URL: &str = "https://api.kimi.com/coding/v1";

// kimi OAuth 刷新端点与 client_id（来源：kimi-code 开源包 packages/oauth/src/constants.ts）
const KIMI_TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

// ═══ 路径工具 ═══

fn kimi_credentials_path() -> Option<std::path::PathBuf> {
    Some(meowo_reporter::kimi::kimi_share_dir()?.join("credentials").join("kimi-code.json"))
}

fn read_kimi_credentials() -> Option<Value> {
    super::read_json(&kimi_credentials_path()?)
}

// ═══ 配置读取 ═══

/// 读 base_url：env KIMI_CODE_BASE_URL > kimi_share_dir/config.toml > 缺省。
fn kimi_base_url() -> String {
    if let Ok(url) = std::env::var("KIMI_CODE_BASE_URL") {
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.is_empty() {
            return url;
        }
    }
    if let Some(url) = read_config_base_url() {
        return url;
    }
    DEFAULT_BASE_URL.to_string()
}

/// 从 kimi_share_dir()/config.toml 简单逐行解析 [providers."managed:kimi-code"].base_url。
/// 不引入 toml 依赖，best-effort，失败返回 None。
fn read_config_base_url() -> Option<String> {
    let path = meowo_reporter::kimi::kimi_share_dir()?.join("config.toml");
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

// ═══ Token 刷新辅助（纯函数，便于单测） ═══

/// 当前 Unix 秒时间戳。
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// token 是否过期（留 60s 余量）。
/// `expires_at_secs` 是凭据文件中的 Unix 秒整数（与 kimi 写法相同）。
pub fn is_kimi_token_expired(expires_at_secs: i64, now_secs: i64) -> bool {
    now_secs >= expires_at_secs - 60
}

/// 把刷新结果合并进原凭据 JSON：**只更新** access_token/refresh_token/expires_in/expires_at，
/// 其余字段（scope/token_type 等）原样保留。
/// expires_at = now_secs + expires_in，与 kimi-code 源码 `Math.floor(Date.now()/1000)+expiresIn` 完全一致。
pub fn merge_kimi_credentials(
    original: &Value,
    access_token: &str,
    refresh_token: &str,
    expires_in: i64,
    now_secs: i64,
) -> Value {
    let mut out = original.clone();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("access_token".into(), serde_json::json!(access_token));
        obj.insert("refresh_token".into(), serde_json::json!(refresh_token));
        obj.insert("expires_in".into(), serde_json::json!(expires_in));
        obj.insert("expires_at".into(), serde_json::json!(now_secs + expires_in));
    }
    out
}

/// 原子写回凭据文件（避免半截文件）。
fn write_kimi_credentials_atomic(path: &std::path::Path, value: &Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(path, &body).map_err(|e| e.to_string())
}

/// 按需刷新 kimi token（过期才刷 + Mutex 串行化 + 双检 + 原子写回）。
///
/// 流程：
/// 1. 读凭据；若 expires_at < now+60 → 需刷新。
/// 2. 持 REFRESH_LOCK 后重读（双检）：若已被另一调用刷新则直接用新 token，不重复刷。
/// 3. POST kimi 刷新端点（x-www-form-urlencoded）。
/// 4. 成功 → 原子写回（仅更新 4 个字段，保留其余）；失败 → 不碰文件，返回 None。
///
/// 兜底：刷新失败（invalid_grant/网络错）→ 返回 None → 上层 usage 降级为 None；
///         不打印/日志 token 原文。
fn ensure_valid_kimi_token() -> Option<String> {
    use std::sync::Mutex;
    // 串行化并发刷新：kimi refresh_token 单次使用后即失效，并发刷新会互相覆盖。
    // 持锁后下方重读凭据（双检）：若刚被另一线程刷新过则直接走 fast-path，不再重刷。
    static REFRESH_LOCK: Mutex<()> = Mutex::new(());

    let creds = read_kimi_credentials()?;
    let access_token = creds.get("access_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let expires_at = creds.get("expires_at").and_then(|v| v.as_i64()).unwrap_or(0);

    // Fast path：token 仍有效，直接返回，不加锁。
    if !access_token.is_empty() && !is_kimi_token_expired(expires_at, now_secs()) {
        return Some(access_token);
    }

    // Token 可能过期 → 加锁后双检。
    let _guard = REFRESH_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // 双检：持锁后重读文件，若已被并发刷新过，直接用新 token。
    let creds = read_kimi_credentials()?;
    let access_token = creds.get("access_token").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let expires_at = creds.get("expires_at").and_then(|v| v.as_i64()).unwrap_or(0);
    let current_secs = now_secs();

    if !access_token.is_empty() && !is_kimi_token_expired(expires_at, current_secs) {
        return Some(access_token);
    }

    // 仍过期 → 执行刷新。expires_at 缺失/字段名不同会被读成 0 → 恒判过期而走到这里，
    // 旧版鉴权若与新版 kimi-code 的刷新端点/client_id 不兼容，这里就会失败。
    eprintln!("Meowo usage[kimi]: access_token 已过期或无 expires_at，尝试刷新…");
    let Some(refresh_token) = creds.get("refresh_token").and_then(|v| v.as_str()).map(str::to_string) else {
        eprintln!("Meowo usage[kimi]: 凭据缺 refresh_token 字段，无法刷新");
        return None;
    };
    if refresh_token.is_empty() {
        eprintln!("Meowo usage[kimi]: refresh_token 为空，无法刷新");
        return None;
    }

    let resp = match ureq::post(KIMI_TOKEN_URL)
        .timeout(HTTP_TIMEOUT)
        .set("Accept", "application/json")
        .send_form(&[
            ("grant_type", "refresh_token"),
            ("client_id", KIMI_CLIENT_ID),
            ("refresh_token", &refresh_token),
        ]) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, resp)) => {
            // OAuth 错误体形如 {"error":"invalid_grant"|"invalid_client",...}——只含错误码不含 token，
            // 可安全打印（截断防超长）。invalid_grant=refresh_token 失效（重登可解）；
            // invalid_client=client_id 不匹配（需按旧版适配）。
            let body = resp.into_string().unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            eprintln!("Meowo usage[kimi]: 刷新 token 返回 HTTP {code}，响应体：{snippet}");
            return None;
        }
        Err(e) => {
            eprintln!("Meowo usage[kimi]: 刷新 token 网络错误：{e}");
            return None;
        }
    };

    let body: Value = resp.into_json().ok()?;

    let new_access = body.get("access_token").and_then(|v| v.as_str())?;
    if new_access.is_empty() {
        return None;
    }
    let new_refresh = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(&refresh_token);
    // 钳下限 60s：服务端若异常返回极小值，避免写回立刻过期的 expires_at 致每次都刷。
    // 钳上限 86400s（24h）：防服务端异常返回超大值导致溢出。
    let expires_in = body
        .get("expires_in")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(900)
        .clamp(60, 86400);

    let refresh_secs = now_secs();
    let merged = merge_kimi_credentials(&creds, new_access, new_refresh, expires_in, refresh_secs);

    // 仅在刷新成功后写回（失败不碰文件）。
    let path = kimi_credentials_path()?;
    if write_kimi_credentials_atomic(&path, &merged).is_err() {
        // 写回失败（权限/磁盘），但刷新本身成功 → 本次仍可用新 token（内存中），
        // 下次仍会再刷（未持久化）。
        eprintln!("Meowo: kimi 凭据写回失败");
    }

    eprintln!("Meowo usage[kimi]: token 刷新成功");
    Some(new_access.to_string())
}

// ═══ 用量解析（纯函数） ═══

/// 复用 codex 模块的 unix 秒 → ISO 8601 实现。
fn unix_to_iso8601(ts: i64) -> String {
    super::codex::unix_to_iso8601(ts)
}

/// ISO 字符串纳秒精度截断：若形如 `....<frac>Z` 且 frac > 3 位纯数字，截断到毫秒（保留前 3 位）。
/// kimi API 有时返回纳秒级时间戳（如 "2026-06-30T12:00:00.123456789Z"），直接 parse 会报错。
fn truncate_frac_to_millis(s: &str) -> String {
    if let Some(s_no_z) = s.strip_suffix('Z') {
        if let Some(dot_pos) = s_no_z.rfind('.') {
            let frac = &s_no_z[dot_pos + 1..];
            if frac.len() > 3 && frac.chars().all(|c| c.is_ascii_digit()) {
                return format!("{}.{}Z", &s_no_z[..dot_pos], &frac[..3]);
            }
        }
    }
    s.to_string()
}

/// 解析 resetAt 字段族，兼容多种形态（容错）。
///
/// 字符串型（按序尝试）：`resetAt` / `reset_at` / `resetTime` / `reset_time`；
/// 取到后若纳秒精度（>3位小数）则截断到毫秒再作为 ISO 返回；数字型则 unix秒→ISO。
///
/// 整数秒偏移型（按序尝试）：`reset_in` / `resetIn` / `ttl` / `window`（i64，兼容 f64→i64）；
/// 计算 now+secs 后转 ISO（`window` 为对象时 as_i64/as_f64 均返回 None，自动跳过）。
fn parse_resets_at(v: &Value) -> Option<String> {
    // 字符串型：四种别名均支持
    for key in &["resetAt", "reset_at", "resetTime", "reset_time"] {
        if let Some(val) = v.get(key) {
            if let Some(s) = val.as_str() {
                return Some(truncate_frac_to_millis(s));
            }
            // 数字（可能带小数）→ unix 秒 → ISO
            if let Some(ts) = val.as_i64().or_else(|| val.as_f64().map(|f| f as i64)) {
                return Some(unix_to_iso8601(ts));
            }
        }
    }
    // 整数秒偏移型：四种别名；window 为对象时 as_i64/as_f64 = None，自动略过
    for key in &["reset_in", "resetIn", "ttl", "window"] {
        if let Some(secs) = v
            .get(key)
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            return Some(unix_to_iso8601(now + secs));
        }
    }
    None
}

/// 从 JSON 值中提取数字，兼容字符串与数字两种格式。
/// kimi /usages 的 used/limit/remaining 字段有时返回字符串 "100" 而非数字 100。
fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// 从含 used/remaining/limit 的对象提取 (used, limit)。
/// 字段漂移容错：优先 used；无 used 时从 remaining 反推（used = limit - remaining）。
/// 数字格式容错：used/limit/remaining 可为字符串 "100" 或数字 100（kimi 真实响应为字符串）。
fn extract_used_limit(v: &Value) -> Option<(f64, f64)> {
    let limit = v.get("limit").and_then(num)?;
    let used = if let Some(u) = v.get("used").and_then(num) {
        u
    } else if let Some(r) = v.get("remaining").and_then(num) {
        limit - r
    } else {
        return None;
    };
    Some((used, limit))
}

/// window {duration, timeUnit} → UsageKind。
/// timeUnit 用 contains 匹配（容忍 "TIME_UNIT_HOUR" 等前缀），统一换算成小时后比较。
/// MINUTE 换算：duration/60（300 MINUTE = 5h；须可整除才能精确命中 5h/168h 窗口）。
fn window_to_kind(duration: f64, time_unit: &str) -> UsageKind {
    let up = time_unit.to_ascii_uppercase();
    let hours = if up.contains("MINUTE") {
        duration / 60.0
    } else if up.contains("HOUR") {
        duration
    } else if up.contains("DAY") {
        duration * 24.0
    } else {
        return UsageKind::Other;
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
/// label 优先 usage.name → usage.title → 默认 "Weekly limit"（不存入结构体，仅供调试参考）。
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
/// detail 若不是对象（缺失或 null）则退回用 item 本身取 used/limit。
fn parse_limit_item(item: &Value) -> Option<UsageLane> {
    let window = item.get("window")?;
    let time_unit = window.get("timeUnit").and_then(|v| v.as_str()).unwrap_or("");
    let duration = window.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let kind = window_to_kind(duration, time_unit);

    // detail 若不是对象则退回 item 本身取 used/limit
    let data = match item.get("detail") {
        Some(d) if d.is_object() => d,
        _ => item,
    };

    let (used, limit) = extract_used_limit(data)?;
    let used_pct = if limit > 0.0 { Some(used / limit * 100.0) } else { None };
    let resets_at = parse_resets_at(data);

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
/// 支持两种推断 schema（容错，同时存在时叠加）：
/// - **Schema A** `{usage:{name|title, used, limit, resetAt|reset_at|resetTime|reset_time|…}}` → Weekly lane
/// - **Schema B** `{limits:[{detail:{used,limit}(或退回 item 本身), window:{duration,timeUnit(含前缀)}}]}` → 按 window 派生 lane
///
/// 字段漂移容错：
/// - `used ↔ remaining`（remaining 时 used = limit - remaining）
/// - resetAt/reset_at/resetTime/reset_time（字符串→纳秒截断→ISO；数字→unix→ISO）
/// - reset_in/resetIn/ttl/window（整数秒→now+secs→ISO）
/// - detail 缺失或非对象 → 退回 item 本身
///
/// 注：balance（`data.available_balance`）属于 open-platform `/users/me/balance` 端点，
/// 不在 /usages 响应中，不在此处解析。
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

    if lanes.is_empty() { None } else { Some(ProviderUsage { lanes, note: None }) }
}

// ═══ 联网取用量 ═══

/// 联网拉 GET {base}/usages（按需刷新 token + 原子写回凭据）。
/// kimi access_token 寿命仅约 15 分钟，过期前 60s 自动刷新；
/// 刷新写回仅更新 access_token/refresh_token/expires_in/expires_at，其余字段不动。
/// 任何非 2xx / 网络错 / 解析失败 → None（安静降级）。
fn fetch_kimi_usage_live() -> Option<ProviderUsage> {
    let Some(access_token) = ensure_valid_kimi_token() else {
        eprintln!("Meowo usage[kimi]: 无有效 access_token（读不到或刷新失败），跳过取用量");
        return None;
    };
    let base = kimi_base_url();
    let url = format!("{base}/usages");

    let resp = match ureq::get(&url)
        .timeout(HTTP_TIMEOUT)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .call()
    {
        Ok(r) => r,
        // 4xx/5xx：ureq 归为 Error::Status。401 多为 token 失效/旧版鉴权不兼容；404 多为端点不对。
        Err(ureq::Error::Status(code, _)) => {
            eprintln!("Meowo usage[kimi]: GET {url} 返回 HTTP {code}（401=鉴权失效/不兼容，404=端点不对）");
            return None;
        }
        Err(e) => {
            eprintln!("Meowo usage[kimi]: GET {url} 网络错误：{e}");
            return None;
        }
    };
    let v: Value = match resp.into_json() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Meowo usage[kimi]: /usages 响应不是合法 JSON：{e}");
            return None;
        }
    };
    match parse_kimi_usage(&v) {
        Some(pu) => Some(pu),
        None => {
            eprintln!("Meowo usage[kimi]: /usages 解析不出任何用量 lane（响应 schema 可能与旧版不同）");
            None
        }
    }
}

// ═══ ProviderAccount impl ═══

pub struct KimiProviderAccount;

impl ProviderAccount for KimiProviderAccount {
    fn key(&self) -> meowo_store::ProviderKey {
        meowo_store::ProviderKey::Kimi
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
                login_label: Some("已登录".to_string()),
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

    // ── is_kimi_token_expired ──

    #[test]
    fn token_not_expired_when_well_before_buffer() {
        // expires_at = 1000, now = 900 → 900 < 1000-60=940 → 未过期
        assert!(!is_kimi_token_expired(1000, 900));
    }

    #[test]
    fn token_expired_when_within_buffer() {
        // expires_at = 1000, now = 945 → 945 >= 940 → 已过期（含 60s 余量）
        assert!(is_kimi_token_expired(1000, 945));
    }

    #[test]
    fn token_expired_when_past_expiry() {
        // expires_at = 1000, now = 1100 → 已过期
        assert!(is_kimi_token_expired(1000, 1100));
    }

    #[test]
    fn token_not_expired_exactly_at_buffer_boundary() {
        // expires_at = 1000, now = 939 → 939 < 940 → 未过期
        assert!(!is_kimi_token_expired(1000, 939));
    }

    #[test]
    fn token_expired_exactly_at_buffer() {
        // expires_at = 1000, now = 940 → 940 >= 940 → 已过期
        assert!(is_kimi_token_expired(1000, 940));
    }

    // ── merge_kimi_credentials ──

    #[test]
    fn merge_updates_token_fields_only() {
        // 原凭据含 scope/token_type 等额外字段，merge 后应保留
        let original = json!({
            "access_token": "old_access",
            "refresh_token": "old_refresh",
            "expires_in": 900,
            "expires_at": 1000000,
            "scope": "openid profile",
            "token_type": "Bearer"
        });
        let merged = merge_kimi_credentials(&original, "new_access", "new_refresh", 1800, 2000000);

        assert_eq!(merged["access_token"], "new_access");
        assert_eq!(merged["refresh_token"], "new_refresh");
        assert_eq!(merged["expires_in"], 1800);
        // expires_at = now_secs + expires_in = 2000000 + 1800
        assert_eq!(merged["expires_at"], 2001800i64);
        // 保留未涉及字段
        assert_eq!(merged["scope"], "openid profile");
        assert_eq!(merged["token_type"], "Bearer");
    }

    #[test]
    fn merge_expires_at_calculation() {
        // expires_at = now_secs + expires_in（Unix 秒），与 kimi 源码完全一致
        let original = json!({"access_token": "a", "refresh_token": "r", "expires_at": 0, "expires_in": 0});
        let merged = merge_kimi_credentials(&original, "a2", "r2", 900, 1751000000);
        assert_eq!(merged["expires_at"], 1751000900i64);
    }

    #[test]
    fn merge_preserves_extra_fields() {
        // 凭据文件可能含 kimi 内部字段，不应被清除
        let original = json!({
            "access_token": "a",
            "refresh_token": "r",
            "expires_at": 0,
            "expires_in": 900,
            "scope": "openid",
            "token_type": "Bearer",
            "some_device_field": "device_value"
        });
        let merged = merge_kimi_credentials(&original, "a2", "r2", 900, 1000);
        assert_eq!(merged["some_device_field"], "device_value");
        assert_eq!(merged["scope"], "openid");
        assert_eq!(merged["token_type"], "Bearer");
    }

    #[test]
    fn merge_does_not_modify_original() {
        let original = json!({"access_token": "old", "refresh_token": "oldr", "expires_at": 0, "expires_in": 0});
        let _ = merge_kimi_credentials(&original, "new", "newr", 900, 1000);
        // original 不变
        assert_eq!(original["access_token"], "old");
    }

    // ── expires_in 上界测试（防溢出）──

    #[test]
    fn ensure_valid_kimi_token_clamps_expires_in() {
        // 这是单元测试演示；实际 ensure_valid_kimi_token 涉及文件 I/O，
        // 此处验证钳制逻辑是否被正确应用于 merge 后的 expires_at。
        // 若 expires_in 被 clamp 到 [60, 86400]，则：
        // - now_secs + 86400 不溢出（int64 足够容纳）
        // - 下次刷新时间合理（最多 24 小时后）
        let original = json!({"access_token": "a", "refresh_token": "r", "expires_at": 0, "expires_in": 0});

        // 模拟 expires_in 被服务端异常设置为极大值 (100000000)
        // clamp 后应为 86400
        let clamped_expires_in = 100000000i64.clamp(60, 86400);
        assert_eq!(clamped_expires_in, 86400);

        // 模拟 expires_in 被异常设置为极小值 (5)
        // clamp 后应为 60
        let clamped_small = 5i64.clamp(60, 86400);
        assert_eq!(clamped_small, 60);

        // 验证 merge 计算不溢出
        let now = i64::MAX / 2; // 模拟大数时间戳
        let merged = merge_kimi_credentials(&original, "a2", "r2", clamped_expires_in, now);
        let expires_at = merged["expires_at"].as_i64().unwrap();
        // now + 86400 不应溢出（仍是有效的 i64）
        assert_eq!(expires_at, now + 86400);
    }

    // ── truncate_frac_to_millis ──

    #[test]
    fn truncate_nanos_to_millis() {
        assert_eq!(
            truncate_frac_to_millis("2026-06-30T12:00:00.123456789Z"),
            "2026-06-30T12:00:00.123Z"
        );
    }

    #[test]
    fn truncate_micros_to_millis() {
        assert_eq!(
            truncate_frac_to_millis("2026-06-30T12:00:00.123456Z"),
            "2026-06-30T12:00:00.123Z"
        );
    }

    #[test]
    fn truncate_already_millis_unchanged() {
        assert_eq!(
            truncate_frac_to_millis("2026-06-30T12:00:00.123Z"),
            "2026-06-30T12:00:00.123Z"
        );
    }

    #[test]
    fn truncate_no_frac_unchanged() {
        assert_eq!(
            truncate_frac_to_millis("2026-06-30T12:00:00Z"),
            "2026-06-30T12:00:00Z"
        );
    }

    // ── parse_resets_at 新字段名 + 纳秒截断 ──

    #[test]
    fn parse_resets_at_reset_time_camel() {
        let v = json!({"resetTime": "2026-06-30T12:00:00Z"});
        assert_eq!(parse_resets_at(&v).as_deref(), Some("2026-06-30T12:00:00Z"));
    }

    #[test]
    fn parse_resets_at_reset_time_snake() {
        let v = json!({"reset_time": "2026-07-01T00:00:00Z"});
        assert_eq!(parse_resets_at(&v).as_deref(), Some("2026-07-01T00:00:00Z"));
    }

    #[test]
    fn parse_resets_at_reset_at_nanos_truncated() {
        // 纳秒精度（9位）→ 截断到毫秒（3位）
        let v = json!({"resetAt": "2026-06-30T12:00:00.123456789Z"});
        assert_eq!(parse_resets_at(&v).as_deref(), Some("2026-06-30T12:00:00.123Z"));
    }

    #[test]
    fn parse_resets_at_reset_in_camel() {
        // resetIn 整数秒偏移（驼峰别名）
        let v = json!({"resetIn": 7200});
        assert!(parse_resets_at(&v).is_some(), "resetIn 应产生 Some");
    }

    #[test]
    fn parse_resets_at_window_int() {
        // window 为整数秒时作偏移处理（为对象时 as_i64=None 自动跳过）
        let v = json!({"window": 3600});
        assert!(parse_resets_at(&v).is_some(), "window 整数秒应产生 Some");
    }

    #[test]
    fn parse_resets_at_window_object_ignored() {
        // window 为对象时不应当作秒偏移
        let v = json!({"window": {"duration": 5, "timeUnit": "HOUR"}});
        assert!(parse_resets_at(&v).is_none(), "window 对象不应产生 reset 偏移");
    }

    #[test]
    fn parse_resets_at_missing_returns_none() {
        let v = json!({"other_key": "val"});
        assert!(parse_resets_at(&v).is_none());
    }

    // ── window_to_kind 前缀容忍 ──

    #[test]
    fn window_to_kind_time_unit_prefixes() {
        // "TIME_UNIT_HOUR/DAY/MINUTE" 均通过 contains 识别
        assert_eq!(window_to_kind(5.0, "TIME_UNIT_HOUR"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(168.0, "TIME_UNIT_HOUR"), UsageKind::SevenDay);
        assert_eq!(window_to_kind(7.0, "TIME_UNIT_DAY"), UsageKind::SevenDay);
        assert_eq!(window_to_kind(300.0, "TIME_UNIT_MINUTE"), UsageKind::FiveHour);
    }

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
        assert!((lane.used_pct.unwrap() - 50.0).abs() < 0.01);
        assert_eq!(lane.used, Some(500.0));
        assert_eq!(lane.limit, Some(1000.0));
        assert_eq!(lane.unit.as_deref(), Some("tokens"));
        assert_eq!(lane.resets_at.as_deref(), Some("2026-06-30T12:00:00Z"));
    }

    #[test]
    fn schema_a_label_title_fallback() {
        // name 缺失时使用 title 字段不会崩溃，kind 仍为 Weekly
        let v = json!({
            "usage": {"title": "Weekly quota", "used": 100, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("title fallback 不应崩溃");
        assert_eq!(pu.lanes[0].kind, UsageKind::Weekly);
    }

    #[test]
    fn schema_a_no_used_only_remaining() {
        let v = json!({
            "usage": {"remaining": 700, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("remaining 漂移应解析成功");
        assert_eq!(pu.lanes[0].used, Some(300.0));
        assert!((pu.lanes[0].used_pct.unwrap() - 30.0).abs() < 0.01);
    }

    #[test]
    fn schema_a_used_takes_priority_over_remaining() {
        let v = json!({
            "usage": {"used": 100, "remaining": 700, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"}
        });
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].used, Some(100.0));
    }

    #[test]
    fn schema_a_reset_at_unix_seconds() {
        let v = json!({"usage": {"used": 100, "limit": 1000, "resetAt": 1782820800i64}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.as_deref().unwrap_or("").contains("2026-06-30"));
    }

    #[test]
    fn schema_a_reset_at_alias() {
        let v = json!({"usage": {"used": 100, "limit": 1000, "reset_at": "2026-07-01T00:00:00Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-07-01T00:00:00Z"));
    }

    #[test]
    fn schema_a_reset_time_camel() {
        // resetTime 新增字段名
        let v = json!({"usage": {"used": 100, "limit": 1000, "resetTime": "2026-07-01T00:00:00Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-07-01T00:00:00Z"));
    }

    #[test]
    fn schema_a_reset_time_snake() {
        // reset_time 新增下划线别名
        let v = json!({"usage": {"used": 100, "limit": 1000, "reset_time": "2026-07-02T00:00:00Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-07-02T00:00:00Z"));
    }

    #[test]
    fn schema_a_reset_at_nanos_truncated() {
        // resetAt 纳秒精度 → 截断到毫秒
        let v = json!({"usage": {"used": 100, "limit": 1000, "resetAt": "2026-06-30T12:00:00.123456789Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-06-30T12:00:00.123Z"));
    }

    #[test]
    fn schema_a_reset_in_seconds_offset() {
        let v = json!({"usage": {"used": 100, "limit": 1000, "reset_in": 3600}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some());
    }

    #[test]
    fn schema_a_reset_in_camel_alias() {
        // resetIn 驼峰别名
        let v = json!({"usage": {"used": 100, "limit": 1000, "resetIn": 7200}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some());
    }

    #[test]
    fn schema_a_ttl_seconds_offset() {
        let v = json!({"usage": {"used": 50, "limit": 500, "ttl": 7200}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some());
    }

    #[test]
    fn schema_a_window_int_seconds_offset() {
        // usage.window 为整数秒时作偏移
        let v = json!({"usage": {"used": 50, "limit": 500, "window": 3600}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].resets_at.is_some());
    }

    #[test]
    fn schema_a_zero_limit_no_pct() {
        let v = json!({"usage": {"used": 0, "limit": 0, "resetAt": "2026-06-30T00:00:00Z"}});
        let pu = parse_kimi_usage(&v).expect("should parse");
        assert!(pu.lanes[0].used_pct.is_none(), "limit=0 时 used_pct 应为 None");
    }

    // ── parse_kimi_usage · Schema B（limits 数组）──

    #[test]
    fn schema_b_five_hour_hour_unit() {
        let v = json!({
            "limits": [{"detail": {"used": 200, "limit": 400}, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("HOUR 5 应为 FiveHour");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
        assert!((pu.lanes[0].used_pct.unwrap() - 50.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_five_hour_minute_unit() {
        let v = json!({
            "limits": [{"detail": {"used": 50, "limit": 100}, "window": {"duration": 300, "timeUnit": "MINUTE"}}]
        });
        let pu = parse_kimi_usage(&v).expect("MINUTE 300 应为 FiveHour");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
    }

    #[test]
    fn schema_b_seven_day_day_unit() {
        let v = json!({
            "limits": [{"detail": {"used": 1000, "limit": 10000}, "window": {"duration": 7, "timeUnit": "DAY"}}]
        });
        let pu = parse_kimi_usage(&v).expect("DAY 7 应为 SevenDay");
        assert_eq!(pu.lanes[0].kind, UsageKind::SevenDay);
        assert!((pu.lanes[0].used_pct.unwrap() - 10.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_seven_day_hour_unit() {
        let v = json!({
            "limits": [{"detail": {"used": 500, "limit": 5000}, "window": {"duration": 168, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("HOUR 168 应为 SevenDay");
        assert_eq!(pu.lanes[0].kind, UsageKind::SevenDay);
    }

    #[test]
    fn schema_b_time_unit_hour_prefix() {
        // "TIME_UNIT_HOUR" 带前缀 → contains 识别
        let v = json!({
            "limits": [{"detail": {"used": 100, "limit": 200}, "window": {"duration": 5, "timeUnit": "TIME_UNIT_HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("TIME_UNIT_HOUR 应识别为 FiveHour");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
    }

    #[test]
    fn schema_b_detail_missing_falls_back_to_item() {
        // detail 缺失 → 退回 item 本身取 used/limit
        let v = json!({
            "limits": [{"used": 100, "limit": 400, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("detail 缺失应退回 item 本身");
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
        assert_eq!(pu.lanes[0].used, Some(100.0));
        assert_eq!(pu.lanes[0].limit, Some(400.0));
    }

    #[test]
    fn schema_b_detail_null_falls_back_to_item() {
        // detail 为 null（非对象）→ 退回 item 本身
        let v = json!({
            "limits": [{"used": 200, "limit": 1000, "detail": null, "window": {"duration": 168, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("detail=null 应退回 item 本身");
        assert_eq!(pu.lanes[0].kind, UsageKind::SevenDay);
        assert_eq!(pu.lanes[0].used, Some(200.0));
    }

    #[test]
    fn schema_b_remaining_drift_in_detail() {
        let v = json!({
            "limits": [{"detail": {"remaining": 300, "limit": 400}, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("remaining 漂移应解析成功");
        assert_eq!(pu.lanes[0].used, Some(100.0));
        assert!((pu.lanes[0].used_pct.unwrap() - 25.0).abs() < 0.01);
    }

    #[test]
    fn schema_b_multiple_limits() {
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

    // ── balance 已从 /usages 解析中删除 ──

    #[test]
    fn balance_in_data_not_parsed() {
        // data.available_balance 属于另一个端点，/usages 响应不含此字段，应忽略
        let v = json!({"data": {"available_balance": 5.42}});
        assert!(parse_kimi_usage(&v).is_none(), "data.available_balance 应被忽略");
    }

    // ── 混合 / 畸形 ──

    #[test]
    fn mixed_schemas_a_and_b() {
        // Schema A + B 同时存在 → 叠加 2 条 lane（无 balance 第三条）
        let v = json!({
            "usage": {"used": 100, "limit": 1000, "resetAt": "2026-06-30T12:00:00Z"},
            "limits": [{"detail": {"used": 50, "limit": 100}, "window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        let pu = parse_kimi_usage(&v).expect("混合 schema 应解析成功");
        assert_eq!(pu.lanes.len(), 2);
    }

    #[test]
    fn empty_object_returns_none() {
        assert!(parse_kimi_usage(&json!({})).is_none());
    }

    #[test]
    fn malformed_no_limit_returns_none() {
        let v = json!({"usage": {"used": 100}});
        assert!(parse_kimi_usage(&v).is_none());
    }

    #[test]
    fn malformed_no_used_no_remaining_returns_none() {
        let v = json!({"usage": {"limit": 1000}});
        assert!(parse_kimi_usage(&v).is_none());
    }

    #[test]
    fn malformed_limits_no_window_skipped() {
        // window 缺失 → 该项跳过 → None
        let v = json!({
            "limits": [{"detail": {"used": 50, "limit": 100}}]
        });
        assert!(parse_kimi_usage(&v).is_none(), "缺 window 应跳过");
    }

    #[test]
    fn malformed_limits_no_used_limit_skipped() {
        // window 存在但 item 和 detail 均无 used/limit → None
        let v = json!({
            "limits": [{"window": {"duration": 5, "timeUnit": "HOUR"}}]
        });
        assert!(parse_kimi_usage(&v).is_none(), "无 used/limit 应跳过");
    }

    // ── 真实 /usages 响应回归测试（字符串数字 + 微秒精度 resetTime）──

    #[test]
    fn real_response_string_numbers_two_lanes() {
        // 权威测试基准：kimi /usages 实测响应，used/limit/remaining 均为 JSON 字符串
        let v = json!({
            "user": {"userId":"XXX","region":"REGION_CN","membership":{"level":"LEVEL_INTERMEDIATE"}},
            "usage": {"limit":"100","used":"8","remaining":"92","resetTime":"2026-07-06T02:00:13.307440Z"},
            "limits": [{
                "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                "detail": {"limit":"100","used":"10","remaining":"90","resetTime":"2026-07-01T10:00:13.307440Z"}
            }],
            "parallel": {"limit":"20"},
            "totalQuota": {"limit":"100","remaining":"99"},
            "authentication": {"method":"METHOD_ACCESS_TOKEN","scope":"FEATURE_CODING"},
            "subType": "TYPE_PURCHASE"
        });

        let pu = parse_kimi_usage(&v).expect("真实响应应解析成功（字符串数字）");
        // totalQuota/parallel/user 均非用量窗口，忽略 → 恰好 2 条 lane
        assert_eq!(pu.lanes.len(), 2, "应产生 2 条 lane（Weekly + FiveHour）");

        // Lane 0：Weekly（顶层 usage 对象）
        let weekly = &pu.lanes[0];
        assert_eq!(weekly.kind, UsageKind::Weekly, "顶层 usage → Weekly");
        assert_eq!(weekly.used, Some(8.0), "used 字符串 '8' 解析为 8.0");
        assert_eq!(weekly.limit, Some(100.0), "limit 字符串 '100' 解析为 100.0");
        assert!((weekly.used_pct.unwrap() - 8.0).abs() < 0.01, "used_pct 应为 8%");
        assert_eq!(
            weekly.resets_at.as_deref(),
            Some("2026-07-06T02:00:13.307Z"),
            "微秒精度 .307440Z 截断到毫秒 .307Z"
        );

        // Lane 1：FiveHour（limits[0]，300 TIME_UNIT_MINUTE = 5 小时）
        let five_hour = &pu.lanes[1];
        assert_eq!(five_hour.kind, UsageKind::FiveHour, "300 MINUTE → FiveHour");
        assert_eq!(five_hour.used, Some(10.0), "used 字符串 '10' 解析为 10.0");
        assert_eq!(five_hour.limit, Some(100.0), "limit 字符串 '100' 解析为 100.0");
        assert!((five_hour.used_pct.unwrap() - 10.0).abs() < 0.01, "used_pct 应为 10%");
        assert_eq!(
            five_hour.resets_at.as_deref(),
            Some("2026-07-01T10:00:13.307Z"),
            "微秒精度 .307440Z 截断到毫秒 .307Z"
        );
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
        // 大小写不敏感（to_ascii_uppercase + contains）
        assert_eq!(window_to_kind(5.0, "hour"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(7.0, "day"), UsageKind::SevenDay);
        // TIME_UNIT_* 前缀变体
        assert_eq!(window_to_kind(5.0, "TIME_UNIT_HOUR"), UsageKind::FiveHour);
        assert_eq!(window_to_kind(7.0, "TIME_UNIT_DAY"), UsageKind::SevenDay);
        assert_eq!(window_to_kind(300.0, "TIME_UNIT_MINUTE"), UsageKind::FiveHour);
    }
}
