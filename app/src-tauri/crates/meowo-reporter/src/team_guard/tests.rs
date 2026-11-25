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

// ---- 做空 redteam-1 的复现（4904c94a 上都被放行） ----

fn in_dir(agent: &str, cwd: &str, tool: &str, input: Value) -> &'static str {
    let raw = json!({"cwd": cwd, "hook_event_name": "PreToolUse", "agent_id": "a1", "agent_type": agent, "tool_name": tool, "tool_input": input}).to_string();
    if check(&raw, config, &temp()).is_some() { "deny" } else { "allow" }
}

/// G1-1：主会话开在受保护仓库里，命令不带路径。
#[test]
fn mutating_commands_inside_a_protected_cwd_are_denied() {
    let repo = "/home/me/work/acme-gui";
    for c in ["git commit -am x", "rm -rf src", "git add -A", "echo x > a.txt", "git stash"] {
        assert_eq!(in_dir(D, repo, "Bash", json!({ "command": c })), "deny", "{c}");
    }
    for c in ["git status", "git log --oneline", "cat README.md", "cargo test 2>&1 | tail -5", "ls > /dev/null"] {
        assert_eq!(in_dir(D, repo, "Bash", json!({ "command": c })), "allow", "{c}");
    }
}

/// G1-2：产品经理和审查员用 Bash/PowerShell 写源码。
#[test]
fn limited_roles_cannot_write_through_the_shell() {
    let src = "/home/me/work/pet";
    for (role, c) in [
        (R, "echo x > /home/me/work/pet/src/a.rs"),
        (R, "printf 'x' > src/a.rs"),
        (P, "sed -i s/a/b/ src/a.rs"),
        (P, "Set-Content -Path /home/me/work/pet/src/a.rs -Value x"),
        (R, "rm -rf src"),
        (R, "echo x > /tmp/ok.txt && echo y > src/b.rs"),
    ] {
        assert_eq!(in_dir(role, src, "Bash", json!({ "command": c })), "deny", "{role} {c}");
    }
    for (role, c) in [
        (R, "echo x > /tmp/report.txt"),
        (R, r"echo x > C:\Users\me\AppData\Local\Temp\x\report.txt"),
        (P, "echo x > /home/me/.meowo/projects/p/.lead/t/agents/pm-1.md"),
        (R, "cargo test 2>&1 | tail -20"),
        (R, "npm test > $null"),
        (R, "git diff --stat"),
    ] {
        assert_eq!(in_dir(role, src, "Bash", json!({ "command": c })), "allow", "{role} {c}");
    }
    // 开发不受这条限制
    assert_eq!(in_dir(D, src, "Bash", json!({"command": "echo x > src/a.rs"})), "allow");
    // 认不出身份的子代理同样受限
    let raw = json!({"cwd": src, "agent_id": "a1", "tool_name": "Bash", "tool_input": {"command": "echo x > src/a.rs"}}).to_string();
    assert!(check(&raw, config, &temp()).is_some());
}

/// G1-3：不用 lark-cli 也能发消息、改远端。
#[test]
fn outbound_write_requests_are_denied() {
    for c in [
        "gh issue comment 12 --body hi",
        "gh issue create -t x -b y",
        "curl -X POST https://open.example.com/hook/xxx -d '{}'",
        "curl https://hooks.example.com/x --data '{\"text\":\"hi\"}'",
        "curl.exe --json '{}' https://api.example.com/x",
        "wget --post-data=a https://x.example.com",
        "Invoke-RestMethod -Method Post -Uri https://hooks.example.com/x -Body $b",
        "iwr https://x.example.com -Body 'a'",
    ] {
        assert_eq!(bash(Some(D), c), "deny", "{c}");
    }
    for c in [
        "curl https://example.com/readme",
        "curl -s http://localhost:3000/api -d '{}'",
        "curl -X POST http://127.0.0.1:8080/x",
        "Invoke-RestMethod http://localhost:5173 -Method Post",
        "gh issue view 12",
    ] {
        assert_eq!(bash(Some(D), c), "allow", "{c}");
    }
}

/// G1-4：config.json 写坏时不能当空配置放行。
#[test]
fn a_broken_personal_config_blocks_writes_instead_of_dropping_protection() {
    let broken = || Config::parse(r#"{"protected":["acme-*",],}"#);
    assert!(broken().broken.is_some());
    let write = payload(Some(D), "Write", json!({"file_path": "/home/me/pet/a.rs"}));
    assert!(check(&write, broken, &temp()).is_some());
    let mutate = payload(Some(D), "Bash", json!({"command": "git commit -am x"}));
    assert!(check(&mutate, broken, &temp()).is_some());
    let read = payload(Some(D), "Bash", json!({"command": "git log"}));
    assert!(check(&read, broken, &temp()).is_none(), "只读命令照常");
    assert!(Config::parse("[1]").broken.is_some());
    // 文件不存在不算坏
    let dir = tempfile::tempdir().unwrap();
    assert!(Config::load(Some(dir.path())).broken.is_none());
    std::fs::write(dir.path().join("config.json"), "{oops").unwrap();
    assert!(Config::problem(dir.path()).is_some());
}

/// G1-5：团队角色不能改守卫自己的配置。
#[test]
fn guard_configuration_is_off_limits() {
    let dir = tempfile::tempdir().unwrap();
    let personal = dir.path().join("team");
    std::fs::create_dir_all(&personal).unwrap();
    let cfg = || Config::load(Some(&personal));
    let personal_file = personal.join("config.json").to_string_lossy().into_owned();
    for path in [
        personal_file.as_str(),
        "/home/me/.claude/settings.json",
        r"C:\Users\me\.claude\settings.local.json",
        r"C:\Users\me\.meowo\profiles\claude\p2\settings.json",
        "/home/me/.claude/meowo-team/knowledge/developer/craft.md",
    ] {
        let raw = payload(Some(D), "Write", json!({ "file_path": path }));
        assert!(check(&raw, cfg, &temp()).is_some(), "{path}");
    }
    let raw = payload(Some(D), "Bash", json!({"command": "echo '{}' > /home/me/.claude/settings.json"}));
    assert!(check(&raw, cfg, &temp()).is_some());
    let raw = payload(Some(D), "Write", json!({"file_path": "/home/me/work/app/settings.json"}));
    assert!(check(&raw, cfg, &temp()).is_none(), "项目自己的 settings.json 不算");
}

// ---- 做空 redteam-2 的复现（981782e0） ----

/// 误拦：只读搜索里引号中的 `>`、写到临时目录的测试输出、名叫 meowo-team 的目录、受保护仓库里往临时目录落日志。
#[test]
fn ordinary_review_work_is_not_blocked() {
    let src = "/home/me/work/pet";
    for (role, c) in [
        (R, r#"grep -rn "=>" src"#),
        (R, r#"grep -n "fn .* -> bool" src/main.rs"#),
        (R, r#"rg -n "Option<String>" src"#),
        (R, r#"git log --format="%h %an <%ae>" -5"#),
        (R, "cargo test > /tmp/test.log 2>&1"),
        (R, "cargo test 2>&1 | tee /tmp/test.log"),
        (R, "npm test *> $null"),
        (R, "cargo test --lib team:: 2>&1 | tail -30"),
        (R, "grep -w foo src/a.rs"),
        (R, "git show --patch HEAD"),
    ] {
        assert_eq!(in_dir(role, src, "Bash", json!({ "command": c })), "allow", "{role} {c}");
    }
    // 别处叫 meowo-team 的目录（`<仓库>-<用途>` 命名的 worktree）不是守卫配置
    assert_eq!(write(Some(D), r"C:\Users\me\work\meowo-team\app\src\x.ts"), "allow");
    assert_eq!(in_dir(D, "/home/me/work/meowo-team", "Bash", json!({"command": "mkdir -p app/src/new"})), "allow");
    // 受保护仓库里，命令的目标都在 Lead 目录或临时目录：放行
    let repo = "/home/me/work/acme-gui";
    assert_eq!(in_dir(P, repo, "Bash", json!({"command": "mkdir -p /home/me/.meowo/projects/p/.lead/t/agents"})), "allow");
    assert_eq!(in_dir(R, repo, "Bash", json!({"command": "cargo test > /tmp/t.log"})), "allow");
}

#[cfg(windows)]
#[test]
fn git_bash_drive_paths_are_understood_on_windows() {
    let temp = std::env::temp_dir().to_string_lossy().replace('\\', "/");
    // C:/Users/x/AppData/Local/Temp → /c/Users/x/AppData/Local/Temp
    let bash_temp = format!("/{}{}", temp[..1].to_lowercase(), &temp[2..]);
    let raw = |c: String| json!({"cwd": "/home/me/work/pet", "agent_id": "a1", "agent_type": R, "tool_name": "Bash", "tool_input": {"command": c}}).to_string();
    assert!(check(&raw(format!("git diff HEAD~3 > {bash_temp}/d.patch")), config, &default_temp_dirs()).is_none());
    assert!(check(&raw("echo x > /c/Users/me/work/pet/src/a.rs".into()), config, &default_temp_dirs()).is_some());
}

/// 绕过：格式化、lint 修复、解释器内联、perl -i、git apply、find -delete 也是改动。
#[test]
fn common_in_place_writers_count_as_changes() {
    let src = "/home/me/work/pet";
    for (role, c) in [
        (R, r#"python -c "open('src/a.rs','w').write('x')""#),
        (R, r#"node -e "require('fs').writeFileSync('src/a.rs','x')""#),
        (R, "cargo fmt"),
        (R, "npx prettier --write src"),
        (P, "perl -pi -e s/a/b/ src/a.rs"),
        (R, "git apply /tmp/fix.patch"),
        (R, "find . -name '*.ts' -delete"),
        (R, "npm run lint -- --fix"),
    ] {
        assert_eq!(in_dir(role, src, "Bash", json!({ "command": c })), "deny", "{role} {c}");
    }
    let repo = "/home/me/work/acme-gui";
    for c in ["cargo fmt", r#"python -c "open('a','w')""#, "find . -name '*.ts' -delete", "npm run lint -- --fix"] {
        assert_eq!(in_dir(D, repo, "Bash", json!({ "command": c })), "deny", "受保护仓库 {c}");
    }
    let raw = json!({"cwd": src, "agent_id": "a1", "tool_name": "Bash", "tool_input": {"command": "python -c \"open('src/a.rs','w')\""}}).to_string();
    assert!(check(&raw, config, &temp()).is_some(), "认不出的角色");
    let broken = || Config::parse("{oops");
    let raw = payload(Some(D), "Bash", json!({"command": "python -c \"open('a','w')\""}));
    assert!(check(&raw, broken, &temp()).is_some(), "坏配置下的内联脚本");
}

/// 绕过：发消息的 curl/gh 变体。
#[test]
fn outbound_request_variants_are_denied() {
    for c in [
        "gh api --method=POST repos/a/b/issues/1/comments --raw-field=body=hi",
        "curl -d@body https://hooks.example.com/x",
        r#"curl -d'{"text":"hi"}' https://hooks.example.com/x"#,
        r#"curl "https://api.example.com/send?a=1&b=2" -X POST -d '{}'"#,
        r#"curl -X POST https://hooks.example.com/x -H "Origin: http://localhost""#,
        r#"python -c "import requests; requests.post('https://x.example.com', data='hi')""#,
    ] {
        assert_eq!(bash(Some(D), c), "deny", "{c}");
    }
    assert_eq!(bash(Some(D), "curl -X POST http://localhost:3000/api?a=1&b=2 -d x"), "allow");
}

/// 绕过：用 `~`、`$HOME`、`$env:USERPROFILE` 改守卫配置；改团队角色文件。
#[test]
fn guard_configuration_cannot_be_reached_through_shell_shorthands() {
    let Some(home) = home() else { return };
    let personal = PathBuf::from(&home).join(".meowo").join("team");
    let cfg = || Config { personal_dir: Some(personal.clone()), ..Config::default() };
    for c in [
        "rm ~/.meowo/team/config.json",
        "echo '{}' > $HOME/.meowo/team/config.json",
        r#"Remove-Item "$env:USERPROFILE\.meowo\team" -Recurse"#,
        "cd ~/.claude && echo {} > settings.json",
        "rm -rf ~/.claude",
    ] {
        let raw = payload(Some(D), "Bash", json!({ "command": c }));
        assert!(check(&raw, cfg, &temp()).is_some(), "{c}");
    }
    for path in ["/home/me/.claude/agents/team-reviewer.md", "/home/me/.claude/skills/team-staff/SKILL.md"] {
        let raw = payload(Some(D), "Write", json!({ "file_path": path }));
        assert!(check(&raw, cfg, &temp()).is_some(), "{path}");
    }
    let raw = payload(Some(D), "Write", json!({"file_path": "/home/me/.claude/agents/my-own.md"}));
    assert!(check(&raw, cfg, &temp()).is_none(), "用户自己的角色不归守卫管");
}

/// 隐含改 cwd 的命令：cwd 在临时目录（审查员在临时 worktree 里复现）时放行。
#[test]
fn implicit_cwd_writers_are_fine_inside_scratch_dirs() {
    assert_eq!(in_dir(R, "/tmp/rv", "Bash", json!({"command": "git apply /tmp/fix.patch"})), "allow");
    assert_eq!(in_dir(R, "/tmp/rv", "Bash", json!({"command": "cargo fmt"})), "allow");
    assert_eq!(in_dir(R, "/tmp/rv", "Bash", json!({"command": "echo x > /home/me/work/pet/a.rs"})), "deny", "目标仍在源码里");
}

/// 审查员把要复现的版本检出到临时目录（redteam-2 M12）：worktree 的目标路径写在命令里，不算隐含改 cwd。
#[test]
fn reviewer_may_check_out_a_worktree_into_temp() {
    assert_eq!(in_dir(R, "/home/me/work/pet", "Bash", json!({"command": "git worktree add /tmp/rv HEAD~1"})), "allow");
    assert_eq!(in_dir(R, "/home/me/work/pet", "Bash", json!({"command": "git worktree remove ../pet-old"})), "deny");
}
