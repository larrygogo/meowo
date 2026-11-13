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
