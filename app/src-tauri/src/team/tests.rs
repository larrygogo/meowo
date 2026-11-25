use super::*;

const REPORTER: &str = "C:/m/meowo-reporter.exe";

fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join(".claude");
    let personal = tmp.path().join(".meowo/team");
    std::fs::create_dir_all(&claude).unwrap();
    (tmp, claude, personal)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn guard_hooks(claude: &Path) -> Vec<Value> {
    let v: Value = serde_json::from_str(&read(&claude.join("settings.json"))).unwrap();
    v["hooks"]["PreToolUse"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|g| g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| h["command"].as_str().is_some_and(is_team_hook))))
        .cloned()
        .collect()
}

/// 新增的托管文件忘了登记进 ASSETS 就不会被装；登记了却没有文件编译不过。两头都要对上。
#[test]
fn asset_list_matches_the_directory() {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/team_assets");
    let mut on_disk = Vec::new();
    walk(&root, &root, &mut on_disk);
    on_disk.sort();
    let mut listed: Vec<String> = ASSETS.iter().map(|(r, _)| (*r).to_owned()).collect();
    listed.sort();
    assert_eq!(listed, on_disk);
}

#[test]
fn every_asset_is_marked_and_roles_are_named_after_their_files() {
    for (relative, body) in ASSETS {
        assert!(safe_relative(relative), "{relative} 不在托管范围内");
        if relative.ends_with(".md") {
            assert!(body.contains(MANAGED_MARK), "{relative} 缺托管标记");
        }
        if let Some(name) = relative.strip_prefix("agents/").and_then(|r| r.strip_suffix(".md")) {
            // agents 目录按 frontmatter 的 name 认角色，守卫按 team- 前缀认人：两者必须和文件名一致。
            assert!(body.replace("\r\n", "\n").starts_with(&format!("---\nname: {name}\n")), "{relative} frontmatter");
            assert!(name.starts_with(meowo_reporter::team_guard::ROLE_PREFIX));
        }
        if let Some(skill) = relative.strip_prefix("skills/").and_then(|r| r.strip_suffix("/SKILL.md")) {
            assert!(body.replace("\r\n", "\n").starts_with(&format!("---\nname: {skill}\n")), "{relative} frontmatter");
        }
    }
}

#[test]
fn install_writes_every_role_with_paths_filled_in_and_the_guard_hook() {
    let (_tmp, claude, personal) = dirs();
    install_into(&claude, REPORTER, &personal).unwrap();
    assert!(installed_in(&claude));
    for (relative, _) in ASSETS {
        let body = read(&claude.join(relative));
        assert!(!body.contains("{{"), "{relative} 留着没换的占位符");
    }
    let pm = read(&claude.join("agents/team-pm.md"));
    let personal_slash = personal.to_string_lossy().replace('\\', "/");
    assert!(pm.contains(&personal_slash), "产品经理要读个人层");
    assert!(pm.contains(&claude.join(KNOWLEDGE_DIR).to_string_lossy().replace('\\', "/")));
    let hooks = guard_hooks(&claude);
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["matcher"], GUARD_MATCHER);
    assert_eq!(hooks[0]["hooks"][0]["command"], format!("\"{REPORTER}\" team guard"));
    // 托管层从不写个人层（团队规则 R2）
    assert!(!personal.exists());
}

#[test]
fn reinstall_is_a_no_op_and_keeps_other_hooks() {
    let (_tmp, claude, personal) = dirs();
    let lead = json!({"hooks": {"PreToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "\"C:/m/meowo-reporter.exe\" lead guard", "timeout": 10}]}]}, "model": "x"});
    std::fs::write(claude.join("settings.json"), serde_json::to_string_pretty(&lead).unwrap()).unwrap();
    install_into(&claude, REPORTER, &personal).unwrap();
    let first = read(&claude.join("settings.json"));
    install_into(&claude, REPORTER, &personal).unwrap();
    assert_eq!(read(&claude.join("settings.json")), first);
    assert_eq!(guard_hooks(&claude).len(), 1, "重装不重复加");
    let v: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["model"], "x");
    assert!(first.contains("lead guard"), "Lead 的守卫原样保留");
    // reporter 换了位置：旧条目换成新路径，不叠加
    install_into(&claude, "D:/new/meowo-reporter.exe", &personal).unwrap();
    let hooks = guard_hooks(&claude);
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["hooks"][0]["command"], "\"D:/new/meowo-reporter.exe\" team guard");
}

#[test]
fn lead_and_team_hook_commands_are_told_apart() {
    assert!(is_team_hook("\"C:/m/meowo-reporter.exe\" team guard"));
    assert!(is_team_hook(r#""C:\m\meowo-reporter.exe" team guard"#));
    assert!(!is_team_hook("\"C:/m/meowo-reporter.exe\" lead guard"));
    assert!(!is_team_hook("python -I guard.py"));
    // Lead 那边按「meowo-reporter … lead …」认自己的条目，团队条目里不能出现 lead 一词
    assert!(!team_cmd(REPORTER).contains(" lead"));
}

#[test]
fn user_files_with_the_same_name_are_never_overwritten() {
    let (_tmp, claude, personal) = dirs();
    std::fs::create_dir_all(claude.join("agents")).unwrap();
    std::fs::write(claude.join("agents/team-pm.md"), "---\nname: team-pm\n---\n我自己的").unwrap();
    // R5 边角 2：被挡住的只跳过它自己，其他角色照装，没装上的文件列给设置页。
    install_into(&claude, REPORTER, &personal).unwrap();
    assert_eq!(read(&claude.join("agents/team-pm.md")), "---\nname: team-pm\n---\n我自己的");
    assert!(owned_by_mark(&claude.join("agents/team-designer.md")));
    assert!(installed_in(&claude));
    assert_eq!(blocked_files(&claude), vec![claude.join("agents/team-pm.md")]);
    assert_eq!(guard_hooks(&claude).len(), 1, "守卫照装");
    // 刷新、卸载都不碰它
    install_into(&claude, REPORTER, &personal).unwrap();
    uninstall_from(&claude).unwrap();
    assert_eq!(read(&claude.join("agents/team-pm.md")), "---\nname: team-pm\n---\n我自己的");
    assert!(!claude.join("agents/team-designer.md").exists());

    // 知识目录被占：所有角色都要读它，整个不装并报出目录
    let (_tmp, claude, personal) = dirs();
    std::fs::create_dir_all(claude.join(KNOWLEDGE_DIR)).unwrap();
    std::fs::write(claude.join(KNOWLEDGE_DIR).join("notes.md"), "我的").unwrap();
    let err = install_into(&claude, REPORTER, &personal).unwrap_err();
    assert!(err.starts_with("team/user_file_exists："), "{err}");
    assert!(err.contains(KNOWLEDGE_DIR), "{err}");
    assert!(!claude.join("agents/team-designer.md").exists());
    assert!(!claude.join("settings.json").exists());
    assert_eq!(blocked_files(&claude), vec![claude.join(KNOWLEDGE_DIR)]);
}

#[test]
fn uninstall_removes_only_what_meowo_installed() {
    let (_tmp, claude, personal) = dirs();
    install_into(&claude, REPORTER, &personal).unwrap();
    std::fs::write(claude.join("agents/my-own.md"), "x").unwrap();
    std::fs::write(claude.join(KNOWLEDGE_DIR).join("my-notes.md"), "x").unwrap();
    std::fs::create_dir_all(claude.join("skills/mine")).unwrap();
    std::fs::write(claude.join("skills/mine/SKILL.md"), "x").unwrap();
    uninstall_from(&claude).unwrap();
    assert!(!installed_in(&claude));
    for (relative, _) in ASSETS {
        assert!(!claude.join(relative).exists(), "{relative}");
    }
    assert!(!claude.join("skills/team-staff").exists(), "空的技能目录一起删");
    assert!(claude.join("agents/my-own.md").exists());
    assert!(claude.join(KNOWLEDGE_DIR).join("my-notes.md").exists(), "用户放进知识目录的文件保留");
    assert!(claude.join("skills/mine/SKILL.md").exists());
    assert!(guard_hooks(&claude).is_empty());
    // 卸载后还能再装（知识目录里留着用户文件、没有归属文件 → 当作用户的目录，不覆盖）
    assert!(install_into(&claude, REPORTER, &personal).is_err());
    std::fs::remove_file(claude.join(KNOWLEDGE_DIR).join("my-notes.md")).unwrap();
    std::fs::remove_dir(claude.join(KNOWLEDGE_DIR)).unwrap();
    install_into(&claude, REPORTER, &personal).unwrap();
}

#[test]
fn uninstall_on_a_clean_dir_touches_nothing() {
    let (_tmp, claude, _personal) = dirs();
    uninstall_from(&claude).unwrap();
    assert!(!claude.join("settings.json").exists());
    std::fs::create_dir_all(claude.join("agents")).unwrap();
    std::fs::write(claude.join("agents/team-pm.md"), "用户的").unwrap();
    uninstall_from(&claude).unwrap();
    assert_eq!(read(&claude.join("agents/team-pm.md")), "用户的");
}

#[test]
fn files_dropped_by_a_newer_version_are_removed_and_the_owner_list_cannot_escape() {
    let (_tmp, claude, personal) = dirs();
    install_into(&claude, REPORTER, &personal).unwrap();
    let owner = claude.join(KNOWLEDGE_DIR).join(OWNER_FILE);
    let outside = claude.join("precious.txt");
    std::fs::write(&outside, "x").unwrap();
    std::fs::write(claude.join(KNOWLEDGE_DIR).join("old.md"), "旧版").unwrap();
    std::fs::write(claude.join("agents/team-old.md"), MANAGED_MARK).unwrap();
    std::fs::write(claude.join("agents/team-users.md"), "没有标记").unwrap();
    let list = format!("{}\nmeowo-team/old.md\nagents/team-old.md\nagents/team-users.md\n../precious.txt\nmeowo-team/../precious.txt\nsettings.json\n", read(&owner));
    std::fs::write(&owner, list).unwrap();
    install_into(&claude, REPORTER, &personal).unwrap();
    assert!(!claude.join(KNOWLEDGE_DIR).join("old.md").exists());
    assert!(!claude.join("agents/team-old.md").exists());
    assert!(claude.join("agents/team-users.md").exists(), "去掉标记的是用户的");
    assert!(outside.exists());
    assert!(claude.join("settings.json").exists());
}

#[test]
fn broken_settings_are_left_alone() {
    let (_tmp, claude, personal) = dirs();
    std::fs::write(claude.join("settings.json"), "[1,2]").unwrap();
    assert_eq!(install_into(&claude, REPORTER, &personal).unwrap_err(), "lead/settings_invalid");
    assert_eq!(read(&claude.join("settings.json")), "[1,2]");
}

#[test]
fn missing_claude_dir_is_not_created() {
    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join("nope");
    assert_eq!(install_into(&claude, REPORTER, &tmp.path().join("p")).unwrap_err(), "lead/claude_missing");
    assert!(!claude.exists());
}

#[test]
fn plugin_named_team_is_recognised() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("team");
    std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    std::fs::write(dir.join(".claude-plugin/plugin.json"), "\u{feff}{\"name\": \"team\", \"version\": \"0.1.0\"}").unwrap();
    assert_eq!(plugin_name(&dir).as_deref(), Some("team"));
    std::fs::write(dir.join(".claude-plugin/plugin.json"), "{\"name\": \"other\"}").unwrap();
    assert_eq!(plugin_name(&dir).as_deref(), Some("other"));
    assert_eq!(plugin_name(&tmp.path().join("missing")), None);
}

/// 示例配置是给用户照抄的：守卫必须能按它读出受保护目录和追加的禁止命令，两边说法不能漂。
#[test]
fn the_example_config_is_what_the_guard_reads() {
    let (_, body) = ASSETS.iter().find(|(r, _)| *r == "meowo-team/config.example.json").unwrap();
    let config = meowo_reporter::team_guard::Config::parse(body);
    assert!(config.protected.iter().any(|p| p == "acme-*"));
    assert_eq!(config.blocked_commands.len(), 1);
    assert!(regex::Regex::new(&config.blocked_commands[0].0).unwrap().is_match("python notify.py send 3"));
}
