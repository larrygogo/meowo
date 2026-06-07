//! Claude Code 账号、实时用量、每日用量的读取与解析。
//! 纯解析/判定/合并函数在此可单测；I/O 与网络见同文件后半部分。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Account {
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DailyEntry {
    pub date: String,
    pub message_count: i64,
    pub session_count: i64,
    pub tokens: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DailyStats {
    pub days: Vec<DailyEntry>,
    pub last_computed_date: String,
}

/// 从 ~/.claude.json 的根 JSON 解析账号（取 oauthAccount）。无则 None。
pub fn parse_account(root: &serde_json::Value) -> Option<Account> {
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
    Some(Account { email, display_name, organization, plan })
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

/// 从 stats-cache.json 根 JSON 取最近 max_days 天（dailyActivity ⨝ dailyModelTokens by date）。
pub fn parse_daily(root: &serde_json::Value, max_days: usize) -> Option<DailyStats> {
    let activity = root.get("dailyActivity")?.as_array()?;
    let last_computed_date = root
        .get("lastComputedDate")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // date -> 该日各模型 token 求和
    let mut tokens_by_date: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    if let Some(dmt) = root.get("dailyModelTokens").and_then(|v| v.as_array()) {
        for e in dmt {
            let date = e.get("date").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let sum: i64 = e
                .get("tokensByModel")
                .and_then(|m| m.as_object())
                .map(|m| m.values().filter_map(|x| x.as_i64()).sum())
                .unwrap_or(0);
            *tokens_by_date.entry(date).or_insert(0) += sum;
        }
    }
    let mut days: Vec<DailyEntry> = activity
        .iter()
        .map(|e| {
            let date = e.get("date").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let tokens = tokens_by_date.get(&date).copied().unwrap_or(0);
            DailyEntry {
                message_count: e.get("messageCount").and_then(|v| v.as_i64()).unwrap_or(0),
                session_count: e.get("sessionCount").and_then(|v| v.as_i64()).unwrap_or(0),
                tokens,
                date,
            }
        })
        .collect();
    // 取最近 max_days（数组按日期升序，取末尾）。
    if days.len() > max_days {
        days = days.split_off(days.len() - max_days);
    }
    Some(DailyStats { days, last_computed_date })
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
    fn parse_daily_joins_and_limits() {
        let root = json!({
            "lastComputedDate":"2026-05-17",
            "dailyActivity":[
                {"date":"2026-05-15","messageCount":10,"sessionCount":1,"toolCallCount":5},
                {"date":"2026-05-16","messageCount":20,"sessionCount":2,"toolCallCount":8},
                {"date":"2026-05-17","messageCount":40,"sessionCount":4,"toolCallCount":34}
            ],
            "dailyModelTokens":[
                {"date":"2026-05-16","tokensByModel":{"opus":100,"sonnet":50}},
                {"date":"2026-05-17","tokensByModel":{"opus":8568}}
            ]
        });
        let d = parse_daily(&root, 2).unwrap();
        assert_eq!(d.last_computed_date, "2026-05-17");
        assert_eq!(d.days.len(), 2);
        assert_eq!(d.days[0].date, "2026-05-16");
        assert_eq!(d.days[0].tokens, 150);
        assert_eq!(d.days[1].date, "2026-05-17");
        assert_eq!(d.days[1].tokens, 8568);
        assert_eq!(d.days[1].message_count, 40);
    }

    #[test]
    fn is_token_expired_margin() {
        let now = 1_000_000_000_000i64;
        assert!(is_token_expired(now, now));
        assert!(is_token_expired(now + 30_000, now));
        assert!(!is_token_expired(now + 120_000, now));
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
}
