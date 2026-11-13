//! 会话设置（模型 / 模式）的宿主分派：清单从哪来、选了怎么生效（任务 .lead/20261114-tui-model-switch）。
//!
//! 起因：2026-11-14 leader 手机上给 Kimi 切模型卡在「启动被卡住」——原先切模型是往终端敲 `/model`
//! 再识别弹出的菜单，CLI 一改画面、会话没起来就卡死。现在按插件声明的**机制**分派，不按 provider 名：
//! - 清单：插件读 CLI 配置（`model_catalog`），读不到用 CLI 自带的离线子命令（`model_catalog_command`）；
//!   TuiDialog 机制的会话有服务时用服务列的（与 TUI 对话框一致）。
//! - 切换：`ModeSwitch::Server` / `ModelSwitch::ServerSettings` 经会话服务改；`Restart` / `RestartResume`
//!   空闲时带参数重启接回同一会话；`TuiDialog` 经服务打开 TUI 自己的对话框再输入名字。
//! - 运行中、有待批审批时一律不在这里做：前端按 activityTone 记成「这一轮结束后切换」（与 Lead 送达同一套
//!   判定），回合结束再调本命令。往终端写字的路径（TuiDialog）还要服务端二次复核。
//!
//! 启动参数的值只认清单里的 id（[`meowo_agent::resolve_session_args`]），用户输入不进 argv。

use meowo_agent::{AgentPlugin, DynamicModeChoice, ModelCatalogEntry};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

/// 某 agent 的模型清单（不连会话服务）：插件读配置；读不到时问 CLI 的离线子命令。
pub(crate) fn catalog_for(
    plugin: &'static dyn AgentPlugin,
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Vec<ModelCatalogEntry> {
    let listed = plugin.model_catalog(env);
    if !listed.is_empty() {
        return listed;
    }
    let Some(args) = plugin.model_catalog_command() else {
        return Vec::new();
    };
    crate::run_cli_capture_in(plugin, args, std::time::Duration::from_secs(15), env, cwd)
        .map(|output| plugin.parse_model_catalog_output(&output))
        .unwrap_or_default()
}

/// 运行时才知道的模式选项（opencode 自定义 agent）。没声明的 agent 不起进程。
pub(crate) fn dynamic_for(
    plugin: &'static dyn AgentPlugin,
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Vec<DynamicModeChoice> {
    let Some((_, args, _)) = plugin.dynamic_mode_choices() else {
        return Vec::new();
    };
    crate::run_cli_capture_in(plugin, args, std::time::Duration::from_secs(15), env, cwd)
        .map(|output| plugin.parse_dynamic_mode_choices(&output))
        .unwrap_or_default()
}

/// 拼启动参数要用的清单：只有选择里真有模型 / 动态档时才去读（opencode 的离线子命令要 1~2 秒）。
pub(crate) fn args_for_selections(
    plugin: &'static dyn AgentPlugin,
    selections: &HashMap<String, String>,
    env: &[(String, String)],
    cwd: Option<&Path>,
    resuming: bool,
) -> Vec<String> {
    let static_model = plugin.launch_options().iter().any(|o| o.id == "model");
    let catalog = if !static_model && selections.contains_key("model") {
        catalog_for(plugin, env, cwd)
    } else {
        Vec::new()
    };
    let wants_dynamic = plugin
        .dynamic_mode_choices()
        .and_then(|(option, _, _)| {
            let picked = selections.get(option)?;
            let declared = plugin
                .launch_options()
                .iter()
                .find(|o| o.id == option)
                .is_some_and(|o| o.choices.iter().any(|c| c.id == picked));
            (!declared).then_some(())
        })
        .is_some();
    let dynamic = if wants_dynamic {
        dynamic_for(plugin, env, cwd)
    } else {
        Vec::new()
    };
    meowo_agent::resolve_session_args(plugin, selections, &catalog, &dynamic, resuming)
}

/// 选择里有模型、而这家用 `-m` 一类参数指定模型时，模型必须在清单里——否则 resolve_session_args 会
/// 静默丢掉它，会话按默认模型启动、界面却显示选中的模型（做空 H3）。
pub(crate) fn check_model_selection(
    plugin: &'static dyn AgentPlugin,
    selections: &HashMap<String, String>,
    env: &[(String, String)],
    cwd: Option<&Path>,
) -> Result<(), String> {
    let Some(model) = selections.get("model") else {
        return Ok(());
    };
    if plugin.launch_options().iter().any(|o| o.id == "model") || plugin.model_launch_flag().is_none() {
        return Ok(());
    }
    if meowo_agent::model_id_allowed(model, &catalog_for(plugin, env, cwd)) {
        Ok(())
    } else {
        Err("settings/model_unknown".into())
    }
}

fn session_env(provider: &str, session_id: i64) -> Vec<(String, String)> {
    let Some(plugin) = meowo_agent::resolve(Some(provider)) else {
        return Vec::new();
    };
    let profile = crate::open_store(&crate::db_path())
        .ok()
        .and_then(|store| store.session_profile(session_id).ok().flatten());
    crate::profile::env_of(plugin.id(), profile.as_deref())
}

/// 新建页默认值：CLI 自己配置里此刻的默认模型与各栏默认档（打开新建页、换 agent 时现读）。
#[tauri::command]
pub(crate) async fn agent_launch_defaults(
    provider: String,
    cwd: Option<String>,
) -> meowo_agent::LaunchDefaults {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(plugin) = meowo_agent::resolve(Some(&provider)) else {
            return meowo_agent::LaunchDefaults::default();
        };
        let profile = crate::profile::active_id(plugin.id().as_str());
        let env = crate::profile::env_of(plugin.id(), profile.as_deref());
        plugin.launch_defaults(&env, cwd.as_deref().map(Path::new))
    })
    .await
    .unwrap_or_default()
}

/// 运行时才知道的模式选项（新建页 OpenCode 的自定义 agent）。
#[tauri::command]
pub(crate) async fn agent_dynamic_mode_choices(
    provider: String,
    cwd: Option<String>,
) -> Vec<DynamicModeChoice> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(plugin) = meowo_agent::resolve(Some(&provider)) else {
            return Vec::new();
        };
        let profile = crate::profile::active_id(plugin.id().as_str());
        let env = crate::profile::env_of(plugin.id(), profile.as_deref());
        dynamic_for(plugin, &env, cwd.as_deref().map(Path::new))
    })
    .await
    .unwrap_or_default()
}

/// 会话里的模型清单。TuiDialog 机制的会话有服务时用服务列的（已连接 provider，与 TUI 对话框分组一致，
/// research-codex-opencode §1）；其余（含 Codex：只在用户真的切换时才连 app-server，连一次 UA 后缀就会变，
/// §5）读配置或离线子命令。
pub(crate) fn session_catalog(
    db: &Path,
    plugin: &'static dyn AgentPlugin,
    session_id: Option<i64>,
    env: &[(String, String)],
    cwd: Option<&str>,
    app: Option<&tauri::AppHandle>,
) -> Vec<ModelCatalogEntry> {
    if let (Some(sid), Some(meowo_agent::ModelSwitch::TuiDialog)) =
        (session_id.filter(|sid| *sid > 0), plugin.model_switch())
    {
        if let Some(list) = dialog_catalog(db, sid, cwd, app) {
            return list;
        }
    }
    catalog_for(plugin, env, cwd.map(Path::new))
}

fn dialog_catalog(
    db: &Path,
    sid: i64,
    cwd: Option<&str>,
    app: Option<&tauri::AppHandle>,
) -> Option<Vec<ModelCatalogEntry>> {
    use crate::lead_native::opencode_http as oc;
    let presence = oc::session_presence(db, sid, app).ok()?;
    let catalog = oc::list_models(db, presence, cwd).ok()?;
    let mut out = Vec::new();
    for provider in catalog.providers {
        let default = catalog.defaults.get(&provider.id);
        for model in provider.models {
            let mut entry =
                ModelCatalogEntry::new(format!("{}/{}", provider.id, model.id), model.name);
            entry.group = Some(provider.name.clone());
            entry.efforts = model.variants;
            entry.is_default = default == Some(&model.id);
            out.push(entry);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// 会话此刻的模型与模式（服务 / 会话文件里读到的；读不到的项不出现）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveSettings {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub modes: Vec<LiveMode>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LiveMode {
    pub dimension: String,
    pub value: String,
}

/// 读会话当前的模型 / 模式，回合结束与打开菜单时调（leader 2026-11-14：合适时机同步，不定时轮询）。
/// 按切换机制分派：会话服务机制读 rollout 最新段的 turn_context（TUI 里自己 /model、Shift+Tab 切的，
/// 下一回合起就进 rollout）；TUI 对话框机制读最后一条用户消息带的 model / agent（上游限制：TUI 里刚切、
/// 还没发消息的要等下一条，research-codex-opencode §8）。带参重启机制的两家由 transcript 增量负责，这里不读。
#[tauri::command]
pub(crate) async fn session_live_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
) -> Result<LiveSettings, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(live_blocking(&app, &db, session_id).unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn live_blocking(app: &tauri::AppHandle, db: &Path, session_id: i64) -> Option<LiveSettings> {
    if session_id <= 0 {
        return None;
    }
    let store = crate::open_store(db).ok()?;
    let header = store.session_header(session_id).ok()?;
    let plugin = meowo_agent::resolve(Some(&header.provider))?;
    let mode = |dimension: &str, value: String| LiveMode {
        dimension: dimension.to_string(),
        value,
    };
    match plugin.model_switch()? {
        meowo_agent::ModelSwitch::ServerSettings => {
            let (model, effort, modes) =
                crate::lead_native::codex_app_server::live_settings(&header.cc_session_id)?;
            Some(LiveSettings {
                model,
                effort,
                modes: modes.into_iter().map(|(d, v)| mode(&d, v)).collect(),
            })
        }
        meowo_agent::ModelSwitch::TuiDialog => {
            use crate::lead_native::opencode_http as oc;
            let presence = oc::session_presence(db, session_id, Some(app)).ok()?;
            let choice = oc::current_choice(
                db,
                presence,
                &header.cc_session_id,
                header.cwd.as_deref(),
            )
            .ok()??;
            let model = match (choice.provider_id, choice.model_id) {
                (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
                (_, model) => model,
            };
            // agent 记在插件里「经对话框切」的那一栏。
            let option = plugin
                .launch_options()
                .iter()
                .find(|o| o.switch == meowo_agent::ModeSwitch::Dialog)
                .map(|o| o.id);
            Some(LiveSettings {
                model,
                effort: choice.variant,
                modes: option
                    .zip(choice.agent)
                    .map(|(option, agent)| vec![mode(option, agent)])
                    .unwrap_or_default(),
            })
        }
        meowo_agent::ModelSwitch::RestartResume { .. } => None,
    }
}

/// 前端在切换前如实告知的会话状态（OpenCode 对话框路线的调用约定，服务端另会复核）。
pub(crate) type TuiStateArg = meowo_protocol::bridge::SessionTuiState;

/// 一次切换的结果。`kind`：
/// - `applied`：经会话服务 / TUI 对话框已改（下一回合生效）；`confirmed` = 画面上已看到新值。
/// - `restarted`：带参数重启接回了同一会话。
/// - `reopened`：会话还没建（临时 id），带新参数重开，`session_id` 是新的临时 id。
/// - `needs_new_session`：CLI 接回会话时只能放宽权限，收紧到这一档要新会话（Kimi）。
/// - `unsupported`：这个会话够不着服务（Lead 未装、外部终端），前端退回终端内联命令兜底。
/// - `next_open`：会话不在托管中，选择已记成下次打开的启动参数。
/// - `not_now`：服务端复核此刻不能动（运行中 / 有审批 / 输入框有字），`reason` 说明，回合结束再试。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsOutcome {
    pub kind: &'static str,
    pub session_id: i64,
    pub confirmed: bool,
    pub reason: Option<String>,
}

impl SettingsOutcome {
    fn new(kind: &'static str, session_id: i64) -> Self {
        Self {
            kind,
            session_id,
            confirmed: false,
            reason: None,
        }
    }
}

/// 一次请求按机制拆好的工作。纯函数，便于单测（不碰进程与库）。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Plan {
    /// 经会话服务改的设置（CodexSettingsUpdate 的 JSON）。
    pub server: serde_json::Map<String, serde_json::Value>,
    /// 带参重启时合并进会话启动选项的选择（含 `model`）。
    pub restart: HashMap<String, String>,
    /// 经 TUI 对话框切的模型 id。
    pub dialog_model: Option<String>,
    /// 经 TUI 对话框切的 agent。
    pub dialog_agent: Option<String>,
    /// 收紧到接回时表达不了的档（Kimi 降回 Always Ask）。
    pub needs_new_session: bool,
}

/// 校验并按机制拆分。`current` 是会话此刻的实际值（transcript / 底栏读到的，栏 id → 档 id，`model` 键是
/// 当前模型 id），`stored` 是会话存档的启动选择。未知栏、未知档、清单外的模型一律报错，不进 argv。
///
/// 带参重启时**每一栏都按当前实际值拼**，不按存档回放（做空审查 H1）：用户在 TUI 里 `/permission` 收紧、
/// `/plan off`、Shift+Tab 退回的，存档不知道；按存档回放会把收紧的权限重新放开（Kimi 接回只升不降，K:558700）。
/// 某栏存档会带参数、却读不到当前值时宁可不重启，返回 `settings/current_unknown`。
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    plugin: &dyn AgentPlugin,
    model: Option<&str>,
    effort: Option<&str>,
    modes: &HashMap<String, String>,
    catalog: &[ModelCatalogEntry],
    dynamic: &[DynamicModeChoice],
    current: &HashMap<String, String>,
    stored: &HashMap<String, String>,
) -> Result<Plan, String> {
    let mut out = Plan::default();
    if let Some(model) = model {
        if !meowo_agent::model_id_allowed(model, catalog) {
            return Err("settings/model_unknown".into());
        }
        match plugin.model_switch() {
            Some(meowo_agent::ModelSwitch::ServerSettings) => {
                out.server.insert("model".into(), model.into());
                if let Some(effort) = effort {
                    out.server.insert("effort".into(), effort.into());
                }
            }
            Some(meowo_agent::ModelSwitch::RestartResume { .. }) => {
                out.restart.insert("model".into(), model.into());
            }
            Some(meowo_agent::ModelSwitch::TuiDialog) => out.dialog_model = Some(model.into()),
            None => return Err("settings/unsupported".into()),
        }
    }
    for (option_id, choice_id) in modes {
        let option = plugin
            .launch_options()
            .iter()
            .find(|o| o.id == option_id)
            .ok_or("settings/mode_unknown")?;
        let choice = option.choices.iter().find(|c| c.id == choice_id);
        let dynamic_ok = dynamic
            .iter()
            .any(|d| &d.option == option_id && &d.id == choice_id);
        if choice.is_none() && !dynamic_ok {
            return Err("settings/mode_unknown".into());
        }
        match option.switch {
            meowo_agent::ModeSwitch::Server => {
                // 没有服务端片段的档（Codex「按配置」）会话里切不了：前端不列它，这里兜底（做空 M6）。
                let patch = choice
                    .and_then(|c| c.server)
                    .ok_or("settings/mode_launch_only")?;
                let value: serde_json::Value =
                    serde_json::from_str(patch).map_err(|_| "settings/mode_unknown")?;
                if let Some(map) = value.as_object() {
                    out.server.extend(map.clone());
                }
            }
            meowo_agent::ModeSwitch::Restart => {
                // 接回只能放宽：目标档不带参数（Kimi Always Ask）而会话现在不是这一档 = 收紧，做不到。
                let tightening = option.resume_upgrade_only
                    && choice.is_some_and(|c| c.args.is_empty())
                    && current.get(option_id.as_str()) != Some(choice_id);
                if tightening {
                    out.needs_new_session = true;
                } else {
                    out.restart.insert(option_id.clone(), choice_id.clone());
                }
            }
            // OpenCode agent：经服务打开 TUI 的 agent 对话框选中（switch_agent）。Control 型（Kimi `/plan on|off`）
            // 由前端用 mode_controls 的直达输入切，不来这里。
            meowo_agent::ModeSwitch::Dialog => out.dialog_agent = Some(choice_id.clone()),
            _ => return Err("settings/mode_launch_only".into()),
        }
    }
    if !out.restart.is_empty() && !out.needs_new_session {
        fill_restart_with_current(plugin, &mut out.restart, current, stored)?;
    }
    Ok(out)
}

/// 带参重启的其余各栏：按会话当前实际值拼（见 [`plan`]）。读不到当前值、而存档那一档会带参数的，拒绝。
fn fill_restart_with_current(
    plugin: &dyn AgentPlugin,
    restart: &mut HashMap<String, String>,
    current: &HashMap<String, String>,
    stored: &HashMap<String, String>,
) -> Result<(), String> {
    for option in plugin.launch_options() {
        if option.in_session_only || restart.contains_key(option.id) {
            continue;
        }
        let known = current
            .get(option.id)
            .filter(|value| option.choices.iter().any(|c| c.id == value.as_str()));
        match known {
            Some(value) => {
                restart.insert(option.id.to_string(), value.clone());
            }
            None => {
                let stored_has_args = stored
                    .get(option.id)
                    .and_then(|id| option.choices.iter().find(|c| c.id == id.as_str()))
                    .is_some_and(|c| !c.args.is_empty());
                if stored_has_args {
                    return Err("settings/current_unknown".into());
                }
            }
        }
    }
    // 模型同理：没换模型时按当前模型拼，不回放存档里的旧模型（做空 M5）；当前模型读不到、存档又有模型时拒绝。
    if !restart.contains_key("model") {
        match current.get("model") {
            Some(model) => {
                restart.insert("model".into(), model.clone());
            }
            None if stored.contains_key("model") => return Err("settings/current_unknown".into()),
            None => {}
        }
    }
    Ok(())
}

/// 切模型 / 模式。前端只在空闲且没有待批审批时调（运行中排到这一轮之后，见模块文档）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn apply_session_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    model: Option<String>,
    effort: Option<String>,
    modes: Option<HashMap<String, String>>,
    current: Option<HashMap<String, String>>,
    tui: Option<TuiStateArg>,
) -> Result<SettingsOutcome, String> {
    let broker = state.ptys.clone();
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        apply_blocking(
            &app,
            &broker,
            &db,
            session_id,
            model,
            effort,
            modes.unwrap_or_default(),
            current.unwrap_or_default(),
            tui.unwrap_or_default(),
        )
        .map_err(|reason| settings_reason(&reason))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 切换语境下的失败原因：服务通道的连接类错误（`lead/codex_server_*`、`lead/opencode_http_*` 等）在 i18n
/// 里是 Lead「回答已保存…」的文案，切模型时说这个是错的（做空 M8），统一换成切换语境的键；
/// 切换专用的那几个原样保留。
pub(crate) fn settings_reason(reason: &str) -> String {
    const KEEP: &[&str] = &[
        "lead/codex_model_unknown",
        "lead/codex_settings_invalid",
        "lead/opencode_tui_not_ready",
        "lead/opencode_busy",
        "lead/opencode_composer_not_empty",
        "lead/opencode_approval_pending",
        "lead/opencode_dialog_open",
        "lead/opencode_dialog_not_open",
        "lead/opencode_dialog_lost",
        "lead/opencode_choice_not_found",
        "lead/opencode_choice_mismatch",
        "lead/opencode_switch_invalid",
    ];
    if reason.starts_with("lead/") && !KEEP.contains(&reason) {
        "settings/server_unreachable".into()
    } else {
        reason.to_string()
    }
}

/// 重启会杀掉 TUI 进程：输入框里没发的字会跟着没了。前端先把草稿搬进对话页输入框、确认 TUI 输入框
/// 空了才调（做空 H2）；这里再守一道，读不准或不空就不重启。
pub(crate) fn restart_guard(tui: &TuiStateArg) -> Result<(), &'static str> {
    if tui.composer_empty {
        Ok(())
    } else {
        Err("settings/composer_not_empty")
    }
}

fn stored_selections(store: &meowo_store::Store, session_id: i64) -> HashMap<String, String> {
    store
        .session_launch_args(session_id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// 未认领会话（临时 id）的改选也要过声明表：未知栏 / 未知档不进 pending 选择（做空 L5）。
fn validate_modes(
    plugin: &dyn AgentPlugin,
    modes: &HashMap<String, String>,
    dynamic: &[DynamicModeChoice],
) -> Result<(), String> {
    for (option_id, choice_id) in modes {
        let option = plugin
            .launch_options()
            .iter()
            .find(|o| o.id == option_id)
            .ok_or("settings/mode_unknown")?;
        let declared = option.choices.iter().any(|c| c.id == choice_id);
        let dynamic_ok = dynamic
            .iter()
            .any(|d| &d.option == option_id && &d.id == choice_id);
        if !declared && !dynamic_ok {
            return Err("settings/mode_unknown".into());
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_blocking(
    app: &tauri::AppHandle,
    broker: &crate::pty::PtyBroker,
    db: &Path,
    session_id: i64,
    model: Option<String>,
    effort: Option<String>,
    modes: HashMap<String, String>,
    current: HashMap<String, String>,
    tui: TuiStateArg,
) -> Result<SettingsOutcome, String> {
    // 还没建会话（临时 id）：没有任何东西会丢，直接带新参数重开（Kimi 2.6 首条消息前不建会话，
    // research-kimi-gemini §2.3 p7 实测 `kimi -m` 重开后第一条请求就用新模型、不改配置）。
    if session_id < 0 {
        let launch = broker
            .pending_launch(session_id)
            .ok_or("settings/session_gone")?;
        let plugin = meowo_agent::resolve(Some(&launch.provider)).ok_or("settings/unsupported")?;
        restart_guard(&tui)?;
        let profile = crate::profile::active_id(plugin.id().as_str());
        let env = crate::profile::env_of(plugin.id(), profile.as_deref());
        let cwd = launch.cwd.as_deref().map(Path::new);
        let catalog = if model.is_some() {
            catalog_for(plugin, &env, cwd)
        } else {
            Vec::new()
        };
        let needs_dynamic = modes.iter().any(|(option, choice)| {
            plugin
                .launch_options()
                .iter()
                .find(|o| o.id == option)
                .is_some_and(|o| !o.choices.iter().any(|c| c.id == choice))
        });
        let dynamic = if needs_dynamic {
            dynamic_for(plugin, &env, cwd)
        } else {
            Vec::new()
        };
        validate_modes(plugin, &modes, &dynamic)?;
        let mut overrides = modes.clone();
        if let Some(model) = &model {
            if !meowo_agent::model_id_allowed(model, &catalog) {
                return Err("settings/model_unknown".into());
            }
            overrides.insert("model".into(), model.clone());
        }
        let new_id = reopen_pending(app, broker, session_id, &overrides)?;
        return Ok(SettingsOutcome::new("reopened", new_id));
    }
    let store = crate::open_store(db)?;
    let header = store.session_header(session_id).map_err(|e| e.to_string())?;
    let plugin = meowo_agent::resolve(Some(&header.provider)).ok_or("settings/unsupported")?;
    let env = session_env(&header.provider, session_id);
    let catalog = if model.is_some() || current.contains_key("model") {
        session_catalog(db, plugin, Some(session_id), &env, header.cwd.as_deref(), Some(app))
    } else {
        Vec::new()
    };
    let needs_dynamic = modes.iter().any(|(option, choice)| {
        plugin
            .launch_options()
            .iter()
            .find(|o| o.id == option)
            .is_some_and(|o| !o.choices.iter().any(|c| c.id == choice))
    });
    let dynamic = if needs_dynamic {
        dynamic_for(plugin, &env, header.cwd.as_deref().map(Path::new))
    } else {
        Vec::new()
    };
    // 当前模型不在清单里（自定义 provider 等）就当读不到，交给 fill_restart_with_current 判。
    let mut current = current;
    if current
        .get("model")
        .is_some_and(|model| !meowo_agent::model_id_allowed(model, &catalog))
    {
        current.remove("model");
    }
    let stored = stored_selections(&store, session_id);
    let plan = plan(
        plugin,
        model.as_deref(),
        effort.as_deref(),
        &modes,
        &catalog,
        &dynamic,
        &current,
        &stored,
    )?;
    if plan.needs_new_session {
        return Ok(SettingsOutcome::new("needs_new_session", session_id));
    }
    // 会话不在托管中（已退出、Meowo 重启后没恢复）：什么进程都动不了。把这次的选择记成下次打开的启动参数，
    // 如实说「下次打开时生效」，不报成功（做空 M4）。
    if !broker.is_active(session_id) {
        let mut next = modes.clone();
        if let Some(model) = &model {
            next.insert("model".into(), model.clone());
        }
        persist(&store, session_id, &next, None);
        return Ok(SettingsOutcome::new("next_open", session_id));
    }
    let mut outcome = SettingsOutcome::new("applied", session_id);
    if !plan.server.is_empty() {
        use crate::lead_native::codex_app_server as codex;
        let update: codex::CodexSettingsUpdate =
            serde_json::from_value(serde_json::Value::Object(plan.server.clone()))
                .map_err(|_| "settings/mode_unknown")?;
        match codex::update_settings(db, session_id, &header.cc_session_id, &update) {
            Ok(()) => outcome.confirmed = true,
            Err(codex::ControlError::Unsupported) => {
                return Ok(SettingsOutcome::new("unsupported", session_id))
            }
            // 没握手 / 主线程不在：lead 那几个 reason 的文案是「答复稍后送达」，这里换成切换语境的。
            Err(codex::ControlError::NotReady(_)) => return Err("settings/server_not_ready".into()),
            Err(codex::ControlError::Failed(reason)) => return Err(reason),
        }
        persist(&store, session_id, &modes, model.as_deref());
    }
    if plan.dialog_model.is_some() || plan.dialog_agent.is_some() {
        match dialog_switch(app, broker, db, session_id, &header, &catalog, &plan, tui) {
            Ok(confirmed) => outcome.confirmed = confirmed,
            Err(DialogError::Unsupported) => {
                return Ok(SettingsOutcome::new("unsupported", session_id))
            }
            Err(DialogError::NotNow(reason)) => {
                let mut not_now = SettingsOutcome::new("not_now", session_id);
                not_now.reason = Some(settings_reason(&reason));
                return Ok(not_now);
            }
            Err(DialogError::Failed(reason)) => return Err(reason),
        }
        persist(&store, session_id, &modes, model.as_deref());
    }
    if !plan.restart.is_empty() {
        restart_guard(&tui)?;
        // 后台子任务还在跑（主回合停了，tone 是空闲）：重启会把它们一起杀掉。与看板 tab_class 的
        // background_busy 同一份原料（做空 M7）。
        use tauri::Manager as _;
        if let Some(state) = app.try_state::<crate::AppState>() {
            if crate::session_query::subagents_busy(
                &state.tx_cache,
                &header.provider,
                header.cwd.as_deref(),
                &header.cc_session_id,
            ) {
                let mut not_now = SettingsOutcome::new("not_now", session_id);
                not_now.reason = Some("settings/background_busy".into());
                return Ok(not_now);
            }
        }
        if !restart(app, broker, &store, session_id, &header.provider, plan.restart.clone())? {
            // 恰在此刻自己退出了：同上，记成下次打开的启动参数。
            persist(&store, session_id, &plan.restart, None);
            return Ok(SettingsOutcome::new("next_open", session_id));
        }
        outcome.kind = "restarted";
    }
    Ok(outcome)
}

/// 回合结束时对话页读到的会话当前模型 / 模式，写进存档（做空 M5）：之后的恢复、切账号、带参重启按它回放，
/// 不再拿旧存档把用户在 TUI 里的选择改回去。只收清单里的模型与声明过的档。
#[tauri::command]
pub(crate) async fn record_session_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    model: Option<String>,
    modes: Option<HashMap<String, String>>,
) -> Result<(), String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if session_id <= 0 {
            return Ok(());
        }
        let store = crate::open_store(&db)?;
        let header = store.session_header(session_id).map_err(|e| e.to_string())?;
        let plugin = meowo_agent::resolve(Some(&header.provider)).ok_or("settings/unsupported")?;
        let modes: HashMap<String, String> = modes
            .unwrap_or_default()
            .into_iter()
            .filter(|(option, choice)| {
                plugin.launch_options().iter().any(|o| {
                    o.id == option && !o.in_session_only && o.choices.iter().any(|c| c.id == choice)
                })
            })
            .collect();
        let model = model.filter(|model| {
            let env = session_env(&header.provider, session_id);
            let catalog =
                session_catalog(&db, plugin, Some(session_id), &env, header.cwd.as_deref(), Some(&app));
            meowo_agent::model_id_allowed(model, &catalog)
        });
        if !modes.is_empty() || model.is_some() {
            persist(&store, session_id, &modes, model.as_deref());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 「这一轮结束后切换」的排队（做空 M3）：存在宿主进程里、按会话 id 记，换会话、刷新手机页都还在；
/// 后来的选择合并进同一份。内容是前端的请求原样（模型、各栏、提示名），执行仍走 apply_session_settings
/// 的全部校验，这里不解释它。
static QUEUED: std::sync::LazyLock<std::sync::Mutex<HashMap<i64, serde_json::Value>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub(crate) fn merge_queued(prev: Option<&serde_json::Value>, next: &serde_json::Value) -> serde_json::Value {
    let mut merged = prev.cloned().unwrap_or_else(|| serde_json::json!({}));
    if let (Some(target), Some(source)) = (merged.as_object_mut(), next.as_object()) {
        for (key, value) in source {
            if key == "modes" {
                let modes = target
                    .entry("modes")
                    .or_insert_with(|| serde_json::json!({}));
                if let (Some(modes), Some(more)) = (modes.as_object_mut(), value.as_object()) {
                    modes.extend(more.clone());
                }
            } else if !value.is_null() {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

#[tauri::command]
pub(crate) fn queue_session_settings(session_id: i64, request: serde_json::Value) -> serde_json::Value {
    let mut map = QUEUED.lock().unwrap_or_else(|e| e.into_inner());
    let merged = merge_queued(map.get(&session_id), &request);
    map.insert(session_id, merged.clone());
    merged
}

#[tauri::command]
pub(crate) fn queued_session_settings(session_id: i64) -> Option<serde_json::Value> {
    QUEUED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&session_id)
        .cloned()
}

#[tauri::command]
pub(crate) fn take_queued_session_settings(session_id: i64) -> Option<serde_json::Value> {
    QUEUED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&session_id)
}

/// 经服务 / 对话框改过的也记进会话的启动选项：恢复时按它回放（模型是否回放由插件机制决定，见
/// resolve_session_args 的 `resuming`），对话页回显也用它。
fn persist(
    store: &meowo_store::Store,
    session_id: i64,
    modes: &HashMap<String, String>,
    model: Option<&str>,
) {
    let mut selections: HashMap<String, String> = store
        .session_launch_args(session_id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    selections.extend(modes.clone());
    if let Some(model) = model {
        selections.insert("model".into(), model.into());
    }
    if let Ok(json) = serde_json::to_string(&selections) {
        let _ = store.set_session_launch_args(session_id, &json);
    }
}

enum DialogError {
    Unsupported,
    NotNow(String),
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
fn dialog_switch(
    app: &tauri::AppHandle,
    broker: &crate::pty::PtyBroker,
    db: &Path,
    session_id: i64,
    header: &meowo_store::store::SessionHeader,
    catalog: &[ModelCatalogEntry],
    plan: &Plan,
    tui: TuiStateArg,
) -> Result<bool, DialogError> {
    use crate::lead_native::opencode_http as oc;
    let map = |error: oc::ControlError| match error {
        oc::ControlError::Unsupported => DialogError::Unsupported,
        oc::ControlError::NotReady(reason) | oc::ControlError::NotNow(reason) => {
            DialogError::NotNow(reason)
        }
        oc::ControlError::Failed(reason) => DialogError::Failed(reason),
    };
    let presence = oc::session_presence(db, session_id, Some(app)).map_err(DialogError::Failed)?;
    let request = oc::DialogSwitch {
        db,
        presence,
        native_session: &header.cc_session_id,
        cwd: header.cwd.as_deref(),
        state: oc::TuiState {
            idle: tui.idle,
            composer_empty: tui.composer_empty,
            approval_pending: tui.approval_pending,
        },
    };
    // 写字经宿主现有的 PTY 写入接口，读屏用同一托管终端的仿真画面。
    let write_broker = broker.clone();
    let screen_broker = broker.clone();
    let mut io = (
        move |text: &str| write_broker.write(session_id, text.as_bytes()),
        move || {
            screen_broker
                .screen_probe_snapshot(session_id)
                .map(|(snapshot, _)| snapshot.lines.join("\n"))
        },
    );
    let mut confirmed = true;
    if let Some(model) = &plan.dialog_model {
        // 对话框按显示名搜索（oc4），清单的 label 就是 TUI 列的名字。
        // 分组就是 provider 显示名（dialog_catalog），底栏按「模型名 provider 名」精确核对（做空 M2）。
        let entry = catalog
            .iter()
            .find(|entry| &entry.id == model)
            .ok_or(DialogError::Failed("settings/model_unknown".into()))?;
        confirmed &= oc::switch_model(&request, &entry.label, entry.group.as_deref(), &mut io)
            .map_err(map)?
            .confirmed;
    }
    if let Some(agent) = &plan.dialog_agent {
        confirmed &= oc::switch_agent(&request, agent, &mut io).map_err(map)?.confirmed;
    }
    Ok(confirmed)
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn restart(
    app: &tauri::AppHandle,
    broker: &crate::pty::PtyBroker,
    store: &meowo_store::Store,
    session_id: i64,
    provider: &str,
    overrides: HashMap<String, String>,
) -> Result<bool, String> {
    crate::terminal::restart_session_with(app, broker, store, session_id, provider, Some(overrides))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn restart(
    _app: &tauri::AppHandle,
    _broker: &crate::pty::PtyBroker,
    _store: &meowo_store::Store,
    _session_id: i64,
    _provider: &str,
    _overrides: HashMap<String, String>,
) -> Result<bool, String> {
    Err("settings/unsupported".into())
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn reopen_pending(
    app: &tauri::AppHandle,
    broker: &crate::pty::PtyBroker,
    temp_id: i64,
    overrides: &HashMap<String, String>,
) -> Result<i64, String> {
    crate::terminal::reopen_pending_with(app, broker, temp_id, overrides)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn reopen_pending(
    _app: &tauri::AppHandle,
    _broker: &crate::pty::PtyBroker,
    _temp_id: i64,
    _overrides: &HashMap<String, String>,
) -> Result<i64, String> {
    Err("settings/unsupported".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn none() -> HashMap<String, String> {
        HashMap::new()
    }

    /// Codex：模型、权限、协作模式都并成一份服务端设置，不重启、不写终端。
    #[test]
    fn server_mechanism_collects_one_settings_update() {
        let codex = meowo_agent::by_id("codex").unwrap();
        let catalog = vec![ModelCatalogEntry::new("gpt-5.5", "GPT-5.5")];
        let plan = plan(
            codex,
            Some("gpt-5.5"),
            Some("high"),
            &modes(&[("approval", "approveForMe"), ("collaboration", "plan")]),
            &catalog,
            &[],
            &none(),
            &none(),
        )
        .unwrap();
        assert!(plan.restart.is_empty() && plan.dialog_model.is_none());
        let update: crate::lead_native::codex_app_server::CodexSettingsUpdate =
            serde_json::from_value(serde_json::Value::Object(plan.server)).unwrap();
        assert_eq!(update.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(update.effort.as_deref(), Some("high"));
        assert_eq!(update.approvals_reviewer.as_deref(), Some("auto_review"));
        assert_eq!(update.sandbox.as_deref(), Some("workspace-write"));
        assert_eq!(update.collaboration.as_deref(), Some("plan"));
    }

    /// 做空 M6：Codex「按配置」没有服务端片段，会话里切不了，如实说只能新建时选。
    #[test]
    fn server_choice_without_patch_is_launch_only() {
        let codex = meowo_agent::by_id("codex").unwrap();
        let err = plan(codex, None, None, &modes(&[("approval", "default")]), &[], &[], &none(), &none());
        assert_eq!(err.unwrap_err(), "settings/mode_launch_only");
    }

    /// Kimi：模型与放宽权限走重启接回；收紧回 Always Ask 做不到（2.6 接回只升不降，K:558700），
    /// 如实报需要新会话；会话本来就在这一档时不算收紧。
    #[test]
    fn restart_mechanism_refuses_to_pretend_a_tightening() {
        let kimi = meowo_agent::by_id("kimi").unwrap();
        if kimi.launch_options().is_empty() {
            return; // 本机没装 modern kimi（CI）时启动选项为空
        }
        let catalog = vec![ModelCatalogEntry::new("kimi-code/k3", "K3")];
        let up = plan(
            kimi,
            Some("kimi-code/k3"),
            None,
            &modes(&[("permission", "yolo")]),
            &catalog,
            &[],
            &modes(&[("permission", "manual")]),
            &none(),
        )
        .unwrap();
        assert_eq!(up.restart, modes(&[("model", "kimi-code/k3"), ("permission", "yolo")]));
        assert!(!up.needs_new_session);
        let down = plan(kimi, None, None, &modes(&[("permission", "manual")]), &[], &[], &modes(&[("permission", "auto")]), &none()).unwrap();
        assert!(down.needs_new_session);
        let same = plan(kimi, None, None, &modes(&[("permission", "manual")]), &[], &[], &modes(&[("permission", "manual")]), &none()).unwrap();
        assert!(!same.needs_new_session);
    }

    /// 做空 H1 复现 a/b：存档是 --yolo、--plan，用户在 TUI 里 /permission 收紧到 Always Ask、/plan off。
    /// 切模型时其余各栏按**当前实际值**拼，不按存档回放（否则 -r <id> --yolo --plan 把收紧的又放开）。
    #[test]
    fn restart_uses_current_values_not_stale_stored_ones() {
        let kimi = meowo_agent::by_id("kimi").unwrap();
        if kimi.launch_options().is_empty() {
            return;
        }
        let catalog = vec![ModelCatalogEntry::new("kimi-code/k3", "K3")];
        let stored = modes(&[("permission", "yolo"), ("work", "plan"), ("model", "kimi-code/old")]);
        let current = modes(&[("permission", "manual"), ("work", "default"), ("model", "kimi-code/k3")]);
        let switched = plan(kimi, Some("kimi-code/k3"), None, &none(), &catalog, &[], &current, &stored).unwrap();
        assert_eq!(
            switched.restart,
            modes(&[("model", "kimi-code/k3"), ("permission", "manual"), ("work", "default")])
        );
        let args = meowo_agent::resolve_session_args(kimi, &switched.restart, &catalog, &[], true);
        assert!(!args.iter().any(|a| a == "--yolo" || a == "--plan"), "{args:?}");
        // 只换权限：当前模型照当前的拼，不回放存档里的旧模型（做空 M5）。
        let perm = plan(kimi, None, None, &modes(&[("permission", "auto")]), &catalog, &[], &current, &stored).unwrap();
        assert_eq!(perm.restart.get("model").map(String::as_str), Some("kimi-code/k3"));
    }

    /// 做空 H1 复现 c（Gemini）：存档 yolo、底栏读到当前是 default。读不到当前值、存档那一档又带参数时不重启。
    #[test]
    fn restart_refuses_when_current_value_is_unknown() {
        let gemini = meowo_agent::by_id("gemini").unwrap();
        let catalog = vec![ModelCatalogEntry::new("gemini-2.5-pro", "gemini-2.5-pro")];
        let stored = modes(&[("approval", "yolo")]);
        let known = plan(gemini, Some("gemini-2.5-pro"), None, &none(), &catalog, &[], &modes(&[("approval", "default")]), &stored).unwrap();
        assert_eq!(known.restart.get("approval").map(String::as_str), Some("default"));
        let args = meowo_agent::resolve_session_args(gemini, &known.restart, &catalog, &[], true);
        assert!(!args.iter().any(|a| a == "--yolo"), "{args:?}");
        let unknown = plan(gemini, Some("gemini-2.5-pro"), None, &none(), &catalog, &[], &none(), &stored);
        assert_eq!(unknown.unwrap_err(), "settings/current_unknown");
        // 存档里有模型、当前模型读不到：同样不重启（否则回放旧模型）。
        let model_unknown = plan(gemini, None, None, &modes(&[("approval", "plan")]), &catalog, &[], &none(), &modes(&[("model", "gemini-2.5-pro")]));
        assert_eq!(model_unknown.unwrap_err(), "settings/current_unknown");
    }

    /// 做空 H2：重启前 TUI 输入框必须是空的（前端先把草稿搬进对话页）；读不准 / 有字一律不重启。
    #[test]
    fn restart_guard_requires_an_empty_composer() {
        let empty = TuiStateArg { idle: true, composer_empty: true, approval_pending: false };
        let draft = TuiStateArg { idle: true, composer_empty: false, approval_pending: false };
        assert!(restart_guard(&empty).is_ok());
        assert_eq!(restart_guard(&draft), Err("settings/composer_not_empty"));
    }

    /// 做空 M8：服务通道的连接类错误在切换语境下不能落到 Lead「回答已保存…」。
    #[test]
    fn transport_errors_are_reworded_for_switching() {
        assert_eq!(settings_reason("lead/opencode_http_status_500"), "settings/server_unreachable");
        assert_eq!(settings_reason("lead/codex_server_timeout"), "settings/server_unreachable");
        assert_eq!(settings_reason("lead/opencode_choice_mismatch"), "lead/opencode_choice_mismatch");
        assert_eq!(settings_reason("settings/model_unknown"), "settings/model_unknown");
    }

    /// 做空 M3：排队按会话记，后选的合并进同一份（各栏逐个覆盖），取走后清空。
    #[test]
    fn queued_settings_merge_and_survive_until_taken() {
        let sid = 987_654;
        queue_session_settings(sid, serde_json::json!({ "model": { "id": "a", "label": "A" }, "modes": { "permission": "yolo" }, "label": "A" }));
        let merged = queue_session_settings(sid, serde_json::json!({ "modes": { "work": "plan" }, "label": "计划：开" }));
        assert_eq!(merged["model"]["id"], "a");
        assert_eq!(merged["modes"]["permission"], "yolo");
        assert_eq!(merged["modes"]["work"], "plan");
        assert_eq!(merged["label"], "计划：开");
        assert_eq!(queued_session_settings(sid), Some(merged.clone()));
        assert_eq!(take_queued_session_settings(sid), Some(merged));
        assert_eq!(queued_session_settings(sid), None);
    }

    /// 清单外的模型、未知的档一律拒绝（防注入）；OpenCode 的模型走对话框。
    #[test]
    fn unknown_values_never_reach_any_mechanism() {
        let gemini = meowo_agent::by_id("gemini").unwrap();
        let catalog = vec![ModelCatalogEntry::new("gemini-2.5-pro", "gemini-2.5-pro")];
        assert!(plan(gemini, Some("x; calc"), None, &none(), &catalog, &[], &none(), &none()).is_err());
        assert!(plan(gemini, None, None, &modes(&[("approval", "bogus")]), &catalog, &[], &none(), &none()).is_err());
        let ok = plan(gemini, Some("gemini-2.5-pro"), None, &modes(&[("approval", "plan")]), &catalog, &[], &none(), &none()).unwrap();
        assert_eq!(ok.restart, modes(&[("model", "gemini-2.5-pro"), ("approval", "plan")]));
        let opencode = meowo_agent::by_id("opencode").unwrap();
        let oc_catalog = vec![ModelCatalogEntry::new("fake/fake-model-2", "Fake Two")];
        let oc = plan(opencode, Some("fake/fake-model-2"), None, &modes(&[("agent", "plan")]), &oc_catalog, &[], &none(), &none()).unwrap();
        assert_eq!(oc.dialog_model.as_deref(), Some("fake/fake-model-2"));
        assert_eq!(oc.dialog_agent.as_deref(), Some("plan"));
        assert!(oc.restart.is_empty() && oc.server.is_empty());
        // 做空 L5：临时会话的改选同样过声明表。
        assert!(validate_modes(gemini, &modes(&[("approval", "bogus")]), &[]).is_err());
        assert!(validate_modes(gemini, &modes(&[("approval", "plan")]), &[]).is_ok());
    }
}
