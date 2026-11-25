use super::*;

const D: &str = "team-developer";
const R: &str = "team-reviewer";
const P: &str = "team-pm";

/// Claude Code 2.1.332 实拍的子代理 PreToolUse 载荷形状（2026-11-25 探针）：子代理带 agent_id 与
/// agent_type（agents/*.md 的 name），主会话两个字段都没有。
fn payload(agent: Option<&str>, tool: &str, input: Value) -> String {
    let mut v = json!({"session_id": "e92ff6f4-bdd2-4485-a6f7-b2d50c2d4705", "cwd": "/p/proj",
        "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input});
    if let Some(agent) = agent {
        v["agent_id"] = json!("a933751836f0fb28b");
        v["agent_type"] = json!(agent);
    }
    v.to_string()
}

fn temp() -> Vec<PathBuf> {
    vec![PathBuf::from(r"C:\Users\me\AppData\Local\Temp"), PathBuf::from("/tmp")]
}

fn config() -> Config {
    Config::parse(
        r#"{"protected": ["acme-*", "secret-repo", "D:/work/client"],
            "blocked_commands": [{"pattern": "notify\\.py\\s+send", "what": "发通知"}, {"pattern": "(", "what": "写错的正则"}]}"#,
    )
}

fn decide(agent: Option<&str>, tool: &str, input: Value) -> &'static str {
    match check(&payload(agent, tool, input), config, &temp()) {
        None => "allow",
        Some(out) => {
            let v: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
            assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny");
            "deny"
        }
    }
}

fn bash(agent: Option<&str>, command: &str) -> &'static str {
    decide(agent, "Bash", json!({ "command": command }))
}

fn write(agent: Option<&str>, path: &str) -> &'static str {
    decide(agent, "Write", json!({"file_path": path, "content": "x"}))
}

const DENY_CMDS: &[&str] = &[
    "git push", "git -C /x push origin main", "git push --force", "bash -c \"git push\"", "cmd //c git push",
    "gh pr create --fill", "gh pr merge 3 --squash", "gh pr review 3 --approve", "gh pr comment 3 -b x",
    "gh release create v1", "gh workflow run ci.yml", "gh run rerun 123", "gh api -X POST repos/a/b/issues",
    "gh api repos/a/b/pulls -f title=x", "kubectl get pods", "helm upgrade x y", "npm publish", "cargo publish",
    "lark-cli im +messages-send --text hi", "python -I notify.py send 3",
];
const ALLOW_CMDS: &[&str] = &[
    "git status --short", "git commit -m 'x'", "git log --oneline -3", "npm test", "npm run typecheck",
    "gh pr view 3", "gh api repos/a/b/pulls", "gh run view 123 --log-failed", "node server.js",
];

#[test]
fn outward_actions_are_denied_for_every_team_role_in_bash_and_powershell() {
    for role in [D, R, P, "team-designer"] {
        for c in DENY_CMDS {
            assert_eq!(bash(Some(role), c), "deny", "{role} {c}");
            assert_eq!(decide(Some(role), "PowerShell", json!({ "command": c })), "deny", "{role} PowerShell {c}");
        }
    }
    for c in ALLOW_CMDS {
        assert_eq!(bash(Some(D), c), "allow", "{c}");
    }
}

#[test]
fn main_session_and_other_subagents_are_untouched() {
    for c in DENY_CMDS {
        assert_eq!(bash(None, c), "allow", "主会话 {c}");
        assert_eq!(bash(Some("general-purpose"), c), "allow", "其他子代理 {c}");
        assert_eq!(bash(Some("lead-redteam"), c), "allow", "lead guard 管的角色 {c}");
    }
    assert_eq!(write(None, "/p/proj/acme-api/a.rs"), "allow");
    assert_eq!(write(Some("Explore"), "/p/proj/src/a.rs"), "allow");
}

#[test]
fn protected_dirs_come_from_the_personal_config() {
    assert_eq!(write(Some(D), r"C:\Users\me\work\acme-dashboard\a.java"), "deny");
    assert_eq!(decide(Some(D), "Edit", json!({"file_path": "/home/me/work/ACME-api/src/x.rs"})), "deny");
    assert_eq!(write(Some(D), "/home/me/secret-repo/x.md"), "deny");
    assert_eq!(write(Some(D), r"D:\work\client\sub\x.md"), "deny", "路径前缀，反斜杠");
    assert_eq!(write(Some(D), "/home/me/secret-repo-not/x.md"), "allow", "目录名整段匹配");
    assert_eq!(bash(Some(D), "git -C /home/me/acme-admin commit -am x"), "deny");
    assert_eq!(bash(Some(D), "echo x > /home/me/acme-admin/a.txt"), "deny");
    assert_eq!(bash(Some(D), "cat /home/me/acme-admin/README.md"), "allow", "只读放行");
    assert_eq!(bash(Some(D), "git -C \"D:/work/client\" log"), "allow");
    assert_eq!(write(Some(D), "/home/me/pet/server.js"), "allow");
    // 受保护目录也挡受限角色在那里的 .lead
    assert_eq!(write(Some(P), "/home/me/acme-gui/.lead/rules.md"), "deny");
}

#[test]
fn without_personal_config_nothing_is_protected() {
    let out = check(&payload(Some(D), "Write", json!({"file_path": "/home/me/acme-x/a.rs"})), Config::default, &temp());
    assert!(out.is_none());
    // 写错的个人层文件当作空配置
    assert!(Config::parse("{not json").protected.is_empty());
    assert!(Config::load(None).protected.is_empty());
}

#[test]
fn pm_and_reviewer_may_only_write_lead_dirs_and_temp() {
    for role in [P, R] {
        assert_eq!(write(Some(role), r"C:\Users\me\work\pet\.lead\t\agents\reviewer-1.md"), "allow", "{role}");
        assert_eq!(write(Some(role), r"C:\Users\me\.meowo\projects\x\.lead\20261125-t\agents\pm-1.md"), "allow", "{role}");
        assert_eq!(write(Some(role), r"C:\Users\me\AppData\Local\Temp\claude\x\scratchpad\t.py"), "allow", "{role}");
        assert_eq!(write(Some(role), "/tmp/x.py"), "allow", "{role}");
        assert_eq!(write(Some(role), ".lead/rules.md"), "allow", "相对路径按 cwd 解析 {role}");
        assert_eq!(write(Some(role), r"C:\Users\me\work\pet\server.js"), "deny", "{role}");
        assert_eq!(write(Some(role), "/p/proj/.lead/t/../../src/a.rs"), "deny", "不能借 .. 跳出 {role}");
        assert_eq!(write(Some(role), "/tmpx/a.py"), "deny", "临时目录要整段匹配 {role}");
        assert_eq!(decide(Some(role), "NotebookEdit", json!({"notebook_path": "/p/proj/n.ipynb"})), "deny", "{role}");
    }
    assert_eq!(write(Some(D), "/p/proj/src/a.rs"), "allow", "开发能写源码");
    assert_eq!(write(Some("team-designer"), "/p/proj-design-x/index.html"), "allow", "设计师能写样稿");
}

/// 团队规则 R12 边角 2：载荷里有 agent_id 却没有 agent_type（格式变了、认不出是谁）→ 按最严的团队角色处理。
#[test]
fn unidentified_subagent_is_treated_as_the_strictest_role() {
    let raw = |tool: &str, input: Value| {
        json!({"cwd": "/p/proj", "hook_event_name": "PreToolUse", "agent_id": "a1", "tool_name": tool, "tool_input": input}).to_string()
    };
    assert!(check(&raw("Bash", json!({"command": "git push"})), config, &temp()).is_some());
    assert!(check(&raw("Write", json!({"file_path": "/p/proj/src/a.rs"})), config, &temp()).is_some());
    assert!(check(&raw("Write", json!({"file_path": "/p/proj/.lead/t/agents/x.md"})), config, &temp()).is_none());
    // 主会话：没有 agent_id
    let main = json!({"cwd": "/p/proj", "tool_name": "Bash", "tool_input": {"command": "git push"}}).to_string();
    assert!(check(&main, config, &temp()).is_none());
}

#[test]
fn malformed_payloads_pass() {
    for raw in ["", "{", "[1]", r#"{"agent_type": 3}"#, r#"{"agent_type": "team-pm"}"#] {
        assert!(check(raw, config, &temp()).is_none(), "{raw}");
    }
}

#[test]
fn glob_matches_whole_names() {
    assert!(glob("acme-*", "acme-api"));
    assert!(glob("*secret*", "my-secret-repo"));
    assert!(glob("a*b*c", "axxbyyc"));
    assert!(!glob("acme-*", "xacme-api"));
    assert!(!glob("acme", "acme-api"));
    assert!(glob("*", ""));
}

/// 主会话的每条命令都会进守卫：不是子代理的载荷不能走到 JSON 解析和读个人层。
#[test]
fn main_session_never_loads_the_personal_config() {
    let main = payload(None, "Bash", json!({"command": "git push"}));
    let out = check(&main, || panic!("主会话不该读个人层"), &temp());
    assert!(out.is_none());
}
