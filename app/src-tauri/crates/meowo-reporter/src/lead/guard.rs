//! `lead guard`（Claude 的 PreToolUse，matcher 只挂 Write|Edit|NotebookEdit）：lead-redteam 只许写
//! 任务目录下 `agents/` 里的审查报告，别的写入当场拒绝。
//!
//! 为什么要拦：Claude 版 redteam 为了把报告落盘才有 Write，「只写报告」原本只靠正文约束。
//! 不挂 Bash：每条命令都起一个进程太重；redteam 经 Bash 写文件靠主会话前后对比 git status 事后核对。
//!
//! 身份来自载荷里的 `agent_type`：Claude Code 2.1.331 实测（2026-11-24，`claude -p` + 自定义子代理
//! probe-writer + 记录 stdin 的 PreToolUse hook），子代理里的调用带 `agent_id` 与
//! `agent_type:"probe-writer"`（即 agents/*.md 的 name），主会话的调用两个字段都没有。
//!
//! 与其他 lead 模式一样，任何拿不准的情况一律放行：不是 redteam、载荷解析不了、没有路径。

use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub(super) const REVIEWER: &str = "lead-redteam";

/// 返回 Some(stdout) 表示拒绝；None 放行。
pub(super) fn check(data: &Value) -> Option<String> {
    if data.get("agent_type").and_then(Value::as_str) != Some(REVIEWER) {
        return None;
    }
    let input = data.get("tool_input")?;
    let target = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())?;
    let mut path = PathBuf::from(target);
    if path.is_relative() {
        path = Path::new(data.get("cwd").and_then(Value::as_str)?).join(path);
    }
    if report_path(&path) {
        return None;
    }
    let reason = format!(
        "lead-redteam 只能写任务目录下 agents/ 里的审查报告（如 .lead/<任务>/agents/redteam-1.md），不能写 {target}。不要改用其他方式写这个文件；把需要改的地方写进报告交给主会话。"
    );
    Some(
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }})
        .to_string(),
    )
}

/// `…/.lead/<任务>/agents/<文件>`：路径里不能有 `..`（否则可以借报告目录跳出去）。只认 agents/ 下一层：
/// `agents/snapshots/` 是主会话放前后对比快照的地方，对比时会排除它，审查者不能写进去。
fn report_path(path: &Path) -> bool {
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::ParentDir => return false,
            Component::Normal(s) => parts.push(s.to_string_lossy()),
            _ => {}
        }
    }
    // 找最后一个 `.lead`：任务目录在它下一层，报告是任务目录 agents/ 里的文件（恰好再多一段）。
    parts.iter().rposition(|p| p.eq_ignore_ascii_case(".lead")).is_some_and(|i| {
        parts.len() == i + 4
            && super::slug_ok(&parts[i + 1])
            && parts[i + 2].eq_ignore_ascii_case("agents")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code 2.1.331 实拍的子代理 PreToolUse 载荷（路径换成测试路径，content 缩短）。
    fn payload(agent_type: Option<&str>, tool: &str, input: Value) -> Value {
        let mut v = json!({"session_id": "e92ff6f4-bdd2-4485-a6f7-b2d50c2d4705", "cwd": "/p/proj",
            "permission_mode": "bypassPermissions", "hook_event_name": "PreToolUse",
            "tool_name": tool, "tool_input": input, "tool_use_id": "toolu_01JcPNRNoxZ8AJMhri2gMC6G"});
        if let Some(t) = agent_type {
            v["agent_id"] = json!("ae649ff17b52ab4c2");
            v["agent_type"] = json!(t);
        }
        v
    }

    fn denied(v: &Value) -> bool {
        check(v).is_some_and(|out| {
            let o: Value = serde_json::from_str(&out).unwrap();
            o["hookSpecificOutput"]["permissionDecision"] == "deny"
                && o["hookSpecificOutput"]["hookEventName"] == "PreToolUse"
        })
    }

    #[test]
    fn redteam_may_only_write_reports_under_task_agents_dir() {
        let w = |p: &str| payload(Some(REVIEWER), "Write", json!({"file_path": p, "content": "x"}));
        for ok in [
            "/p/proj/.lead/20261124-x/agents/redteam-1.md",
            ".lead/20261124-x/agents/redteam-2.md",
            "/p/proj/.LEAD/20261124-x/Agents/redteam-1.md",
        ] {
            assert!(!denied(&w(ok)), "{ok}");
        }
        for bad in [
            "/p/proj/src/main.rs",
            "/p/proj/.lead/20261124-x/SPEC.md",
            "/p/proj/.lead/20261124-x/acceptance.json",
            "/p/proj/.lead/20261124-x/agents",
            "/p/proj/.lead/agents/x.md",
            "/p/proj/.lead/20261124-x/agents/snapshots/redteam-1-after.txt",
            "/p/proj/.lead/20261124-x/agents/../progress.md",
            "/p/proj/.lead/20261124-x/agents/../../../src/main.rs",
            "../escape/.lead/t/agents/../../x.md",
        ] {
            assert!(denied(&w(bad)), "{bad}");
        }
        let e = payload(Some(REVIEWER), "Edit", json!({"file_path": "/p/proj/src/a.rs", "old_string": "a", "new_string": "b"}));
        assert!(denied(&e));
        let n = payload(Some(REVIEWER), "NotebookEdit", json!({"notebook_path": "/p/proj/n.ipynb", "new_source": ""}));
        assert!(denied(&n));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_from_the_real_payload_are_understood() {
        let w = |p: &str| payload(Some(REVIEWER), "Write", json!({"file_path": p, "content": "x"}));
        assert!(!denied(&w(r"C:\p\proj\.lead\20261124-x\agents\redteam-1.md")));
        assert!(denied(&w(r"C:\p\proj\.lead\20261124-x\progress.md")));
        assert!(denied(&w(r"C:\p\proj\.lead\20261124-x\agents\..\gate.json")));
    }

    /// 主会话、其他子代理、拿不准的载荷一律放行，不能因为 guard 卡住正常会话。
    #[test]
    fn everyone_else_and_malformed_payloads_pass() {
        let src = json!({"file_path": "/p/proj/src/main.rs", "content": "x"});
        assert!(check(&payload(None, "Write", src.clone())).is_none());
        assert!(check(&payload(Some("general-purpose"), "Write", src.clone())).is_none());
        assert!(check(&payload(Some("lead-redteam-x"), "Write", src)).is_none());
        assert!(check(&payload(Some(REVIEWER), "Write", json!({}))).is_none());
        let mut no_cwd = payload(Some(REVIEWER), "Write", json!({"file_path": "rel.md"}));
        no_cwd.as_object_mut().unwrap().remove("cwd");
        assert!(check(&no_cwd).is_none());
    }

    /// 走 `lead::run` 的完整入口：快速路径、坏 JSON、拒绝输出都恒为退出码 0。
    #[test]
    fn run_entry_outputs_deny_json_with_exit_zero() {
        let reg = std::env::temp_dir().join("meowo-lead-guard-unused.json");
        let bad = payload(Some(REVIEWER), "Write", json!({"file_path": "/p/proj/src/main.rs", "content": "x"}));
        let out = super::super::run("guard", &bad.to_string(), &reg);
        assert_eq!(out.exit, 0);
        assert!(out.stdout.as_deref().is_some_and(|s| s.contains("\"deny\"")));
        assert_eq!(super::super::run("guard", "{not json lead-redteam", &reg), super::super::Output::default());
        let main = payload(None, "Write", json!({"file_path": "/p/proj/src/main.rs"}));
        assert_eq!(super::super::run("guard", &main.to_string(), &reg), super::super::Output::default());
    }
}
