//! Claude Code 账号、实时用量的读取与解析。
//! 纯解析/判定/合并函数在此可单测；I/O 与网络见同文件后半部分。
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use super::{Account, ProviderAccount, ProviderUsage, UsageKind, UsageLane};

/// Claude 账号原始信息（内部类型，供 ClaudeProvider::account() 转换使用）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClaudeAccountInfo {
    pub email: String,
    pub display_name: String,
    pub organization: Option<String>,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageWindow {
    pub utilization: f64,
    pub resets_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Usage {
    pub five_hour: Option<UsageWindow>,
    pub seven_day: Option<UsageWindow>,
    pub seven_day_opus: Option<UsageWindow>,
    pub seven_day_sonnet: Option<UsageWindow>,
    pub extra_usage_enabled: bool,
}

/// 从 ~/.claude.json 的根 JSON 解析账号（取 oauthAccount）。无则 None。
pub fn parse_account(root: &serde_json::Value) -> Option<ClaudeAccountInfo> {
    let a = root.get("oauthAccount")?;
    let email = a.get("emailAddress").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if email.is_empty() {
        return None;
    }
    let display_name = a
        .get("displayName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&email)
        .to_string();
    let organization = a
        .get("organizationName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    // 套餐标签：优先 seatTier，否则 billingType。
    let plan = ["seatTier", "billingType"]
        .iter()
        .find_map(|k| a.get(*k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()))
        .map(|s| s.to_string());
    Some(ClaudeAccountInfo { email, display_name, organization, plan })
}

/// 解析 /api/oauth/usage 响应。各 bucket 可能为 null/缺失 → Option。
pub fn parse_usage(v: &serde_json::Value) -> Usage {
    fn win(v: &serde_json::Value, key: &str) -> Option<UsageWindow> {
        let w = v.get(key)?;
        if w.is_null() {
            return None;
        }
        let utilization = w.get("utilization").and_then(|x| x.as_f64())?;
        let resets_at = w.get("resets_at").and_then(|x| x.as_str()).unwrap_or("").to_string();
        Some(UsageWindow { utilization, resets_at })
    }
    let extra_usage_enabled = v
        .get("extra_usage")
        .and_then(|e| e.get("is_enabled"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    Usage {
        five_hour: win(v, "five_hour"),
        seven_day: win(v, "seven_day"),
        seven_day_opus: win(v, "seven_day_opus"),
        seven_day_sonnet: win(v, "seven_day_sonnet"),
        extra_usage_enabled,
    }
}

/// token 是否需要刷新：到期前留 60s 余量。
pub fn is_token_expired(expires_at_ms: i64, now_ms: i64) -> bool {
    now_ms >= expires_at_ms - 60_000
}

/// 把新 token 合并进原 credentials JSON：只改 claudeAiOauth.{accessToken,refreshToken,expiresAt}，
/// 保留 mcpOAuth 等其它所有字段。返回新 JSON。
pub fn merge_credentials(
    original: &serde_json::Value,
    access_token: &str,
    refresh_token: &str,
    expires_at_ms: i64,
) -> serde_json::Value {
    let mut out = original.clone();
    if let Some(obj) = out.as_object_mut() {
        let oauth = obj
            .entry("claudeAiOauth")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(o) = oauth.as_object_mut() {
            o.insert("accessToken".into(), serde_json::json!(access_token));
            o.insert("refreshToken".into(), serde_json::json!(refresh_token));
            o.insert("expiresAt".into(), serde_json::json!(expires_at_ms));
        }
    }
    out
}

/// 用量不可查的标记码：读不到可用的 Anthropic OAuth 凭据（多为第三方/中转登录，
/// 或尚未在终端登录）。前端据此显示「当前登录方式不支持用量查询」而非通用报错。
pub const USAGE_UNSUPPORTED: &str = "USAGE_UNSUPPORTED";

/// 凭据根是否缺少可用的 Anthropic OAuth（→ 第三方/非官方登录，用量接口不适用）。
/// 判定：根缺失、缺 claudeAiOauth、或 access+refresh 双空。纯函数便于单测。
pub fn oauth_credentials_missing(root: Option<&serde_json::Value>) -> bool {
    let Some(oauth) = root.and_then(|r| r.get("claudeAiOauth")) else {
        return true;
    };
    let access = oauth.get("accessToken").and_then(|v| v.as_str()).unwrap_or("");
    let refresh = oauth.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("");
    access.is_empty() && refresh.is_empty()
}

/// 用量端点是 account 侧的事，不进 `AuthScheme`（后者只管「凭据在哪 + 怎么刷新」）。
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);

/// claude 的鉴权声明，取自插件层变体表（`meowo_agent::plugins::claude`）：OAuth client_id、
/// 刷新端点、凭据位置不再是本文件里的一把常量——换个版本形态只改变体表一处。
fn claude_auth() -> Option<&'static meowo_agent::AuthScheme> {
    meowo_reporter::claude::claude_install()?.auth
}

/// 账号信息住在 home 下的 `~/.claude.json`（**不在** data_dir 内，与凭据不同源），故不经
/// `Installation`。
fn claude_json_path() -> Option<PathBuf> {
    super::home_dir().map(|h| h.join(".claude.json"))
}

/// 非 macOS：Claude Code 把 OAuth 凭据写在 `<data_dir>/.credentials.json`（尊重
/// `CLAUDE_CONFIG_DIR`）。macOS 改存 Keychain（见 keychain_* 函数）。
#[cfg(not(target_os = "macos"))]
fn credentials_path() -> Option<PathBuf> {
    meowo_reporter::claude::claude_install()?.credentials_path()
}

fn read_json_file(path: &std::path::Path) -> Option<serde_json::Value> {
    super::read_json(path)
}

/// 读账号（~/.claude.json）。
pub fn read_account() -> Option<ClaudeAccountInfo> {
    parse_account(&read_json_file(&claude_json_path()?)?)
}

/// 原子写回 credentials 文件。仅非 macOS（macOS 写 Keychain）。
#[cfg(not(target_os = "macos"))]
fn write_credentials_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(path, &body).map_err(|e| e.to_string())
}

/// macOS 上 Claude Code 把 OAuth 凭据存在登录 Keychain 的通用密码里（不写 .credentials.json）。
/// service 与「写回时的 account 兜底值」均来自变体表的 `CredentialSource::KeychainOrFile`。
#[cfg(target_os = "macos")]
fn keychain_spec() -> (&'static str, &'static str) {
    match claude_auth().map(|a| a.credentials) {
        Some(meowo_agent::CredentialSource::KeychainOrFile { service, account, .. }) => (service, account),
        // 变体表被改成非 Keychain 形态（不该发生）：退回历史常量，至少不崩。
        _ => ("Claude Code-credentials", "root"),
    }
}

/// 从 `security find-generic-password -g` 的属性输出里抠出 account（"acct"<blob>=...）。
/// 形如 `"acct"<blob>="root"`，或 non-UTF8 时 `0x726F6F74  "root"`（hex + 可读串）→ 取引号内。
/// 纯函数便于单测；仅 macOS 调用，其它平台放行 dead_code。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_keychain_account(attrs: &str) -> Option<String> {
    let key = "\"acct\"<blob>=";
    for line in attrs.lines() {
        let Some(idx) = line.find(key) else { continue };
        let rest = line[idx + key.len()..].trim();
        if rest == "<NULL>" {
            return None;
        }
        let after = &rest[rest.find('"')? + 1..];
        let v = &after[..after.find('"')?];
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// 读 Keychain 里那条凭据的密码（即 `{"claudeAiOauth":{...}}` 的 JSON 字符串）。
#[cfg(target_os = "macos")]
fn keychain_read_password() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", keychain_spec().0, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim_end_matches(['\r', '\n']).to_string();
    (!s.is_empty()).then_some(s)
}

/// 读 Keychain 条目的 account 名（写回时按同名更新；读不到则上层退回默认 "root"）。
#[cfg(target_os = "macos")]
fn keychain_read_account() -> Option<String> {
    // `-g`：属性打到 stdout、密码打到 stderr，这里只取属性。
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", keychain_spec().0, "-g"])
        .output()
        .ok()?;
    parse_keychain_account(&String::from_utf8_lossy(&out.stdout))
}

/// 写回 Keychain（-U：条目存在则更新）。password 经 argv 传入，仅同用户进程可见，与本仓既有 shell-out 一致。
#[cfg(target_os = "macos")]
fn keychain_write_password(account: &str, password: &str) -> Result<(), String> {
    let status = std::process::Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            keychain_spec().0,
            "-a",
            account,
            "-w",
            password,
        ])
        .status()
        .map_err(|e| format!("写回 Keychain 失败：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("写回 Keychain 失败（security add-generic-password 非零退出）".into())
    }
}

/// 读 Claude Code 的 OAuth 凭据根 JSON（形如 `{"claudeAiOauth": {...}}`）。
/// macOS 取自登录 Keychain，其它平台取自 ~/.claude/.credentials.json。
#[cfg(target_os = "macos")]
pub(super) fn read_credentials_root() -> Option<serde_json::Value> {
    serde_json::from_str(&keychain_read_password()?).ok()
}
#[cfg(not(target_os = "macos"))]
pub(super) fn read_credentials_root() -> Option<serde_json::Value> {
    read_json_file(&credentials_path()?)
}

/// 刷新 token 后把新凭据写回原存储（保留其余字段）。macOS → Keychain，其它平台 → 原子写文件。
#[cfg(target_os = "macos")]
fn write_credentials_root(value: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_string(value).map_err(|e| e.to_string())?;
    // 读得到实际 account 就按同名更新；读不到用变体表声明的兜底值。
    let account = keychain_read_account().unwrap_or_else(|| keychain_spec().1.to_string());
    keychain_write_password(&account, &body)
}
#[cfg(not(target_os = "macos"))]
fn write_credentials_root(value: &serde_json::Value) -> Result<(), String> {
    let path = credentials_path().ok_or("解析不到 claude 凭据路径")?;
    write_credentials_atomic(&path, value)
}

/// 确保有有效 access token：未过期直接返回；过期则刷新并写回原存储，再返回新 token。
fn ensure_valid_token() -> Result<String, String> {
    use std::sync::Mutex;
    // 串行化刷新：并发刷新（多窗口/连点）会各自用旋转后失效的 refresh_token 重复请求、互相覆盖。
    // 持锁后下面会重读凭据并重新判过期（双检）——若刚被另一调用方刷新过，直接走 fast-path 返回新
    // token，不再重复刷新。
    static REFRESH_LOCK: Mutex<()> = Mutex::new(());
    let _guard = REFRESH_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let root = read_credentials_root();
    // 读不到可用 OAuth 凭据 → 视为第三方/非官方登录，用量接口不适用，返回标记码。
    if oauth_credentials_missing(root.as_ref()) {
        return Err(USAGE_UNSUPPORTED.into());
    }
    let root = root.expect("credentials present: oauth_credentials_missing 已排除 None");
    let oauth = root.get("claudeAiOauth").ok_or("凭据缺 claudeAiOauth")?;
    let access = oauth.get("accessToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let refresh = oauth.get("refreshToken").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let expires_at = oauth.get("expiresAt").and_then(|v| v.as_i64()).unwrap_or(0);

    if !access.is_empty() && !is_token_expired(expires_at, super::now_ms()) {
        return Ok(access);
    }
    if refresh.is_empty() {
        return Err("token 已过期且无 refreshToken".into());
    }
    // 刷新端点与 client_id 来自变体表；返回 `invalid_client` 即该变体的 client_id 与本机 claude 不符。
    let Some(spec) = claude_auth().and_then(|a| a.refresh) else {
        return Err("claude 变体表未声明 OAuth 刷新参数".into());
    };
    let resp = ureq::post(spec.token_url)
        .timeout(HTTP_TIMEOUT)
        .send_json(serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": spec.client_id,
        }))
        .map_err(|e| format!("刷新 token 失败：{e}"))?;
    let body: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    let new_access = body.get("access_token").and_then(|v| v.as_str()).ok_or("刷新响应缺 access_token")?.to_string();
    let new_refresh = body.get("refresh_token").and_then(|v| v.as_str()).unwrap_or(&refresh).to_string();
    // 钳下限 600s：服务端若异常返回 0/负/极小值，避免写回一个立刻过期的 expiresAt 而陷入每次都刷新。
    let expires_in = body
        .get("expires_in")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
        .unwrap_or(3600)
        .max(600);
    let new_expires_at = super::now_ms() + expires_in * 1000;

    let merged = merge_credentials(&root, &new_access, &new_refresh, new_expires_at);
    write_credentials_root(&merged)?;
    Ok(new_access)
}

/// 联网拉实时用量（含按需刷新 token），成功则写缓存。
pub fn fetch_usage_live() -> Result<Usage, String> {
    let token = ensure_valid_token()?;
    let resp = ureq::get(USAGE_URL)
        .timeout(HTTP_TIMEOUT)
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", OAUTH_BETA)
        .call()
        .map_err(|e| format!("请求用量失败：{e}"))?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    // 缓存统一由调用方经 mod.rs 的 write_cached_usage（providers.claude 合并写入）落盘。
    // 这里绝不能整文件覆写旧扁平格式——那会清掉 kimi/codex 等其它 provider 的缓存条目，
    // 且引用旧扁平格式的旧命令已全部移除（read_cached_usage 的容错读取保留一个版本周期即可）。
    Ok(parse_usage(&v))
}

/// 将旧 Usage 映射为通用 ProviderUsage 泳道格式。
/// five_hour→FiveHour, seven_day→SevenDay, seven_day_opus→Opus（utilization→used_pct,
/// unit "percent", resets_at 原样），extra_usage_enabled→note。seven_day_sonnet 忽略（保持现视觉）。
pub fn map_to_provider_usage(u: &Usage) -> ProviderUsage {
    let mut lanes: Vec<UsageLane> = Vec::new();

    if let Some(w) = &u.five_hour {
        lanes.push(UsageLane {
            kind: UsageKind::FiveHour,
            used_pct: Some(w.utilization),
            used: None,
            limit: None,
            unit: Some("percent".to_string()),
            resets_at: non_empty_str(&w.resets_at),
        });
    }
    if let Some(w) = &u.seven_day {
        lanes.push(UsageLane {
            kind: UsageKind::SevenDay,
            used_pct: Some(w.utilization),
            used: None,
            limit: None,
            unit: Some("percent".to_string()),
            resets_at: non_empty_str(&w.resets_at),
        });
    }
    if let Some(w) = &u.seven_day_opus {
        lanes.push(UsageLane {
            kind: UsageKind::Opus,
            used_pct: Some(w.utilization),
            used: None,
            limit: None,
            unit: Some("percent".to_string()),
            resets_at: non_empty_str(&w.resets_at),
        });
    }
    // seven_day_sonnet 忽略（保持现视觉）

    let note = u.extra_usage_enabled.then(|| "extra_usage_enabled".to_string());

    ProviderUsage { lanes, note }
}

/// 空字符串→ None（resets_at 字段处理）。
fn non_empty_str(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_string()) }
}

/// 当前凭据是否有效的 Anthropic OAuth（usage 接口是否可用）。
pub fn has_oauth_credentials() -> bool {
    !oauth_credentials_missing(read_credentials_root().as_ref())
}

/// Claude 的 ProviderAccount 实现：包装现有读取逻辑，映射到通用泳道类型。
pub struct ClaudeProviderAccount;

impl ProviderAccount for ClaudeProviderAccount {
    fn key(&self) -> meowo_store::ProviderKey {
        meowo_store::ProviderKey::Claude
    }

    fn account(&self) -> Option<Account> {
        read_account().map(|a| Account {
            email: Some(a.email),
            display_name: Some(a.display_name),
            organization: a.organization,
            plan: a.plan,
            login_label: None,
        })
    }

    fn usage(&self, force: bool) -> Option<ProviderUsage> {
        let key = self.key();
        if !force {
            // 从缓存读，不联网
            return super::read_cached_usage(key);
        }
        // 尊重 60s 限频：新/旧两种缓存格式均支持；未过期直接返回，否则联网拉取。
        // 使用 super::cache_is_fresh（mod.rs 通用版）而非私有版，消除死代码。
        if super::cache_is_fresh(key, 60_000) {
            if let Some(cached) = super::read_cached_usage(key) {
                return Some(cached);
            }
        }
        match fetch_usage_live() {
            Ok(old_usage) => {
                let pu = map_to_provider_usage(&old_usage);
                // 唯一写入方：mod.rs 的 write_cached_usage（providers.claude 分键合并写入）。
                super::write_cached_usage(key, &pu);
                Some(pu)
            }
            Err(_) => None,
        }
    }

    fn usage_supported(&self) -> bool {
        has_oauth_credentials()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_account_extracts_fields() {
        let root = json!({"oauthAccount":{"emailAddress":"a@b.com","displayName":"Larry","organizationName":"Acme","seatTier":"max","billingType":"subscription"}});
        let a = parse_account(&root).unwrap();
        assert_eq!(a.email, "a@b.com");
        assert_eq!(a.display_name, "Larry");
        assert_eq!(a.organization.as_deref(), Some("Acme"));
        assert_eq!(a.plan.as_deref(), Some("max"));
    }

    #[test]
    fn parse_account_none_without_email() {
        assert!(parse_account(&json!({"oauthAccount":{}})).is_none());
        assert!(parse_account(&json!({})).is_none());
    }

    #[test]
    fn parse_usage_full_and_nulls() {
        let v = json!({
            "five_hour":{"utilization":13.0,"resets_at":"2026-06-07T08:50:01Z"},
            "seven_day":{"utilization":19.0,"resets_at":"2026-06-11T12:00:00Z"},
            "seven_day_opus":null,
            "seven_day_sonnet":{"utilization":2.0,"resets_at":"x"},
            "extra_usage":{"is_enabled":false}
        });
        let u = parse_usage(&v);
        assert_eq!(u.five_hour.as_ref().unwrap().utilization, 13.0);
        assert_eq!(u.five_hour.as_ref().unwrap().resets_at, "2026-06-07T08:50:01Z");
        assert!(u.seven_day_opus.is_none());
        assert_eq!(u.seven_day_sonnet.as_ref().unwrap().utilization, 2.0);
        assert!(!u.extra_usage_enabled);
    }

    #[test]
    fn parse_usage_missing_fields_degrade() {
        let u = parse_usage(&json!({}));
        assert!(u.five_hour.is_none() && u.seven_day.is_none() && !u.extra_usage_enabled);
    }

    #[test]
    fn is_token_expired_margin() {
        let now = 1_000_000_000_000i64;
        assert!(is_token_expired(now, now));
        assert!(is_token_expired(now + 30_000, now));
        assert!(!is_token_expired(now + 120_000, now));
    }

    #[test]
    fn parse_keychain_account_extracts_acct() {
        let attrs = "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\n    \"acct\"<blob>=\"root\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(parse_keychain_account(attrs).as_deref(), Some("root"));
        // non-UTF8 时 security 打成 hex + 可读串，取引号内。
        let hexed = "    \"acct\"<blob>=0x726F6F74  \"root\"\n";
        assert_eq!(parse_keychain_account(hexed).as_deref(), Some("root"));
        // 没有 acct 行 / NULL → None。
        assert_eq!(parse_keychain_account("nothing here"), None);
        assert_eq!(parse_keychain_account("    \"acct\"<blob>=<NULL>\n"), None);
    }

    #[test]
    fn oauth_credentials_missing_detects_third_party() {
        // 无根、缺 claudeAiOauth、access+refresh 双空 → 缺失（第三方/未登录）。
        assert!(oauth_credentials_missing(None));
        assert!(oauth_credentials_missing(Some(&json!({"mcpOAuth": {}}))));
        assert!(oauth_credentials_missing(Some(&json!({"claudeAiOauth": {}}))));
        assert!(oauth_credentials_missing(Some(
            &json!({"claudeAiOauth": {"accessToken": "", "refreshToken": ""}})
        )));
        // 有 access 或有 refresh → 视为官方 OAuth 凭据，不缺失。
        assert!(!oauth_credentials_missing(Some(
            &json!({"claudeAiOauth": {"accessToken": "a", "refreshToken": ""}})
        )));
        assert!(!oauth_credentials_missing(Some(
            &json!({"claudeAiOauth": {"accessToken": "", "refreshToken": "r"}})
        )));
    }

    #[test]
    fn merge_credentials_preserves_other_fields() {
        let original = json!({
            "mcpOAuth":{"some":"value"},
            "claudeAiOauth":{"accessToken":"old","refreshToken":"oldr","expiresAt":1,"scopes":["a"],"subscriptionType":"max"}
        });
        let merged = merge_credentials(&original, "newA", "newR", 999);
        assert_eq!(merged["mcpOAuth"]["some"], "value");
        assert_eq!(merged["claudeAiOauth"]["accessToken"], "newA");
        assert_eq!(merged["claudeAiOauth"]["refreshToken"], "newR");
        assert_eq!(merged["claudeAiOauth"]["expiresAt"], 999);
        assert_eq!(merged["claudeAiOauth"]["scopes"][0], "a");
        assert_eq!(merged["claudeAiOauth"]["subscriptionType"], "max");
    }

    #[test]
    fn map_to_provider_usage_maps_lanes_correctly() {
        let u = Usage {
            five_hour: Some(UsageWindow { utilization: 13.0, resets_at: "2026-06-07T08:50:01Z".into() }),
            seven_day: Some(UsageWindow { utilization: 55.0, resets_at: "2026-06-11T12:00:00Z".into() }),
            seven_day_opus: Some(UsageWindow { utilization: 30.0, resets_at: "2026-06-11T12:00:00Z".into() }),
            seven_day_sonnet: Some(UsageWindow { utilization: 2.0, resets_at: "x".into() }),
            extra_usage_enabled: true,
        };
        let pu = map_to_provider_usage(&u);
        // 应有 3 条泳道（sonnet 忽略）。
        assert_eq!(pu.lanes.len(), 3);
        assert_eq!(pu.lanes[0].kind, UsageKind::FiveHour);
        assert_eq!(pu.lanes[0].used_pct, Some(13.0));
        assert_eq!(pu.lanes[0].unit.as_deref(), Some("percent"));
        assert_eq!(pu.lanes[0].resets_at.as_deref(), Some("2026-06-07T08:50:01Z"));
        assert_eq!(pu.lanes[1].kind, UsageKind::SevenDay);
        assert_eq!(pu.lanes[2].kind, UsageKind::Opus);
        // extra_usage_enabled → note。
        assert_eq!(pu.note.as_deref(), Some("extra_usage_enabled"));
    }

    #[test]
    fn map_to_provider_usage_empty_resets_at_becomes_none() {
        let u = Usage {
            five_hour: Some(UsageWindow { utilization: 1.0, resets_at: "".into() }),
            ..Default::default()
        };
        let pu = map_to_provider_usage(&u);
        assert!(pu.lanes[0].resets_at.is_none());
    }
}
