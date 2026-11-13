//! Lead 安装只负责托管资产及原生 hook 配置；写入成功不等于自动唤醒/离线恢复已经通过。
//! 各家的目录与角色语法由插件声明，共享编排不按 provider 身份分支。

use crate::variant::Installation;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const MANAGED_MARK: &str = "由 Meowo 安装和管理";
pub const SKILL: &str = include_str!("lead_assets/SKILL.md");
pub const REVIEW: &str = include_str!("lead_assets/review.md");

#[derive(Clone, Copy)]
pub enum LeadHookFormat {
    Json,
    Toml,
    Plugin,
}

#[derive(Clone, Copy)]
pub struct LeadHook {
    pub event: &'static str,
    pub mode: &'static str,
    pub timeout: u64,
}

pub struct LeadInstallSpec {
    pub provider: &'static str,
    /// 已实测的最低版本（major, minor）：同主版本、不低于它即放行，见 [`version_matches`]。
    pub version_family: (u32, u32),
    /// 实际跑过安装/hook 回归的版本；高于它照常可用，状态里标 `lead/version_untested`。
    pub verified: (u32, u32, u32),
    pub skill_rel: &'static str,
    pub reviewer_rel: &'static str,
    pub hook_rel: &'static str,
    pub format: LeadHookFormat,
    pub hooks: &'static [LeadHook],
    pub limitations: &'static [&'static str],
    pub powershell: bool,
    pub windows_override: bool,
}

pub struct LeadConfigChange {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
}

/// 原生 TUI 会话的自动继续机制（见 [`LeadCap::native_continue`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeContinue {
    /// TUI 里有一个后台等待进程（`meowo-reporter lead wait`），答复落盘它就退出，CLI 在空闲时
    /// 收到「后台任务完成」自己开新回合（kimi 后台 Bash + notify；gemini 后台 shell + inject +
    /// modelSteering）。只在 TUI 开着时生效：TUI 不在时宿主只保存答复，不起任何后台进程
    /// （leader 2026-11-13「不需要离线」），用户打开会话后说一句话由 hook 把答复交给模型。
    InSessionWaiter {
        /// CLI 执行后台命令用的 shell：决定等待器命令怎么引用 reporter 路径（[`waiter_command_prefix`]）。
        shell: WaiterShell,
    },
    /// TUI 以客户端模式连在宿主管理的服务上，宿主经该服务把答复排进同一个会话。
    ServerQueue(ServerProtocol),
}

/// CLI 跑后台 shell 命令时用的 shell 方言。由插件声明，共享层不按 provider 身份猜。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiterShell {
    /// 处处都是 POSIX `sh -c`（Windows 上是 Git Bash）：路径用正斜杠、双引号包住。正斜杠也让
    /// kimi permission 规则（picomatch）里不出现反斜杠（它在 picomatch 里是转义符）。
    PosixDoubleQuoted,
    /// Windows 上用 PowerShell（gemini 0.68 getShellConfiguration）：`& '路径'`；其余平台 POSIX 单引号。
    PowerShellOnWindows,
}

/// 宿主管理的服务所用的协议（按协议区分，不按 provider 身份）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerProtocol {
    /// Codex app-server（WebSocket JSON-RPC + capability token）；TUI 以 `--remote` 连上来，
    /// 投递用 `thread/queue/add`。宿主须等 TUI 先 initialize，originator 才是 codex-tui。
    CodexAppServer,
    /// OpenCode 内置 HTTP 服务（Basic Auth，OPENCODE_SERVER_PASSWORD）；TUI 以 `--port` 开放，
    /// 投递用 `POST /session/:id/prompt_async`。
    OpencodeHttp,
}

/// 宿主管理服务的本机鉴权（Basic Auth）。由插件声明，宿主共享代码里不出现 provider 的身份串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerAuth {
    /// Basic Auth 用户名。
    pub username: &'static str,
    /// 服务端读取密码的环境变量名（服务进程与 TUI 内置服务都只认环境变量）。
    pub password_env: &'static str,
}

/// 只描述原生会话身份的可信来源，安装了资产不能自动获得绑定资格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadBindingPolicy {
    NativeThreadMeta,
    ExclusiveProcess,
    Unavailable,
}

pub trait LeadCap: Sync {
    fn binding_policy(&self) -> LeadBindingPolicy {
        LeadBindingPolicy::Unavailable
    }
    /// 原生终端会话里，关卡答复能否不经用户输入就让模型开新回合。codex/kimi/gemini/opencode
    /// 的原生 hook 都不能自行起新回合（2026-11-12/13 实测），默认 false；能做到的插件覆盖它。
    /// 宿主据此决定是否提示「在会话里发一句话即可继续」，不按 provider 身份分支。
    fn native_auto_continue(&self) -> bool {
        self.native_continue().is_some()
    }
    /// 原生 TUI 会话里，关卡答复靠什么机制让同一个会话自动开新回合（任务 20261113-lead-tui）。
    /// 只描述机制，不描述 provider 身份；宿主按机制分发（lead_native）。None = 做不到，面板提示手动继续。
    /// 一律不走 ACP（leader 2026-11-13：Provider 会把 ACP 流量另算），也不往 PTY 里打字。
    fn native_continue(&self) -> Option<NativeContinue> {
        None
    }
    /// `ServerQueue` 机制下宿主连本机服务用的鉴权；不走服务的插件保持 None。
    fn server_auth(&self) -> Option<ServerAuth> {
        None
    }
    /// [`Self::native_continue`] 查证过的最低 CLI 版本（含补丁号）。低于它（或版本读不出）时宿主
    /// 当作做不到、走手动提示（[`native_continue_for`]）：Lead 安装的版本门比续跑机制宽，
    /// 不能拿安装放行当续跑也成立。None = 不另设门槛。
    fn native_continue_floor(&self) -> Option<(u32, u32, u32)> {
        None
    }
    fn spec(&self, installation: &Installation) -> Option<&'static LeadInstallSpec>;
    fn reviewer(&self) -> String;
    fn activation_issue(&self, _installation: &Installation) -> Option<String> {
        None
    }
    fn plugin(&self, _reporter: &str) -> Option<String> {
        None
    }
    fn extra_config(
        &self,
        _installation: &Installation,
        _before: &str,
        _after: &str,
        _enabled: bool,
    ) -> Result<Vec<LeadConfigChange>, String> {
        Ok(Vec::new())
    }
    /// hook 编辑之后对同一个配置文件（`spec.hook_rel`）的附加编辑：打开自动续跑依赖的 CLI 设置
    /// （kimi 的 permission 放行规则、gemini 的后台完成注入与 modelSteering）。只动 Meowo 自己的项，
    /// 卸载时还原。`saved` 是首次安装时记下的原值（[`Self::saved_settings_rel`] 文件内容）；返回
    /// （新文本，首次安装要记下的原值）。必须幂等：inspect 靠「再编辑一次不变」判断已安装。
    fn edit_hook_file(
        &self,
        _installation: &Installation,
        text: &str,
        _reporter: &str,
        _enabled: bool,
        _saved: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        Ok((text.to_owned(), None))
    }
    /// 记录被 [`Self::edit_hook_file`] 改掉的用户原值的托管文件（相对 data_dir）。只在首次安装时写，
    /// 卸载还原后删除；不参与「是否已安装」的比对（安装后已看不到原值，无法重算）。
    fn saved_settings_rel(&self) -> Option<&'static str> {
        None
    }
    /// 其余整文件托管的附加文件（相对 data_dir，如 gemini 的放行策略）。正文必须含 [`MANAGED_MARK`]。
    fn managed_file_rels(&self) -> &'static [&'static str] {
        &[]
    }
    fn managed_file_body(
        &self,
        _installation: &Installation,
        _relative: &str,
        _reporter: &str,
    ) -> Result<String, String> {
        Err("lead/asset_path_invalid".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LeadInstallState {
    pub supported: bool,
    pub installed: bool,
    pub assets_present: bool,
    pub hooks_present: bool,
    pub limitations: Vec<String>,
    pub reason: Option<String>,
}

/// 从 `--version` 输出里取第一个 `x.y.z`（容忍 `codex-cli `、`v` 前缀和 `-beta`/`+build` 后缀）。
pub fn parse_version(line: Option<&str>) -> Option<(u32, u32, u32)> {
    line.into_iter()
        .flat_map(str::split_whitespace)
        .find_map(|word| {
            let mut parts = word.trim_start_matches('v').split('.');
            Some((
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
                parts
                    .next()?
                    .split(['-', '+'])
                    .next()?
                    .parse::<u32>()
                    .ok()?,
            ))
        })
}

/// 版本门：同主版本、且不低于已实测的最低版本即放行。
///
/// 为什么不卡精确版本：2026-11-13 一天内 gemini 0.65→0.68、kimi 2.5.3→2.6.0、
/// opencode 1.19.3→1.19.5，精确匹配让用户一升级就装不上 Lead、托管会话起不来。
/// 新版本破坏协议/布局靠 CLI 漂移扫描与回归测试提前发现，确认破坏时在插件里单独拒绝该版本。
/// 低于下限的旧版本没有实测过协议形状，照旧拒绝。
pub fn version_matches(line: Option<&str>, floor: (u32, u32)) -> bool {
    parse_version(line).is_some_and(|(major, minor, _)| major == floor.0 && minor >= floor.1)
}

/// 精确到补丁号的下限版本门（托管协议只在某个补丁号实测过时用）。
pub fn version_at_least(line: Option<&str>, floor: (u32, u32, u32)) -> bool {
    parse_version(line).is_some_and(|v| v.0 == floor.0 && v >= floor)
}

/// 不是实测过的那个版本：照常可用，但状态里如实标「未实测」，不冒充已验证。
/// 低于已实测版本同样算（第一轮做空 M6：kimi 门放行 2.5.x 而 verified=2.6.0，原先只标高于的）。
pub fn version_untested(line: Option<&str>, verified: (u32, u32, u32)) -> bool {
    parse_version(line).is_some_and(|v| v != verified)
}

/// 按版本过滤后的续跑机制：插件声明了 [`LeadCap::native_continue_floor`] 时，版本不到（或读不出）
/// 就返回 None，宿主走手动提示，而不是在没查证过的版本上起等待器、后台续写。
pub fn native_continue_for(cap: &dyn LeadCap, version: Option<&str>) -> Option<NativeContinue> {
    match cap.native_continue_floor() {
        Some(floor) if !version_at_least(version, floor) => None,
        _ => cap.native_continue(),
    }
}

fn managed_file(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.contains(MANAGED_MARK))
}
fn asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("lead/asset_path_invalid".into());
    }
    Ok(root.join(path))
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("lead/read_failed".into()),
    }
}

fn valid_reporter(path: &str) -> bool {
    !path.is_empty()
        && !path
            .chars()
            .any(|c| matches!(c, '"' | '`' | '$' | '%' | '\r' | '\n'))
}
pub fn hook_command(spec: &LeadInstallSpec, reporter: &str, mode: &str) -> Result<String, String> {
    if !valid_reporter(reporter) {
        return Err("lead/reporter_path_invalid".into());
    }
    let prefix = if cfg!(windows) && spec.powershell {
        "& "
    } else {
        ""
    };
    Ok(format!(
        "{prefix}\"{reporter}\" lead {mode} --provider {}",
        spec.provider
    ))
}
pub fn windows_hook_command(
    spec: &LeadInstallSpec,
    reporter: &str,
    mode: &str,
) -> Result<String, String> {
    if !valid_reporter(reporter) {
        return Err("lead/reporter_path_invalid".into());
    }
    Ok(format!("powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command \"& '{}' lead {mode} --provider {}\"", reporter.replace('\'',"''"),spec.provider))
}
/// 原生 TUI 关卡等待器命令的固定前缀（不含 `--session <id>`）。reporter 生成命令、插件生成放行规则
/// 共用这一处，两边一字不差才能让规则命中。
///
/// 引用方式由插件声明的 [`WaiterShell`] 决定，不按 provider 身份分支（第一轮做空 M7）。
pub fn waiter_command_prefix(shell: WaiterShell, provider: &str, reporter: &str) -> String {
    let tail = format!("lead wait --provider {provider}");
    match shell {
        WaiterShell::PowerShellOnWindows if cfg!(windows) => {
            format!("& '{}' {tail}", reporter.replace('\'', "''"))
        }
        WaiterShell::PowerShellOnWindows => {
            format!("'{}' {tail}", reporter.replace('\'', r"'\''"))
        }
        WaiterShell::PosixDoubleQuoted => format!("\"{}\" {tail}", reporter.replace('\\', "/")),
    }
}

/// 该 provider 声明的等待器 shell（不看版本）；没有会话内等待器机制的返回 None。
/// 只给「等待器已经起来了」的一侧用（`lead wait` 校验参数）：要不要**要求**模型启动等待器，
/// 必须走带版本门的 [`waiter_shell_for`]。
pub fn waiter_shell(provider: &str) -> Option<WaiterShell> {
    shell_of(crate::by_id(provider)?.lead()?.native_continue()?)
}

/// 按本机 CLI 版本过滤后的等待器 shell。宿主（lead_native::mechanism → [`native_continue_for`]）与
/// reporter（hook 里要求启动等待器）用同一个判定：做空 N5——原先 reporter 不看版本，kimi 2.5 /
/// gemini 0.67 仍被 hook 要求起等待器，面板却按版本门说「手动继续」，两边对不上。
pub fn waiter_shell_for(provider: &str, version: Option<&str>) -> Option<WaiterShell> {
    shell_of(native_continue_for(crate::by_id(provider)?.lead()?, version)?)
}

/// 该 provider 的续跑机制设了版本门吗：没设就不必为它探测 CLI 版本（做空 N2：`--version` 曾进了
/// 每次开终端的热路径，而 codex/opencode 根本没有版本门）。
pub fn continue_needs_version(provider: &str) -> bool {
    crate::by_id(provider)
        .and_then(|plugin| plugin.lead())
        .is_some_and(|cap| cap.native_continue_floor().is_some())
}

fn shell_of(mechanism: NativeContinue) -> Option<WaiterShell> {
    match mechanism {
        NativeContinue::InSessionWaiter { shell } => Some(shell),
        NativeContinue::ServerQueue(_) => None,
    }
}

pub fn managed_command(command: &str, provider: &str) -> bool {
    let Some((path, args)) = crate::config::parse_hook_command(command) else {
        return false;
    };
    let basename = path
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(basename.as_str(), "meowo-reporter" | "meowo-reporter.exe")
        && args.len() == 4
        && args[0] == "lead"
        && matches!(args[1].as_str(), "session" | "claim" | "stop" | "gate")
        && args[2] == "--provider"
        && args[3] == provider
}

fn edit_json(
    spec: &LeadInstallSpec,
    before: &str,
    reporter: &str,
    enabled: bool,
) -> Result<String, String> {
    let mut root: Value = serde_json::from_str(before.trim_start_matches('\u{feff}'))
        .map_err(|_| "lead/settings_invalid")?;
    if !root.is_object() || root.get("hooks").is_some_and(|value| !value.is_object()) {
        return Err("lead/settings_invalid".into());
    }
    let original = root.clone();
    if root.get("hooks").is_none() {
        if !enabled {
            return Ok(before.into());
        }
        root["hooks"] = json!({});
    }
    let hooks = root
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .ok_or("lead/settings_invalid")?;
    let mut slots = std::collections::HashMap::new();
    for (event, groups) in hooks.iter_mut() {
        let groups = groups.as_array_mut().ok_or("lead/settings_invalid")?;
        for (index, group) in groups.iter_mut().enumerate() {
            let handlers = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .ok_or("lead/settings_invalid")?;
            let old = handlers.len();
            handlers.retain(|handler| {
                !handler
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| managed_command(command, spec.provider))
            });
            // 保留空槽位，Codex 用户 hook 的信任键含数组下标，删除组会让无关 hook 失信。
            if old != handlers.len() && handlers.is_empty() {
                slots.entry(event.clone()).or_insert(index);
            }
        }
    }
    if enabled {
        for hook in spec.hooks {
            let mut handler = json!({"type":"command","command":hook_command(spec,reporter,hook.mode)?,"timeout":hook.timeout});
            if cfg!(windows) && spec.windows_override {
                handler["commandWindows"] = json!(windows_hook_command(spec, reporter, hook.mode)?);
            }
            let groups = hooks
                .entry(hook.event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("lead/settings_invalid")?;
            let group = json!({"hooks":[handler]});
            if let Some(index) = slots.remove(hook.event) {
                groups[index] = group;
            } else {
                groups.push(group);
            }
        }
    }
    if root == original {
        Ok(before.into())
    } else {
        serde_json::to_string_pretty(&root)
            .map(|text| format!("{text}\n"))
            .map_err(|_| "lead/settings_invalid".into())
    }
}

fn edit_toml(
    spec: &LeadInstallSpec,
    before: &str,
    reporter: &str,
    enabled: bool,
) -> Result<String, String> {
    let mut doc = before
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| "lead/settings_invalid")?;
    let mut changed = false;
    if let Some(item) = doc.get("hooks") {
        if item.as_array().is_some_and(|array| array.is_empty()) {
            doc["hooks"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
        } else if !item.is_array_of_tables() {
            return Err("lead/settings_invalid".into());
        }
    } else if enabled {
        doc["hooks"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    if let Some(hooks) = doc
        .get_mut("hooks")
        .and_then(toml_edit::Item::as_array_of_tables_mut)
    {
        let mut desired = Vec::new();
        if enabled {
            for hook in spec.hooks {
                let mut table = toml_edit::Table::new();
                table.insert("event", toml_edit::value(hook.event));
                table.insert(
                    "command",
                    toml_edit::value(hook_command(spec, reporter, hook.mode)?),
                );
                table.insert("timeout", toml_edit::value(hook.timeout as i64));
                desired.push(table);
            }
        }
        let old_managed: Vec<_> = hooks
            .iter()
            .filter(|table| {
                table
                    .get("command")
                    .and_then(toml_edit::Item::as_str)
                    .is_some_and(|command| managed_command(command, spec.provider))
            })
            .map(ToString::to_string)
            .collect();
        let next_managed: Vec<_> = desired.iter().map(ToString::to_string).collect();
        if old_managed != next_managed {
            for index in (0..hooks.len()).rev() {
                if hooks
                    .get(index)
                    .and_then(|table| table.get("command"))
                    .and_then(toml_edit::Item::as_str)
                    .is_some_and(|command| managed_command(command, spec.provider))
                {
                    hooks.remove(index);
                }
            }
            for table in desired {
                hooks.push(table);
            }
            changed = true;
        }
    }
    if changed {
        Ok(doc.to_string())
    } else {
        Ok(before.into())
    }
}

fn hook_edit(
    cap: &dyn LeadCap,
    spec: &LeadInstallSpec,
    before: &str,
    reporter: &str,
    enabled: bool,
) -> Result<String, String> {
    match spec.format {
        LeadHookFormat::Json => edit_json(spec, before, reporter, enabled),
        LeadHookFormat::Toml => edit_toml(spec, before, reporter, enabled),
        LeadHookFormat::Plugin => {
            if enabled {
                cap.plugin(reporter)
                    .ok_or_else(|| "lead/native_hooks_unsupported".into())
            } else {
                Ok(String::new())
            }
        }
    }
}

/// hook 编辑 + 插件的附加编辑（同一文件）。
fn full_edit(
    cap: &dyn LeadCap,
    installation: &Installation,
    spec: &LeadInstallSpec,
    before: &str,
    reporter: &str,
    enabled: bool,
) -> Result<(String, Option<String>), String> {
    let after = hook_edit(cap, spec, before, reporter, enabled)?;
    if matches!(spec.format, LeadHookFormat::Plugin) {
        return Ok((after, None));
    }
    let saved = match cap.saved_settings_rel() {
        Some(relative) => read(&asset_path(&installation.data_dir, relative)?)?,
        None => None,
    };
    cap.edit_hook_file(installation, &after, reporter, enabled, saved.as_deref())
}

struct WritePlan {
    path: PathBuf,
    before: Option<String>,
    after: Option<String>,
    backup: bool,
}
fn apply(plans: &[WritePlan]) -> Result<(), String> {
    for plan in plans {
        if read(&plan.path)? != plan.before {
            return Err("lead/config_changed".into());
        }
        if plan.backup && plan.before.is_some() && plan.before != plan.after {
            crate::wiring::backup_once(&plan.path).map_err(|_| "lead/backup_failed")?;
        }
    }
    let mut written: Vec<&WritePlan> = Vec::new();
    for plan in plans {
        if plan.before == plan.after {
            continue;
        }
        let result = match read(&plan.path) {
            Err(error) => Err(error),
            Ok(current) if current != plan.before => Err("lead/config_changed".to_string()),
            Ok(_) => match &plan.after {
                Some(after) => crate::fsutil::write_atomic(&plan.path, after)
                    .map_err(|_| "lead/write_failed".into()),
                None => fs::remove_file(&plan.path).map_err(|_| "lead/write_failed".into()),
            },
        };
        if let Err(error) = result {
            let mut rollback_failed = false;
            for previous in written.into_iter().rev() {
                // 回滚也不能覆盖安装途中用户刚写下的新内容。
                if read(&previous.path).ok().flatten() == previous.after {
                    let restored = match &previous.before {
                        Some(before) => crate::fsutil::write_atomic(&previous.path, before),
                        None => fs::remove_file(&previous.path),
                    };
                    rollback_failed |= restored.is_err();
                } else {
                    rollback_failed = true;
                }
            }
            return Err(if rollback_failed {
                "lead/rollback_incomplete".into()
            } else {
                error
            });
        }
        written.push(plan);
    }
    Ok(())
}

pub fn install(
    cap: &dyn LeadCap,
    installation: &Installation,
    version: Option<&str>,
    reporter: &str,
) -> Result<LeadInstallState, String> {
    let spec = cap.spec(installation).ok_or("lead/variant_unsupported")?;
    if !version_matches(version, spec.version_family) {
        return Err("lead/version_unsupported".into());
    }
    if !installation.data_dir.is_dir() {
        return Err("lead/provider_missing".into());
    }
    let assets = [
        (spec.skill_rel, SKILL.to_string()),
        (spec.reviewer_rel, cap.reviewer()),
    ];
    let mut plans = Vec::new();
    for (relative, body) in assets {
        let path = asset_path(&installation.data_dir, relative)?;
        let before = read(&path)?;
        if before
            .as_ref()
            .is_some_and(|text| !text.contains(MANAGED_MARK))
        {
            return Err("lead/user_file_exists".into());
        }
        plans.push(WritePlan {
            path,
            before,
            after: Some(body),
            backup: false,
        });
    }
    for relative in cap.managed_file_rels() {
        let path = asset_path(&installation.data_dir, relative)?;
        let before = read(&path)?;
        if before
            .as_ref()
            .is_some_and(|text| !text.contains(MANAGED_MARK))
        {
            return Err("lead/user_file_exists".into());
        }
        let body = cap.managed_file_body(installation, relative, reporter)?;
        plans.push(WritePlan {
            path,
            before,
            after: Some(body),
            backup: false,
        });
    }
    let path = asset_path(&installation.data_dir, spec.hook_rel)?;
    let existing = read(&path)?;
    if matches!(spec.format, LeadHookFormat::Plugin)
        && existing
            .as_ref()
            .is_some_and(|text| !text.contains(MANAGED_MARK))
    {
        return Err("lead/user_file_exists".into());
    }
    let before = existing.clone().unwrap_or_else(|| {
        if matches!(spec.format, LeadHookFormat::Json) {
            "{}".into()
        } else {
            String::new()
        }
    });
    let (after, record) = full_edit(cap, installation, spec, &before, reporter, true)?;
    if let (Some(record), Some(relative)) = (record, cap.saved_settings_rel()) {
        let saved = asset_path(&installation.data_dir, relative)?;
        // 原值只记第一次：重装/刷新时配置里已是 Meowo 写的值，再记就把用户原值冲掉了。
        if read(&saved)?.is_none() {
            plans.push(WritePlan {
                path: saved,
                before: None,
                after: Some(record),
                backup: false,
            });
        }
    }
    let extra = cap.extra_config(installation, &before, &after, true)?;
    plans.push(WritePlan {
        path,
        before: existing,
        after: Some(after),
        backup: !matches!(spec.format, LeadHookFormat::Plugin),
    });
    // Codex 信任哈希必须最后写，不能先信任尚未落盘的命令。
    for change in extra {
        plans.push(WritePlan {
            path: change.path,
            before: change.before,
            after: Some(change.after),
            backup: true,
        });
    }
    apply(&plans)?;
    Ok(inspect(cap, installation, version, reporter))
}

pub fn uninstall(cap: &dyn LeadCap, installation: &Installation) -> Result<(), String> {
    let spec = cap.spec(installation).ok_or("lead/variant_unsupported")?;
    let mut plans = Vec::new();
    for relative in [spec.skill_rel, spec.reviewer_rel] {
        let path = asset_path(&installation.data_dir, relative)?;
        if managed_file(&path) {
            plans.push(WritePlan {
                before: read(&path)?,
                path,
                after: None,
                backup: false,
            });
        }
    }
    for relative in cap.managed_file_rels() {
        let path = asset_path(&installation.data_dir, relative)?;
        if managed_file(&path) {
            plans.push(WritePlan {
                before: read(&path)?,
                path,
                after: None,
                backup: false,
            });
        }
    }
    let path = asset_path(&installation.data_dir, spec.hook_rel)?;
    if let Some(before) = read(&path)? {
        if matches!(spec.format, LeadHookFormat::Plugin) {
            if before.contains(MANAGED_MARK) {
                plans.push(WritePlan {
                    path,
                    before: Some(before),
                    after: None,
                    backup: false,
                });
            }
        } else {
            let (after, _) = full_edit(cap, installation, spec, &before, "", false)?;
            for change in cap.extra_config(installation, &before, &after, false)? {
                plans.push(WritePlan {
                    path: change.path,
                    before: change.before,
                    after: Some(change.after),
                    backup: true,
                });
            }
            plans.push(WritePlan {
                path,
                before: Some(before),
                after: Some(after),
                backup: true,
            });
        }
    }
    // 还原完原值才删记录：上面的编辑已在规划时读过它。
    if let Some(relative) = cap.saved_settings_rel() {
        let saved = asset_path(&installation.data_dir, relative)?;
        if let Some(before) = read(&saved)? {
            plans.push(WritePlan {
                path: saved,
                before: Some(before),
                after: None,
                backup: false,
            });
        }
    }
    apply(&plans)
}

pub fn inspect(
    cap: &dyn LeadCap,
    installation: &Installation,
    version: Option<&str>,
    reporter: &str,
) -> LeadInstallState {
    let Some(spec) = cap.spec(installation) else {
        return LeadInstallState {
            supported: false,
            installed: false,
            assets_present: false,
            hooks_present: false,
            limitations: Vec::new(),
            reason: Some("lead/variant_unsupported".into()),
        };
    };
    let supported = version_matches(version, spec.version_family);
    let assets_present = fs::read_to_string(installation.data_dir.join(spec.skill_rel))
        .is_ok_and(|body| body == SKILL)
        && fs::read_to_string(installation.data_dir.join(spec.reviewer_rel))
            .is_ok_and(|body| body == cap.reviewer());
    let hooks_present = read(&installation.data_dir.join(spec.hook_rel))
        .ok()
        .flatten()
        .is_some_and(|before| {
            full_edit(cap, installation, spec, &before, reporter, true).is_ok_and(|(after, _)| {
                after == before
                    && cap
                        .extra_config(installation, &before, &after, true)
                        .is_ok_and(|changes| changes.is_empty())
            })
        })
        && cap.managed_file_rels().iter().all(|relative| {
            cap.managed_file_body(installation, relative, reporter)
                .is_ok_and(|body| {
                    read(&installation.data_dir.join(relative))
                        .ok()
                        .flatten()
                        .as_ref()
                        == Some(&body)
                })
        });
    let activation_issue = cap.activation_issue(installation);
    LeadInstallState {
        supported,
        installed: supported && assets_present && hooks_present && activation_issue.is_none(),
        assets_present,
        hooks_present,
        limitations: spec
            .limitations
            .iter()
            .map(|text| (*text).into())
            .chain(
                (supported && version_untested(version, spec.verified))
                    .then(|| "lead/version_untested".into()),
            )
            .collect(),
        reason: if !supported {
            Some("lead/version_unsupported".into())
        } else {
            activation_issue
        },
    }
}

/// 旧版本资产或部分安装也必须可卸载；不能用正文恰好等于当前版本来判断归属。
pub fn managed_present(cap: &dyn LeadCap, installation: &Installation) -> bool {
    let Some(spec) = cap.spec(installation) else {
        return false;
    };
    if [spec.skill_rel, spec.reviewer_rel]
        .iter()
        .chain(cap.managed_file_rels())
        .any(|relative| managed_file(&installation.data_dir.join(relative)))
        || cap
            .saved_settings_rel()
            .is_some_and(|relative| installation.data_dir.join(relative).is_file())
    {
        return true;
    }
    let Ok(Some(before)) = read(&installation.data_dir.join(spec.hook_rel)) else {
        return false;
    };
    match spec.format {
        LeadHookFormat::Plugin => before.contains(MANAGED_MARK),
        _ => full_edit(cap, installation, spec, &before, "", false)
            .is_ok_and(|(after, _)| after != before),
    }
}

#[cfg(test)]
mod tests;
