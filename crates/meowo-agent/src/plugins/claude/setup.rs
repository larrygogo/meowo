//! claude 的接线副作用：把 `statusLine` 包成「先写库再跑原 statusLine」的脚本，让 Context 百分比
//! 自动有准确数据。
//!
//! 它与 hooks 同住 settings.json，故走**写前改写**（`WiringCap::amend`）而非 `after_write`。
//! 脚本落在 meowo 自己的数据目录下——那是宿主的知识，经 `WiringContext::meowo_dir` 传入，
//! 插件因此不需要认识 `db_path()`。

use serde_json::{json, Value};

use crate::config::RepairReason;
use crate::variant::Installation;
use crate::wiring::{WiringCap, WiringContext};

pub struct ClaudeWiring;
pub static WIRING: ClaudeWiring = ClaudeWiring;

impl WiringCap for ClaudeWiring {
    fn amend(&self, _inst: &Installation, text: &str, ctx: &WiringContext, reporter: &str) -> Result<String, RepairReason> {
        statusline_amend(text, ctx, reporter)
    }

    fn after_write(&self, inst: &Installation, _written: &str) -> Option<RepairReason> {
        // 只对多账号（profile）做。默认账号的 `.claude.json` 是用户自己用出来的，不该由我们代笔。
        if inst.profile.is_some() {
            mark_onboarded(inst);
        }
        None
    }
}

/// 给 profile 的 `.claude.json` 补上 `hasCompletedOnboarding`。
///
/// # 为什么必须补
///
/// 新 profile 的 `.claude.json` 是 `claude auth login` 写出来的——它含 `oauthAccount`（所以 meowo
/// 能读出邮箱、判定「已登录」），却**不含 `hasCompletedOnboarding`**：那个标记只有走完 TUI 的
/// 首次引导才会写。
///
/// 而 claude 的 **TUI 启动时看不到这个标记，就会跑一遍首次引导——其中包含「登录」一步**，哪怕
/// 凭据早就躺在同一个目录里。于是用户切到新账号、新建会话，迎面就是「请登录」。
///
/// 这个症状极具迷惑性，实测确认过：**同一份凭据，`claude -p`（非交互，跳过引导）跑得好好的，
/// 一开 TUI 就要你重新登录**。查凭据、查环境变量注入、查隔离目录，全都是对的——问题压根不在
/// 「登录」上，而在「引导」上。
///
/// **只加不改**：`.claude.json` 里的其余字段（`oauthAccount`、`userID`…）一概原样保留。
fn mark_onboarded(inst: &Installation) {
    // profile 模式下 `.claude.json` 落在数据目录**里**（默认账号则在 home 根上，是它的兄弟）。
    let path = inst.data_dir.join(".claude.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        // 还没登录过（文件尚不存在）→ 什么都不做。登录成功后会再接线一次，那时补。
        return;
    };
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return; // 解析不了就别碰，绝不写坏用户文件。
    };
    let Some(obj) = v.as_object_mut() else { return };
    if obj.get("hasCompletedOnboarding").and_then(|x| x.as_bool()) == Some(true) {
        return; // 已就位，保持幂等（别无谓地重写文件）。
    }
    obj.insert("hasCompletedOnboarding".into(), serde_json::json!(true));
    let Ok(body) = serde_json::to_string_pretty(&v) else { return };
    if crate::fsutil::write_atomic(&path, &body).is_err() {
        eprintln!("Meowo profile[claude]: 补写 hasCompletedOnboarding 失败，TUI 可能会要求重新登录");
    }
}

/// Windows 路径转 bash 可用形式：`C:\a\b` -> `C:/a/b`（Git Bash 接受 `C:/...`）。
pub fn to_bash_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// 我方生成的包装脚本的识别标记。两代品牌（meowo / 前身 cc-kanban）的脚本都带这句注释，
/// 故用它——而非脚本路径——判定「这条 statusLine 命令指向的是我方产物，不是用户自己的命令」。
///
/// 靠路径判定曾酿成 fork 炸弹：改名换目录后，新版把 `~/.cc-kanban/statusline.sh` 当成用户原
/// 命令包进 `~/.meowo/statusline.sh`；用户再跑一次旧版，旧版又把 meowo 的包进 cc-kanban 的。
/// 两个脚本互相 `bash` 对方，Claude Code 每渲染一次状态栏就点燃一次无限派生。
const WRAPPER_MARK: &str = "自动生成：写入会话上下文用量";

/// 再入守卫的环境变量名。见 [`build_script`]。
const GUARD_ENV: &str = "MEOWO_STATUSLINE_ACTIVE";

/// statusLine 命令里指向的包装脚本路径（`bash "…/statusline.sh"` 形态）。
/// 引号内整体优先——Windows 家目录常含空格，按空白切会把路径切断。
fn wrapper_target(cmd: &str) -> Option<std::path::PathBuf> {
    let is_script = |t: &str| t.to_ascii_lowercase().ends_with("statusline.sh");
    let tok = cmd
        .split('"')
        .nth(1)
        .filter(|t| is_script(t))
        .or_else(|| cmd.split_whitespace().find(|t| is_script(t)))?;
    Some(std::path::PathBuf::from(tok))
}

/// 从包装脚本正文里取出它内嵌的 inner（下游 statusLine 命令）。写库那行（重定向到 /dev/null）
/// 不是 inner；自渲染版没有 inner。
fn inner_of(script: &str) -> String {
    script
        .lines()
        .filter(|l| l.contains("$input") && l.contains('|') && !l.contains("/dev/null"))
        .filter_map(|l| l.split_once('|').map(|(_, r)| r.trim()))
        .find(|r| !r.contains(" statusline"))
        .unwrap_or("")
        .to_string()
}

/// 把一条 statusLine 命令层层剥到最内的**用户真实命令**：只要它指向我方生成的包装脚本，就取出
/// 该脚本的 inner 接着剥。剥到成环（两个包装脚本互指）则整条丢弃——环里没有用户的命令，只有
/// 我们自己的历史残留，留着就是那颗 fork 炸弹。
fn unwrap_chain(cmd: &str, read: &dyn Fn(&std::path::Path) -> Option<String>) -> String {
    let mut cur = cmd.trim().to_string();
    let mut seen: Vec<String> = Vec::new();
    loop {
        let Some(path) = wrapper_target(&cur) else { return cur };
        let Some(text) = read(&path) else { return cur }; // 读不到 → 当用户自己的命令，不动
        if !text.contains(WRAPPER_MARK) {
            return cur; // 同名但非我方产物（用户自己也写了个 statusline.sh）→ 不动
        }
        let key = path.to_string_lossy().to_ascii_lowercase();
        if seen.contains(&key) {
            return String::new(); // 成环
        }
        seen.push(key);
        cur = inner_of(&text);
        if cur.trim().is_empty() {
            return String::new();
        }
    }
}

/// 删除一个 statusline 包装脚本——**仅当**它确实是我方（含前代品牌）生成的产物，即正文带
/// [`WRAPPER_MARK`]。同名的用户自有脚本一概不碰。返回是否真的删了。
///
/// 用于清除前代品牌遗留的那半个环（`~/.cc-kanban/statusline.sh`）。**调用方必须在接线之后
/// 再调**：接线要读它才能认出它是包装、并从中剥出用户真正的 statusLine 命令——先删就等于
/// 把用户的原命令一起丢了，还会让 [`unwrap_chain`] 把一个已不存在的脚本当成「用户命令」
/// 重新包进新脚本里。
pub fn remove_generated_wrapper(path: &std::path::Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(t) if t.contains(WRAPPER_MARK) => std::fs::remove_file(path).is_ok(),
        _ => false,
    }
}

/// 该命令是否指向我方生成的包装脚本。
fn is_wrapper_cmd(cmd: &str, read: &dyn Fn(&std::path::Path) -> Option<String>) -> bool {
    wrapper_target(cmd)
        .and_then(|p| read(&p))
        .is_some_and(|t| t.contains(WRAPPER_MARK))
}

/// 探测 statusLine 接线状态（只读不改）：
///   - Some(inner)：需要生成脚本并改写 settings；inner 是要内嵌的下游 statusLine 命令（无则空串）；
///   - None：已是我们的包装且脚本内部干净，幂等跳过。
///
/// 只探测不改写——settings 的实际改写由调用方在**脚本落盘成功之后**执行：先改 settings 再写脚本、
/// 写失败再回滚的顺序会在回滚代码里反向编码本函数的副作用，脆弱且曾造成「settings 指向不存在的
/// 脚本、原 statusLine 命令永久丢失」。
///
/// `read` 注入脚本读取（纯函数便于单测）：判定「这是包装脚本还是用户命令」必须看**内容**里的
/// [`WRAPPER_MARK`]，不能看路径——见该常量的说明。
///
/// 幂等分支同样要验脚本内部：settings 已指向我们、而我们的脚本内部却回指另一个包装脚本，正是
/// 中毒机器的现状。此时必须重建（把环剥掉），否则幂等判定会让那颗炸弹永远不被拆除。
pub fn probe_statusline(
    settings: &Value,
    script_marker: &str,
    read: &dyn Fn(&std::path::Path) -> Option<String>,
) -> Option<String> {
    let cur = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(|x| x.as_str())
        .unwrap_or("");

    if cur.contains(script_marker) {
        let ours_inner = read(std::path::Path::new(script_marker))
            .map(|t| inner_of(&t))
            .unwrap_or_default();
        if !is_wrapper_cmd(&ours_inner, read) {
            return None; // 内部干净 → 真幂等
        }
        return Some(unwrap_chain(&ours_inner, read)); // 内部有环 → 重建自愈
    }
    Some(unwrap_chain(cur, read))
}

/// 生成包装脚本内容：读 stdin → 喂 meowo-reporter 写库（丢弃其输出）→ 跑下游 statusLine（如有）渲染状态栏。
/// `reporter_bash` 为 bash 形式的 meowo-reporter 路径；`inner` 为下游 statusLine 命令（空则不渲染）。
///
/// 开头的再入守卫是**硬止血**：环境变量经 `export` 传给所有子孙进程，任何再入（无论经由哪个
/// 包装脚本、哪一代品牌、还是用户手改出的环）立刻退出。[`unwrap_chain`] 从配置上杜绝成环，
/// 这里保证即使配置又被搞坏，最坏结果也只是状态栏空白——而不是拖垮整台机器。
pub fn build_script(reporter_bash: &str, inner: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str("# 本文件由 Meowo 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n");
    s.push_str(&format!(
        "if [ -n \"${{{GUARD_ENV}:-}}\" ]; then exit 0; fi\nexport {GUARD_ENV}=1\n"
    ));
    s.push_str("input=$(cat)\n");
    if inner.trim().is_empty() {
        // 无下游 statusLine：meowo-reporter 写库并自渲染极简状态栏（输出即状态栏）。
        s.push_str(&format!(
            "printf '%s' \"$input\" | \"{reporter_bash}\" statusline\n"
        ));
    } else {
        // 有下游（如 claude-hud）：meowo-reporter 只写库（丢弃输出），再跑下游渲染真正的状态栏。
        s.push_str(&format!(
            "printf '%s' \"$input\" | \"{reporter_bash}\" statusline >/dev/null 2>&1\n"
        ));
        s.push_str(&format!("printf '%s' \"$input\" | {inner}\n"));
    }
    s
}

/// 写出 statusline 包装脚本（先建目录）。返回错误供调用方决定是否改写 settings。
fn write_statusline_script(path: &std::path::Path, script: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, script)
}

/// 包装脚本路径：`<meowo 数据目录>/statusline.sh`（与 board.db 同目录）。
fn script_path(ctx: &WiringContext) -> std::path::PathBuf {
    ctx.meowo_dir.join("statusline.sh")
}

/// `amend`：在 hooks 合并后、落盘前把 statusLine 指向包装脚本。
///
/// **顺序纪律（勿改）**：脚本先落盘，成功后 settings 才指向它。写失败（目录不可写/杀软拦截/磁盘满）
/// 时 settings 原样不动——否则 Claude Code 状态栏会指向不存在的脚本，用户原 statusLine 命令
/// （inner）只存在于没写出去的脚本里而永久丢失，且后续启动因幂等判定命中 marker 而跳过重建、
/// 永不自愈。settings 未动则下次启动整段重试。
///
/// 无改动时**原样返回入参文本**（不重新序列化），否则 `wire_hooks` 的幂等判定会误判为有改动。
fn statusline_amend(text: &str, ctx: &WiringContext, reporter: &str) -> Result<String, RepairReason> {
    let Some(mut settings) = crate::config::parse_json_config(text) else {
        return Err(RepairReason::ConfigUnreadable);
    };
    let script_path = script_path(ctx);
    let script_bash = to_bash_path(&script_path.to_string_lossy());
    let read = |p: &std::path::Path| std::fs::read_to_string(p).ok();

    match probe_statusline(&settings, &script_bash, &read) {
        Some(inner) => {
            let script = build_script(&to_bash_path(reporter), &inner);
            if write_statusline_script(&script_path, &script).is_err() {
                eprintln!("Meowo repair[claude]: statusline 脚本写入失败，settings 原样不动（下次启动重试）");
                return Ok(text.to_string());
            }
            settings["statusLine"] =
                json!({ "type": "command", "command": format!("bash \"{script_bash}\"") });
            let pretty =
                serde_json::to_string_pretty(&settings).map_err(|_| RepairReason::WriteFailed)?;
            Ok(format!("{pretty}\n"))
        }
        None => {
            // 幂等命中（settings 已指向我们的脚本）但脚本文件缺失：用户删 ~/.meowo 重置数据时
            // board.db 会被 Store::open 自动重建，本脚本却不会——不补建则状态栏每次渲染报
            // No such file、Context% 永久断供。原 inner 已无从恢复，退化为自渲染版兜底。
            if !script_path.exists() {
                let script = build_script(&to_bash_path(reporter), "");
                let _ = write_statusline_script(&script_path, &script);
            }
            Ok(text.to_string())
        }
    }
}

#[cfg(test)]
mod onboarding_tests {
    use super::*;

    /// 造一份 profile 实况（data_dir = profile 根）。
    fn profile_inst(root: &std::path::Path) -> Installation {
        crate::by_id("claude")
            .unwrap()
            .installation_for_profile(root)
            .expect("claude 支持多账号")
    }

    /// **补 `hasCompletedOnboarding`，否则新账号一开 TUI 就要你重新登录。**
    ///
    /// `claude auth login` 写出的 `.claude.json` 有 `oauthAccount`（于是 meowo 判定「已登录」），
    /// 却没有 `hasCompletedOnboarding`——那个标记只有走完 TUI 首次引导才会写。而 claude 的 TUI
    /// 看不到它就会跑一遍引导，其中包含「登录」一步，哪怕凭据就在同一个目录里。
    ///
    /// 症状极具迷惑性（实测踩过）：同一份凭据，`claude -p` 跑得好好的，一开 TUI 就要重新登录。
    #[test]
    fn marks_profile_as_onboarded_without_touching_other_fields() {
        let root = std::env::temp_dir().join(format!("meowo-onboard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(".claude.json");

        // `claude auth login` 写出来的形状：有账号，无引导标记。
        std::fs::write(
            &path,
            r#"{"oauthAccount":{"emailAddress":"a@b.c"},"userID":"u1"}"#,
        )
        .unwrap();

        mark_onboarded(&profile_inst(&root));

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["hasCompletedOnboarding"], serde_json::json!(true));
        // 只加不改：原有字段一个都不能动。
        assert_eq!(v["oauthAccount"]["emailAddress"], "a@b.c");
        assert_eq!(v["userID"], "u1");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 文件不存在（还没登录过）→ 什么都不做，绝不凭空造一个 `.claude.json`。
    /// 坏 JSON → 也不碰，绝不写坏用户文件。
    #[test]
    fn never_creates_or_corrupts_the_file() {
        let root = std::env::temp_dir().join(format!("meowo-onboard-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(".claude.json");

        mark_onboarded(&profile_inst(&root));
        assert!(!path.exists(), "还没登录就不该造出 .claude.json");

        std::fs::write(&path, "{not json").unwrap();
        mark_onboarded(&profile_inst(&root));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json", "坏文件不该被改写");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 默认账号的 `.claude.json` 是用户自己用出来的——**绝不代笔**。
    /// `after_write` 只在 profile 实况下补标记。
    #[test]
    fn default_account_is_never_touched() {
        let root = std::env::temp_dir().join(format!("meowo-onboard-def-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // 默认账号实况：profile 为 None。
        let inst = crate::by_id("claude").unwrap().resolve().unwrap();
        assert!(inst.profile.is_none());
        // after_write 对它不做任何事（没有 profile → 不补标记）。
        assert_eq!(WIRING.after_write(&inst, ""), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 没有任何脚本落盘的世界（大多数用例不关心磁盘）。
    fn no_scripts(_: &std::path::Path) -> Option<String> {
        None
    }

    /// 用一张「路径 → 脚本正文」表冒充磁盘。
    fn disk(files: &[(&str, String)]) -> impl Fn(&std::path::Path) -> Option<String> {
        let owned: Vec<(String, String)> =
            files.iter().map(|(p, t)| (p.to_ascii_lowercase(), t.clone())).collect();
        move |p: &std::path::Path| {
            let key = p.to_string_lossy().to_ascii_lowercase();
            owned.iter().find(|(k, _)| *k == key).map(|(_, t)| t.clone())
        }
    }

    #[test]
    fn probe_statusline_wraps_existing_and_is_idempotent() {
        let mut v =
            json!({ "statusLine": { "type": "command", "command": "bash -c 'claude-hud'" } });
        let marker = "C:/Users/me/.meowo/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        let inner = probe_statusline(&v, marker, &no_scripts).expect("应需要生成脚本");
        assert_eq!(inner, "bash -c 'claude-hud'"); // 捕获到原命令
        assert_eq!(v["statusLine"]["command"], "bash -c 'claude-hud'"); // 探测不改写

        // 模拟 amend：脚本落盘成功后才改写 settings。
        v["statusLine"] = json!({ "type": "command", "command": inv });
        let fs = disk(&[(marker, build_script("C:/x/meowo-reporter.exe", "bash -c 'claude-hud'"))]);
        // 再探测：已引用我们的脚本、内部干净 → None（幂等，不再重复捕获/递归）
        assert!(probe_statusline(&v, marker, &fs).is_none());
    }

    #[test]
    fn probe_statusline_handles_absent() {
        let v = json!({});
        let marker = "/home/me/.meowo/statusline.sh";
        let inner = probe_statusline(&v, marker, &no_scripts).expect("无 statusLine 也应接线");
        assert_eq!(inner, ""); // 无原命令
    }

    /// 回归（fork 炸弹）：settings 指向**前代品牌**的包装脚本时，绝不能把它当用户命令包进来——
    /// 那会让两个脚本互相 `bash` 对方，Claude Code 每渲染一次状态栏就点燃一次无限派生
    /// （真机上滚出 13,920 个 bash 进程、98.9% 提交内存）。前代脚本必须被**剥开**，
    /// 取出它内嵌的用户真实命令。
    #[test]
    fn probe_unwraps_legacy_brand_wrapper_instead_of_nesting_it() {
        let legacy = "C:/Users/me/.cc-kanban/statusline.sh";
        let ours = "C:/Users/me/.meowo/statusline.sh";
        // 前代脚本（同样的生成标记），内嵌用户真实的 statusLine。
        let legacy_text = "#!/usr/bin/env bash\n\
            # 本文件由 cc-kanban 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n\
            input=$(cat)\n\
            printf '%s' \"$input\" | \"C:/old/cc-reporter.exe\" statusline >/dev/null 2>&1\n\
            printf '%s' \"$input\" | bash -c 'claude-hud'\n";
        let v = json!({ "statusLine": { "type": "command", "command": format!("bash \"{legacy}\"") } });
        let fs = disk(&[(legacy, legacy_text.to_string())]);

        let inner = probe_statusline(&v, ours, &fs).expect("应接线");
        assert_eq!(inner, "bash -c 'claude-hud'", "必须剥到用户真实命令，而不是套娃");
        assert!(!inner.contains("statusline.sh"), "inner 绝不能再指向任何包装脚本");
    }

    /// 回归（自愈）：机器已经中毒——settings 指向我们的脚本，而我们的脚本回指前代脚本，
    /// 前代脚本又回指我们的，环已闭合。幂等判定必须**看穿脚本内部**，否则这颗炸弹永远拆不掉。
    #[test]
    fn probe_rebuilds_when_our_script_points_back_into_a_cycle() {
        let legacy = "C:/Users/me/.cc-kanban/statusline.sh";
        let ours = "C:/Users/me/.meowo/statusline.sh";
        // 真机现场：两个脚本互指。
        let ours_text = build_script("C:/new/meowo-reporter.exe", &format!("bash \"{legacy}\""));
        let legacy_text = format!(
            "#!/usr/bin/env bash\n\
             # 本文件由 cc-kanban 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n\
             input=$(cat)\n\
             printf '%s' \"$input\" | \"C:/old/cc-reporter.exe\" statusline >/dev/null 2>&1\n\
             printf '%s' \"$input\" | bash \"{ours}\"\n"
        );
        let v = json!({ "statusLine": { "type": "command", "command": format!("bash \"{ours}\"") } });
        let fs = disk(&[(ours, ours_text), (legacy, legacy_text)]);

        // marker 命中，但内部成环 → 必须返回 Some（重建），而不是 None（幂等放过）。
        let inner = probe_statusline(&v, ours, &fs).expect("成环时必须重建脚本，不能判定为幂等");
        assert_eq!(inner, "", "环里没有用户的命令，只有我们的历史残留 → 整条丢弃");
    }

    /// 用户自己写的 statusline.sh（同名但没有我方生成标记）是真·用户命令，必须原样内嵌，不能剥。
    #[test]
    fn probe_keeps_user_authored_script_of_same_name() {
        let user = "C:/Users/me/scripts/statusline.sh";
        let ours = "C:/Users/me/.meowo/statusline.sh";
        let fs = disk(&[(user, "#!/usr/bin/env bash\necho hi\n".to_string())]);
        let v = json!({ "statusLine": { "type": "command", "command": format!("bash \"{user}\"") } });
        assert_eq!(probe_statusline(&v, ours, &fs).unwrap(), format!("bash \"{user}\""));
    }

    #[test]
    fn build_script_with_and_without_inner() {
        let with = build_script("C:/x/meowo-reporter.exe", "bash -c 'hud'");
        assert!(with.contains("\"C:/x/meowo-reporter.exe\" statusline >/dev/null"));
        assert!(with.contains("| bash -c 'hud'\n"));
        let without = build_script("C:/x/meowo-reporter.exe", "  ");
        // 无 inner：让 meowo-reporter 自渲染（不丢弃输出）
        assert!(without.contains("| \"C:/x/meowo-reporter.exe\" statusline\n"));
        assert!(!without.contains(">/dev/null"));
    }

    /// 硬止血：脚本自带再入守卫。配置层面的解环（`unwrap_chain`）是第一道防线，这是第二道——
    /// 即便日后又有谁把配置搞成环，最坏也只是状态栏空白，而不是拖垮整台机器。
    #[test]
    fn build_script_carries_reentry_guard() {
        let s = build_script("C:/x/meowo-reporter.exe", "bash -c 'hud'");
        assert!(s.contains(&format!("if [ -n \"${{{GUARD_ENV}:-}}\" ]; then exit 0; fi")));
        assert!(s.contains(&format!("export {GUARD_ENV}=1")));
        // 守卫必须在读 stdin 之前——否则再入的那层会先阻塞在 `cat` 上。
        let guard = s.find("exit 0").unwrap();
        let read = s.find("input=$(cat)").unwrap();
        assert!(guard < read, "守卫必须早于 stdin 读取");
    }

    /// `inner_of` 要能从两种形态的包装脚本里取出下游命令：写库那行（重定向到 /dev/null）不是 inner。
    #[test]
    fn inner_of_extracts_downstream_only() {
        assert_eq!(inner_of(&build_script("C:/x/meowo-reporter.exe", "bash -c 'hud'")), "bash -c 'hud'");
        assert_eq!(inner_of(&build_script("C:/x/meowo-reporter.exe", "")), "", "自渲染版没有 inner");
    }

    /// 清除前代残留：只删我方（含前代品牌）生成的包装脚本，同名的用户自有脚本一概不碰。
    #[test]
    fn remove_generated_wrapper_only_deletes_our_own() {
        let dir = std::env::temp_dir().join(format!("meowo-sweep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 前代品牌的产物（带生成标记）→ 删。
        let legacy = dir.join("legacy-statusline.sh");
        std::fs::write(
            &legacy,
            "#!/usr/bin/env bash\n# 本文件由 cc-kanban 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n",
        )
        .unwrap();
        assert!(remove_generated_wrapper(&legacy));
        assert!(!legacy.exists());

        // 用户自己写的同名脚本（无生成标记）→ 不碰。
        let user = dir.join("user-statusline.sh");
        std::fs::write(&user, "#!/usr/bin/env bash\necho hi\n").unwrap();
        assert!(!remove_generated_wrapper(&user));
        assert!(user.exists(), "用户自有脚本绝不能删");

        // 不存在的文件 → 安静地什么都不做。
        assert!(!remove_generated_wrapper(&dir.join("nope.sh")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 路径含空格（Windows 家目录常见）时仍要认得出包装脚本——按空白切会把路径切断，
    /// 认不出就会又包一层。
    #[test]
    fn wrapper_target_survives_spaces_in_path() {
        let p = wrapper_target("bash \"C:/Users/John Doe/.meowo/statusline.sh\"").unwrap();
        assert_eq!(to_bash_path(&p.to_string_lossy()), "C:/Users/John Doe/.meowo/statusline.sh");
        assert!(wrapper_target("bash -c 'claude-hud'").is_none());
    }

    #[test]
    fn to_bash_path_normalizes_windows_separators() {
        assert_eq!(
            to_bash_path(r"C:\Users\me\.meowo\statusline.sh"),
            "C:/Users/me/.meowo/statusline.sh"
        );
        assert_eq!(to_bash_path("/home/me/x"), "/home/me/x");
    }

    /// 从 install-hooks.mjs 里抠出 `const SPECS = [...]` 的 `["事件", "matcher"],` 各行。
    fn parse_mjs_specs(src: &str) -> Vec<(String, String)> {
        let start = src
            .find("const SPECS = [")
            .expect("install-hooks.mjs 里找不到 const SPECS");
        let block = &src[start..];
        let end = block.find("];").expect("SPECS 数组未闭合");
        block[..end]
            .lines()
            .filter_map(|l| {
                let inner = l.trim().strip_prefix('[')?.split(']').next()?;
                let mut it = inner.split(',').map(|s| s.trim().trim_matches('"'));
                Some((it.next()?.to_string(), it.next()?.to_string()))
            })
            .collect()
    }

    /// 绊线：`plugins/claude.rs` 的 EVENTS 与 `scripts/install-hooks.mjs` 的 SPECS 必须逐条一致。
    /// 两处各维护一份（一个给 app 无感接线，一个给手动脚本），此前只有注释在提醒，靠不住——
    /// 漏同步会让脚本装出的 hooks 与 app 认领的规格对不上，出现「装了却显示未接入」。
    #[test]
    fn hook_specs_match_install_hooks_mjs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/install-hooks.mjs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
        let from_js = parse_mjs_specs(&src);
        assert_eq!(
            from_js.len(),
            8,
            "解析出的 SPECS 条数不对，脚本格式可能变了"
        );

        let events = crate::by_id("claude")
            .expect("claude 应已注册")
            .variants()[0]
            .hooks
            .events;
        let from_rs: Vec<(String, String)> = events
            .iter()
            .map(|e| {
                (
                    e.name.to_string(),
                    e.matcher.unwrap_or_default().to_string(),
                )
            })
            .collect();

        assert_eq!(
            from_rs, from_js,
            "plugins/claude.rs 的 EVENTS 与 install-hooks.mjs 的 SPECS 不一致"
        );
    }

    /// dry-run：对 CLAUDE_CONFIG_DIR/settings.json（真实文件的副本）跑一次接线，核对产物。
    /// 用法：复制 ~/.claude 到临时目录，
    ///       CLAUDE_CONFIG_DIR=<副本> MEOWO_DIR=<副本> \
    ///       cargo test -p meowo-agent dryrun_claude -- --ignored --nocapture
    ///
    /// 只打印结构性摘要，**绝不 dump 配置原文**——真实 settings.json 可能含 env 里的密钥。
    #[test]
    #[ignore]
    fn dryrun_claude() {
        use crate::registry::AgentPlugin;
        let meowo_dir = std::path::PathBuf::from(
            std::env::var("MEOWO_DIR").expect("请设置 MEOWO_DIR 指向临时目录"),
        );
        let ctx = WiringContext { fallback_reporter: None, meowo_dir: &meowo_dir };
        let reason = super::super::Claude.wire(&ctx);
        let inst = crate::registry::installation(crate::id::CLAUDE).expect("应解析出实况");
        let text = std::fs::read_to_string(inst.config_path()).expect("读不回 settings.json");
        let v: Value = serde_json::from_str(&text).expect("产物应为合法 JSON");

        eprintln!(
            "变体={} 配置={}",
            inst.variant_tag,
            inst.config_path().display()
        );
        eprintln!("wire reason={reason:?}");
        eprintln!(
            "顶层键（应全部保留）={:?}",
            v.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        eprintln!(
            "hooks 事件={:?}",
            v["hooks"].as_object().unwrap().keys().collect::<Vec<_>>()
        );
        eprintln!(
            "SessionStart 已接线={}",
            inst.hooks.has_reporter(&text, "claude")
        );

        // 8 条 (event, matcher) 齐。
        let want: [(&str, &str); 8] = [
            ("SessionStart", "*"),
            ("UserPromptSubmit", "*"),
            ("PostToolUse", "*"),
            ("Stop", "*"),
            ("SessionEnd", "*"),
            ("PermissionRequest", "*"),
            ("PreToolUse", "AskUserQuestion"),
            ("PreToolUse", "ExitPlanMode"),
        ];
        for (ev, m) in want {
            let arr = v["hooks"][ev]
                .as_array()
                .unwrap_or_else(|| panic!("缺事件 {ev}"));
            assert!(arr.iter().any(|e| e["matcher"] == m), "缺 ({ev}, {m})");
        }
        // statusLine 指向脚本且脚本存在。
        let sl = v["statusLine"]["command"].as_str().unwrap();
        assert!(sl.contains("statusline.sh"), "statusLine 未指向脚本：{sl}");
        assert!(super::script_path(&ctx).exists(), "statusLine 脚本未落盘");
        eprintln!("statusLine 指向脚本且脚本存在 ✓");
    }
}
