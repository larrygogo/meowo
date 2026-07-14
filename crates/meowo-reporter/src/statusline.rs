//! Claude Code statusline 集成：CC 每次渲染状态栏会把会话 JSON 传给 statusline 命令的 stdin，
//! 其中 `context_window` 带有「准确的窗口大小与已用百分比」（transcript / hook 都拿不到）。
//! 这里解析出来写入 `session_context` 表，供 meowo-app 准确显示，再把原始 stdin 透传给下游 HUD。
use meowo_store::Store;

/// 解析 statusline JSON 并写库（best-effort：任何字段缺失或解析失败都静默跳过，绝不影响透传）。
/// 字段路径：`session_id`、`context_window.used_percentage`、`context_window.context_window_size`。
pub fn record(store: &Store, input: &str, now_ms: i64) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(input) else {
        return;
    };
    let Some(sid) = v.get("session_id").and_then(|x| x.as_str()) else {
        return;
    };
    let cw = v.get("context_window");
    // used_percentage 可能是整数或小数（如 23.5），统一四舍五入为整数。
    let used_pct = cw
        .and_then(|c| c.get("used_percentage"))
        .and_then(|x| x.as_f64())
        .map(|f| f.round() as i64);
    let window = cw
        .and_then(|c| c.get("context_window_size"))
        .and_then(|x| x.as_i64());
    let model = v
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(|x| x.as_str());
    let _ = store.set_session_context(sid, used_pct, window, model, now_ms);
}

/// 无下游 statusLine 时 meowo-reporter 自渲染的极简状态栏：`<模型> · NN% ctx`。
/// 字段缺失则尽量降级；全缺则空串。用于「装了 Meowo 但没有其它 statusLine」的用户。
pub fn minimal_line(input: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(input) else {
        return String::new();
    };
    let model = v
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(|x| x.as_str());
    let pct = v
        .get("context_window")
        .and_then(|c| c.get("used_percentage"))
        .and_then(|x| x.as_f64())
        .map(|p| p.round() as i64);
    match (model, pct) {
        (Some(m), Some(p)) => format!("{m} · {p}% ctx"),
        (Some(m), None) => m.to_string(),
        (None, Some(p)) => format!("{p}% ctx"),
        (None, None) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_writes_context_for_session() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
        let _ = store.start_session(pid, "sess-1", 1).unwrap();
        let json = r#"{"session_id":"sess-1","context_window":{"used_percentage":42,"context_window_size":1000000}}"#;
        record(&store, json, 100);
        let live = store.live_sessions(None, None, None, None, 1000).unwrap();
        let s = live
            .iter()
            .find(|l| l.session.cc_session_id == "sess-1")
            .unwrap();
        assert_eq!(s.context_pct, Some(42));
        assert_eq!(s.context_window, Some(1_000_000));
    }

    #[test]
    fn record_rounds_fractional_percentage() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
        let _ = store.start_session(pid, "s2", 1).unwrap();
        record(
            &store,
            r#"{"session_id":"s2","context_window":{"used_percentage":23.6,"context_window_size":200000}}"#,
            1,
        );
        let live = store.live_sessions(None, None, None, None, 1000).unwrap();
        let s = live
            .iter()
            .find(|l| l.session.cc_session_id == "s2")
            .unwrap();
        assert_eq!(s.context_pct, Some(24));
    }

    #[test]
    fn minimal_line_renders_model_and_pct() {
        let j = r#"{"model":{"display_name":"Opus"},"context_window":{"used_percentage":32}}"#;
        assert_eq!(minimal_line(j), "Opus · 32% ctx");
        assert_eq!(minimal_line(r#"{"model":{"display_name":"Opus"}}"#), "Opus");
        assert_eq!(minimal_line("garbage"), "");
    }

    #[test]
    fn record_ignores_bad_or_incomplete_json() {
        let store = Store::open_in_memory().unwrap();
        // 不 panic、不写入即可。
        record(&store, "not json at all", 1);
        record(&store, r#"{"no_session":true}"#, 1);
    }

    #[test]
    fn record_writes_model_for_session() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
        let _ = store.start_session(pid, "sm-1", 1).unwrap();
        let json = r#"{"session_id":"sm-1","model":{"display_name":"Opus"},"context_window":{"used_percentage":10,"context_window_size":200000}}"#;
        record(&store, json, 100);
        let live = store.live_sessions(None, None, None, None, 1000).unwrap();
        let s = live
            .iter()
            .find(|l| l.session.cc_session_id == "sm-1")
            .unwrap();
        assert_eq!(s.model.as_deref(), Some("Opus"));
    }

    #[test]
    fn record_missing_model_keeps_previous() {
        let store = Store::open_in_memory().unwrap();
        let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
        let _ = store.start_session(pid, "sm-2", 1).unwrap();
        record(
            &store,
            r#"{"session_id":"sm-2","model":{"display_name":"Sonnet"}}"#,
            1,
        );
        // 后续 statusline 不带 model（仅上下文）→ 不应抹掉已存的模型
        record(
            &store,
            r#"{"session_id":"sm-2","context_window":{"used_percentage":20}}"#,
            2,
        );
        let live = store.live_sessions(None, None, None, None, 1000).unwrap();
        let s = live
            .iter()
            .find(|l| l.session.cc_session_id == "sm-2")
            .unwrap();
        assert_eq!(s.model.as_deref(), Some("Sonnet"));
    }
}
