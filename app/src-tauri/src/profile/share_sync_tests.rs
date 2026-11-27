use super::*;
use meowo_agent::profile::{ShareFormat, SharePick, ShareSurface as S};
use serde_json::json;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!(
            "meowo-share-sync-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("home/.claude")).unwrap();
        std::fs::create_dir_all(dir.join("profile")).unwrap();
        Tmp(dir)
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn default_data(&self) -> PathBuf {
        self.0.join("home/.claude")
    }
    fn profile(&self) -> PathBuf {
        self.0.join("profile")
    }
    fn run(&self, surfaces: &[ShareSurface]) -> SyncReport {
        let home = self.home();
        let default_data = self.default_data();
        let profile = self.profile();
        let record = profile.join(RECORD_FILE);
        sync(
            surfaces,
            &SyncDirs { home: &home, default_data: &default_data, profile_data: &profile, record: &record },
        )
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        // 先拆链接再删：remove_dir_all 不跟随 junction，但保险起见不给它机会。
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_json(path: &Path, v: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

fn points_to(link: &Path, target: &Path) -> bool {
    is_link(link) && std::fs::canonicalize(link).unwrap() == std::fs::canonicalize(target).unwrap()
}

const SETTINGS: S = S::Keys {
    file: "settings.json",
    format: ShareFormat::Json,
    at: &[],
    pick: SharePick::All,
    exact_first: true,
};

/// Claude 的真实声明（排除表、.claude.json 白名单都取插件里的那份）。
fn claude_surfaces() -> &'static [ShareSurface] {
    meowo_agent::by_id("claude").unwrap().profile().unwrap().share
}

fn surface_of(pred: impl Fn(&ShareSurface) -> bool) -> ShareSurface {
    *claude_surfaces().iter().find(|s| pred(s)).unwrap()
}

#[test]
fn decide_applies_only_what_the_default_changed() {
    use Action::*;
    // 默认没变：账号保持（账号自己改的 /model 留着）。
    assert_eq!(decide(Some("a"), Some("a"), Some("x")), Leave);
    assert_eq!(decide(Some("a"), Some("a"), None), Leave);
    // 默认变了：账号改成默认的（账号自己改过也一样，Orca 的规则）。
    assert_eq!(decide(Some("b"), Some("a"), Some("x")), Set);
    assert_eq!(decide(Some("b"), Some("a"), None), Set);
    assert_eq!(decide(Some("b"), Some("a"), Some("b")), Leave, "已经一样就不写");
    // 默认删了：账号也删。
    assert_eq!(decide(None, Some("a"), Some("x")), Delete);
    // 没记录（首次）：等同默认改过。
    assert_eq!(decide(Some("a"), None, Some("x")), Set);
    assert_eq!(decide(None, None, Some("x")), Delete);
}

#[test]
fn settings_first_sync_matches_the_default_whole_and_backs_up() {
    let t = Tmp::new("settings-first");
    write_json(&t.default_data().join("settings.json"), &json!({"language": "简体中文", "model": "opus"}));
    write_json(&t.profile().join("settings.json"), &json!({"model": "sonnet", "profileOnly": 1}));
    let report = t.run(&[SETTINGS]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.rewritten, vec!["settings.json"]);
    assert_eq!(
        read_json(&t.profile().join("settings.json")),
        json!({"language": "简体中文", "model": "opus"}),
        "Orca：没有记录时默认整份胜出"
    );
    let backup = t.profile().join(format!("settings.json{BACKUP_SUFFIX}"));
    assert_eq!(read_json(&backup), json!({"model": "sonnet", "profileOnly": 1}), "改写前备份");
}

#[test]
fn settings_later_sync_applies_changed_keys_and_keeps_the_rest() {
    let t = Tmp::new("settings-later");
    write_json(&t.default_data().join("settings.json"), &json!({"language": "简体中文", "model": "opus", "theme": "dark"}));
    t.run(&[SETTINGS]);
    // 用户在这个账号里 /model 换了模型、自己加了一个键。
    write_json(
        &t.profile().join("settings.json"),
        &json!({"language": "简体中文", "model": "sonnet", "theme": "dark", "own": true}),
    );
    // 默认改 language、删 theme，model 没动。
    write_json(&t.default_data().join("settings.json"), &json!({"language": "English", "model": "opus"}));
    let report = t.run(&[SETTINGS]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let p = read_json(&t.profile().join("settings.json"));
    assert_eq!(p["language"], "English", "默认改了的键跟着变");
    assert_eq!(p["model"], "sonnet", "默认没动的键保留账号自己的值");
    assert!(p.get("theme").is_none(), "默认删了的键也删");
    assert_eq!(p["own"], true, "账号独有、默认从没有过的键不动");
    // 再跑一次：没有变化，什么都不写。
    let before = std::fs::metadata(t.profile().join("settings.json")).unwrap().modified().unwrap();
    let report = t.run(&[SETTINGS]);
    assert!(report.changed.is_empty() && report.rewritten.is_empty(), "{:?}", report.changed);
    assert_eq!(std::fs::metadata(t.profile().join("settings.json")).unwrap().modified().unwrap(), before);
}

#[test]
fn an_empty_default_does_not_make_every_sync_a_first_sync() {
    let t = Tmp::new("settings-empty");
    write_json(&t.default_data().join("settings.json"), &json!({}));
    t.run(&[SETTINGS]);
    write_json(&t.profile().join("settings.json"), &json!({"model": "sonnet"}));
    t.run(&[SETTINGS]);
    assert_eq!(read_json(&t.profile().join("settings.json"))["model"], "sonnet");
}

#[test]
fn claude_state_overlays_the_whitelist_and_never_touches_identity() {
    let t = Tmp::new("claudejson");
    let state = surface_of(|s| matches!(s, S::ClaudeState { .. }));
    write_json(
        &t.home().join(".claude.json"),
        &json!({
            "oauthAccount": {"emailAddress": "default@example.com"},
            "userID": "default-user",
            "theme": "dark",
            "hasCompletedOnboarding": true,
            "customApiKeyResponses": {"rejected": ["k1", 3]},
            "mcpServers": {"kimi-cu": {"command": "kimi-cu"}},
            "projects": {
                "C:/w/a": {"hasTrustDialogAccepted": false, "allowedTools": ["Bash"], "history": ["secret"]}
            },
            "numStartups": 99
        }),
    );
    write_json(
        &t.profile().join(".claude.json"),
        &json!({
            "oauthAccount": {"emailAddress": "profile@example.com"},
            "userID": "profile-user",
            "hasCompletedOnboarding": false,
            "customApiKeyResponses": {"rejected": ["k0"]},
            "mcpServers": {"own": {"command": "own"}},
            "projects": {"C:/w/a": {"hasTrustDialogAccepted": true, "lastCost": 1}}
        }),
    );
    let report = t.run(&[state]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let p = read_json(&t.profile().join(".claude.json"));
    assert_eq!(p["oauthAccount"]["emailAddress"], "profile@example.com", "账号身份绝不串");
    assert_eq!(p["userID"], "profile-user");
    assert!(p.get("numStartups").is_none(), "白名单外的不共享");
    assert_eq!(p["theme"], "dark");
    assert_eq!(p["hasCompletedOnboarding"], false, "只在账号没有时补");
    assert_eq!(p["customApiKeyResponses"]["rejected"], json!(["k0", "k1"]), "取并集，只要字符串");
    assert_eq!(p["mcpServers"]["kimi-cu"]["command"], "kimi-cu");
    assert_eq!(p["mcpServers"]["own"]["command"], "own", "账号自己的 MCP 保留");
    let project = &p["projects"]["C:/w/a"];
    assert_eq!(project["hasTrustDialogAccepted"], true, "账号已信任的保持信任");
    assert_eq!(project["allowedTools"], json!(["Bash"]));
    assert_eq!(project["lastCost"], 1);
    assert!(project.get("history").is_none(), "项目里没挑的键不共享");
    assert!(!t.profile().join(".claude.json.lock").exists(), "锁用完要放");
    assert!(t.profile().join(format!(".claude.json{BACKUP_SUFFIX}")).exists());
    // 第二次：没有变化不写。
    assert!(t.run(&[state]).changed.is_empty());
}

#[test]
fn claude_state_creates_the_profile_file_when_missing() {
    let t = Tmp::new("claudejson-new");
    let state = surface_of(|s| matches!(s, S::ClaudeState { .. }));
    write_json(&t.home().join(".claude.json"), &json!({"hasCompletedOnboarding": true, "theme": "light", "userID": "x"}));
    t.run(&[state]);
    assert_eq!(
        read_json(&t.profile().join(".claude.json")),
        json!({"hasCompletedOnboarding": true, "theme": "light"})
    );
}

#[test]
fn a_held_config_lock_postpones_the_write_and_a_stale_one_is_taken_over() {
    let t = Tmp::new("lock");
    let state = surface_of(|s| matches!(s, S::ClaudeState { .. }));
    write_json(&t.home().join(".claude.json"), &json!({"mcpServers": {"a": {}}}));
    write_json(&t.profile().join(".claude.json"), &json!({}));
    let lock = t.profile().join(".claude.json.lock");
    std::fs::create_dir(&lock).unwrap();
    let report = t.run(&[state]);
    assert_eq!(report.errors.len(), 1, "别人拿着锁时这次不写：{:?}", report.errors);
    assert_eq!(read_json(&t.profile().join(".claude.json")), json!({}));
    assert!(lock.exists(), "别人的锁不能删");

    // 陈旧锁（超过 10 秒没刷新）接管。
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::File::open(&lock).and_then(|f| f.set_modified(old)).ok();
    if std::fs::metadata(&lock).unwrap().modified().unwrap() <= old + std::time::Duration::from_secs(1) {
        let report = t.run(&[state]);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(read_json(&t.profile().join(".claude.json"))["mcpServers"].get("a").is_some());
        assert!(!lock.exists());
    }
}

#[test]
fn broken_files_are_never_overwritten_or_read_as_empty() {
    let t = Tmp::new("broken");
    write_json(&t.default_data().join("settings.json"), &json!({"language": "简体中文"}));
    t.run(&[SETTINGS]);
    // 默认坏了：不能当成「默认全删了」去删账号里的值。
    write(&t.default_data().join("settings.json"), "{ not json");
    let report = t.run(&[SETTINGS]);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(read_json(&t.profile().join("settings.json"))["language"], "简体中文");
    // 账号的坏了：不覆盖。
    write_json(&t.default_data().join("settings.json"), &json!({"language": "English"}));
    write(&t.profile().join("settings.json"), "{ broken");
    let report = t.run(&[SETTINGS]);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(read(&t.profile().join("settings.json")), "{ broken");
    // .claude.json 坏了同样不覆盖。
    let state = surface_of(|s| matches!(s, S::ClaudeState { .. }));
    write_json(&t.home().join(".claude.json"), &json!({"theme": "dark"}));
    write(&t.profile().join(".claude.json"), "{ broken");
    assert_eq!(t.run(&[state]).errors.len(), 1);
    assert_eq!(read(&t.profile().join(".claude.json")), "{ broken");
}

#[test]
fn all_entries_share_everything_but_runtime_state() {
    let t = Tmp::new("all");
    let all = surface_of(|s| matches!(s, S::AllEntries { .. }));
    let d = t.default_data();
    write(&d.join("skills/lead/SKILL.md"), "lead");
    write(&d.join("commands/review.md"), "review");
    write(&d.join("plugins/installed_plugins.json"), "{}");
    write(&d.join("keybindings.json"), "[]");
    write(&d.join("CLAUDE.md"), "# rules\n");
    write(&d.join(".credentials.json"), "secret");
    write(&d.join("sessions/1.json"), "{}");
    write(&d.join("statsig/x"), "x");
    write(&d.join("projects/p/s.jsonl"), "{}");
    write(&d.join("daemon.log"), "log");
    write(&d.join("agent-memory-x/m.md"), "m");
    write(&d.join("settings.json"), "{}");
    write(&d.join("history.jsonl"), "");

    let report = t.run(&[all]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let p = t.profile();
    for dir in ["skills", "commands", "plugins"] {
        assert!(points_to(&p.join(dir), &d.join(dir)), "{dir} 链到默认目录");
    }
    assert_eq!(read(&p.join("keybindings.json")), "[]");
    assert!(!is_link(&p.join("keybindings.json")), "文件复制，不链接");
    assert_eq!(read(&p.join("CLAUDE.md")), "@~/.claude/CLAUDE.md\n", "CLAUDE.md 写成一行引用");
    for name in [".credentials.json", "sessions", "statsig", "projects", "daemon.log", "agent-memory-x", "settings.json", "history.jsonl"] {
        assert!(std::fs::symlink_metadata(p.join(name)).is_err(), "{name} 不走这条路");
    }

    // 无变化：什么都不写。
    let report = t.run(&[all]);
    assert!(report.changed.is_empty(), "{:?}", report.changed);
    // 默认改了、账号没动：账号副本直接跟着变，不留存。
    write(&d.join("keybindings.json"), "[1]");
    t.run(&[all]);
    assert_eq!(read(&p.join("keybindings.json")), "[1]");
    assert!(!p.join("keybindings.json.meowo-profile-merge").exists());
    // 用户在账号里改过（/keybindings）：先留存再写默认的（红队 1：之前静默覆盖）。
    write(&p.join("keybindings.json"), "[mine]");
    t.run(&[all]);
    assert_eq!(read(&p.join("keybindings.json")), "[1]");
    assert_eq!(read(&p.join("keybindings.json.meowo-profile-merge")), "[mine]");
    // 用户在账号里用 /memory 往 CLAUDE.md 追加的规则留着，引用行还在就不动。
    write(&p.join("CLAUDE.md"), "@~/.claude/CLAUDE.md\n\n- 这个账号自己的规则\n");
    let report = t.run(&[all]);
    assert!(report.changed.is_empty(), "{:?}", report.changed);
    assert!(read(&p.join("CLAUDE.md")).contains("这个账号自己的规则"));
    // 引用行被删掉了：用户内容留存，补回引用行。
    write(&p.join("CLAUDE.md"), "- 只剩自己的规则\n");
    t.run(&[all]);
    assert_eq!(read(&p.join("CLAUDE.md")), "@~/.claude/CLAUDE.md\n");
    assert_eq!(read(&p.join("CLAUDE.md.meowo-profile-merge")), "- 只剩自己的规则\n");
}

#[test]
fn backup_files_are_not_shared_and_old_set_asides_go_back() {
    for name in ["settings.json.bak", "settings.json.bak-lead", "settings.json.cckb-bak", "settings.json.before-fix-20260713.bak", "x.backup"] {
        assert!(is_backup_name(name), "{name} 是备份");
    }
    for name in ["backups", "settings.json", "keybindings.json", "bake.md", "CLAUDE.md"] {
        assert!(!is_backup_name(name), "{name} 不是备份");
    }

    let t = Tmp::new("backups");
    let all = surface_of(|s| matches!(s, S::AllEntries { .. }));
    let d = t.default_data();
    let p = t.profile();
    write(&d.join("settings.json.bak-team"), "default-bak");
    write(&d.join("settings.json.cckb-bak"), "default-cckb");
    write(&d.join("settings.json.bak-other"), "default-other");
    // 旧版留下的现场（本机 2026-11-27）：原名上是抄来的默认那份，账号自己的被挤成待合并。
    write(&p.join("settings.json.bak-team"), "default-bak");
    write(&p.join("settings.json.bak-team.meowo-profile-merge"), "mine-bak");
    // 原名空着：直接放回。
    write(&p.join("settings.json.cckb-bak.meowo-profile-merge"), "mine-cckb");
    // 原名上是用户后来自己写的：不动。
    write(&p.join("settings.json.bak-other"), "mine-later");
    write(&p.join("settings.json.bak-other.meowo-profile-merge"), "mine-older");

    let report = t.run(&[all]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(read(&p.join("settings.json.bak-team")), "mine-bak");
    assert_eq!(read(&p.join("settings.json.cckb-bak")), "mine-cckb");
    assert!(!p.join("settings.json.bak-team.meowo-profile-merge").exists());
    assert!(!p.join("settings.json.cckb-bak.meowo-profile-merge").exists());
    assert_eq!(read(&p.join("settings.json.bak-other")), "mine-later");
    assert_eq!(read(&p.join("settings.json.bak-other.meowo-profile-merge")), "mine-older");

    // 默认目录新出现的备份不再抄进账号。
    write(&d.join("settings.json.bak-new"), "new");
    t.run(&[all]);
    assert!(!p.join("settings.json.bak-new").exists());
}

#[test]
fn first_setup_merges_existing_profile_folders_without_losing_anything() {
    let t = Tmp::new("first-setup");
    let all = surface_of(|s| matches!(s, S::AllEntries { .. }));
    let d = t.default_data();
    let p = t.profile();
    write(&d.join("skills/lark/SKILL.md"), "lark");
    write(&d.join("skills/lead/SKILL.md"), "lead v2");
    write(&d.join("plugins/installed_plugins.json"), "{\"default\":1}");
    // 旧版 Meowo 留下的：按 skill 逐个建的 junction、单独装的 Lead（与默认相同）。
    std::fs::create_dir_all(p.join("skills")).unwrap();
    super::super::share::link_dir(&p.join("skills/lark"), &d.join("skills/lark")).unwrap();
    write(&p.join("skills/lead/SKILL.md"), "lead v2");
    // 用户在这个账号里装的 skill、CLI 建的不同插件清单。
    write(&p.join("skills/mine/SKILL.md"), "mine");
    write(&p.join("plugins/installed_plugins.json"), "{\"profile\":1}");
    write(&p.join("plugins/cache/x/y.txt"), "cached");
    write(&d.join("keybindings.json"), "[default]");
    write(&p.join("keybindings.json"), "[profile]");
    write(&d.join("CLAUDE.md"), "# rules\n");
    write(&p.join("CLAUDE.md"), "# profile rules\n");

    let report = t.run(&[all]);
    assert!(points_to(&p.join("skills"), &d.join("skills")));
    assert!(points_to(&p.join("plugins"), &d.join("plugins")));
    assert_eq!(read(&d.join("skills/mine/SKILL.md")), "mine", "账号独有的并进默认目录");
    assert_eq!(read(&d.join("skills/lark/SKILL.md")), "lark", "旧链接只拆链接，目标完好");
    assert_eq!(read(&d.join("skills/lead/SKILL.md")), "lead v2");
    assert_eq!(read(&d.join("plugins/cache/x/y.txt")), "cached");
    assert_eq!(read(&d.join("plugins/installed_plugins.json")), "{\"default\":1}", "冲突不覆盖默认");
    let pending = p.join(format!("plugins{}", share_link::MERGE_PENDING_SUFFIX));
    assert_eq!(read(&pending.join("installed_plugins.json")), "{\"profile\":1}", "冲突的留在待合并目录");
    assert!(!p.join(format!("skills{}", share_link::MERGE_PENDING_SUFFIX)).exists(), "没冲突的待合并目录清空删掉");
    assert!(report.errors.iter().any(|e| e.contains("同名冲突")), "冲突要报：{:?}", report.errors);
    assert_eq!(read(&p.join("keybindings.json")), "[default]");
    assert_eq!(read(&p.join(format!("keybindings.json{}", share_link::MERGE_PENDING_SUFFIX))), "[profile]", "账号原有的文件留存");
    assert_eq!(read(&p.join(format!("CLAUDE.md{}", share_link::MERGE_PENDING_SUFFIX))), "# profile rules\n");
    // 之后默认再改：照 Orca 直接覆盖，不再留存。
    write(&d.join("keybindings.json"), "[default2]");
    t.run(&[all]);
    assert_eq!(read(&p.join("keybindings.json")), "[default2]");
    assert!(!p.join(format!("keybindings.json{}-1", share_link::MERGE_PENDING_SUFFIX)).exists());

    // 首次对齐之后，账号里出现的非空真实目录是用户有意放的：不再并。
    write(&d.join("agents/a.md"), "a");
    write(&p.join("agents/own.md"), "own");
    t.run(&[all]);
    assert!(!is_link(&p.join("agents")));
    assert!(!d.join("agents/own.md").exists());
}

#[test]
fn history_dirs_merge_into_the_default_and_keep_every_conversation() {
    let t = Tmp::new("history");
    let hist = surface_of(|s| matches!(s, S::HistoryDirs { .. }));
    let d = t.default_data();
    let p = t.profile();
    write(&p.join("projects/p1/s1.jsonl"), "a\n");
    write(&d.join("projects/p2/s2.jsonl"), "x\n");
    write(&p.join("projects/p2/s2.jsonl"), "x\ny\n"); // 账号里续写过：留长的
    write(&d.join("projects/p2/s3.jsonl"), "m\nd\n");
    write(&p.join("projects/p2/s3.jsonl"), "m\np\n"); // 分歧：两份都留
    write(&p.join("projects/p3/s4.jsonl"), "z\n"); // 默认没有的项目：整个改名过去
    write(&p.join("file-history/s1/v1"), "v1");
    write(&p.join("sessions/123.json"), "{}"); // 进程登记表，不在列表里

    let report = t.run(&[hist]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(points_to(&p.join("projects"), &d.join("projects")));
    assert!(points_to(&p.join("file-history"), &d.join("file-history")));
    assert_eq!(read(&d.join("projects/p1/s1.jsonl")), "a\n");
    assert_eq!(read(&d.join("projects/p2/s2.jsonl")), "x\ny\n");
    assert_eq!(read(&d.join("projects/p2/s3.jsonl")), "m\nd\n");
    assert_eq!(read(&d.join("projects/p2/s3.jsonl.merge-conflict.bak")), "m\np\n");
    assert_eq!(read(&d.join("projects/p3/s4.jsonl")), "z\n");
    assert_eq!(read(&d.join("file-history/s1/v1")), "v1");
    assert!(!p.join("sessions").is_symlink() && p.join("sessions/123.json").exists(), "sessions 保持私有");
    assert!(!p.join(format!("projects{}", share_link::MERGE_PENDING_SUFFIX)).exists());
    // 通过账号路径写的会话落到默认目录。
    write(&p.join("projects/p1/s5.jsonl"), "new\n");
    assert_eq!(read(&d.join("projects/p1/s5.jsonl")), "new\n");
    // 再跑：已是链接，不动。
    assert!(t.run(&[hist]).changed.is_empty());
}

#[test]
fn an_interrupted_history_merge_resumes_from_the_pending_folder() {
    let t = Tmp::new("history-resume");
    let hist = surface_of(|s| matches!(s, S::HistoryDirs { .. }));
    let d = t.default_data();
    let p = t.profile();
    // 上次改名之后、建链接之前断了：条目还在待合并目录里，账号里没有 projects。
    let pending = p.join(format!("projects{}", share_link::MERGE_PENDING_SUFFIX));
    write(&pending.join("p1/s1.jsonl"), "a\n");
    let report = t.run(&[hist]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(read(&d.join("projects/p1/s1.jsonl")), "a\n");
    assert!(!pending.exists());
    assert!(points_to(&p.join("projects"), &d.join("projects")));
}

#[test]
fn prompt_history_is_merged_linked_and_relinked_without_reviving_deletions() {
    let t = Tmp::new("prompts");
    let prompts = surface_of(|s| matches!(s, S::PromptHistory { .. }));
    let d = t.default_data().join("history.jsonl");
    let p = t.profile().join("history.jsonl");
    write(&d, "b\nc\n");
    write(&p, "a\nb\n");

    let report = t.run(&[prompts]);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(read(&d), "b\nc\na\n", "去重追加");
    assert_eq!(share_link::identity(&p).unwrap(), share_link::identity(&d).unwrap(), "账号里是同一个文件");
    // 账号里追加，默认看得到。
    std::fs::OpenOptions::new().append(true).open(&p).and_then(|mut f| std::io::Write::write_all(&mut f, b"e\n")).unwrap();
    assert_eq!(read(&d), "b\nc\na\ne\n");
    // 待合并的旧文件下一次确认没有新追加才删。
    t.run(&[prompts]);
    let leftovers: Vec<_> = std::fs::read_dir(t.profile())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(share_link::MERGE_PENDING_SUFFIX))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    // CLI 改写了账号这边（写临时文件再改名）：断开的链接上多了 f，并回去再链上。
    write(&t.profile().join("tmp"), "b\nc\na\ne\nf\n");
    std::fs::rename(t.profile().join("tmp"), &p).unwrap();
    t.run(&[prompts]);
    assert_eq!(read(&d), "b\nc\na\ne\nf\n");
    assert_eq!(share_link::identity(&p).unwrap(), share_link::identity(&d).unwrap());

    // 默认那边被整个换掉（用户删了 c）：账号里的旧链接还是旧内容，不能把 c 并回来。
    write(&t.default_data().join("tmp"), "b\na\ne\nf\n");
    std::fs::rename(t.default_data().join("tmp"), &d).unwrap();
    let report = t.run(&[prompts]);
    assert_eq!(read(&d), "b\na\ne\nf\n", "删掉的条目不复活");
    assert_eq!(share_link::identity(&p).unwrap(), share_link::identity(&d).unwrap());
    assert!(report.errors.iter().any(|e| e.contains("换掉")), "{:?}", report.errors);
}

#[test]
fn entries_and_files_follow_default_changes() {
    let t = Tmp::new("entries");
    let src = t.default_data().join("prompts");
    write(&src.join("review.md"), "review");
    write(&src.join(".hidden"), "x");
    write(&src.join("group/x.md"), "x");
    let dst = t.profile().join("prompts");
    write(&dst.join("mine.md"), "profile mine");
    let surface = [S::Entries { dir: "prompts", skip: &[] }];

    let report = t.run(&surface);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(read(&dst.join("review.md")), "review");
    assert!(!dst.join(".hidden").exists());
    assert!(points_to(&dst.join("group"), &src.join("group")));

    write(&src.join("review.md"), "review v2");
    std::fs::remove_dir_all(src.join("group")).unwrap();
    t.run(&surface);
    assert_eq!(read(&dst.join("review.md")), "review v2");
    assert!(std::fs::symlink_metadata(dst.join("group")).is_err(), "默认删了的目录，链接拆掉");
    std::fs::remove_file(src.join("review.md")).unwrap();
    t.run(&surface);
    assert!(!dst.join("review.md").exists());
    assert_eq!(read(&dst.join("mine.md")), "profile mine", "账号自己的不动");
}

#[test]
fn codex_config_mirrors_settings_without_provider_and_merges_trust() {
    let t = Tmp::new("codex");
    let surfaces = meowo_agent::by_id("codex").unwrap().profile().unwrap().share;
    let keys: Vec<ShareSurface> = surfaces.iter().copied().filter(|s| matches!(s, S::Keys { .. })).collect();
    write(
        &t.default_data().join("config.toml"),
        "model = \"gpt-5\"\nmodel_provider = \"proxy\"\n\n[mcp_servers.docs]\nurl = \"https://example.com/mcp\"\n\n[projects.'C:\\w\\a']\ntrust_level = \"trusted\"\n",
    );
    write(
        &t.profile().join("config.toml"),
        "# 账号自己的注释\nmodel = \"o3\"\nstale = 1\n\n[hooks.state]\nx = 1\n\n[projects.'C:\\w\\own']\ntrust_level = \"trusted\"\n",
    );
    let report = t.run(&keys);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let text = read(&t.profile().join("config.toml"));
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(doc["model"].as_str(), Some("gpt-5"), "首次与默认一致：{text}");
    assert!(doc.get("stale").is_none(), "{text}");
    assert!(doc.get("model_provider").is_none(), "固定 provider 不带过去：{text}");
    assert_eq!(doc["hooks"]["state"]["x"].as_integer(), Some(1), "Meowo 接线写的状态保留");
    assert_eq!(doc["mcp_servers"]["docs"]["url"].as_str(), Some("https://example.com/mcp"));
    assert_eq!(doc["projects"]["C:\\w\\a"]["trust_level"].as_str(), Some("trusted"));
    assert_eq!(doc["projects"]["C:\\w\\own"]["trust_level"].as_str(), Some("trusted"), "账号自己的信任保留");
    assert!(t.run(&keys).changed.is_empty());
}

#[test]
fn kimi_mcp_servers_merge_keeps_profile_servers() {
    let t = Tmp::new("kimi");
    let surfaces = meowo_agent::by_id("kimi").unwrap().profile().unwrap().share;
    write_json(&t.default_data().join("mcp.json"), &json!({"mcpServers": {"kimi-cu": {"command": "k"}}}));
    write_json(&t.profile().join("mcp.json"), &json!({"mcpServers": {"own": {"command": "o"}}}));
    let report = t.run(surfaces);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let p = read_json(&t.profile().join("mcp.json"));
    assert_eq!(p["mcpServers"]["kimi-cu"]["command"], "k");
    assert_eq!(p["mcpServers"]["own"]["command"], "o");
}

#[test]
fn import_path_is_absolute_outside_home() {
    assert_eq!(import_path(Path::new("/h/.claude/CLAUDE.md"), Path::new("/h")), "~/.claude/CLAUDE.md");
    let outside = import_path(Path::new("/elsewhere/claude/CLAUDE.md"), Path::new("/h"));
    assert!(outside.ends_with("/elsewhere/claude/CLAUDE.md") && !outside.starts_with('~'), "{outside}");
    assert!(!outside.contains('\\'));
}

#[test]
fn every_declared_surface_keeps_credentials_out() {
    for plugin in meowo_agent::registry::all() {
        let Some(spec) = plugin.profile() else { continue };
        let first = spec.creds_rel.split('/').next().unwrap_or_default();
        for surface in spec.share {
            let names: Vec<&str> = match *surface {
                S::AllEntries { exclude, .. } => {
                    assert!(exclude.contains(&first), "{} 的整目录共享没排除凭据 {}", plugin.id(), spec.creds_rel);
                    continue;
                }
                S::HistoryDirs { dirs } => dirs.to_vec(),
                S::Entries { dir, .. } => vec![dir],
                S::File { path } | S::PromptHistory { file: path } => vec![path],
                S::Keys { file, .. } | S::ClaudeState { file, .. } => vec![file],
            };
            for path in names {
                assert_ne!(path, spec.creds_rel, "{} 把凭据声明成了共享面", plugin.id());
                assert!(
                    !spec.creds_rel.starts_with(&format!("{path}/")),
                    "{} 共享的目录 {path} 包着凭据 {}",
                    plugin.id(),
                    spec.creds_rel
                );
            }
        }
    }
}

/// 真实目录副本上的耗时（验收 9）。**全部在副本上跑**：`MEOWO_SHARE_BENCH_ROOT` 下放
/// `home/.claude`（默认目录副本）、`home/.claude.json` 与 `profile/`（账号副本）。历史合并会写
/// 默认目录，所以绝不能指向真实的 `~/.claude`。
/// `cargo test -p meowo-app --lib bench_real_dirs -- --ignored --nocapture`。
#[test]
#[ignore]
fn bench_real_dirs() {
    let Ok(root) = std::env::var("MEOWO_SHARE_BENCH_ROOT") else {
        eprintln!("未设 MEOWO_SHARE_BENCH_ROOT，跳过");
        return;
    };
    let root = PathBuf::from(root);
    let home = root.join("home");
    let default_data = home.join(".claude");
    let profile = root.join("profile");
    let real = meowo_agent::home_dir().unwrap().join(".claude");
    assert_ne!(
        std::fs::canonicalize(&default_data).unwrap(),
        std::fs::canonicalize(&real).unwrap(),
        "必须是副本"
    );
    let record = profile.join(RECORD_FILE);
    let surfaces = claude_surfaces();
    let dirs = SyncDirs { home: &home, default_data: &default_data, profile_data: &profile, record: &record };

    let first = std::time::Instant::now();
    let report = sync(surfaces, &dirs);
    eprintln!("首次：{} ms，改动 {} 项，提醒/错误 {:?}", first.elapsed().as_millis(), report.changed.len(), report.errors);
    for line in &report.changed {
        eprintln!("  {line}");
    }
    let mut samples: Vec<f64> = (0..50)
        .map(|_| {
            let t = std::time::Instant::now();
            let r = sync(surfaces, &dirs);
            assert!(r.changed.is_empty(), "第二轮起不该再有改动：{:?}", r.changed);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    eprintln!("无变化 50 次：P50 {:.1} ms，P95 {:.1} ms，最大 {:.1} ms", samples[24], samples[47], samples[49]);
}
