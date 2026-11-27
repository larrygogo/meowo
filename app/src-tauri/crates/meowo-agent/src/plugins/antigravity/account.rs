//! Antigravity 的账号与用量（只在 Windows 注册，见 mod.rs `account()`）。
//!
//! # 凭据从哪来
//!
//! agy 用 go-keyring 把 OAuth token 存进系统凭据库：Windows 凭据管理器里 target 为
//! `gemini:antigravity`（cmdkey /list 实测，1.5.18），内容是一段 JSON：
//! `{"token":{"access_token","token_type","refresh_token","expiry","expires_in"},"auth_method","id_token_email","writer_id"}`。
//! `expiry` 是 Go 写的 RFC 3339（带本地时区偏移，如 `2026-11-26T22:34:01.93+08:00`）。
//! 我们**只读不写**：token 由 agy 自己续期（有效期 1 小时），过期了就请用户开一次 agy。
//!
//! macOS 读别家的 Keychain 条目会弹系统授权框，所以那边不注册这项能力（卡片照旧只显示版本）。
//!
//! # 用量接口
//!
//! 服务地址是 `https://daily-cloudcode-pa.googleapis.com/v1internal:`，这是 agy 命令行版的默认值：
//! 它的运行日志里 loadCodeAssist / fetchAvailableModels 都打到这里，程序文件里 `cloud_code_endpoint`
//! 的默认值也是它。不要换成 `cloudcode-pa`（不带 daily）：那边同一个账号、同一个项目也能查，
//! 但额度永远是满的。2026-11-26 实测，跑完一段 Claude Sonnet 后 daily 这边 5 小时额度掉了 3%，
//! 另一边纹丝不动；0.8.8 安装版就因为查错地址一直显示 0%。
//!
//! 两个接口，body 都是 `{"project": <agy 缓存的项目号>}`，项目号在 `<data_dir>/cache/default_project_id.txt`：
//! - `retrieveUserQuotaSummary`：`groups[].buckets[]`，每个 bucket 有 `window`（`5h` / `weekly`）和
//!   `remainingFraction`（剩余比例）。个人账号只返回一组「Claude and GPT models」。
//! - `retrieveUserQuota`：按模型列 `buckets[]{modelId, remainingFraction, resetTime}`。汇总里没有 Gemini，
//!   只能从这里取。实测所有 `gemini-*` 模型的数字完全一样，是同一份额度：用 Gemini 4 Neon、
//!   Gemini 3.8 Flash 都扣这一份，Claude 那几档模型在这里的数字不动（它们算在汇总里）。
//!   窗口是 5 小时：当天第一次用 agy 在 12:51:01Z，Gemini 的 `resetTime` 是 17:51:02Z。
//!
//! 两个接口都要求：带 project（否则 403 SUBSCRIPTION_REQUIRED），User-Agent 以 `antigravity` 开头
//! （否则同样 403；`antigravity`、`antigravity/1.0.0 windows/amd64` 都行，版本号不校验）。

use std::time::Duration;

use serde_json::Value;

use crate::account::{Account, AccountCap, ProviderUsage, UsageKind, UsageLane, USAGE_UNSUPPORTED};
use crate::ports::{Body, HttpError, HttpRequest, Ports};
use crate::variant::Installation;

const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
/// agy 命令行版的服务地址（见模块注释，别换成不带 daily 的那个）。
const SUMMARY_URL: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";
const PER_MODEL_URL: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";
/// 汇总接口不含 Gemini，Meowo 自己补一行，组名用品牌名，不用翻译。
const GEMINI_LABEL: &str = "Gemini";
/// 服务端按 UA 前缀认客户端，别的 UA 一律 403（见模块注释）。
const USER_AGENT: &str = "antigravity";
/// go-keyring 的 service / user（Windows target 即 `gemini:antigravity`）。
const KEYRING_SERVICE: &str = "gemini";
const KEYRING_USER: &str = "antigravity";

pub struct AntigravityAccount;
pub static ACCOUNT: AntigravityAccount = AntigravityAccount;

/// 凭据库里那段 JSON 里我们要的几样。
#[derive(Debug, PartialEq)]
struct StoredToken {
    access_token: String,
    /// Unix 秒；解析不出时为 None（当作未过期，交给服务端判）。
    expires_at: Option<i64>,
    email: Option<String>,
}

fn parse_stored_token(text: &str) -> Option<StoredToken> {
    let v: Value = serde_json::from_str(text).ok()?;
    let token = v.get("token")?;
    let access_token = token.get("access_token")?.as_str().filter(|s| !s.is_empty())?.to_string();
    let expires_at = token.get("expiry").and_then(Value::as_str).and_then(rfc3339_to_unix);
    let email = v.get("id_token_email").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    Some(StoredToken { access_token, expires_at, email })
}

fn read_token(ports: &Ports) -> Option<StoredToken> {
    parse_stored_token(&ports.keychain.read_foreign_secret(KEYRING_SERVICE, KEYRING_USER)?)
}

/// RFC 3339（`Z` 或 `±HH:MM` 结尾）→ Unix 秒。[`crate::codec::iso8601_to_unix_ms`] 只认 `Z`，
/// 这里先把偏移剥下来再折回 UTC。
fn rfc3339_to_unix(s: &str) -> Option<i64> {
    if s.ends_with('Z') {
        return crate::codec::iso8601_to_unix_ms(s).map(|ms| ms / 1000);
    }
    let (base, off) = s.split_at(s.len().checked_sub(6)?);
    let sign = match off.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = off[1..].split_once(':')?;
    let offset = sign * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60);
    let local = crate::codec::iso8601_to_unix_ms(&format!("{base}Z"))? / 1000;
    Some(local - offset)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn project_id(inst: &Installation) -> Option<String> {
    let text = std::fs::read_to_string(inst.data_dir.join("cache").join("default_project_id.txt")).ok()?;
    let id = text.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// `retrieveUserQuotaSummary` 的响应 → 用量泳道。
///
/// 每条都带上组名：这份额度只管 Claude 和 GPT 模型，只写「每周配额」会被当成总额度。`remainingFraction` 是剩余比例，换成已用百分比；重置时间字段没在满额时
/// 见过，按 `resetTime` 读、读不到就不显示。
pub(super) fn parse_quota_summary(v: &Value) -> Option<ProviderUsage> {
    let groups = v.get("groups")?.as_array()?;
    let mut lanes = Vec::new();
    for group in groups {
        let name = group.get("displayName").and_then(Value::as_str).filter(|s| !s.is_empty());
        for bucket in group.get("buckets").and_then(Value::as_array).into_iter().flatten() {
            let kind = match bucket.get("window").and_then(Value::as_str) {
                Some("5h") => UsageKind::FiveHour,
                Some("weekly") => UsageKind::Weekly,
                _ => continue,
            };
            let Some(remaining) = bucket.get("remainingFraction").and_then(Value::as_f64) else { continue };
            lanes.push(UsageLane {
                kind,
                used_pct: Some(used_pct(remaining)),
                used: None,
                limit: None,
                unit: Some("percent".into()),
                resets_at: bucket.get("resetTime").and_then(Value::as_str).map(str::to_string),
                label: name.map(str::to_string),
            });
        }
    }
    (!lanes.is_empty()).then_some(ProviderUsage { lanes, note: None, plan: None })
}

fn used_pct(remaining: f64) -> f64 {
    ((1.0 - remaining.clamp(0.0, 1.0)) * 1000.0).round() / 10.0
}

/// `retrieveUserQuota`（按模型）的响应 → Gemini 那一行。
///
/// 所有 `gemini-*` 共用一份额度（见模块注释），取剩余最少的那个，万一以后拆开了也不会显示得比实际宽松。
/// 只认带 `resetTime` 的：`tab_*`、`chat_*` 这类补全/内部模型没有窗口，不算。
pub(super) fn parse_gemini_lane(v: &Value) -> Option<UsageLane> {
    let mut worst: Option<(f64, Option<String>)> = None;
    for b in v.get("buckets")?.as_array()? {
        let Some(model) = b.get("modelId").and_then(Value::as_str) else { continue };
        let Some(reset) = b.get("resetTime").and_then(Value::as_str) else { continue };
        let Some(remaining) = b.get("remainingFraction").and_then(Value::as_f64) else { continue };
        if !model.starts_with("gemini-") {
            continue;
        }
        if worst.as_ref().is_none_or(|(r, _)| remaining < *r) {
            worst = Some((remaining, Some(reset.to_string())));
        }
    }
    let (remaining, resets_at) = worst?;
    Some(UsageLane {
        kind: UsageKind::FiveHour,
        used_pct: Some(used_pct(remaining)),
        used: None,
        limit: None,
        unit: Some("percent".into()),
        resets_at,
        label: Some(GEMINI_LABEL.into()),
    })
}

fn post_quota(ports: &Ports, url: &str, bearer: &str, project: &str) -> Result<Value, String> {
    let text = match ports.http.send(&HttpRequest {
        method: "POST",
        url,
        headers: &[("Authorization", bearer), ("User-Agent", USER_AGENT)],
        body: Body::Json(serde_json::json!({ "project": project })),
        timeout: HTTP_TIMEOUT,
    }) {
        Ok(t) => t,
        Err(HttpError::Status(code, _)) => return Err(format!("POST {url} 返回 HTTP {code}")),
        Err(e) => return Err(format!("POST {url} 网络错误：{e}")),
    };
    serde_json::from_str(&text).map_err(|e| format!("配额响应不是合法 JSON：{e}"))
}

impl AccountCap for AntigravityAccount {
    /// 只读本地凭据库（CredReadW，不联网），轮询登录态时调用也便宜。
    fn account(&self, _inst: &Installation, ports: &Ports) -> Option<Account> {
        let token = read_token(ports)?;
        Some(Account { email: token.email, display_name: None, organization: None, plan: None, login_label: None })
    }

    fn fetch_usage(&self, inst: &Installation, ports: &Ports) -> Result<ProviderUsage, String> {
        let Some(token) = read_token(ports) else { return Err(USAGE_UNSUPPORTED.into()) };
        // 我们不替 agy 续期（不碰它的 refresh token），过期了直接说，不白发一次必然 401 的请求。
        if token.expires_at.is_some_and(|t| t <= now_secs()) {
            return Err("antigravity/token_expired：登录凭据已过期，打开一次 Antigravity 会话后再刷新".into());
        }
        let Some(project) = project_id(inst) else {
            return Err("antigravity/no_project：没找到 agy 的项目号，打开一次 Antigravity 会话后再刷新".into());
        };
        let bearer = format!("Bearer {}", token.access_token);
        let summary = post_quota(ports, SUMMARY_URL, &bearer, &project)?;
        let mut usage =
            parse_quota_summary(&summary).ok_or("配额响应里没有可显示的额度（响应形状可能变了）")?;
        // Gemini 那一行是补充：按模型的接口失败了，照样显示 Claude/GPT 两行，不让整张卡报错。
        if let Some(lane) = post_quota(ports, PER_MODEL_URL, &bearer, &project).ok().as_ref().and_then(parse_gemini_lane) {
            usage.lanes.push(lane);
        }
        Ok(usage)
    }

    fn usage_supported(&self, _inst: &Installation, ports: &Ports) -> bool {
        read_token(ports).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 2026-11-26 实拍的响应（个人账号，额度未用）。
    fn real_response() -> Value {
        json!({
            "groups": [{
                "buckets": [
                    {"bucketId": "3p-weekly", "displayName": "Weekly Limit Remaining", "window": "weekly", "remainingFraction": 1},
                    {"bucketId": "3p-5h", "displayName": "Five Hour Limit Remaining", "window": "5h", "remainingFraction": 1}
                ],
                "displayName": "Claude and GPT models",
                "description": "Models within this group: Claude Opus 5.5, Claude Sonnet 5.5, GPT-OSS 120B"
            }],
            "description": "Within each group, models share a weekly limit and a 5-hour limit."
        })
    }

    #[test]
    fn real_response_maps_to_labelled_lanes() {
        let u = parse_quota_summary(&real_response()).unwrap();
        assert_eq!(u.lanes.len(), 2);
        assert_eq!(u.lanes[0].kind, UsageKind::Weekly);
        assert_eq!(u.lanes[1].kind, UsageKind::FiveHour);
        assert!(u.lanes.iter().all(|l| l.used_pct == Some(0.0) && l.label.as_deref() == Some("Claude and GPT models")));
    }

    #[test]
    fn remaining_fraction_becomes_used_percent() {
        let v = json!({"groups": [
            {"displayName": "Claude and GPT models", "buckets": [{"window": "5h", "remainingFraction": 0.25, "resetTime": "2026-11-27T03:00:00Z"}]},
            {"displayName": "Gemini models", "buckets": [{"window": "weekly", "remainingFraction": 0.903}, {"window": "daily", "remainingFraction": 0.5}]}
        ]});
        let u = parse_quota_summary(&v).unwrap();
        assert_eq!(u.lanes.len(), 2, "认不出的窗口跳过");
        assert_eq!(u.lanes[0].used_pct, Some(75.0));
        assert_eq!(u.lanes[0].resets_at.as_deref(), Some("2026-11-27T03:00:00Z"));
        assert_eq!(u.lanes[0].label.as_deref(), Some("Claude and GPT models"));
        assert_eq!(u.lanes[1].used_pct, Some(9.7));
        assert_eq!(u.lanes[1].label.as_deref(), Some("Gemini models"));
    }

    /// 2026-11-26 daily 地址实拍的按模型响应（节选，跑过 Gemini 4 Neon 和 Gemini 3.8 Flash 之后）。
    fn real_per_model_response() -> Value {
        json!({"buckets": [
            {"tokenType": "WTUS", "modelId": "chat_20706", "remainingFraction": 1},
            {"resetTime": "2026-11-26T20:50:36Z", "tokenType": "WTUS", "modelId": "claude-opus-5-5-high", "remainingFraction": 1},
            {"resetTime": "2026-11-26T17:51:02Z", "tokenType": "WTUS", "modelId": "gemini-3-flash", "remainingFraction": 0.9163317},
            {"resetTime": "2026-11-26T17:51:02Z", "tokenType": "WTUS", "modelId": "gemini-4-hydrogen-sonar", "remainingFraction": 0.9163317},
            {"resetTime": "2026-11-26T20:50:36Z", "tokenType": "WTUS", "modelId": "gpt-oss-120b-medium", "remainingFraction": 1},
            {"tokenType": "WTUS", "modelId": "tab_flash_lite_preview", "remainingFraction": 1}
        ]})
    }

    #[test]
    fn gemini_lane_comes_from_per_model_buckets() {
        let lane = parse_gemini_lane(&real_per_model_response()).unwrap();
        assert_eq!(lane.kind, UsageKind::FiveHour);
        assert_eq!(lane.used_pct, Some(8.4));
        assert_eq!(lane.resets_at.as_deref(), Some("2026-11-26T17:51:02Z"));
        assert_eq!(lane.label.as_deref(), Some("Gemini"));
    }

    #[test]
    fn gemini_lane_takes_the_tightest_bucket_and_skips_windowless_models() {
        let v = json!({"buckets": [
            {"resetTime": "2026-11-26T17:00:00Z", "modelId": "gemini-2.5-pro", "remainingFraction": 0.9},
            {"resetTime": "2026-11-26T18:00:00Z", "modelId": "gemini-3-flash", "remainingFraction": 0.4},
            {"modelId": "gemini-x-internal", "remainingFraction": 0.0},
            {"resetTime": "2026-11-26T18:00:00Z", "modelId": "claude-sonnet-5-5-low", "remainingFraction": 0.1}
        ]});
        let lane = parse_gemini_lane(&v).unwrap();
        assert_eq!(lane.used_pct, Some(60.0));
        assert_eq!(lane.resets_at.as_deref(), Some("2026-11-26T18:00:00Z"));
        assert!(parse_gemini_lane(&json!({"buckets": [{"modelId": "claude-opus-5-5-high", "resetTime": "x", "remainingFraction": 1}]})).is_none());
    }

    #[test]
    fn empty_or_unknown_response_is_none() {
        assert!(parse_quota_summary(&json!({"groups": []})).is_none());
        assert!(parse_quota_summary(&json!({"error": {}})).is_none());
    }

    #[test]
    fn stored_token_reads_expiry_with_offset() {
        // 形状照凭据库实读（值已替换）。
        let text = r#"{"token":{"access_token":"ya29.x","token_type":"Bearer","refresh_token":"1//y","expiry":"2026-11-26T22:34:01.9303895+08:00","expires_in":3599},"auth_method":"consumer","id_token_email":"a@b.com","writer_id":"w"}"#;
        let t = parse_stored_token(text).unwrap();
        assert_eq!(t.access_token, "ya29.x");
        assert_eq!(t.email.as_deref(), Some("a@b.com"));
        // 22:34:01+08:00 = 14:34:01Z
        assert_eq!(t.expires_at, rfc3339_to_unix("2026-11-26T14:34:01Z"));
        assert!(parse_stored_token(r#"{"token":{"access_token":""}}"#).is_none());
    }

    #[test]
    fn rfc3339_handles_negative_offset_and_garbage() {
        assert_eq!(rfc3339_to_unix("2026-11-26T09:34:01-05:00"), rfc3339_to_unix("2026-11-26T14:34:01Z"));
        assert_eq!(rfc3339_to_unix("garbage"), None);
    }
}
