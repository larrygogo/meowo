//! 端到端：**代理真的被写进 Claude Code 自己的 settings.json 了吗。**
//!
//! 这是整个功能的命门。单测覆盖了 `ensure_env` 的合并与认领纪律，但「Meowo 的 settings.json →
//! 解析出该 agent 的代理 → 找到 claude 的配置文件 → 写进 `env` 块」这条链路要跨三个模块 + 真实
//! 文件系统，任何一环接错（路径解析、序列化字段名、per_agent 覆盖没生效）单测都发现不了。
//!
//! 跑在**独立进程**里（集成测试各有自己的二进制）：它要设 `CLAUDE_CONFIG_DIR` / `MEOWO_DB` 这类
//! 进程级环境变量，与 lib 单测并行会互相串味。本文件内的用例必须串行——故只写一个用例，
//! 分阶段断言。

use std::collections::BTreeMap;
use std::path::PathBuf;

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("meowo-proxy-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建临时目录");
    p
}

fn read_json(p: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(p).expect("读配置")).expect("配置应为合法 JSON")
}

/// 把 Meowo 的 settings.json 写成指定的 proxy 段。
fn write_meowo_settings(meowo_dir: &std::path::Path, proxy: serde_json::Value) {
    let s = serde_json::json!({ "proxy": proxy });
    std::fs::write(
        meowo_dir.join("settings.json"),
        serde_json::to_string_pretty(&s).unwrap(),
    )
    .unwrap();
}

#[test]
fn proxy_lands_in_claude_settings_json_end_to_end() {
    // claude 的数据目录（CLAUDE_CONFIG_DIR 指向它，变体表据此 probe 到实况）。
    let claude_dir = tmp_dir("claude");
    // Meowo 自己的数据目录（MEOWO_DB 决定 settings.json / proxy-applied.json 落在哪）。
    let meowo_dir = tmp_dir("meowo");

    std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);
    std::env::set_var("MEOWO_DB", meowo_dir.join("board.db"));

    let settings_json = claude_dir.join("settings.json");
    // 用户既有的配置：hooks + statusLine + 自己的一个 env 键。一个都不能丢。
    std::fs::write(
        &settings_json,
        r#"{
  "env": { "FOO": "bar" },
  "statusLine": { "type": "command", "command": "hud" },
  "hooks": { "Stop": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "x" }] }] }
}"#,
    )
    .unwrap();

    // ── 1. 开代理 → 写进 env 块 ──
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({ "mode": "custom", "url": "http://127.0.0.1:7890" }),
    );
    let reports = meowo_app_lib::proxy::apply_to_agent_configs();

    let v = read_json(&settings_json);
    assert_eq!(
        v["env"]["HTTPS_PROXY"], "http://127.0.0.1:7890",
        "代理必须落进 claude 的 env 块"
    );
    assert_eq!(v["env"]["HTTP_PROXY"], "http://127.0.0.1:7890");
    // 用户既有内容原样保留——写坏用户的 settings.json 是这里最不可接受的失败。
    assert_eq!(v["env"]["FOO"], "bar");
    assert_eq!(v["statusLine"]["command"], "hud");
    assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "x");
    // 写前必备份。
    assert!(
        claude_dir.join("settings.json.cckb-bak").exists(),
        "写前应留一份备份"
    );

    let claude_rep = reports
        .iter()
        .find(|r| r.agent == "claude")
        .expect("应有 claude 的结果");
    assert!(
        claude_rep.error.is_none(),
        "不该报错：{:?}",
        claude_rep.error
    );
    assert!(claude_rep.skipped.is_empty());

    // 幂等：再跑一次内容不变。
    let before = std::fs::read_to_string(&settings_json).unwrap();
    meowo_app_lib::proxy::apply_to_agent_configs();
    assert_eq!(
        std::fs::read_to_string(&settings_json).unwrap(),
        before,
        "幂等：不该反复重写"
    );

    // ── 2. per-agent 覆盖：把 claude 单独设成直连 → 我们写的键被清掉 ──
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({
            "mode": "custom",
            "url": "http://127.0.0.1:7890",
            "per_agent": { "claude": { "mode": "off", "url": "" } }
        }),
    );
    meowo_app_lib::proxy::apply_to_agent_configs();

    let v = read_json(&settings_json);
    assert!(
        v["env"].get("HTTPS_PROXY").is_none(),
        "关代理后我们写的键必须清干净"
    );
    assert!(v["env"].get("HTTP_PROXY").is_none());
    assert_eq!(v["env"]["FOO"], "bar", "用户的 env 键不受影响");

    // ── 3. 用户自己设了 HTTPS_PROXY → 绝不覆盖，且如实回传 ──
    std::fs::write(
        &settings_json,
        r#"{ "env": { "HTTPS_PROXY": "http://corp-proxy:8080" } }"#,
    )
    .unwrap();
    // 清掉认领记录，模拟「这个值从来不是我们写的」。
    let _ = std::fs::remove_file(meowo_dir.join("proxy-applied.json"));
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({ "mode": "custom", "url": "http://mine:7890" }),
    );
    let reports = meowo_app_lib::proxy::apply_to_agent_configs();

    let v = read_json(&settings_json);
    assert_eq!(
        v["env"]["HTTPS_PROXY"], "http://corp-proxy:8080",
        "用户手设的企业代理**绝不能**被覆盖"
    );
    let claude_rep = reports.iter().find(|r| r.agent == "claude").unwrap();
    assert_eq!(
        claude_rep.skipped,
        vec!["HTTPS_PROXY".to_string()],
        "跳过的键必须如实回传，否则用户以为代理生效了"
    );

    // ── 4. SOCKS：claude 官方不支持 → 一个键都不写，并给出原因 ──
    std::fs::write(&settings_json, r#"{"env":{}}"#).unwrap();
    let _ = std::fs::remove_file(meowo_dir.join("proxy-applied.json"));
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({ "mode": "custom", "url": "socks5://127.0.0.1:1080" }),
    );
    let reports = meowo_app_lib::proxy::apply_to_agent_configs();

    let v = read_json(&settings_json);
    assert!(
        v["env"].get("HTTPS_PROXY").is_none() && v["env"].get("ALL_PROXY").is_none(),
        "claude 不支持 SOCKS，绝不能把一个它不认识的串塞进去"
    );
    let claude_rep = reports.iter().find(|r| r.agent == "claude").unwrap();
    assert!(
        claude_rep.unsupported.is_some(),
        "必须告知为什么没生效，而不是静默不写"
    );

    // ── 5. 认领记录确实落盘了（下次运行据此判断哪些键是我们的） ──
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({ "mode": "custom", "url": "http://127.0.0.1:7890" }),
    );
    meowo_app_lib::proxy::apply_to_agent_configs();
    let applied: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(
        &std::fs::read_to_string(meowo_dir.join("proxy-applied.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(applied["claude"]["HTTPS_PROXY"], "http://127.0.0.1:7890");

    // ── 6. 所有权状态写不下来 → 回滚刚写入的代理，不能留下一个日后无法安全删除的键 ──
    std::fs::write(&settings_json, r#"{"env":{"FOO":"bar"}}"#).unwrap();
    let applied_path = meowo_dir.join("proxy-applied.json");
    std::fs::remove_file(&applied_path).unwrap();
    std::fs::create_dir(&applied_path).unwrap(); // 用同名目录稳定制造 rename 失败
    write_meowo_settings(
        &meowo_dir,
        serde_json::json!({ "mode": "custom", "url": "http://rollback:7890" }),
    );
    let reports = meowo_app_lib::proxy::apply_to_agent_configs();
    let v = read_json(&settings_json);
    assert!(
        v["env"].get("HTTPS_PROXY").is_none(),
        "状态落盘失败后必须回滚代理写入"
    );
    assert_eq!(v["env"]["FOO"], "bar");
    let claude_rep = reports.iter().find(|r| r.agent == "claude").unwrap();
    assert!(claude_rep
        .error
        .as_deref()
        .is_some_and(|e| e.contains("已回滚")));
    std::fs::remove_dir(&applied_path).unwrap();

    std::env::remove_var("CLAUDE_CONFIG_DIR");
    std::env::remove_var("MEOWO_DB");
    let _ = std::fs::remove_dir_all(&claude_dir);
    let _ = std::fs::remove_dir_all(&meowo_dir);
}
