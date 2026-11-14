use super::*;
use crate::registry::AgentPlugin;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "meowo-lead-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn installation(&self, plugin: &dyn AgentPlugin) -> Installation {
        let path = self.0.join(plugin.id().as_str());
        fs::create_dir_all(&path).unwrap();
        plugin.variants()[0].installation_at(plugin.id(), path, Some(&self.0))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn providers() -> Vec<(&'static dyn AgentPlugin, &'static str)> {
    vec![
        (&crate::plugins::codex::Codex, "codex-cli 0.167.2"),
        (&crate::plugins::kimi::Kimi, "kimi 2.5.3"),
        (&crate::plugins::gemini::Gemini, "0.65.0"),
        (&crate::plugins::opencode::Opencode, "1.19.3"),
    ]
}

#[test]
fn native_install_refresh_and_uninstall_preserve_user_assets() {
    let fixture = Fixture::new();
    for (plugin, version) in providers() {
        let inst = fixture.installation(plugin);
        let cap = plugin.lead().unwrap();
        let spec = cap.spec(&inst).unwrap();
        let user = inst.data_dir.join("user-keep.txt");
        fs::write(&user, "keep").unwrap();
        let hooks = inst.data_dir.join(spec.hook_rel);
        let original=match spec.format {
            LeadHookFormat::Json=>"{\"theme\":\"keep\",\"hooks\":{\"SessionStart\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"user-hook\"}]}]}}",
            LeadHookFormat::Toml=>"# 用户配置注释\ndefault_model = 'keep'\n[[hooks]]\nevent = 'SessionStart'\ncommand = 'user-hook'\ntimeout = 3\n",
            LeadHookFormat::Plugin=>"",
        };
        if !original.is_empty() {
            fs::write(&hooks, original).unwrap();
        }
        let state = install(
            cap,
            &inst,
            Some(version),
            "C:/Program Files/meowo-reporter.exe",
        )
        .unwrap();
        assert!(state.installed, "{} {:?}", plugin.id(), state);
        assert!(!state.limitations.is_empty());
        // 离线续跑已删（leader 2026-11-13「不需要离线」），限制声明里不能再留它的死数据（做空 R9）。
        assert!(
            !state.limitations.iter().any(|l| l.contains("offline")),
            "{} {:?}",
            plugin.id(),
            state.limitations
        );
        let snapshot = fs::read_to_string(&hooks).unwrap();
        assert_eq!(
            install(
                cap,
                &inst,
                Some(version),
                "C:/Program Files/meowo-reporter.exe"
            )
            .unwrap(),
            state
        );
        assert_eq!(snapshot, fs::read_to_string(&hooks).unwrap());
        assert!(
            install(
                cap,
                &inst,
                Some(version),
                "C:/Meowo Next/meowo-reporter.exe"
            )
            .unwrap()
            .installed
        );
        let refreshed = fs::read_to_string(&hooks).unwrap();
        assert!(refreshed.contains("Meowo Next"));
        assert!(!refreshed.contains("Program Files"));
        uninstall(cap, &inst).unwrap();
        uninstall(cap, &inst).unwrap();
        assert!(!inst.data_dir.join(spec.skill_rel).exists());
        assert!(!inst.data_dir.join(spec.reviewer_rel).exists());
        assert_eq!(fs::read_to_string(user).unwrap(), "keep");
        match spec.format {
            LeadHookFormat::Plugin => assert!(!hooks.exists()),
            _ => {
                let text = fs::read_to_string(&hooks).unwrap();
                assert!(text.contains("user-hook"));
                assert!(!text.contains("meowo-reporter"));
                assert!(text.contains("keep"));
            }
        }
    }
}

#[test]
fn conflicts_and_invalid_settings_abort_before_asset_writes() {
    let fixture = Fixture::new();
    for (plugin, version) in providers() {
        let inst = fixture.installation(plugin);
        let cap = plugin.lead().unwrap();
        let spec = cap.spec(&inst).unwrap();
        let reviewer = inst.data_dir.join(spec.reviewer_rel);
        fs::create_dir_all(reviewer.parent().unwrap()).unwrap();
        fs::write(&reviewer, "my reviewer").unwrap();
        assert_eq!(
            install(cap, &inst, Some(version), "meowo-reporter.exe").unwrap_err(),
            "lead/user_file_exists"
        );
        assert!(!inst.data_dir.join(spec.skill_rel).exists());
        assert!(!inst.data_dir.join(spec.hook_rel).exists());
        fs::remove_file(&reviewer).unwrap();
        let hook = inst.data_dir.join(spec.hook_rel);
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, "this is not a valid native config").unwrap();
        assert!(install(cap, &inst, Some(version), "meowo-reporter.exe").is_err());
        assert!(!inst.data_dir.join(spec.skill_rel).exists());
        assert!(!reviewer.exists());
        assert_eq!(
            fs::read_to_string(hook).unwrap(),
            "this is not a valid native config"
        );
    }
}

#[test]
fn native_reviewer_formats_and_disabled_settings_are_preserved() {
    let fixture = Fixture::new();
    for (plugin, version) in providers() {
        let inst = fixture.installation(plugin);
        let cap = plugin.lead().unwrap();
        let reviewer = cap.reviewer();
        match plugin.id().as_str() {
            "codex" => {
                let doc = reviewer.parse::<toml_edit::DocumentMut>().unwrap();
                assert_eq!(doc["name"].as_str(), Some("lead-redteam"));
                assert_eq!(doc["developer_instructions"].as_str(), Some(REVIEW));
                fs::write(
                    inst.data_dir.join("config.toml"),
                    "[agents]\nenabled=false\n",
                )
                .unwrap();
            }
            "gemini" => {
                assert!(reviewer.contains("kind: local\n"));
                assert!(reviewer.contains("  - read_file\n"));
                assert!(!reviewer.contains("  - Read\n"));
                fs::write(
                    inst.data_dir.join("settings.json"),
                    "{\"experimental\":{\"enableAgents\":false}}",
                )
                .unwrap();
            }
            "kimi" => {
                assert!(reviewer.contains("  - Read\n"));
                assert!(reviewer.contains("subagents: []"));
            }
            "opencode" => {
                assert!(reviewer.contains("mode: subagent\n"));
                assert!(reviewer.contains("  edit: deny\n"));
            }
            _ => unreachable!(),
        }
        let state = install(cap, &inst, Some(version), "meowo-reporter.exe").unwrap();
        if matches!(plugin.id().as_str(), "codex" | "gemini") {
            assert!(!state.installed);
            assert!(state.assets_present && state.hooks_present);
            assert_eq!(state.reason.as_deref(), Some("lead/reviewer_disabled"));
        } else {
            assert!(state.installed);
        }
    }
}

#[test]
fn unknown_versions_and_legacy_variants_never_install() {
    let fixture = Fixture::new();
    for (plugin, _) in providers() {
        let inst = fixture.installation(plugin);
        let cap = plugin.lead().unwrap();
        assert_eq!(
            install(cap, &inst, Some("999.999.0"), "meowo-reporter.exe").unwrap_err(),
            "lead/version_unsupported"
        );
        assert_eq!(fs::read_dir(&inst.data_dir).unwrap().count(), 0);
        for variant in plugin.variants().iter().skip(1) {
            let other =
                variant.installation_at(plugin.id(), inst.data_dir.clone(), Some(&fixture.0));
            assert!(cap.spec(&other).is_none());
        }
    }
}

#[test]
fn codex_install_trusts_effective_commands_without_shifting_user_hook_indices() {
    let fixture = Fixture::new();
    let plugin = &crate::plugins::codex::Codex;
    let inst = fixture.installation(plugin);
    let cap = plugin.lead().unwrap();
    let spec = cap.spec(&inst).unwrap();
    let hooks = inst.data_dir.join(spec.hook_rel);
    let config = inst.data_dir.join("config.toml");
    fs::write(
        &hooks,
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    )
    .unwrap();
    fs::write(
        &config,
        "# 用户信任不能丢\n[hooks.state.user-owned]\ntrusted_hash='keep'\n",
    )
    .unwrap();
    assert!(
        install(cap, &inst, Some("0.167.2"), "C:/Tools/meowo-reporter.exe")
            .unwrap()
            .installed
    );
    let doc = fs::read_to_string(&config)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let state = doc["hooks"]["state"].as_table().unwrap();
    assert_eq!(state.len(), 5);
    assert_eq!(state["user-owned"]["trusted_hash"].as_str(), Some("keep"));
    assert!(state
        .iter()
        .any(|(key, _)| key.ends_with(":session_start:1:0")));
    // hooks.json 完整但命令未经信任时不能报告安装就绪。
    fs::write(&config, "[hooks.state.user-owned]\ntrusted_hash='keep'\n").unwrap();
    assert!(!inspect(cap, &inst, Some("0.167.2"), "C:/Tools/meowo-reporter.exe").hooks_present);
    assert!(
        install(cap, &inst, Some("0.167.2"), "C:/Tools/meowo-reporter.exe")
            .unwrap()
            .installed
    );
    uninstall(cap, &inst).unwrap();
    let root = serde_json::from_str::<Value>(&fs::read_to_string(hooks).unwrap()).unwrap();
    assert_eq!(
        root["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "user-hook"
    );
    assert!(root["hooks"]["SessionStart"][1]["hooks"]
        .as_array()
        .unwrap()
        .is_empty());
    let doc = fs::read_to_string(config)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let state = doc["hooks"]["state"].as_table().unwrap();
    assert_eq!(state.len(), 1);
    assert_eq!(state["user-owned"]["trusted_hash"].as_str(), Some("keep"));
}

#[test]
fn installation_follows_native_default_and_environment_roots() {
    let _env = crate::env_guard();
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let keys = [
        "CODEX_HOME",
        "KIMI_CODE_HOME",
        "GEMINI_CLI_HOME",
        "OPENCODE_CONFIG_DIR",
    ];
    let _restore = Restore(
        keys.iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect(),
    );
    for key in keys {
        std::env::remove_var(key);
    }
    let fixture = Fixture::new();
    for (plugin, version) in providers() {
        let variant = &plugin.variants()[0];
        let cap = plugin.lead().unwrap();
        let home = fixture.0.join(plugin.id().as_str());
        fs::create_dir_all(&home).unwrap();
        let expected = home.join(variant.data_dir.candidates[0]);
        fs::create_dir_all(&expected).unwrap();
        let inst = variant.probe(plugin.id(), &home).unwrap();
        assert_eq!(inst.data_dir, expected);
        assert!(!inst.data_dir_from_env);
        assert!(
            install(cap, &inst, Some(version), "meowo-reporter.exe")
                .unwrap()
                .installed
        );
        let override_root = home.join("custom-root");
        let resolved = override_root.join(variant.data_dir.env_suffix);
        fs::create_dir_all(&resolved).unwrap();
        let key = variant.data_dir.env.unwrap();
        std::env::set_var(key, &override_root);
        let inst = variant.probe(plugin.id(), &home).unwrap();
        assert_eq!(inst.data_dir, resolved);
        assert!(inst.data_dir_from_env);
        assert!(
            install(cap, &inst, Some(version), "meowo-reporter.exe")
                .unwrap()
                .installed
        );
        assert!(inst
            .data_dir
            .join(cap.spec(&inst).unwrap().skill_rel)
            .exists());
        if plugin.id() == crate::id::GEMINI {
            assert!(resolved.ends_with("custom-root/.gemini"));
            assert!(!override_root.join("skills/lead/SKILL.md").exists());
        }
        std::env::remove_var(key);
    }
}

#[test]
fn known_version_family_and_managed_command_are_exact() {
    assert!(version_matches(Some("codex-cli 0.167.2"), (0, 167)));
    // 2026-11-13 本机实际升级：一升级就拒绝会让 Lead 装不上，同主版本向前放行。
    assert!(version_matches(Some("0.168.0"), (0, 167)));
    assert!(version_matches(Some("0.68.0"), (0, 65)));
    assert!(version_matches(Some("2.6.0"), (2, 5)));
    assert!(!version_matches(Some("0.166.9"), (0, 167)));
    assert!(!version_matches(Some("3.0.0"), (2, 5)));
    assert!(!version_matches(None, (0, 167)));
    assert!(version_at_least(Some("1.19.5"), (1, 19, 3)));
    assert!(!version_at_least(Some("1.19.2"), (1, 19, 3)));
    assert!(version_untested(Some("codex-cli 0.167.3"), (0, 167, 2)));
    assert!(!version_untested(Some("codex-cli 0.167.2"), (0, 167, 2)));
    // 低于实测版本也算未实测（做空 M6：kimi 2.5.x 过了版本门却没标）。
    assert!(version_untested(Some("kimi 2.5.3"), (2, 6, 0)));
    assert!(managed_command(
        r#""C:/Program Files/meowo-reporter.exe" lead session --provider codex"#,
        "codex"
    ));
    assert!(!managed_command(
        r#"node tools/meowo-reporter.js lead session --provider codex"#,
        "codex"
    ));
    assert!(!managed_command(
        r#""meowo-reporter.exe" lead session --provider kimi"#,
        "codex"
    ));
}

#[test]
fn native_binding_is_only_declared_where_thread_metadata_is_verified() {
    for (plugin, _) in providers() {
        let expected = if plugin.id() == crate::id::CODEX {
            LeadBindingPolicy::NativeThreadMeta
        } else {
            LeadBindingPolicy::Unavailable
        };
        assert_eq!(plugin.lead().unwrap().binding_policy(), expected);
    }
}

/// 任务 20261113-lead-tui：kimi 安装只加一条放行等待器的规则，用户自己的规则与顺序不动，卸载后还原。
#[test]
fn kimi_waiter_permission_rule_is_exact_and_reversible() {
    let fixture = Fixture::new();
    let plugin: &dyn AgentPlugin = &crate::plugins::kimi::Kimi;
    let inst = fixture.installation(plugin);
    let cap = plugin.lead().unwrap();
    let config = inst.data_dir.join("config.toml");
    let original = "default_model = 'keep'\n\n[[permission.rules]]\ndecision = 'deny'\npattern = 'Bash(rm *)'\n";
    fs::write(&config, original).unwrap();
    let reporter = r"C:\Program Files (x86)\Meowo\meowo-reporter.exe";
    let state = install(cap, &inst, Some("kimi 2.6.0"), reporter).unwrap();
    assert!(state.installed, "{state:?}");
    let doc: toml_edit::DocumentMut = fs::read_to_string(&config).unwrap().parse().unwrap();
    let rules = doc["permission"]["rules"].as_array_of_tables().unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules.get(0).unwrap()["pattern"].as_str(), Some("Bash(rm *)"));
    let ours = rules.get(1).unwrap();
    assert_eq!(ours["decision"].as_str(), Some("allow"));
    // picomatch 特殊字符与引号逐个转义；会话 id 只允许 [A-Za-z0-9_-]（不用 `*`，见 kimi::lead SESSION_GLOB）。
    assert_eq!(
        ours["pattern"].as_str(),
        Some(r#"Bash(\"C:/Program Files \(x86\)/Meowo/meowo-reporter.exe\" lead wait --provider kimi --session +([A-Za-z0-9_-]))"#)
    );
    assert_eq!(
        waiter_command_prefix(WaiterShell::PosixDoubleQuoted, "kimi", reporter),
        r#""C:/Program Files (x86)/Meowo/meowo-reporter.exe" lead wait --provider kimi"#
    );
    uninstall(cap, &inst).unwrap();
    let after = fs::read_to_string(&config).unwrap();
    assert!(!after.contains("meowo-reporter"), "{after}");
    assert!(after.contains("Bash(rm *)"));
    // 只有 Meowo 规则时，卸载连空壳一起去掉。
    fs::write(&config, "default_model = 'keep'\n").unwrap();
    install(cap, &inst, Some("kimi 2.6.0"), reporter).unwrap();
    uninstall(cap, &inst).unwrap();
    assert_eq!(fs::read_to_string(&config).unwrap(), "default_model = 'keep'\n");
}

/// gemini：打开后台完成注入与 modelSteering、写放行策略；卸载还原用户原值、删策略与原值记录。
#[test]
fn gemini_auto_continue_settings_restore_user_values_on_uninstall() {
    let fixture = Fixture::new();
    let plugin: &dyn AgentPlugin = &crate::plugins::gemini::Gemini;
    let inst = fixture.installation(plugin);
    let cap = plugin.lead().unwrap();
    let settings = inst.data_dir.join("settings.json");
    fs::write(
        &settings,
        r#"{"theme":"keep","tools":{"shell":{"backgroundCompletionBehavior":"notify","enableInteractiveShell":true}}}"#,
    )
    .unwrap();
    let reporter = r"C:\Users\o'neil\Meowo\meowo-reporter.exe";
    assert!(install(cap, &inst, Some("0.68.0"), reporter).unwrap().installed);
    let doc: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(doc.pointer("/tools/shell/backgroundCompletionBehavior"), Some(&json!("inject")));
    assert_eq!(doc.pointer("/tools/shell/enableInteractiveShell"), Some(&json!(true)));
    assert_eq!(doc.pointer("/experimental/modelSteering"), Some(&json!(true)));
    assert!(crate::plugins::gemini::lead_model_steering(&inst.data_dir));
    let policy = fs::read_to_string(inst.data_dir.join("policies/meowo-lead.toml")).unwrap();
    assert!(policy.contains(MANAGED_MARK));
    let parsed: toml_edit::DocumentMut = policy.parse().unwrap();
    let rule = parsed["rule"].as_array_of_tables().unwrap().get(0).unwrap();
    assert_eq!(rule["toolName"].as_str(), Some("run_shell_command"));
    assert_eq!(rule["commandPrefix"].as_str(), Some(waiter_command_prefix(WaiterShell::PowerShellOnWindows, "gemini", reporter).as_str()));
    // 刷新安装不改记下的原值。
    assert!(install(cap, &inst, Some("0.68.0"), "C:/Meowo Next/meowo-reporter.exe").unwrap().installed);
    uninstall(cap, &inst).unwrap();
    let doc: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(doc.pointer("/tools/shell/backgroundCompletionBehavior"), Some(&json!("notify")));
    assert_eq!(doc.pointer("/experimental"), None);
    assert_eq!(doc["theme"], "keep");
    assert!(!inst.data_dir.join("policies/meowo-lead.toml").exists());
    assert!(!inst.data_dir.join("meowo-lead-settings.json").exists());
    assert!(!crate::plugins::gemini::lead_model_steering(&inst.data_dir));
    assert!(!managed_present(cap, &inst));
}

/// 第一轮做空 M6：续跑能力按机制查证的版本单独设门，低于它（或读不出版本）走手动提示；
/// 2.5.x 照样能装 Lead，但状态里标未实测。
#[test]
fn native_continue_requires_the_version_its_mechanism_was_verified_on() {
    let kimi = crate::plugins::kimi::Kimi;
    let gemini = crate::plugins::gemini::Gemini;
    let kimi_cap = AgentPlugin::lead(&kimi).unwrap();
    let gemini_cap = AgentPlugin::lead(&gemini).unwrap();
    assert!(native_continue_for(kimi_cap, Some("2.6.0")).is_some());
    assert!(native_continue_for(kimi_cap, Some("kimi, version 2.6.3")).is_some());
    assert!(native_continue_for(kimi_cap, Some("2.5.9")).is_none());
    assert!(native_continue_for(kimi_cap, None).is_none(), "读不出版本不冒险");
    assert!(native_continue_for(gemini_cap, Some("0.68.0")).is_some());
    assert!(native_continue_for(gemini_cap, Some("0.67.2")).is_none());
    assert!(native_continue_for(gemini_cap, Some("0.65.0")).is_none());
    // 没声明门槛的（codex/opencode）不受影响。
    for (plugin, _) in providers() {
        let cap = plugin.lead().unwrap();
        if cap.native_continue_floor().is_none() {
            assert_eq!(native_continue_for(cap, None), cap.native_continue());
        }
    }

    let fixture = Fixture::new();
    let plugin: &dyn AgentPlugin = &kimi;
    let inst = fixture.installation(plugin);
    fs::write(inst.data_dir.join("config.toml"), "").unwrap();
    let state = install(kimi_cap, &inst, Some("2.5.3"), "C:/Meowo/meowo-reporter.exe").unwrap();
    assert!(state.installed);
    assert!(state.limitations.iter().any(|l| l == "lead/version_untested"), "{state:?}");
    let plugin: &dyn AgentPlugin = &gemini;
    let inst = fixture.installation(plugin);
    fs::write(inst.data_dir.join("settings.json"), "{}").unwrap();
    let state = install(gemini_cap, &inst, Some("0.68.0"), "C:/Meowo/meowo-reporter.exe").unwrap();
    assert!(!state.limitations.iter().any(|l| l == "lead/version_untested"), "{state:?}");
}

/// 共享层不按 provider 身份分支（做空 M7）：等待器 shell 由插件声明。
#[test]
fn waiter_shell_comes_from_the_plugin_description() {
    assert_eq!(waiter_shell("kimi"), Some(WaiterShell::PosixDoubleQuoted));
    assert_eq!(waiter_shell("gemini"), Some(WaiterShell::PowerShellOnWindows));
    assert_eq!(waiter_shell("codex"), None);
    assert_eq!(waiter_shell("opencode"), None);
    assert_eq!(waiter_shell("nope"), None);
}

/// 做空 N5：reporter 要求启动等待器与宿主判定续跑走同一个版本门。版本不到（或读不出）时不要求
/// 起等待器，与面板「手动继续」一致；已起来的等待器仍按插件声明认参数。
#[test]
fn waiter_requirement_uses_the_same_version_gate_as_the_host() {
    for (provider, old, ok) in [("kimi", "2.5.3", "2.6.0"), ("gemini", "0.67.0", "0.68.0")] {
        let cap = crate::by_id(provider).unwrap().lead().unwrap();
        for version in [Some(old), Some(ok), None, Some("3.0.0"), Some("1.0.0")] {
            assert_eq!(
                waiter_shell_for(provider, version).is_some(),
                native_continue_for(cap, version).is_some(),
                "{provider} {version:?}"
            );
        }
        assert!(waiter_shell_for(provider, Some(old)).is_none());
        assert!(waiter_shell_for(provider, None).is_none());
        assert_eq!(waiter_shell_for(provider, Some(ok)), waiter_shell(provider));
        assert!(continue_needs_version(provider));
    }
    // 服务投递机制没有版本门：不需要探测版本，也没有等待器。
    for provider in ["codex", "opencode", "nope"] {
        assert!(!continue_needs_version(provider), "{provider}");
        assert_eq!(waiter_shell_for(provider, Some("9.9.9")), None);
    }
}
