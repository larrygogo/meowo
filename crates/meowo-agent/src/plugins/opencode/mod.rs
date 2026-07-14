//! opencode 插件。它与另四家的根本差别只有一条：**opencode 没有 command hook**。
//!
//! 它的扩展面只有 TS 插件——启动时扫 `<配置目录>/{plugin,plugins}/*.ts`（源码 `config/plugin.ts` 的
//! glob，单复数都收，只扫一层）。所以 meowo 的「接线」在这里落成一份由我们生成的桥接插件：它订阅
//! opencode 的事件，把负载 spawn 给 meowo-reporter。生成与认领见 [`ConfigFormat::OpencodeTs`]。
//!
//! 这条路看着绕，实则**是五家里 reporter 侧最干净的一个**：负载由我们自己构造，直接吐成 claude
//! 同款的 `{hook_event_name, session_id, cwd}`，dispatch 一行都不用改（对比 gemini 还得译事件名）。
//!
//! 事件映射（左：opencode 的事件/hook；右：我们构造的规范名）：
//!
//! | opencode | 规范名 | 备注 |
//! |---|---|---|
//! | `event: session.created` | `SessionStart` | `properties.info` 里同时有 id 与 directory |
//! | hook `chat.message` | `UserPromptSubmit` | 顺带把 parts 里的文本拼成 `prompt`（标题靠它） |
//! | hook `tool.execute.after` | `PostToolUse` | |
//! | `event: session.idle` | `Stop` | 每个回合跑完发一次，正是 Stop 语义 |
//! | `event: session.deleted` | `SessionEnd` | opencode 没有「会话结束」，删除是唯一确定的终结 |
//!
//! 因此 opencode 的会话收尾与 codex 一样靠 Stop + 判活，而不是一条可靠的 SessionEnd。

pub mod account;

use crate::{
    auth::{AuthScheme, CredentialSource},
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 多账号：**必须隔离两个目录**——这是 opencode 独有的坑。
///
/// 它把配置与数据分了家：插件读**配置**目录（`OPENCODE_CONFIG_DIR`），凭据写**数据**目录
/// （`~/.local/share/opencode/auth.json`）。只设 `OPENCODE_CONFIG_DIR` 的话，几个 profile 的插件
/// 是分开了，凭据却仍然共用同一份 auth.json——账号看起来切了，其实压根没切。这种「看起来隔离了、
/// 实际没有」是最坏的一种失败：没有任何报错，你只会莫名其妙地在用另一个账号。
///
/// 数据目录的覆盖变量是 **`XDG_DATA_HOME`**（实测：`OPENCODE_DATA_DIR` 无效，而设了 XDG_DATA_HOME
/// 后 `opencode db path` 确实跟着搬家）。opencode 在它下面再拼一层 `opencode/`，故凭据落在
/// `<root>/data/opencode/auth.json`。
static PROFILE: crate::profile::ProfileSpec = crate::profile::ProfileSpec {
    envs: &[
        // 首条必须是 data_dir（hooks/插件落这儿）——profile.rs 有绊线测试盯着。
        ("OPENCODE_CONFIG_DIR", "config"),
        ("XDG_DATA_HOME", "data"),
    ],
    data_rel: "config",
    creds_rel: "data/opencode/auth.json",
};

/// 凭据是 `~/.local/share/opencode/auth.json`——**不在 data_dir 底下**。
///
/// opencode 把配置与数据分了家：插件必须落在**配置**目录（`~/.config/opencode`，即我们的 data_dir），
/// 凭据却写在**数据**目录。另四家两者同处一地，`CredentialSource::File`（相对 data_dir）一直够用；
/// 这一家逼出了 [`CredentialSource::HomeFile`]（相对 home）。实测依据：`opencode auth list` 的输出
/// 直接打印了这个路径。
///
/// 登录入口是 `opencode auth login`（交互式选 provider，再走 OAuth 或粘 API key）。
static AUTH: AuthScheme = AuthScheme {
    credentials: CredentialSource::HomeFile(".local/share/opencode/auth.json"),
    // token 由 opencode 自己维护，Meowo 只读不刷。
    refresh: None,
    default_base_url: "",
    login: Some(&["auth", "login"]),
};

/// 桥接插件转发的事件集，**写的已经是规范名**（负载由我们构造，无需再译）。
///
/// 这张表不驱动生成——模板里那五个 `hook_event_name` 是手写的实在代码。它的作用是让
/// `has_reporter`（只看 `SessionStart`）等通用逻辑有据可依，并由下方绊线测试保证「表里声明的」
/// 与「模板里真发的」永远是同一批。
static EVENTS: [HookEvent; 5] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    HookEvent::plain("SessionEnd"),
];

/// 落点是数据目录下的 `plugin/` 子目录——该子目录**未必存在**（用户没装过插件就没有它），
/// 故 `write_atomic` 会按需建出父目录。
///
/// `MissingConfig::CreateFrom("")`：文件不存在＝还没接线，从空文本起生成即可。这与 kimi 的
/// 「配置缺失＝没登录，拒绝创建」不同——那是**用户的**文件，而这个文件从来只有 meowo 会写。
static HOOKS: HookSpec = HookSpec {
    config_rel: "plugin/meowo-reporter.ts",
    format: ConfigFormat::OpencodeTs,
    missing: MissingConfig::CreateFrom(""),
    events: &EVENTS,
    // 这两项在 OpencodeTs 下不参与「渲染命令行」（插件里 spawn 的是 argv 数组，不拼字符串），
    // 但仍是认领语义的一部分：`--provider opencode` 与模板里的 argv 一致。
    command: CommandSpec {
        quote_exe: false,
        with_provider: true,
    },
};

/// opencode 是 **bun 编译出的原生二进制**（实测 1.17.20：npm 包的 `bin` 直指 `bin/opencode.exe`，
/// 184 MB，postinstall 拉平台包）。所以不像 gemini 那样要 node 包装，也因此有个干净的进程名可查。
static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "opencode",
    candidates: &[
        // npm 全局（实测 Windows）：%APPDATA%\npm\node_modules\opencode-ai\bin\opencode.exe
        LaunchCandidate::Exe {
            root: Root::Env("APPDATA"),
            sub: "npm/node_modules/opencode-ai/bin",
        },
        LaunchCandidate::Exe {
            root: Root::Env("USERPROFILE"),
            sub: "AppData/Roaming/npm/node_modules/opencode-ai/bin",
        },
        // 官方 `curl -fsSL https://opencode.ai/install | bash` 的落点。
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".opencode/bin",
        },
        // brew / scoop / AUR 等：交给 PATH。
        LaunchCandidate::OnPath,
    ],
};

/// 数据目录＝**配置目录**（插件往这里放），不是 `~/.local/share/opencode`（那里只有 SQLite 库）。
///
/// Windows 上同样是 `~/.config/opencode`——opencode 用 `xdg-basedir`，而那个包没有 Windows 特判，
/// 于是 `%APPDATA%` 完全不参与。这一条**已实测**（1.17.20 首次运行即建出 `C:\Users\<u>\.config\opencode`），
/// 别想当然地改成 AppData。
static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec {
        env: Some("OPENCODE_CONFIG_DIR"),
        // `~/.opencode` 也在 opencode 的配置目录搜索链上（`config/paths.ts`），作次选。
        candidates: &[".config/opencode", ".opencode"],
    },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
}];

pub struct Opencode;

impl AgentPlugin for Opencode {
    fn id(&self) -> AgentId {
        id::OPENCODE
    }
    fn display_name(&self) -> &'static str {
        "OpenCode"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    /// 原生二进制，进程名就是它自己——不必像 gemini 那样把 `node` 收进来。
    /// reporter 由插件在 opencode 进程内 `Bun.spawn` 派生，故父链上第一个 agent 进程必是它。
    fn process_names(&self) -> &'static [&'static str] {
        &["opencode", "opencode.exe"]
    }
    /// `opencode --session <id>`（`--continue` 是「续最近一个」，不接 id，表达不了「恢复指定会话」）。
    fn resume_args(&self) -> &'static [&'static str] {
        &["--session"]
    }
    fn account(&self) -> Option<&'static dyn crate::account::AccountCap> {
        Some(&account::ACCOUNT)
    }
    fn profile(&self) -> Option<&'static crate::profile::ProfileSpec> {
        Some(&PROFILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EnsureOutcome;

    /// 生成的插件里真正 `hook_event_name` 的那几个，必须与 [`EVENTS`] 声明的完全一致。
    ///
    /// 这条绊线守的是一个静默故障：模板里加了个事件却忘了写进 EVENTS，`has_reporter` 之类按表
    /// 判断的逻辑就会与实际行为脱节；反过来（表里有、模板不发）则是声明了一个永远不会到来的事件。
    #[test]
    fn template_emits_exactly_the_declared_events() {
        let plugin_src = match HOOKS.ensure_hooks("", "C:/x/meowo-reporter.exe", "opencode") {
            EnsureOutcome::Changed(s) => s,
            other => panic!("空文本应生成插件，实得 {other:?}"),
        };
        for ev in EVENTS {
            assert!(
                plugin_src.contains(&format!("hook_event_name: \"{}\"", ev.name)),
                "模板没发 {}，但 EVENTS 声明了它",
                ev.name
            );
        }
        // 反向：模板里出现的 hook_event_name 不能多于 EVENTS。
        let emitted = plugin_src.matches("hook_event_name: \"").count();
        assert_eq!(emitted, EVENTS.len(), "模板发的事件数与 EVENTS 不符");
    }

    /// 接线产物落在 `plugin/` 子目录里，且认领能把 reporter 路径读回来（换路径即重写）。
    #[test]
    fn plugin_lands_under_plugin_subdir_and_roundtrips() {
        let home = std::env::temp_dir().join(format!("meowo-opencode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(crate::join_rel(&home, ".config/opencode")).unwrap();

        let inst = VARIANTS[0]
            .probe(id::OPENCODE, &home)
            .expect("配置目录在 → 应命中");
        assert_eq!(
            inst.config_path(),
            crate::join_rel(&home, ".config/opencode")
                .join("plugin")
                .join("meowo-reporter.ts")
        );
        assert!(inst.is_configured());

        // 生成 → 已接入；SessionStart 是 has_reporter 的判据。
        let reporter = "C:/x/meowo-reporter.exe";
        let src = match HOOKS.ensure_hooks("", reporter, "opencode") {
            EnsureOutcome::Changed(s) => s,
            other => panic!("期望 Changed，实得 {other:?}"),
        };
        assert!(HOOKS.has_reporter(&src, "opencode"));
        assert_eq!(
            HOOKS.claimed_reporter(&src, "opencode").as_deref(),
            Some(reporter)
        );
        assert_eq!(
            HOOKS.ensure_hooks(&src, reporter, "opencode"),
            EnsureOutcome::Unchanged
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 配置目录优先 `~/.config/opencode`，`~/.opencode` 作次选（两者都在 opencode 的搜索链上）。
    #[test]
    fn prefers_xdg_config_dir_over_dot_opencode() {
        let home = std::env::temp_dir().join(format!("meowo-opencode-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);

        // 只有 ~/.opencode → 用它。
        std::fs::create_dir_all(home.join(".opencode")).unwrap();
        assert_eq!(
            VARIANTS[0].probe(id::OPENCODE, &home).unwrap().data_dir,
            home.join(".opencode")
        );

        // ~/.config/opencode 出现 → 抢先。
        let xdg = crate::join_rel(&home, ".config/opencode");
        std::fs::create_dir_all(&xdg).unwrap();
        assert_eq!(
            VARIANTS[0].probe(id::OPENCODE, &home).unwrap().data_dir,
            xdg
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// npm 全局装的是**原生 exe**（不是 js），故 argv 就是它本身——不该混进 `node`。
    #[test]
    fn npm_global_yields_native_exe_not_node() {
        let home = std::env::temp_dir().join(format!("meowo-opencode-npm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let bin = crate::join_rel(&home, "AppData/Roaming/npm/node_modules/opencode-ai/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(crate::exe_file_name("opencode"));
        std::fs::write(&exe, b"").unwrap();

        let _env = crate::env_guard();
        std::env::remove_var("APPDATA");
        std::env::set_var("USERPROFILE", &home);

        let argv = LAUNCH.probe(None, Some(&home)).expect("exe 在 → 应命中");
        assert_eq!(argv, vec![exe.to_string_lossy().into_owned()]);
        assert_ne!(argv[0], "node");

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 凭据**不在 data_dir 底下**：插件落配置目录（`~/.config/opencode`），凭据在数据目录
    /// （`~/.local/share/opencode/auth.json`）。若按「相对 data_dir」解析，就会去
    /// `~/.config/opencode/auth.json` 找一个永远不存在的文件——登录态于是恒为「未登录」，
    /// 登录按钮永远不消失。
    #[test]
    fn credentials_live_in_the_data_dir_not_the_config_dir() {
        let _env = crate::env_guard();
        let home = std::env::temp_dir().join(format!("meowo-oc-cred-{}", std::process::id()));
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("HOME", &home);
        // OPENCODE_CONFIG_DIR 若被真实环境设过，会改写 data_dir，干扰断言。
        std::env::remove_var("OPENCODE_CONFIG_DIR");

        let inst = VARIANTS[0].installation_at(
            id::OPENCODE,
            crate::join_rel(&home, ".config/opencode"),
            Some(&home),
        );
        assert_eq!(
            inst.credentials_path(),
            Some(crate::join_rel(&home, ".local/share/opencode/auth.json")),
            "凭据该相对 home 解析，而不是拼到配置目录下"
        );
        // 登录入口：`opencode auth login`。
        let login = inst.login_argv().expect("opencode 有登录入口");
        assert_eq!(&login[login.len() - 2..], &["auth".to_string(), "login".to_string()]);
    }

    /// resume 是 `--session <id>`——写成 `--continue` 会拉起「最近一个会话」而不是点开的那个。
    #[test]
    fn resume_targets_the_given_session() {
        let _env = crate::env_guard();
        let argv = Opencode
            .resume_argv("ses_abc")
            .expect("声明了 resume 子命令");
        let n = argv.len();
        assert_eq!(
            &argv[n - 2..],
            &["--session".to_string(), "ses_abc".to_string()]
        );
    }
}
