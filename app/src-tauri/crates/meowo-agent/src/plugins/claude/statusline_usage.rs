//! statusline 带来的订阅额度（照 Orca：Claude Code ≥2.1.80 的 statusline 载荷里有 `rate_limits`，
//! 来自 CLI 自己的 API 响应）。这是额度的主来源：不用去轮询 `/api/oauth/usage`——那个接口不公开、
//! 额度紧，调得勤就 429；也不用碰凭据。
//!
//! meowo-reporter 的 statusline 子命令写，宿主读。文件按 `CLAUDE_CONFIG_DIR` 分键（默认账号键为空串），
//! 每个账号各记各的。statusline 每秒约 3 次，内容没变且距上次写不到 15 秒就不写（Orca 同为 15 秒下限）。
use std::path::Path;

use crate::account::{UsageKind, UsageLane};

/// 与用量缓存同目录（`~/.meowo/`）。
pub const FILE: &str = "statusline-usage.json";
/// 同样的数值多久重写一次（只为刷新「更新于」）。
const REWRITE_MS: i64 = 15_000;

/// `CLAUDE_CONFIG_DIR` → 分键。默认账号（没设）是空串；路径统一斜杠、去尾斜杠，Windows 上不分大小写。
pub fn key(config_dir: Option<&str>) -> String {
    let raw = config_dir.map(str::trim).unwrap_or("");
    let k = raw.replace('\\', "/").trim_end_matches('/').to_string();
    if cfg!(windows) {
        k.to_lowercase()
    } else {
        k
    }
}

/// 从 statusline 载荷取 5 小时 / 7 天两条额度泳道。都没有（API key 计费的会话没有 `rate_limits`）返回 None。
pub fn parse(payload: &serde_json::Value) -> Option<Vec<UsageLane>> {
    let limits = payload.get("rate_limits")?;
    let lane = |name: &str, kind: UsageKind| -> Option<UsageLane> {
        let w = limits.get(name)?;
        // 字段漂移时退到 OAuth 用量接口的 `utilization` 写法（Orca 同样兜这一层）。
        let pct = w.get("used_percentage").or_else(|| w.get("utilization")).and_then(|v| v.as_f64())?;
        let resets_at = match w.get("resets_at") {
            Some(serde_json::Value::Number(n)) => n.as_i64().map(crate::codec::unix_to_iso8601),
            Some(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => None,
        };
        Some(UsageLane { kind, used_pct: Some(pct), used: None, limit: None, unit: Some("percent".into()), resets_at, label: None })
    };
    let lanes: Vec<UsageLane> = [lane("five_hour", UsageKind::FiveHour), lane("seven_day", UsageKind::SevenDay)]
        .into_iter()
        .flatten()
        .collect();
    (!lanes.is_empty()).then_some(lanes)
}

/// reporter 侧：记下这次 statusline 的额度。失败一律静默（不能拖住状态栏）。
pub fn record(path: &Path, config_dir: Option<&str>, payload_text: &str, now_ms: i64) {
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(payload_text) else { return };
    let Some(lanes) = parse(&payload) else { return };
    let Ok(lanes_json) = serde_json::to_value(&lanes) else { return };
    let mut root: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(|v: &serde_json::Value| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    let k = key(config_dir);
    if let Some(prev) = root.get(&k) {
        let same = prev.get("lanes") == Some(&lanes_json);
        let at = prev.get("at").and_then(|v| v.as_i64()).unwrap_or(0);
        if same && now_ms - at < REWRITE_MS {
            return;
        }
    }
    root[k] = serde_json::json!({ "lanes": lanes_json, "at": now_ms });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = crate::fsutil::write_atomic(path, &root.to_string());
}

/// 宿主侧：某账号最近一次 statusline 额度（泳道，记下的时刻 ms），超过 `max_age_ms` 不算。
pub fn read(path: &Path, config_dir: Option<&str>, max_age_ms: i64, now_ms: i64) -> Option<(Vec<UsageLane>, i64)> {
    let root: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let entry = root.get(key(config_dir))?;
    let at = entry.get("at")?.as_i64()?;
    if now_ms - at > max_age_ms {
        return None;
    }
    let lanes: Vec<UsageLane> = serde_json::from_value(entry.get("lanes")?.clone()).ok()?;
    Some((lanes, at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_both_windows_and_tolerates_drift() {
        let lanes = parse(&json!({"rate_limits": {
            "five_hour": {"used_percentage": 62.5, "resets_at": 1_780_000_000},
            "seven_day": {"utilization": 38, "resets_at": "2026-06-24T16:00:00Z"}
        }}))
        .unwrap();
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].kind, UsageKind::FiveHour);
        assert_eq!(lanes[0].used_pct, Some(62.5));
        assert_eq!(lanes[0].resets_at.as_deref(), Some(crate::codec::unix_to_iso8601(1_780_000_000).as_str()));
        assert_eq!(lanes[1].kind, UsageKind::SevenDay);
        assert_eq!(lanes[1].resets_at.as_deref(), Some("2026-06-24T16:00:00Z"));
        assert!(parse(&json!({"model": {}})).is_none(), "API key 计费的会话没有 rate_limits");
        assert!(parse(&json!({"rate_limits": {"five_hour": {}}})).is_none());
    }

    #[test]
    fn records_per_account_and_throttles_identical_ticks() {
        let dir = std::env::temp_dir().join(format!("meowo-sl-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE);
        let payload = |p: f64| json!({"rate_limits": {"five_hour": {"used_percentage": p}}}).to_string();
        record(&path, None, &payload(10.0), 1_000);
        record(&path, Some("C:\\Users\\a\\.meowo\\profiles\\claude\\work\\"), &payload(70.0), 1_000);
        // 同值 15 秒内不重写，「记下的时刻」不动；数值变了立刻写。
        record(&path, None, &payload(10.0), 5_000);
        let default_at = read(&path, None, i64::MAX, 5_000).map(|(_, at)| at);
        record(&path, None, &payload(11.0), 6_000);
        let default_now = read(&path, None, i64::MAX, 6_000);
        let work = read(&path, Some("c:/users/a/.meowo/profiles/claude/work"), i64::MAX, 6_000);
        let stale = read(&path, None, 1_000, 60_000);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(default_at, Some(1_000));
        let (lanes, at) = default_now.unwrap();
        assert_eq!((lanes[0].used_pct, at), (Some(11.0), 6_000));
        if cfg!(windows) {
            assert_eq!(work.unwrap().0[0].used_pct, Some(70.0), "各账号分开记，路径写法不同也认得");
        }
        assert!(stale.is_none(), "太旧的不算");
    }
}
