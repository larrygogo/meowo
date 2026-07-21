//! 代理能力声明：**某个 agent 怎么才能被套上代理**。三家差异极大，全部收敛在这张表里。
//!
//! 调研结论（2026-07 复核，依据是各家官方文档与 issue，不是道听途说）：
//!
//! | agent | 写进自己的配置文件 | 认进程环境变量 | SOCKS |
//! |-------|------------------|--------------|-------|
//! | claude | ✅ `settings.json` 的 `env` 块（官方：作用于每个会话及其子进程） | ✅ | ❌ 官方明确不支持 |
//! | codex  | ❌ 无任何配置键能代理它自己的 API 请求 | ⚠️ 部分 | ❌ 未编译 reqwest 的 socks feature |
//! | kimi   | ❌ config.toml 无处设代理 | ✅ 官方明确全流量支持 | ✅ 支持 |
//!
//! 两条容易踩错、故写明依据的：
//!
//! - **codex 的配置文件里没有一个键能给它自己的 API 请求挂代理。** `shell_environment_policy.set`
//!   官方原文是 "Explicit environment overrides injected into every **subprocess**"——只注入给它
//!   派生的子进程（模型执行的 shell 命令）；`features.network_proxy` 只管**沙箱内**的工具执行。
//!   连「认环境变量」也只是部分成立：见 openai/codex#4242（2025-09 至今 OPEN），它的各个 HTTP
//!   client 并未统一读代理环境变量，主 API 请求靠 reqwest 的默认系统代理检测才走得通，登录之类的
//!   旁路曾经不走。
//! - **kimi 的 `[providers.<name>.env]` 不是 env 块。** 它是给 provider 实例传参用的（官方例子是
//!   `env = { GOOGLE_CLOUD_PROJECT = "..." }`），文档从没说能拿它设代理——别指望。但它的环境变量
//!   支持是三家里最好的：官方明文 "honors the standard proxy environment variables for **all
//!   outbound traffic**"（模型调用 / MCP / 登录 / 更新检查全覆盖），HTTP(S) 与 SOCKS 都认。
//!
//! 由此推出的覆盖面（这是产品语义，不是实现细节）：**只有 claude 能做到「你自己在终端敲 claude
//! 也走代理」**；codex / kimi 只认进程环境变量，而进程环境变量只能注入给**我们自己拉起**的进程，
//! 故它们只覆盖从 Meowo 打开的会话。
//!
//! 「把代理写进用户级环境变量（注册表 / shell profile）」能补上这个缺口，但**刻意不做**：系统里
//! 只有一份 `HTTPS_PROXY`，三家就得共用同一个代理，per-agent 隔离当场作废（「Claude 走境外代理、
//! Kimi 直连」正是最常见的配法），还会波及系统上每一个新开的程序。代价远大于收益。

/// 某 agent 接受代理配置的方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxySpec {
    /// 支持 SOCKS 吗？填错的后果是**静默连不上**（agent 拿到一个它不认识的代理串），
    /// 所以设置页必须按此当场拒绝，而不是等用户去猜。
    pub socks: bool,
    /// 能否把代理写进它**自己的**配置文件——从而不管由谁启动都生效。仅 claude 为 true。
    pub config_env: bool,
    /// HTTP(S) 代理写哪些环境变量名。
    pub http_keys: &'static [&'static str],
    /// SOCKS 代理写哪些环境变量名（`socks` 为 false 时无意义）。
    /// kimi 文档：SOCKS 通常经 `ALL_PROXY` 配置——写进 `HTTPS_PROXY` 未必被识别。
    pub socks_keys: &'static [&'static str],
}

impl ProxySpec {
    /// 该 agent 能否用这个代理串。不能则返回**可直接展示给用户**的原因。
    pub fn accepts(&self, proxy: &str) -> Result<(), String> {
        if is_socks(proxy) && !self.socks {
            return Err("该模型不支持 SOCKS 代理，请改用 HTTP 代理端口（如 Clash 的 7890）".into());
        }
        Ok(())
    }

    /// 该代理串应写成哪些环境变量。`accepts` 为 Err 时返回空（绝不写一个它不认识的串）。
    pub fn env_for(&self, proxy: &str) -> Vec<(&'static str, String)> {
        if self.accepts(proxy).is_err() {
            return Vec::new();
        }
        let keys = if is_socks(proxy) {
            self.socks_keys
        } else {
            self.http_keys
        };
        keys.iter().map(|k| (*k, proxy.to_string())).collect()
    }

    /// 本 agent **可能**写过的全部键。关代理时据此清理——只列 http_keys 会把 socks 时写下的
    /// `ALL_PROXY` 落在配置里，留下一个再也关不掉的代理。
    pub fn all_keys(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.http_keys.to_vec();
        for k in self.socks_keys {
            if !v.contains(k) {
                v.push(k);
            }
        }
        v
    }
}

/// 代理串是不是 SOCKS 系。
///
/// 前缀清单必须盖全设置页允许保存的 SOCKS 形态（app 侧 `SCHEMES` = socks4 / socks4a /
/// socks5）：漏掉一个，该形态就会绕过「不支持 SOCKS 的 agent 当场拒收」与「自更新仅
/// HTTP 代理」两道拦截，静默走错通道。socks5h 虽不在 SCHEMES 里，保守仍算 SOCKS。
pub fn is_socks(proxy: &str) -> bool {
    let s = proxy.trim().to_ascii_lowercase();
    s.starts_with("socks4://")
        || s.starts_with("socks4a://")
        || s.starts_with("socks5://")
        || s.starts_with("socks5h://")
        || s.starts_with("socks://")
}

// ═══ 写进 agent 自己的配置文件（目前只有 claude 的 settings.json `env` 块） ═══

/// [`ensure_env`] 的结果。
#[derive(Debug, PartialEq, Eq)]
pub struct EnvPlan {
    pub outcome: crate::config::EnsureOutcome,
    /// **用户自有**、我们没敢动的键。宿主据此提示：「你在 settings.json 里手设了 HTTPS_PROXY，
    /// Meowo 不会覆盖它」——静默跳过会让用户以为代理配上了，实际走的是别的值。
    pub skipped: Vec<&'static str>,
}

/// 幂等地把代理写进 agent 配置的 `env` 块（JSON 系，即 claude 的 settings.json）。
///
/// - `desired`：本次应生效的键值。**空 = 关代理**，据此清掉我们写过的键。
/// - `owned`：我们**上次写下**的键值（宿主持久化）。这是认领的唯一依据。
///
/// 纪律（与 hooks 的 `CommandSpec::claim` 同源）：某个键当前的值既不等于我们上次写的、也不等于
/// 本次要写的 → 判定为**用户自有**，原样不动并记进 `skipped`。`env` 是扁平 map，没有这份记录就
/// 分不清「我上次写的」和「用户自己写的」——要么不敢关，要么把用户的企业代理覆盖掉。
pub fn ensure_env(
    cur_text: &str,
    desired: &[(&'static str, String)],
    owned: &std::collections::BTreeMap<String, String>,
    all_keys: &[&'static str],
) -> EnvPlan {
    use crate::config::{EnsureOutcome, RepairReason};
    use serde_json::json;

    let plan = |outcome, skipped| EnvPlan { outcome, skipped };

    // 解析失败 / 顶层非对象 → 绝不覆盖（settings.json 还装着用户的 hooks、statusLine、权限规则）。
    let Some(mut root) = crate::config::parse_json_config(cur_text) else {
        return plan(
            EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
            vec![],
        );
    };

    let desired_map: std::collections::BTreeMap<&str, &str> =
        desired.iter().map(|(k, v)| (*k, v.as_str())).collect();

    match root.get("env") {
        // env 键存在但不是 object（用户手改坏）→ 放弃，绝不置空覆盖。
        Some(v) if !v.is_object() => {
            return plan(
                EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
                vec![],
            );
        }
        // 没有 env 键、且本次也不需要写任何东西（关代理）→ 无事可做。
        None if desired.is_empty() => return plan(EnsureOutcome::Unchanged, vec![]),
        None => root["env"] = json!({}),
        Some(_) => {}
    }

    let Some(env) = root.get_mut("env").and_then(|v| v.as_object_mut()) else {
        return plan(
            EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
            vec![],
        );
    };

    let mut changed = false;
    let mut skipped = Vec::new();
    for k in all_keys {
        let want: Option<&str> = desired_map.get(k).copied();
        let cur: Option<String> = env.get(*k).and_then(|x| x.as_str()).map(str::to_string);
        let ours: Option<&str> = owned.get(*k).map(String::as_str);

        // 用户自有值：当前有值、但不等于我们上次明确记录的值 → 一律不动。
        // 即使它碰巧等于本次想写的值，也不能顺手“认领”；否则用户原本的企业代理会在
        // Meowo 日后关闭代理时被误删。
        if let Some(c) = cur.as_deref() {
            if Some(c) != ours {
                skipped.push(*k);
                continue;
            }
        }
        match want {
            Some(v) if cur.as_deref() != Some(v) => {
                env.insert((*k).to_string(), json!(v));
                changed = true;
            }
            None if cur.is_some() => {
                env.remove(*k); // 关代理：把我们写过的键清干净（留着 = 一个再也关不掉的代理）
                changed = true;
            }
            _ => {}
        }
    }

    if !changed {
        return plan(EnsureOutcome::Unchanged, skipped);
    }
    match serde_json::to_string_pretty(&root) {
        Ok(s) => plan(EnsureOutcome::Changed(format!("{s}\n")), skipped),
        Err(_) => plan(EnsureOutcome::Abandon(RepairReason::WriteFailed), skipped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// claude 形态：不支持 SOCKS。
    static NO_SOCKS: ProxySpec = ProxySpec {
        socks: false,
        config_env: true,
        http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
        socks_keys: &[],
    };
    /// kimi 形态：支持 SOCKS，且 SOCKS 走 ALL_PROXY。
    static WITH_SOCKS: ProxySpec = ProxySpec {
        socks: true,
        config_env: false,
        http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
        socks_keys: &["ALL_PROXY"],
    };

    #[test]
    fn socks_is_rejected_where_unsupported() {
        // 这是 claude 的硬约束（官方：Claude Code does not support SOCKS proxies）。
        // 静默放行的后果是用户配完发现 claude 连不上，且毫无线索。
        assert!(NO_SOCKS.accepts("socks5://127.0.0.1:1080").is_err());
        assert!(NO_SOCKS.accepts("http://127.0.0.1:7890").is_ok());
        // 支持的 agent 则两种都收。
        assert!(WITH_SOCKS.accepts("socks5://127.0.0.1:1080").is_ok());
        assert!(WITH_SOCKS.accepts("http://127.0.0.1:7890").is_ok());
    }

    #[test]
    fn env_keys_depend_on_scheme() {
        assert_eq!(
            NO_SOCKS.env_for("http://127.0.0.1:7890"),
            vec![
                ("HTTPS_PROXY", "http://127.0.0.1:7890".to_string()),
                ("HTTP_PROXY", "http://127.0.0.1:7890".to_string())
            ]
        );
        // SOCKS 走 ALL_PROXY，不写进 HTTPS_PROXY（未必被识别）。
        assert_eq!(
            WITH_SOCKS.env_for("socks5://127.0.0.1:1080"),
            vec![("ALL_PROXY", "socks5://127.0.0.1:1080".to_string())]
        );
        // 不支持 socks 的 agent 拿到 socks 串 → 一个键都不写（绝不塞一个它不认识的值）。
        assert!(NO_SOCKS.env_for("socks5://127.0.0.1:1080").is_empty());
    }

    #[test]
    fn all_keys_covers_socks_so_cleanup_is_complete() {
        // 关代理时按 all_keys 清理。若只清 http_keys，socks 时写下的 ALL_PROXY 会留在配置里，
        // 变成一个关不掉的代理。
        assert_eq!(
            WITH_SOCKS.all_keys(),
            vec!["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"]
        );
        assert_eq!(NO_SOCKS.all_keys(), vec!["HTTPS_PROXY", "HTTP_PROXY"]);
    }

    #[test]
    fn is_socks_detects_all_forms() {
        for s in [
            "socks5://h:1",
            "SOCKS5://h:1",
            " socks4://h:1",
            "socks4a://h:1", // 设置页允许保存（app 侧 SCHEMES）此前却漏判：会绕过自更新的 SOCKS 拦截
            "socks5h://h:1",
            "socks://h:1",
        ] {
            assert!(is_socks(s), "{s} 应判为 socks");
        }
        for s in ["http://h:1", "https://h:1", "h:1", ""] {
            assert!(!is_socks(s), "{s} 不该判为 socks");
        }
    }

    // ── ensure_env ──

    use crate::config::EnsureOutcome;
    use std::collections::BTreeMap;

    const KEYS: [&str; 2] = ["HTTPS_PROXY", "HTTP_PROXY"];

    fn owned(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    fn want(url: &str) -> Vec<(&'static str, String)> {
        vec![
            ("HTTPS_PROXY", url.to_string()),
            ("HTTP_PROXY", url.to_string()),
        ]
    }
    /// 跑一次并要求有改动，返回解析后的 settings。
    fn changed(
        text: &str,
        desired: &[(&'static str, String)],
        own: &BTreeMap<String, String>,
    ) -> serde_json::Value {
        let p = ensure_env(text, desired, own, &KEYS);
        match p.outcome {
            EnsureOutcome::Changed(s) => serde_json::from_str(&s).expect("产物应为合法 JSON"),
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    #[test]
    fn writes_env_block_and_is_idempotent() {
        let v = changed("{}", &want("http://127.0.0.1:7890"), &owned(&[]));
        assert_eq!(v["env"]["HTTPS_PROXY"], "http://127.0.0.1:7890");
        assert_eq!(v["env"]["HTTP_PROXY"], "http://127.0.0.1:7890");

        // 再跑一次：无改动（否则每次启动都重写用户配置）。
        let text = serde_json::to_string(&v).unwrap();
        let own = owned(&[
            ("HTTPS_PROXY", "http://127.0.0.1:7890"),
            ("HTTP_PROXY", "http://127.0.0.1:7890"),
        ]);
        let p = ensure_env(&text, &want("http://127.0.0.1:7890"), &own, &KEYS);
        assert_eq!(p.outcome, EnsureOutcome::Unchanged);
    }

    #[test]
    fn preserves_user_hooks_statusline_and_other_env_keys() {
        // settings.json 里装着用户的 hooks / statusLine / 其它 env 键——一个都不能丢。
        let src = r#"{"env":{"FOO":"bar"},"statusLine":{"command":"hud"},
                      "hooks":{"Stop":[{"matcher":"*","hooks":[{"command":"x"}]}]}}"#;
        let v = changed(src, &want("http://p:1"), &owned(&[]));
        assert_eq!(v["env"]["FOO"], "bar");
        assert_eq!(v["statusLine"]["command"], "hud");
        assert!(v["hooks"]["Stop"].is_array());
        assert_eq!(v["env"]["HTTPS_PROXY"], "http://p:1");
    }

    /// 核心纪律：用户自己在 env 里设了 HTTPS_PROXY（企业代理很常见）→ **绝不覆盖**，如实回传。
    /// 静默覆盖会把用户的企业代理换掉，且他毫无察觉。
    #[test]
    fn never_clobbers_a_user_set_proxy() {
        let src = r#"{"env":{"HTTPS_PROXY":"http://corp-proxy:8080"}}"#;
        // owned 为空 = 我们从没写过 → 这个值只能是用户的。
        let p = ensure_env(src, &want("http://mine:7890"), &owned(&[]), &KEYS);
        assert_eq!(p.skipped, vec!["HTTPS_PROXY"], "用户自有的键应如实回传");
        // HTTP_PROXY 我们没写过、用户也没设 → 可以写。HTTPS_PROXY 保持用户的值。
        match p.outcome {
            EnsureOutcome::Changed(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(
                    v["env"]["HTTPS_PROXY"], "http://corp-proxy:8080",
                    "用户的值必须原样保留"
                );
                assert_eq!(v["env"]["HTTP_PROXY"], "http://mine:7890");
            }
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    #[test]
    fn never_claims_a_user_value_that_happens_to_equal_desired() {
        let src = r#"{"env":{"HTTPS_PROXY":"http://mine:7890"}}"#;
        let p = ensure_env(src, &want("http://mine:7890"), &owned(&[]), &KEYS);
        assert!(p.skipped.contains(&"HTTPS_PROXY"));
        // 另一个尚不存在的键仍可正常写入。
        match p.outcome {
            EnsureOutcome::Changed(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                assert_eq!(v["env"]["HTTPS_PROXY"], "http://mine:7890");
                assert_eq!(v["env"]["HTTP_PROXY"], "http://mine:7890");
            }
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    #[test]
    fn updates_only_the_value_we_wrote_last_time() {
        // 上次我们写的是 old；用户没动过 → 认领并更新成 new。
        let src = r#"{"env":{"HTTPS_PROXY":"http://old:1","HTTP_PROXY":"http://old:1"}}"#;
        let own = owned(&[
            ("HTTPS_PROXY", "http://old:1"),
            ("HTTP_PROXY", "http://old:1"),
        ]);
        let v = changed(src, &want("http://new:2"), &own);
        assert_eq!(v["env"]["HTTPS_PROXY"], "http://new:2");
        assert_eq!(v["env"]["HTTP_PROXY"], "http://new:2");
    }

    /// 关代理：把我们写过的键清干净。清不干净 = 一个再也关不掉的代理。
    #[test]
    fn turning_off_removes_only_our_keys() {
        let src =
            r#"{"env":{"HTTPS_PROXY":"http://mine:1","HTTP_PROXY":"http://mine:1","FOO":"bar"}}"#;
        let own = owned(&[
            ("HTTPS_PROXY", "http://mine:1"),
            ("HTTP_PROXY", "http://mine:1"),
        ]);
        let v = changed(src, &[], &own);
        assert!(v["env"].get("HTTPS_PROXY").is_none(), "我们写的键应被清掉");
        assert!(v["env"].get("HTTP_PROXY").is_none());
        assert_eq!(v["env"]["FOO"], "bar", "用户的其它 env 键不受影响");
    }

    /// 关代理时，用户自己设的代理**不能**被我们顺手删掉。
    #[test]
    fn turning_off_does_not_delete_a_user_set_proxy() {
        let src = r#"{"env":{"HTTPS_PROXY":"http://corp:8080"}}"#;
        let p = ensure_env(src, &[], &owned(&[]), &KEYS);
        assert_eq!(p.outcome, EnsureOutcome::Unchanged, "不是我们写的，不该动");
        assert_eq!(p.skipped, vec!["HTTPS_PROXY"]);
    }

    #[test]
    fn nothing_to_do_when_no_env_and_no_desired() {
        assert_eq!(
            ensure_env("{}", &[], &owned(&[]), &KEYS).outcome,
            EnsureOutcome::Unchanged
        );
    }

    #[test]
    fn abandons_on_malformed_config() {
        // env 被手改成非 object、顶层非对象、非法 JSON → 一律放弃，绝不写坏用户文件。
        for src in [r#"{"env":[1,2]}"#, "[]", "{not json"] {
            let p = ensure_env(src, &want("http://p:1"), &owned(&[]), &KEYS);
            assert!(
                matches!(p.outcome, EnsureOutcome::Abandon(_)),
                "src={src:?} 应放弃"
            );
        }
    }

    #[test]
    fn tolerates_utf8_bom() {
        // Windows 编辑器写出的带 BOM 的 settings.json（hooks 侧曾因此静默失败）。
        let v = changed("\u{feff}{}", &want("http://p:1"), &owned(&[]));
        assert_eq!(v["env"]["HTTPS_PROXY"], "http://p:1");
    }
}
