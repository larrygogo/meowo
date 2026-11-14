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
    let mut args = meowo_agent::resolve_session_args(plugin, selections, &catalog, &dynamic, resuming);
    // 声明表表达不了、要按 CLI 当前配置现算的（Codex workspace-write 的档没配 Windows 沙箱时补沙箱实现，
    // 真机第四轮 N7）。新建与接回都加：接回时 thread 存下的 workspace-write 也要靠它才不被降成只读。
    args.extend(plugin.extra_launch_args(selections, env, cwd));
    args
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
            // 最终做空 P2：最后一条用户消息里的模型是切换前的，钉子在时以存档为准。
            let mut current = model.map(|model| HashMap::from([("model".to_string(), model)])).unwrap_or_default();
            pin_switched_model(&mut current, &stored_selections(&store, session_id));
            Some(LiveSettings {
                model: current.remove("model"),
                effort: choice.variant,
                modes: option
                    .zip(choice.agent)
                    .map(|(option, agent)| vec![mode(option, agent)])
                    .unwrap_or_default(),
            })
        }
        // 带参重启的两家没有会话服务：模式由前端从会话文件 / 底栏读；模型只在钉子还在时由宿主给出
        // （最终做空 P1），让打开菜单的同步不会拿旧回复的模型覆盖。
        meowo_agent::ModelSwitch::RestartResume { .. } => {
            let stored = stored_selections(&store, session_id);
            let reported = store.session_context(&header.cc_session_id).ok().and_then(|context| context.model);
            let pin_holds = stored.get(REPLY_RAW_AT_SWITCH).is_some_and(|raw| {
                reported.as_deref().is_none_or(|now| now.is_empty() || now == raw)
            });
            let pinned = stored.get("model").filter(|_| pin_holds).cloned();
            pinned.map(|model| LiveSettings { model: Some(model), effort: None, modes: Vec::new() })
        }
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
/// - `queued`：此刻不能动、要等一个前端看不到的条件，宿主收进队列自己执行（`wait_for`）：
///   `lead` = Lead 在等关卡答复，重启会杀掉会话内的等待器（做空 N3）；`open` = 会话没在运行，Codex /
///   OpenCode 恢复时不回放模型，打开后经服务 / 对话框补一次（做空 N6）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsOutcome {
    pub kind: &'static str,
    pub session_id: i64,
    pub confirmed: bool,
    pub reason: Option<String>,
    /// `queued` 时等的条件，调用方据此入队（不发给前端，前端从队列条目的 `waitFor` 读）。
    #[serde(skip)]
    pub wait_for: Option<&'static str>,
    /// 切完以后会话此刻的模型显示名（只在能当场读回时给）：OpenCode 只切 agent 时 CLI 会换成该 agent
    /// 记住的模型（真机第四轮问题 3），切完从底栏读回，界面立刻跟上，不等下一条消息。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl SettingsOutcome {
    fn new(kind: &'static str, session_id: i64) -> Self {
        Self {
            kind,
            session_id,
            confirmed: false,
            reason: None,
            wait_for: None,
            model: None,
        }
    }

    fn queued(session_id: i64, reason: &str, wait_for: &'static str) -> Self {
        Self {
            reason: Some(reason.into()),
            wait_for: Some(wait_for),
            ..Self::new("queued", session_id)
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

/// 存档里记的「切模型那一刻会话文件报的模型」（不是启动选项，resolve_launch_args 只认声明过的栏，
/// 不会把它拼进命令行）。清单 id 口径，给 apply / live 比较用。
pub(crate) const REPLY_MODEL_AT_SWITCH: &str = "_replyModelAtSwitch";
/// 同上，原样口径（会话上下文 session_context.model 报的字符串），给对话页历史快照比较用。
pub(crate) const REPLY_RAW_AT_SWITCH: &str = "_replyRawAtSwitch";

/// 切模型成功后记下钉子（最终做空 P1/P2）：存档的模型改成新值，并记下切换那一刻会话报的模型（两种口径）。
/// 之后会话报的仍是它（或报不出），就说明还没有新回复，宿主一律以存档为准。
fn pin_model(store: &meowo_store::Store, session_id: i64, model: &str, reply_id: Option<&str>, reply_raw: Option<&str>) {
    let mut pin = HashMap::from([("model".to_string(), model.to_string())]);
    pin.insert(REPLY_MODEL_AT_SWITCH.to_string(), reply_id.unwrap_or_default().to_string());
    pin.insert(REPLY_RAW_AT_SWITCH.to_string(), reply_raw.unwrap_or_default().to_string());
    persist(store, session_id, &pin, None);
}

/// 对话页同步写回存档的模型要不要收（最终做空 P1）：钉子在、传来的是切换那一刻会话报的旧模型时不收。
pub(crate) fn record_model_allowed(stored: &HashMap<String, String>, model: &str) -> bool {
    stored.get(REPLY_MODEL_AT_SWITCH).is_none_or(|old| old.is_empty() || old != model)
}

/// 有了新回复（会话报的模型与切换那一刻不同）：钉子作废，删掉两个标记。
fn clear_pin(store: &meowo_store::Store, session_id: i64) {
    let mut selections = stored_selections(store, session_id);
    let had = selections.remove(REPLY_MODEL_AT_SWITCH).is_some() | selections.remove(REPLY_RAW_AT_SWITCH).is_some();
    if had {
        if let Ok(json) = serde_json::to_string(&selections) {
            let _ = store.set_session_launch_args(session_id, &json);
        }
    }
}

/// 对话页历史快照里的「当前模型」（chat.rs 调）：钉子在、会话报的仍是切换那一刻的（或报不出）时返回存档
/// 的模型；报出别的（有了新回复）时清掉钉子、照会话报的。
pub(crate) fn pinned_history_model(store: &meowo_store::Store, session_id: i64, reported: Option<String>) -> Option<String> {
    let stored = stored_selections(store, session_id);
    let (Some(model), Some(raw)) = (stored.get("model"), stored.get(REPLY_RAW_AT_SWITCH)) else {
        return reported;
    };
    match reported {
        Some(now) if !now.is_empty() && now != *raw => {
            clear_pin(store, session_id);
            Some(now)
        }
        _ => Some(model.clone()),
    }
}

/// 刚切成功的模型优先（真机第四轮问题 2）：存档有模型、且会话文件报的模型仍是切换那一刻的（或报不出），
/// 说明还没有新回复，当前模型以存档为准；报出别的模型（有了新回复）才以回复为准。
pub(crate) fn pin_switched_model(current: &mut HashMap<String, String>, stored: &HashMap<String, String>) {
    let (Some(model), Some(at_switch)) = (stored.get("model"), stored.get(REPLY_MODEL_AT_SWITCH)) else {
        return;
    };
    if current.get("model").is_none_or(|reply| reply == at_switch) {
        current.insert("model".into(), model.clone());
    }
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
        let modes = modes.unwrap_or_default();
        let request = request_json(model.as_deref(), effort.as_deref(), &modes);
        let outcome = apply_blocking(
            &app,
            &broker,
            &db,
            session_id,
            model,
            effort,
            modes,
            current.unwrap_or_default(),
            tui.unwrap_or_default(),
            None,
        )
        .map_err(|reason| settings_reason(&reason))?;
        // 宿主要等的条件前端看不到：收进队列，前端随后用同一请求合并进来补上提示名。
        if let Some(wait) = outcome.wait_for {
            enqueue_waiting(session_id, request, wait);
            ensure_executor(app.clone());
        }
        Ok(outcome)
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
        "lead/opencode_choice_reverted",
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

/// `before_stop`：带参重启时 broker.stop 前一刻的复核（宿主执行排队时传，见 [`run_queued`]；前端路线是
/// 用户当场触发、草稿已搬走，不传）。
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
    before_stop: Option<&dyn Fn() -> Result<(), String>>,
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
        // 重开前排进旧临时 id 的选择跟到新 id（做空 N5：否则谁都不会再取它）。
        migrate_queued(session_id, new_id);
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
    // 会话文件读到的「当前模型」是最近一条回复的；切完模型还没有新回复时它仍是旧的（真机第四轮问题 2）。
    let reply_model = current.get("model").cloned();
    // 会话上下文报的原样模型（对话页历史快照用的那个口径），切换成功后记进钉子。
    let reported_raw = store
        .session_context(&header.cc_session_id)
        .ok()
        .and_then(|context| context.model);
    pin_switched_model(&mut current, &stored);
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
    // 不报成功（做空 M4）。
    if !broker.is_active(session_id) {
        let mut next = modes.clone();
        if let Some(model) = &model {
            next.insert("model".into(), model.clone());
        }
        persist(&store, session_id, &next, None);
        // 带参重启的两家（Kimi/Gemini）恢复时按存档回放，「下次打开时生效」属实。会话服务（Codex）与
        // 对话框（OpenCode）恢复时不回放模型、Codex 协作模式也没有启动参数（做空 N6）：排进队列，
        // 打开后由宿主经服务 / 对话框补一次。
        if !plan.server.is_empty() || plan.dialog_model.is_some() || plan.dialog_agent.is_some() {
            return Ok(SettingsOutcome::queued(session_id, "settings/applies_on_open", "open"));
        }
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
        // 宿主执行时，往 TUI 输入框打字之前重新看一次屏（做空 R9）：从取屏到这里要读清单，用户可能
        // 刚打了字，对话框路线「输入框原本是空的」前提就不成立了。
        if let Some(check) = before_stop {
            check()?;
        }
        match dialog_switch(app, broker, db, session_id, &header, &catalog, &plan, tui) {
            Ok(confirmed) => {
                outcome.confirmed = confirmed;
                // 只切 agent：OpenCode 会换成该 agent 记住的模型（1.19.5 上游行为，TUI 里按 Tab 也一样）。
                // 当场从底栏读回，界面立刻显示实际模型（真机第四轮问题 3）。
                if plan.dialog_model.is_none() {
                    let catalog = if catalog.is_empty() {
                        session_catalog(db, plugin, Some(session_id), &env, header.cwd.as_deref(), Some(app))
                    } else {
                        catalog.clone()
                    };
                    let lines = broker
                        .screen_probe_snapshot(session_id)
                        .map(|(snapshot, _)| snapshot.lines)
                        .unwrap_or_default();
                    if let Some(entry) = footer_model(&lines, &catalog) {
                        outcome.model = Some(entry.label.clone());
                        if Some(&entry.id) != reply_model.as_ref() {
                            pin_model(&store, session_id, &entry.id, reply_model.as_deref(), reported_raw.as_deref());
                        }
                    }
                } else if let Some(model) = plan.dialog_model.as_deref() {
                    pin_model(&store, session_id, model, reply_model.as_deref(), reported_raw.as_deref());
                }
            }
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
        // Lead 在等关卡答复：Kimi/Gemini 的等待器是 TUI 里的后台进程，重启连它一起杀掉，答复后就不会
        // 自动继续（宿主不代投）。排到续跑之后由宿主执行（做空 N3）。
        if lead_holds_restart(&header) {
            return Ok(SettingsOutcome::queued(session_id, "settings/lead_waiting", "lead"));
        }
        if !restart(app, broker, &store, session_id, &header.provider, plan.restart.clone(), before_stop)? {
            // 恰在此刻自己退出了：同上，记成下次打开的启动参数。
            persist(&store, session_id, &plan.restart, None);
            return Ok(SettingsOutcome::new("next_open", session_id));
        }
        outcome.kind = "restarted";
        // 记下这次带参重启时会话报的模型：之后报的仍是它，就说明还没有新回复，当前模型以存档为准。
        if let Some(model) = plan.restart.get("model").filter(|model| Some(*model) != reply_model.as_ref()) {
            pin_model(&store, session_id, model, reply_model.as_deref(), reported_raw.as_deref());
        }
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
        // 最终做空 P1：对话页回合结束同步时，会话报的若仍是切换那一刻的旧模型（钉子在），不写回存档。
        let stored = stored_selections(&store, session_id);
        let model = model.filter(|model| record_model_allowed(&stored, model));
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

/// 新建时选的、会话服务机制各栏的服务端片段合并成一份（只取带 server 片段的档）。没有就 None。
pub(crate) fn launch_server_update(
    plugin: &dyn AgentPlugin,
    selections: &HashMap<String, String>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut merged = serde_json::Map::new();
    for option in plugin.launch_options() {
        if option.switch != meowo_agent::ModeSwitch::Server {
            continue;
        }
        let patch = selections
            .get(option.id)
            .and_then(|id| option.choices.iter().find(|c| c.id == id.as_str()))
            .and_then(|c| c.server)
            .and_then(|patch| serde_json::from_str::<serde_json::Value>(patch).ok());
        if let Some(serde_json::Value::Object(map)) = patch {
            merged.extend(map);
        }
    }
    (!merged.is_empty()).then_some(merged)
}

/// 认领后要不要连会话服务补设：只有选了「自动审查」（Codex approveForMe）才补（leader 2026-11-14 定，
/// 做空 N7）。补设要连 app-server，UA 末尾客户端名会变成 meowo；其他档的启动参数本身就生效，不为它们连入。
/// 自动审查是唯一「启动参数在 0.167.2 Windows 上首轮不生效」的档（真机验证 codex 问题 1）。
pub(crate) fn claim_server_update(
    plugin: &dyn AgentPlugin,
    selections: &HashMap<String, String>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    if selections.get("approval").map(String::as_str) != Some("approveForMe") {
        return None;
    }
    launch_server_update(plugin, selections)
}

/// 会话认领后按新建时选的档经会话服务补设一次（真机验证 codex 问题 1：0.167.2 Windows 上新建带
/// `--sandbox workspace-write` 首轮仍是 read-only，只有 thread/settings/update 设得上）。会话服务机制
/// 才做；TUI 还没握手时隔 2 秒再试，最多 5 次；够不着服务（内嵌模式）就算了，启动参数已经给过。
pub(crate) fn apply_launch_server_settings(
    db: std::path::PathBuf,
    session_id: i64,
    native: String,
    provider: String,
    selections: HashMap<String, String>,
) {
    let Some(plugin) = meowo_agent::resolve(Some(&provider)) else {
        return;
    };
    if plugin.model_switch() != Some(meowo_agent::ModelSwitch::ServerSettings) {
        return;
    }
    let Some(patch) = claim_server_update(plugin, &selections) else {
        return;
    };
    let Ok(update) = serde_json::from_value::<crate::lead_native::codex_app_server::CodexSettingsUpdate>(
        serde_json::Value::Object(patch),
    ) else {
        return;
    };
    std::thread::spawn(move || {
        use crate::lead_native::codex_app_server::{update_settings, ControlError};
        for _ in 0..5 {
            match update_settings(&db, session_id, &native, &update) {
                Err(ControlError::NotReady(_)) => std::thread::sleep(std::time::Duration::from_secs(2)),
                Err(ControlError::Failed(reason)) => {
                    eprintln!("新建会话补设权限失败（{session_id}）：{reason}");
                    return;
                }
                _ => return,
            }
        }
    });
}

/// 「这一轮结束后切换」的排队（做空 M3）：存在宿主进程里、按会话 id 记，换会话、刷新手机页都还在；
/// 后来的选择合并进同一份。内容是前端的请求原样（模型、各栏、提示名），执行仍走 apply_session_settings
/// 的全部校验，这里不解释它。宿主收进来的条目另带 `waitFor`（见 [`SettingsOutcome`] 的 `queued`）。
struct Queued {
    request: serde_json::Value,
    /// 这份选择最近一次被（重新）选的时刻：等 Lead / 等打开的条目超过 [`HOLD_CAP`] 出队（做空 R7）。
    created: std::time::Instant,
    /// 等待条件最近一次仍成立的时刻（Lead 关卡解除后观望 [`LEAD_GRACE`] 用）。
    since: std::time::Instant,
    /// 「会话空闲、却仍切不了」（输入框有字 / 认不出、服务没就绪、被复核拦下）从何时起。会话在跑、在等
    /// Lead、没打开时清空、不计时（做空 R4：一轮跑多久都不该把排着的切换取消）。超过 [`QUEUE_TTL`] 出队。
    blocked: Option<std::time::Instant>,
    /// 临时 id 的启动从何时起不见了（重开时有几秒空档，做空 R6），超过 [`GONE_GRACE`] 才算没了。
    gone: Option<std::time::Instant>,
    /// 宿主**连续**几次认不出输入框，认出一次（不论空不空）就清零（做空 R8）。到 [`COMPOSER_MISS_LIMIT`]
    /// 就不再试，留给对话页（会先搬草稿）执行。
    misses: u32,
}

impl Queued {
    fn new(request: serde_json::Value) -> Self {
        let now = std::time::Instant::now();
        Self {
            request,
            created: now,
            since: now,
            blocked: None,
            gone: None,
            misses: 0,
        }
    }
}

/// 「空闲却切不了」最多耗多久（见 [`Queued::blocked`]）。
const QUEUE_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// 等会话打开的条目最多留多久：再也不打开的会话，条目不能永远留着（做空 R7）。等 Lead 关卡的不限时
/// （关卡可能隔很久才答，第四轮 L1）。
const HOLD_CAP: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
/// 临时 id 的启动不见了多久才算真没了（重开要先停旧进程、等收尾、再起新的，见 reopen_pending_with）。
const GONE_GRACE: std::time::Duration = std::time::Duration::from_secs(20);
/// Lead 关卡等待解除后再观望多久才动（见 [`run_queued`]）。
const LEAD_GRACE: std::time::Duration = std::time::Duration::from_secs(15);
/// 宿主连续认不出输入框的次数上限（执行线程 1.5 秒一轮，约 30 秒）。
const COMPOSER_MISS_LIMIT: u32 = 20;

static QUEUED: std::sync::LazyLock<std::sync::Mutex<HashMap<i64, Queued>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

fn queue() -> std::sync::MutexGuard<'static, HashMap<i64, Queued>> {
    QUEUED.lock().unwrap_or_else(|e| e.into_inner())
}

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
            } else if key == "terminal" {
                // 终端项（前端 TerminalAction）：同一件事（同一栏模式 / 模型 / 思考档）后选的替换先选的。
                let item_key = |item: &serde_json::Value| match item["kind"].as_str() {
                    Some("mode") => format!("mode:{}", item["dimension"].as_str().unwrap_or_default()),
                    other => other.unwrap_or_default().to_string(),
                };
                let incoming: Vec<serde_json::Value> = value.as_array().cloned().unwrap_or_default();
                let keys: Vec<String> = incoming.iter().map(item_key).collect();
                let mut items: Vec<serde_json::Value> = target
                    .get("terminal")
                    .and_then(|old| old.as_array().cloned())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|item| !keys.contains(&item_key(item)))
                    .collect();
                items.extend(incoming);
                target.insert(key.clone(), serde_json::Value::Array(items));
            } else if !value.is_null() {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

/// apply 的参数写成与前端请求同形的 JSON（宿主自己收进队列时用；提示名由前端随后合并进来）。
fn request_json(model: Option<&str>, effort: Option<&str>, modes: &HashMap<String, String>) -> serde_json::Value {
    let mut request = serde_json::json!({ "modes": modes });
    if let Some(model) = model {
        request["model"] = serde_json::json!({ "id": model, "effort": effort });
    }
    request
}

fn wait_for(request: &serde_json::Value) -> Option<&str> {
    request["waitFor"].as_str()
}

#[tauri::command]
pub(crate) fn queue_session_settings(
    app: tauri::AppHandle,
    session_id: i64,
    request: serde_json::Value,
) -> serde_json::Value {
    let merged = enqueue(session_id, &request);
    ensure_executor(app);
    merged
}

/// 合并进会话的队列，返回合并后的整份。新的选择重新起算等待时间。
pub(crate) fn enqueue(session_id: i64, request: &serde_json::Value) -> serde_json::Value {
    let mut map = queue();
    let merged = merge_queued(map.get(&session_id).map(|q| &q.request), request);
    map.insert(session_id, Queued::new(merged.clone()));
    merged
}

/// 宿主收进队列、标上等待条件（见 [`SettingsOutcome`] 的 `queued`）。
fn enqueue_waiting(session_id: i64, mut request: serde_json::Value, wait: &str) {
    request["waitFor"] = wait.into();
    enqueue(session_id, &request);
}

/// 临时 id 换了（重开未建会话、认领成真实 id）：条目跟过去，与新 id 上已有的合并（做空 N5）。
pub(crate) fn migrate_queued(from: i64, to: i64) {
    let mut map = queue();
    let Some(entry) = map.remove(&from) else {
        return;
    };
    let request = match map.get(&to) {
        Some(newer) => merge_queued(Some(&entry.request), &newer.request),
        None => entry.request,
    };
    map.insert(to, Queued { request, ..entry });
}

/// 取走执行后又放回（此刻还不能动）：期间前端又排进来的，以新的为准合并。
fn put_back(session_id: i64, entry: Queued) {
    let mut map = queue();
    let request = match map.get(&session_id) {
        Some(newer) => merge_queued(Some(&entry.request), &newer.request),
        None => entry.request,
    };
    map.insert(session_id, Queued { request, ..entry });
}

fn emit_settings(app: &tauri::AppHandle, session_id: i64, kind: &str, reason: Option<String>) {
    use tauri::Emitter as _;
    let _ = app.emit(
        "session-settings-applied",
        serde_json::json!({ "sessionId": session_id, "kind": kind, "reason": reason }),
    );
}

/// 出队并告诉前端为什么（`expired` / `session_gone`），不让选择悄悄消失（做空 N5）。超时取消另发一条
/// 灵动岛提醒：对话页不在这个会话上时也看得到（做空 R4）。
fn drop_queued(app: &tauri::AppHandle, session_id: i64, kind: &str) {
    let Some(entry) = queue().remove(&session_id) else {
        return;
    };
    emit_settings(app, session_id, kind, None);
    // 超时、会话归档 / 删除都提醒（未认领的临时会话是用户自己关掉的，不提醒）。
    if kind == "expired" || (kind == "session_gone" && session_id > 0) {
        let lang = crate::settings::ui_lang(&crate::settings::load_settings());
        let name = entry.request["label"].as_str().unwrap_or_default();
        let store = crate::open_store(&crate::db_path()).ok();
        let title = store
            .as_ref()
            .and_then(|store| store.session_title(session_id).ok().flatten())
            .unwrap_or_default();
        let project = store
            .as_ref()
            .and_then(|store| store.session_cwd(session_id).ok().flatten())
            .and_then(|cwd| std::path::Path::new(&cwd).file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let label = if kind == "expired" { expired_label(lang, name) } else { gone_label(lang, name) };
        crate::watch::emit_island_alert(app, session_id, "error", &title, &project, Some(label));
    }
}

/// 超时取消的提醒文案（与前端 chat.settingsQueueExpired 同义，带上要切的名字）。
pub(crate) fn expired_label(lang: &str, name: &str) -> String {
    match (lang == "en", name.is_empty()) {
        (true, true) => "The queued switch waited too long and was canceled".into(),
        (true, false) => format!("Switching to \"{name}\" waited too long and was canceled"),
        (false, true) => "排着的切换等太久，已取消".into(),
        (false, false) => format!("切换到「{name}」等太久，已取消"),
    }
}

/// 会话归档 / 删除时取消的提醒文案。
pub(crate) fn gone_label(lang: &str, name: &str) -> String {
    match (lang == "en", name.is_empty()) {
        (true, true) => "The session was archived or removed, so the queued switch was canceled".into(),
        (true, false) => format!("The session was archived or removed, so switching to \"{name}\" was canceled"),
        (false, true) => "会话已归档或删除，排着的切换已取消".into(),
        (false, false) => format!("会话已归档或删除，切换到「{name}」已取消"),
    }
}

/// 改条目上的计时状态（条目可能刚被前端取走，不在了就算了）。
fn with_queued(session_id: i64, update: impl FnOnce(&mut Queued)) {
    if let Some(entry) = queue().get_mut(&session_id) {
        update(entry);
    }
}

/// 会话在跑 / 等待条件仍成立：不算「空闲却切不了」。
fn not_blocked(session_id: i64) {
    with_queued(session_id, |entry| entry.blocked = None);
}

/// 会话空闲却切不了：开始（或继续）计时。
fn mark_blocked(session_id: i64) {
    with_queued(session_id, |entry| {
        entry.blocked.get_or_insert_with(std::time::Instant::now);
    });
}

static EXECUTOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 排队的切换由宿主在回合结束时自己执行（复核 M3：不依赖对话页正停在这个会话上）。只在队列非空时
/// 跑，每 1.5 秒看一眼排着的几个会话，队列空了线程就退出；条目超时、会话没了会出队（做空 N5），所以
/// 线程总能退出。会话空闲的判定见 [`host_idle`]；带参重启与对话框路线还要求 TUI 输入框是空的——宿主没法
/// 把草稿搬进对话页，输入框有字时留在队列里，等对话页打开这个会话时由前端（会先搬草稿）执行。
fn ensure_executor(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if EXECUTOR.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let ids: Vec<i64> = queue().keys().copied().collect();
            if ids.is_empty() {
                EXECUTOR.store(false, Ordering::Release);
                // 释放标记后若又有人排进来，接着跑。
                if queue().is_empty() || EXECUTOR.swap(true, Ordering::AcqRel) {
                    return;
                }
                continue;
            }
            for sid in ids {
                run_queued(&app, sid);
            }
        }
    });
}

/// 这次请求要不要求 TUI 输入框是空的：带参重启会杀进程（草稿跟着没），对话框路线要往输入框里打字。
/// 会话服务（Codex）不碰输入框。
pub(crate) fn needs_empty_composer(
    plugin: &dyn AgentPlugin,
    has_model: bool,
    modes: &HashMap<String, String>,
) -> bool {
    let model = has_model
        && matches!(
            plugin.model_switch(),
            Some(meowo_agent::ModelSwitch::RestartResume { .. } | meowo_agent::ModelSwitch::TuiDialog)
        );
    model
        || modes.keys().any(|id| {
            plugin.launch_options().iter().any(|o| {
                o.id == id
                    && matches!(o.switch, meowo_agent::ModeSwitch::Restart | meowo_agent::ModeSwitch::Dialog)
            })
        })
}

/// 宿主眼里会话此刻空闲、可以动：主回合停了、后台子任务不忙、没有待批审批。屏幕上挂着审批 / 提问
/// （screen_state = blocked）直接算不空闲（做空 N10：tab_class 把 blocked 也归为 waiting，原先只靠
/// 认不出输入框碰巧挡住）。
pub(crate) fn screen_allows_switch(
    connected: bool,
    status: &str,
    pending_review: bool,
    screen: Option<&str>,
    background_busy: impl FnOnce() -> bool,
) -> bool {
    if pending_review || screen == Some("blocked") {
        return false;
    }
    crate::session_query::tab_class(connected, status, None, screen, background_busy) == Some("waiting")
}

fn host_idle(state: &crate::AppState, history: &meowo_protocol::ipc::ChatHistoryDto) -> bool {
    let screen = if history.screen_assumed {
        None
    } else {
        history.screen_state.as_deref()
    };
    screen_allows_switch(
        history.connected,
        &history.status,
        history.pending_review.is_some(),
        screen,
        || {
            crate::session_query::subagents_busy(
                &state.tx_cache,
                &history.provider,
                history.cwd.as_deref(),
                &history.cc_session_id,
            )
        },
    )
}

/// Lead 在这个会话里等关卡答复（会话内等待器活着 / 关卡挂起 / 答复还没交给模型），见
/// meowo_reporter::lead_waiter::holds_restart。
fn lead_holds_restart(header: &meowo_store::store::SessionHeader) -> bool {
    let Some(registry) = crate::lead::lead_registry() else {
        return false;
    };
    meowo_reporter::lead::session_task(header.cwd.as_deref(), &header.cc_session_id, &registry)
        .is_some_and(|task| meowo_reporter::lead_waiter::holds_restart(&task, &header.cc_session_id))
}

/// 执行结果里「此刻还不能动、稍后再试」的那些：放回队列，不打扰前端。
const RETRY_LATER: &[&str] = &[
    "settings/composer_not_empty",
    "settings/server_not_ready",
    "settings/busy_now",
    "settings/background_busy",
    "settings/session_gone",
];

fn run_queued(app: &tauri::AppHandle, session_id: i64) {
    use tauri::Manager as _;
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    let Some((request, created, since, blocked, gone, misses)) = queue()
        .get(&session_id)
        .map(|q| (q.request.clone(), q.created, q.since, q.blocked, q.gone, q.misses))
    else {
        return;
    };
    let expired = blocked.is_some_and(|at| at.elapsed() > QUEUE_TTL);
    if session_id < 0 {
        // 未认领会话由前端处理（重开要切到新的临时 id）。启动不见了一小段（重开的空档）不算没了，
        // 持续不见才出队（做空 R6）。
        if state.ptys.pending_launch(session_id).is_some() {
            with_queued(session_id, |entry| {
                entry.gone = None;
                entry.blocked.get_or_insert_with(std::time::Instant::now);
            });
            if expired {
                drop_queued(app, session_id, "expired");
            }
        } else if gone.is_some_and(|at| at.elapsed() > GONE_GRACE) {
            drop_queued(app, session_id, "session_gone");
        } else if gone.is_none() {
            with_queued(session_id, |entry| entry.gone = Some(std::time::Instant::now()));
        }
        return;
    }
    // 终端项（Kimi `/plan on`、Claude `/model` 等要往终端里打字的切换）由对话页在回合结束时执行，宿主不往
    // 终端打字（lead_native/waiter.rs 同一约束）。混合的条目宿主先做能经后端做的部分（模型 / 会话服务 /
    // 带参重启），终端项留在队列里，提示仍是同一句（leader 2026-11-14）。
    let (request, leftover) = split_terminal(&request);
    let Some(request) = request else {
        // 只含终端项的条目宿主不执行，但会话归档 / 删除时照样出队并提醒（最终做空 P5），队列空了线程退出。
        if let Ok(store) = crate::open_store(&state.db_path) {
            let gone = matches!(store.session_provider_opt(session_id), Ok(None))
                || store.session_header(session_id).is_ok_and(|header| header.archived);
            if gone {
                drop_queued(app, session_id, "session_gone");
            }
        }
        return;
    };
    let wait = wait_for(&request).map(str::to_owned);
    // 等打开：只看 PTY 在不在（不开库），一直不打开的到上限出队并提醒（做空 R7、第四轮 L1）。
    if wait.as_deref() == Some("open") && !state.ptys.is_active(session_id) {
        if created.elapsed() > HOLD_CAP {
            drop_queued(app, session_id, "expired");
        } else {
            not_blocked(session_id);
        }
        return;
    }
    let Ok(store) = crate::open_store(&state.db_path) else {
        return;
    };
    let Ok(header) = store.session_header(session_id) else {
        // 会话行没了：出队；读库出错只是这一轮跳过。
        if matches!(store.session_provider_opt(session_id), Ok(None)) {
            drop_queued(app, session_id, "session_gone");
        }
        return;
    };
    // 归档了的会话不会再被打开、也不会再有回合结束（做空 R7）。
    if header.archived {
        drop_queued(app, session_id, "session_gone");
        return;
    }
    if wait.as_deref() == Some("lead") {
        // 关卡可能隔很久才答（product.md），等 Lead 的条目不限时；只在会话归档 / 删除（上面）或 Meowo
        // 重启（队列在内存里）时取消（第四轮 L1）。
        if lead_holds_restart(&header) {
            with_queued(session_id, |entry| {
                entry.since = std::time::Instant::now();
                entry.blocked = None;
            });
            return;
        }
        // 等待器交出答复就退出，CLI 要过一会儿才在空闲时开新回合：解除后再观望一段，免得赶在
        // 续跑那一轮开始前把它杀掉。之后变回普通的「空闲后切换」（前端也能取走，后端会再查一次 Lead）。
        if since.elapsed() < LEAD_GRACE {
            return;
        }
        with_queued(session_id, |entry| {
            if let Some(map) = entry.request.as_object_mut() {
                map.remove("waitFor");
            }
        });
        emit_settings(app, session_id, "queued", None);
        return;
    }
    if expired {
        drop_queued(app, session_id, "expired");
        return;
    }
    if misses >= COMPOSER_MISS_LIMIT {
        // 宿主连续认不出输入框，不再每轮读历史。等打开 / 等就绪的条目前端取不走，这时去掉等待条件交给
        // 对话页（会先搬草稿再执行）；计时照走，到期出队。
        if wait.is_some() {
            with_queued(session_id, |entry| {
                if let Some(map) = entry.request.as_object_mut() {
                    map.remove("waitFor");
                }
            });
            emit_settings(app, session_id, "queued", None);
        }
        mark_blocked(session_id);
        return;
    }
    let Some(plugin) = meowo_agent::resolve(Some(&header.provider)) else {
        return;
    };
    // 版本探测缓存失效时要起 `<cli> --version`（最长 5 秒）：放在看屏之前，免得拿旧画面做决定（做空 N4）。
    let version = crate::probe_cli_version(plugin);
    let Some(history) = crate::chat::session_snapshot(&state, session_id) else {
        return; // 会话行在（上面查过），读不出来只是这一轮跳过
    };
    // 连着且在跑 / 等审批：继续等，不计时。没连着：执行（Kimi/Gemini 记成下次打开的启动参数，
    // Codex/OpenCode 转成等打开，见 apply_blocking）。
    if history.connected && !host_idle(&state, &history) {
        not_blocked(session_id);
        return;
    }
    let model = request["model"]["id"].as_str().map(str::to_owned);
    let effort = request["model"]["effort"].as_str().map(str::to_owned);
    let modes: HashMap<String, String> = request["modes"]
        .as_object()
        .map(|modes| {
            modes
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    // 当前实际值：transcript 的模式 + 底栏标记 + 当前模型在清单里的 id（与前端 readCurrent 同口径）。
    let lines = state
        .ptys
        .screen_probe_snapshot(session_id)
        .map(|(snapshot, _)| snapshot.lines)
        .unwrap_or_default();
    let mut current: HashMap<String, String> = history
        .agent_modes
        .iter()
        .map(|mode| (mode.dimension.clone(), mode.value.clone()))
        .collect();
    for control in plugin.mode_controls() {
        if let Some(value) = mode_from_screen(&lines.join("\n"), control.screen_markers) {
            current.insert(control.dimension.to_string(), value.to_string());
        }
    }
    if let Some(label) = history.model.as_deref() {
        let env = session_env(&history.provider, session_id);
        let catalog = session_catalog(&state.db_path, plugin, Some(session_id), &env, history.cwd.as_deref(), Some(app));
        if let Some(id) = catalog_id_for(&catalog, label) {
            current.insert("model".into(), id);
        }
    }
    let guard = plugin.composer_guard(version.as_deref());
    let composer = guard.as_ref().and_then(|guard| composer_state(&lines, guard));
    if history.connected && needs_empty_composer(plugin, model.is_some(), &modes) {
        match composer {
            Some(true) => with_queued(session_id, |entry| entry.misses = 0),
            // 有字：等用户发出去，或对话页打开时由前端先搬草稿再执行。认出来了，连续认不出的计数清零。
            Some(false) => {
                with_queued(session_id, |entry| {
                    entry.misses = 0;
                    entry.blocked.get_or_insert_with(std::time::Instant::now);
                });
                return;
            }
            None => {
                with_queued(session_id, |entry| {
                    entry.misses += 1;
                    entry.blocked.get_or_insert_with(std::time::Instant::now);
                });
                return;
            }
        }
    }
    // 取走再执行：前端与宿主谁先拿到谁执行，不会执行两次。
    let Some(taken) = queue().remove(&session_id) else {
        return;
    };
    let tui = TuiStateArg {
        idle: true,
        composer_empty: composer == Some(true),
        approval_pending: false,
    };
    // 动手前一刻复核（带参重启在 broker.stop 前、对话框路线在打字前）：重新取屏和状态，仍空闲、无审批、
    // 输入框仍空才动（做空 N4 / R9）。
    let recheck = || -> Result<(), String> {
        let history = crate::chat::session_snapshot(&state, session_id).ok_or("settings/session_gone")?;
        if !history.connected || !host_idle(&state, &history) {
            return Err("settings/busy_now".into());
        }
        let lines = state
            .ptys
            .screen_probe_snapshot(session_id)
            .map(|(snapshot, _)| snapshot.lines)
            .unwrap_or_default();
        match guard.as_ref().and_then(|guard| composer_state(&lines, guard)) {
            Some(true) => Ok(()),
            _ => Err("settings/composer_not_empty".into()),
        }
    };
    let result = apply_blocking(
        app,
        &state.ptys,
        &state.db_path,
        session_id,
        model,
        effort,
        modes,
        current,
        tui,
        Some(&recheck),
    );
    // 放回时保留计时状态；「空闲却切不了」从这次起算（若还没在计）。
    let retry = |mut entry: Queued| {
        entry.blocked.get_or_insert_with(std::time::Instant::now);
        put_back(session_id, entry);
    };
    match result {
        Ok(outcome) if outcome.wait_for.is_some() => {
            let mut entry = taken;
            entry.request["waitFor"] = outcome.wait_for.unwrap_or_default().into();
            entry.since = std::time::Instant::now();
            entry.created = std::time::Instant::now();
            entry.blocked = None;
            entry.misses = 0;
            put_back(session_id, entry);
            emit_settings(app, session_id, "queued", outcome.reason);
        }
        // 服务端复核说此刻不能动（Codex 服务没握手、OpenCode TUI 没就绪 / 在忙 / 输入框有字）：放回，
        // 等打开的条目保留 waitFor，前端取不走（做空 R5）。
        Ok(outcome) if outcome.kind == "not_now" => retry(taken),
        Err(reason) if RETRY_LATER.contains(&reason.as_str()) => retry(taken),
        Ok(outcome) => {
            emit_settings(app, session_id, outcome.kind, outcome.reason);
            requeue_terminal(app, session_id, leftover);
        }
        Err(reason) => {
            emit_settings(app, session_id, "failed", Some(settings_reason(&reason)));
            requeue_terminal(app, session_id, leftover);
        }
    }
}

/// 拆开混合条目：(宿主能做的部分, 留给对话页的终端项)。没有终端项时原样返回、不留；只有终端项时宿主
/// 那份是 None。留下的那份带原来的提示名，界面上仍是同一句「这一轮结束后切换到「X」」。
pub(crate) fn split_terminal(request: &serde_json::Value) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let Some(items) = request["terminal"].as_array().filter(|items| !items.is_empty()) else {
        return (Some(request.clone()), None);
    };
    let terminal_dims: Vec<&str> = items
        .iter()
        .filter(|item| item["kind"] == "mode")
        .filter_map(|item| item["dimension"].as_str())
        .collect();
    let mut backend_modes = serde_json::Map::new();
    let mut terminal_modes = serde_json::Map::new();
    for (dimension, value) in request["modes"].as_object().into_iter().flatten() {
        if terminal_dims.contains(&dimension.as_str()) {
            terminal_modes.insert(dimension.clone(), value.clone());
        } else {
            backend_modes.insert(dimension.clone(), value.clone());
        }
    }
    let mut leftover = serde_json::json!({ "label": request["label"].clone(), "terminal": items.clone() });
    if !terminal_modes.is_empty() {
        leftover["modes"] = serde_json::Value::Object(terminal_modes);
    }
    let has_model = request["model"]["id"].is_string();
    if !has_model && backend_modes.is_empty() {
        return (None, Some(leftover));
    }
    let mut backend = request.clone();
    if let Some(map) = backend.as_object_mut() {
        map.remove("terminal");
        map.insert("modes".into(), serde_json::Value::Object(backend_modes));
    }
    (Some(backend), Some(leftover))
}

/// 宿主做完能做的部分后，把终端项放回队列（与期间前端新排的合并），并让对话页刷新同一句提示。
fn requeue_terminal(app: &tauri::AppHandle, session_id: i64, leftover: Option<serde_json::Value>) {
    let Some(leftover) = leftover else {
        return;
    };
    put_back(session_id, Queued::new(leftover));
    emit_settings(app, session_id, "queued", None);
}

/// 底栏模式标记（前端 modeFromScreen 的宿主版）：小写子串，取画面里最靠后出现的那个。
fn mode_from_screen(text: &str, markers: &[meowo_agent::ModeScreenMarker]) -> Option<&'static str> {
    let haystack = text.to_lowercase().replace('’', "'");
    markers
        .iter()
        .filter_map(|marker| haystack.rfind(&marker.marker.to_lowercase()).map(|at| (at, marker.value)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, value)| value)
}

/// 会话报出的当前模型（显示名或 id）→ 清单 id（前端 currentPresetId 的保守版：只认精确的 id、显示名、
/// 或「斜杠后那段」，认不出就当读不到）。
fn catalog_id_for(catalog: &[ModelCatalogEntry], current: &str) -> Option<String> {
    let norm = |text: &str| {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let wanted = norm(current);
    catalog
        .iter()
        .find(|entry| norm(&entry.id) == wanted || norm(&entry.label) == wanted)
        .or_else(|| {
            catalog.iter().find(|entry| {
                entry
                    .id
                    .rsplit_once('/')
                    .is_some_and(|(_, tail)| norm(tail) == wanted)
            })
        })
        .map(|entry| entry.id.clone())
}

/// 托管终端输入框是不是空的（前端 composerSafety.composerCandidate 的宿主版）：Some(true) 空、
/// Some(false) 有字、None 认不出。规则全来自插件声明的 ComposerGuard。
pub(crate) fn composer_state(screen: &[String], guard: &meowo_agent::chat_ui::ComposerGuard) -> Option<bool> {
    composer_read(screen, guard).map(|(empty, _)| empty)
}

/// 同 [`composer_state`]，另带输入框里的文字（去掉提示符与首尾空白；占位文字原样）。
pub(crate) fn composer_read(
    screen: &[String],
    guard: &meowo_agent::chat_ui::ComposerGuard,
) -> Option<(bool, String)> {
    let re = |pattern: &str| regex::Regex::new(pattern).ok();
    let top = re(guard.top_pattern)?;
    let bottom = re(guard.bottom_pattern)?;
    let row = re(guard.row_pattern)?;
    let mut end = screen.iter().rposition(|line| bottom.is_match(line))?;
    let mut start;
    let mut body: Vec<&String>;
    if guard.prompt_footer {
        while end > 0 && screen[end - 1].trim().is_empty() {
            end -= 1;
        }
        let mut cursor = end.checked_sub(1)?;
        loop {
            if top.is_match(&screen[cursor]) {
                break;
            }
            if !screen[cursor].trim().is_empty() && !row.is_match(&screen[cursor]) {
                return None;
            }
            cursor = cursor.checked_sub(1)?;
        }
        start = cursor;
        body = screen[start..end].iter().collect();
    } else {
        start = end;
        while start > 0 && row.is_match(&screen[start - 1]) {
            start -= 1;
        }
        body = screen[start..end].iter().collect();
        if body.first().is_some_and(|line| top.is_match(line)) {
            body.remove(0);
            start += 1;
        } else if start == 0 || !top.is_match(&screen[start - 1]) {
            return None;
        }
        if guard.tail_rows > 0 {
            let tail = re(guard.tail_pattern?)?;
            if body.len() <= guard.tail_rows || !tail.is_match(body.last()?) {
                return None;
            }
            body.truncate(body.len() - guard.tail_rows);
        }
    }
    let _ = start;
    if body.is_empty() {
        return None;
    }
    let mut text = Vec::new();
    for line in &body {
        if guard.prompt_footer && line.trim().is_empty() {
            text.push(String::new());
            continue;
        }
        let captures = row.captures(line)?;
        text.push(captures.get(1)?.as_str().trim().to_string());
    }
    // prompt_prefix 可列几个提示符，用 `|` 分隔（Gemini 平常 `>`、YOLO `*`，做空 R2）；空串 = 没有提示符。
    let prefix = guard
        .prompt_prefix
        .split('|')
        .find(|prefix| text[0].starts_with(prefix))?;
    re(guard.input_start_pattern)?.captures(body[0])?.get(1)?;
    text[0] = text[0][prefix.len()..].trim_start().to_string();
    let content = text.join("\n");
    let content = content.trim();
    Some((re(guard.empty_pattern)?.is_match(content), content.to_string()))
}

#[tauri::command]
pub(crate) fn queued_session_settings(session_id: i64) -> Option<serde_json::Value> {
    queue().get(&session_id).map(|q| q.request.clone())
}

/// 前端在回合结束时取走执行。宿主标了等待条件的（Lead 关卡 / 等会话打开）不给：前端看不到那些条件。
#[tauri::command]
pub(crate) fn take_queued_session_settings(session_id: i64) -> Option<serde_json::Value> {
    let mut map = queue();
    if map.get(&session_id).is_some_and(|q| wait_for(&q.request).is_some()) {
        return None;
    }
    map.remove(&session_id).map(|q| q.request)
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

/// OpenCode 底栏（`┃  Build · <模型名> <provider 名>`，oc4 实拍，右侧可能还跟着路径等）里的模型：
/// 按「模型名 provider 名」或「模型名」前缀匹配清单，取最长的那个，后面必须是行尾或空白（GPT-5 与
/// GPT-5 Mini 分得开）。认不出返回 None。
pub(crate) fn footer_model<'a>(lines: &[String], catalog: &'a [ModelCatalogEntry]) -> Option<&'a ModelCatalogEntry> {
    let footer = regex::Regex::new(r"^\s*┃\s+\S[^·]* · (.+)$").ok()?;
    let text = lines.iter().rev().find_map(|line| footer.captures(line.trim_end()))?.get(1)?.as_str().to_string();
    let fits = |prefix: &str| {
        text.strip_prefix(prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    };
    catalog
        .iter()
        .filter_map(|entry| {
            let full = entry.group.as_deref().map(|group| format!("{} {group}", entry.label));
            if full.as_deref().is_some_and(fits) {
                Some((full.map_or(0, |f| f.len()) + 1000, entry))
            } else if fits(&entry.label) {
                Some((entry.label.len(), entry))
            } else {
                None
            }
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, entry)| entry)
}

/// TUI 对话框路线的一步。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DialogStep {
    Agent(String),
    Model(String),
}

/// 一次请求里两步的顺序：**先切 agent 再切模型**。OpenCode 每个 agent 记着自己上次用的模型，先切模型
/// 再切 agent，模型会被 agent 记住的那个盖掉（真机验证 opencode 02d，2026-11-14）。
pub(crate) fn dialog_steps(plan: &Plan) -> Vec<DialogStep> {
    plan.dialog_agent
        .iter()
        .map(|agent| DialogStep::Agent(agent.clone()))
        .chain(plan.dialog_model.iter().map(|model| DialogStep::Model(model.clone())))
        .collect()
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
    let known: Vec<(String, Option<String>)> = catalog
        .iter()
        .map(|entry| (entry.label.clone(), entry.group.clone()))
        .collect();
    let mut confirmed = true;
    for step in dialog_steps(plan) {
        match step {
            DialogStep::Agent(agent) => {
                confirmed &= oc::switch_agent(&request, &agent, &mut io).map_err(map)?.confirmed;
            }
            DialogStep::Model(model) => {
                // 对话框按显示名搜索（oc4），清单的 label 就是 TUI 列的名字；分组就是 provider 显示名
                // （dialog_catalog），底栏按「模型名 provider 名」核对，选错自动重选 / 选回（复核 M2）。
                let entry = catalog
                    .iter()
                    .find(|entry| entry.id == model)
                    .ok_or(DialogError::Failed("settings/model_unknown".into()))?;
                confirmed &= oc::switch_model_checked(&request, &entry.label, entry.group.as_deref(), &known, &mut io)
                    .map_err(map)?
                    .confirmed;
            }
        }
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
    before_stop: Option<&dyn Fn() -> Result<(), String>>,
) -> Result<bool, String> {
    crate::terminal::restart_session_checked(
        app,
        broker,
        store,
        session_id,
        provider,
        Some(overrides),
        before_stop,
    )
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn restart(
    _app: &tauri::AppHandle,
    _broker: &crate::pty::PtyBroker,
    _store: &meowo_store::Store,
    _session_id: i64,
    _provider: &str,
    _overrides: HashMap<String, String>,
    _before_stop: Option<&dyn Fn() -> Result<(), String>>,
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

    /// 真机验证 codex 问题 1：新建选的会话服务档在认领后补设，片段与会话内切到同一档一致。
    #[test]
    fn launch_server_update_merges_declared_patches() {
        let codex = meowo_agent::by_id("codex").unwrap();
        let patch = launch_server_update(codex, &modes(&[("approval", "approveForMe")])).unwrap();
        assert_eq!(patch["sandbox"], "workspace-write");
        assert_eq!(patch["approvalsReviewer"], "auto_review");
        assert!(launch_server_update(codex, &modes(&[("approval", "default")])).is_none());
        let kimi = meowo_agent::by_id("kimi").unwrap();
        assert!(launch_server_update(kimi, &modes(&[("permission", "yolo")])).is_none());
    }

    /// 做空 N7（leader 定）：认领后只有选了「自动审查」才连会话服务补设，其他带服务端片段的档不连。
    #[test]
    fn only_approve_for_me_connects_after_claim() {
        let codex = meowo_agent::by_id("codex").unwrap();
        let patch = claim_server_update(codex, &modes(&[("approval", "approveForMe"), ("collaboration", "plan")])).unwrap();
        assert_eq!(patch["approvalsReviewer"], "auto_review");
        for other in ["default", "readOnly", "ask", "yolo"] {
            assert!(claim_server_update(codex, &modes(&[("approval", other)])).is_none(), "{other}");
        }
        assert!(claim_server_update(codex, &modes(&[("collaboration", "plan")])).is_none());
        let kimi = meowo_agent::by_id("kimi").unwrap();
        assert!(claim_server_update(kimi, &modes(&[("approval", "approveForMe")])).is_none());
    }

    /// 真机第四轮 N7（leader 定范围）：workspace-write 的档在没配 Windows 沙箱时，新建与接回的启动参数都补
    /// `-c windows.sandbox=unelevated`；接回时权限参数由 split_argv 丢掉，这一项照给。
    #[test]
    fn workspace_write_tiers_carry_windows_sandbox_on_new_and_resume() {
        let codex = meowo_agent::by_id("codex").unwrap();
        let home = std::env::temp_dir().join(format!("meowo-codex-n7-resume-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let env = vec![("CODEX_HOME".to_string(), home.to_string_lossy().into_owned())];
        let has = |args: &[String]| args.windows(2).any(|pair| pair[0] == "-c" && pair[1] == "windows.sandbox=unelevated");
        for tier in ["ask", "approveForMe"] {
            for resuming in [false, true] {
                let args = args_for_selections(codex, &modes(&[("approval", tier)]), &env, None, resuming);
                assert_eq!(has(&args), cfg!(windows), "{tier} resuming={resuming}: {args:?}");
            }
        }
        for tier in ["default", "readOnly", "yolo"] {
            assert!(!has(&args_for_selections(codex, &modes(&[("approval", tier)]), &env, None, true)), "{tier}");
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 真机验证 02d：同一请求既切模型又切 agent 时先切 agent，模型才不会被 agent 记住的那个盖掉。
    #[test]
    fn dialog_switches_agent_before_model() {
        let opencode = meowo_agent::by_id("opencode").unwrap();
        let catalog = vec![ModelCatalogEntry::new("fake/fake-model-2", "Fake Two")];
        let plan = plan(opencode, Some("fake/fake-model-2"), None, &modes(&[("agent", "build")]), &catalog, &[], &none(), &none()).unwrap();
        assert_eq!(
            dialog_steps(&plan),
            vec![DialogStep::Agent("build".into()), DialogStep::Model("fake/fake-model-2".into())]
        );
    }

    /// 复核 M3：宿主执行排队前自己认输入框（前端 composerCandidate 的宿主版），规则同插件声明。
    #[test]
    fn host_composer_check_matches_the_kimi_frame() {
        let guard = meowo_agent::chat_ui::ComposerGuard {
            prompt_footer: false,
            input_start_pattern: r"^(\s*│ > )",
            top_pattern: r"^\s*╭─+╮\s*$",
            bottom_pattern: r"^\s*╰─+╯\s*$",
            row_pattern: r"^\s*│(.*)│\s*$",
            prompt_prefix: ">",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r"^$",
        };
        let frame = |row: &str| vec![" ╭──────╮".to_string(), format!(" │{row}│"), " ╰──────╯".to_string()];
        assert_eq!(composer_state(&frame(" >    "), &guard), Some(true));
        assert_eq!(composer_state(&frame(" > hi "), &guard), Some(false));
        assert_eq!(composer_state(&["随便一行".to_string()], &guard), None);
    }

    /// 做空 N2：宿主与前端的输入框判定对同一批实拍帧必须给出同样的结论。帧与规则放在前端测试也读的
    /// app/src/test/fixtures/composer-frames.json；规则先与插件声明逐字核对，免得样例里的规则过期。
    #[test]
    fn host_composer_check_agrees_with_shared_frames() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../src/test/fixtures/composer-frames.json")).unwrap();
        let leak = |value: &serde_json::Value| -> &'static str {
            Box::leak(value.as_str().unwrap().to_string().into_boxed_str())
        };
        let mut checked = 0;
        for frame in fixture["frames"].as_array().unwrap() {
            let provider = frame["provider"].as_str().unwrap();
            let declared = &fixture["guards"][provider];
            let plugin = meowo_agent::by_id(provider).unwrap();
            // Kimi 的规则还要求装着 modern 版，本机没装时插件返回 None，只用样例里的规则测判定。
            if let Some(guard) = plugin.composer_guard(frame["version"].as_str()) {
                assert_eq!(&serde_json::to_value(guard).unwrap(), declared, "{provider} 规则与插件声明不一致");
            }
            let guard = meowo_agent::chat_ui::ComposerGuard {
                prompt_footer: declared["prompt_footer"].as_bool().unwrap(),
                input_start_pattern: leak(&declared["input_start_pattern"]),
                top_pattern: leak(&declared["top_pattern"]),
                bottom_pattern: leak(&declared["bottom_pattern"]),
                row_pattern: leak(&declared["row_pattern"]),
                prompt_prefix: leak(&declared["prompt_prefix"]),
                tail_rows: declared["tail_rows"].as_u64().unwrap() as usize,
                tail_pattern: declared["tail_pattern"].as_str().map(|_| leak(&declared["tail_pattern"])),
                empty_pattern: leak(&declared["empty_pattern"]),
            };
            let lines: Vec<String> = frame["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|line| line.as_str().unwrap().to_string())
                .collect();
            let trimmed: Vec<String> = lines.iter().map(|line| line.trim_end().to_string()).collect();
            let expect = match frame["expect"].as_str().unwrap() {
                "empty" => Some(true),
                "occupied" => Some(false),
                _ => None,
            };
            for screen in [&lines, &trimmed] {
                assert_eq!(composer_state(screen, &guard), expect, "{}", frame["name"]);
            }
            checked += 1;
        }
        assert!(checked >= 10);
    }

    #[test]
    fn host_reads_modes_from_footer_and_maps_model_labels() {
        let markers = [
            meowo_agent::ModeScreenMarker { marker: "shift+tab to accept edits", value: "default" },
            meowo_agent::ModeScreenMarker { marker: "yolo ctrl+y", value: "yolo" },
        ];
        assert_eq!(mode_from_screen("x
 YOLO Ctrl+Y ", &markers), Some("yolo"));
        assert_eq!(mode_from_screen("Shift+Tab to accept edits", &markers), Some("default"));
        assert_eq!(mode_from_screen("nothing", &markers), None);
        let catalog = vec![
            ModelCatalogEntry::new("kimi-code/k3", "K3"),
            ModelCatalogEntry::new("反向推理/DeepSeek-V4.1-Flash-1017", "DeepSeek V4.1 Flash"),
        ];
        assert_eq!(catalog_id_for(&catalog, "K3").as_deref(), Some("kimi-code/k3"));
        assert_eq!(catalog_id_for(&catalog, "DeepSeek-V4.1-Flash-1017").as_deref(), Some("反向推理/DeepSeek-V4.1-Flash-1017"));
        assert_eq!(catalog_id_for(&catalog, "gpt-x"), None);
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
        enqueue(sid, &serde_json::json!({ "model": { "id": "a", "label": "A" }, "modes": { "permission": "yolo" }, "label": "A" }));
        let merged = enqueue(sid, &serde_json::json!({ "modes": { "work": "plan" }, "label": "计划：开" }));
        assert_eq!(merged["model"]["id"], "a");
        assert_eq!(merged["modes"]["permission"], "yolo");
        assert_eq!(merged["modes"]["work"], "plan");
        assert_eq!(merged["label"], "计划：开");
        assert_eq!(queued_session_settings(sid), Some(merged.clone()));
        assert_eq!(take_queued_session_settings(sid), Some(merged));
        assert_eq!(queued_session_settings(sid), None);
    }

    /// 做空 N3 / N5 / N6：宿主标了等待条件的前端取不走；前端合并进提示名后等待条件还在；临时 id 换号时
    /// 条目跟过去并与新 id 上的合并；取走后放回时以期间新排的为准。
    #[test]
    fn host_queue_keeps_wait_conditions_and_follows_id_changes() {
        let modes = HashMap::from([("permission".to_string(), "yolo".to_string())]);
        let sid = 987_655;
        enqueue_waiting(sid, request_json(Some("kimi-code/k3"), None, &modes), "lead");
        assert_eq!(take_queued_session_settings(sid), None);
        let merged = enqueue(sid, &serde_json::json!({ "model": { "id": "kimi-code/k3", "label": "K3" }, "label": "K3" }));
        assert_eq!(merged["waitFor"], "lead");
        assert_eq!(merged["label"], "K3");
        assert_eq!(merged["modes"]["permission"], "yolo");

        let (temp, next, real) = (-987_001, -987_002, 987_656);
        enqueue(temp, &serde_json::json!({ "modes": { "permission": "auto" }, "label": "都不问" }));
        migrate_queued(temp, next);
        enqueue(real, &serde_json::json!({ "model": { "id": "m" }, "label": "M" }));
        migrate_queued(next, real);
        assert_eq!(queued_session_settings(temp), None);
        assert_eq!(queued_session_settings(next), None);
        let moved = take_queued_session_settings(real).unwrap();
        assert_eq!(moved["modes"]["permission"], "auto");
        assert_eq!(moved["model"]["id"], "m");
        assert_eq!(moved["label"], "M");

        let taken = queue().remove(&sid).unwrap();
        enqueue(sid, &serde_json::json!({ "modes": { "permission": "manual" }, "label": "每步都问" }));
        put_back(sid, taken);
        let back = queued_session_settings(sid).unwrap();
        assert_eq!(back["modes"]["permission"], "manual");
        assert_eq!(back["waitFor"], "lead");
        queue().remove(&sid);
    }

    /// 最终做空 P1/P2 回归（redteam-5 复现 2 的场景，宿主口径）：切完模型不发消息，换会话再回来 / 手机看 /
    /// 重启 Meowo（都等于重新从宿主读快照），打开菜单（同步写回）再切模式，命令行仍是新模型；有了新回复且
    /// 模型不同才放开。OpenCode 切 agent 读回的模型同一套。
    #[test]
    fn pinned_model_survives_reload_menu_sync_and_next_switch() {
        let dir = std::env::temp_dir().join(format!("meowo-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("board.db");
        let _ = std::fs::remove_file(&db);
        let store = meowo_store::Store::open(&db).unwrap();
        let now = crate::now_ms();
        let project = store.upsert_project_by_root("/tmp/pin", "pin", now).unwrap();
        let (sid, _) = store.start_session(project, "cc-pin", now).unwrap();
        store.set_session_launch_args(sid, r#"{"model":"gemini-3.8-flash","approval":"autoEdit"}"#).unwrap();

        // 1. 对话页切到 gemini-2.5-pro（restarted）：宿主记钉子。会话上下文报的还是旧回复的模型。
        pin_model(&store, sid, "gemini-2.5-pro", Some("gemini-3.8-flash"), Some("gemini-3.8-flash"));
        // 2. 换会话再回来 / 手机 / 重启 Meowo：对话页重新读快照。
        assert_eq!(pinned_history_model(&store, sid, Some("gemini-3.8-flash".into())).as_deref(), Some("gemini-2.5-pro"));
        // 3. 打开菜单：同步要写回的若是旧模型，不收。
        let stored = stored_selections(&store, sid);
        assert!(!record_model_allowed(&stored, "gemini-3.8-flash"));
        assert!(record_model_allowed(&stored, "gemini-2.5-pro"));
        // 4. 再切审批档：即便前端报的 current 还是旧模型，拼出来的命令行也带新模型。
        let gemini = meowo_agent::by_id("gemini").unwrap();
        let catalog = vec![
            ModelCatalogEntry::new("gemini-3.8-flash", "Gemini 3.8 Flash"),
            ModelCatalogEntry::new("gemini-2.5-pro", "Gemini 2.5 Pro"),
        ];
        let mut current = HashMap::from([
            ("model".to_string(), "gemini-3.8-flash".to_string()),
            ("approval".to_string(), "autoEdit".to_string()),
        ]);
        pin_switched_model(&mut current, &stored);
        let planned = plan(gemini, None, None, &modes(&[("approval", "default")]), &catalog, &[], &current, &stored).unwrap();
        assert_eq!(planned.restart.get("model").map(String::as_str), Some("gemini-2.5-pro"));
        // 5. 有了新回复（模型变了）：钉子作废，照回复。
        assert_eq!(pinned_history_model(&store, sid, Some("gemini-2.5-pro".into())).as_deref(), Some("gemini-2.5-pro"));
        let stored = stored_selections(&store, sid);
        assert!(!stored.contains_key(REPLY_MODEL_AT_SWITCH) && !stored.contains_key(REPLY_RAW_AT_SWITCH));
        assert_eq!(pinned_history_model(&store, sid, Some("gemini-2.5-flash".into())).as_deref(), Some("gemini-2.5-flash"));

        // OpenCode：只切 agent 读回 fake/fake-model；之后最后一条用户消息仍报 fake/fake-model-2，live 以钉子为准。
        pin_model(&store, sid, "fake/fake-model", Some("fake/fake-model-2"), Some("fake-model-2"));
        let mut live = HashMap::from([("model".to_string(), "fake/fake-model-2".to_string())]);
        pin_switched_model(&mut live, &stored_selections(&store, sid));
        assert_eq!(live["model"], "fake/fake-model");
        assert_eq!(pinned_history_model(&store, sid, Some("fake-model-2".into())).as_deref(), Some("fake/fake-model"));
        drop(store);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 真机第四轮问题 3：只切 agent 后从底栏读回 OpenCode 实际用的模型（前缀相同的模型分得开）。
    #[test]
    fn opencode_footer_model_is_read_back() {
        let mut one = ModelCatalogEntry::new("fake/fake-model", "Fake One");
        one.group = Some("Fake".into());
        let mut two = ModelCatalogEntry::new("fake/fake-model-2", "Fake Two");
        two.group = Some("Fake".into());
        let mut gpt = ModelCatalogEntry::new("openai/gpt-5", "GPT-5");
        gpt.group = Some("OpenAI".into());
        let mut mini = ModelCatalogEntry::new("openai/gpt-5-mini", "GPT-5 Mini");
        mini.group = Some("OpenAI".into());
        let catalog = vec![one, two, gpt, mini];
        let screen = |footer: &str| vec!["  ┃".to_string(), footer.to_string(), "  ╹▀▀▀▀".to_string()];
        assert_eq!(footer_model(&screen("  ┃  Plan · Fake One Fake"), &catalog).unwrap().id, "fake/fake-model");
        assert_eq!(footer_model(&screen("  ┃  Build · Fake Two Fake          C:\\proj:master"), &catalog).unwrap().id, "fake/fake-model-2");
        assert_eq!(footer_model(&screen("  ┃  Build · GPT-5 Mini OpenAI"), &catalog).unwrap().id, "openai/gpt-5-mini");
        assert_eq!(footer_model(&screen("  ┃  Build · GPT-5 OpenAI"), &catalog).unwrap().id, "openai/gpt-5");
        assert!(footer_model(&screen("  ┃  Build · Unknown Model X"), &catalog).is_none());
    }

    /// 真机第四轮问题 2：切完模型、还没有新回复就再切模式，不能按会话文件里旧回复的模型拼参数。
    #[test]
    fn switched_model_wins_until_a_new_reply() {
        let stored = HashMap::from([
            ("model".to_string(), "gemini-2.5-pro".to_string()),
            (REPLY_MODEL_AT_SWITCH.to_string(), "gemini-3.8-flash".to_string()),
        ]);
        let mut current = HashMap::from([("model".to_string(), "gemini-3.8-flash".to_string())]);
        pin_switched_model(&mut current, &stored);
        assert_eq!(current["model"], "gemini-2.5-pro", "还没有新回复：以刚切的为准");
        let mut unknown = HashMap::new();
        pin_switched_model(&mut unknown, &stored);
        assert_eq!(unknown["model"], "gemini-2.5-pro", "读不到回复模型：以存档为准");
        let mut replied = HashMap::from([("model".to_string(), "gemini-2.5-flash".to_string())]);
        pin_switched_model(&mut replied, &stored);
        assert_eq!(replied["model"], "gemini-2.5-flash", "有了新回复且模型不同：以回复为准");
        let mut no_marker = HashMap::from([("model".to_string(), "gemini-3.8-flash".to_string())]);
        pin_switched_model(&mut no_marker, &HashMap::from([("model".to_string(), "gemini-2.5-pro".to_string())]));
        assert_eq!(no_marker["model"], "gemini-3.8-flash", "没切过（无标记）：照回复");
        // 标记键不是启动选项，不会拼进命令行。
        let gemini = meowo_agent::by_id("gemini").unwrap();
        assert!(!meowo_agent::resolve_launch_args(gemini.launch_options(), &stored).iter().any(|arg| arg.contains("flash")));
    }

    /// leader 2026-11-14：混合排队宿主先做能经后端做的部分，终端项留给对话页；提示名不变。
    #[test]
    fn mixed_queue_splits_backend_and_terminal_parts() {
        let action = serde_json::json!({ "kind": "mode", "dimension": "work", "value": "plan", "inputs": [{ "data": "/plan on", "submit": true }] });
        let mixed = serde_json::json!({
            "label": "计划：开",
            "model": { "id": "kimi-code/k3", "label": "K3" },
            "modes": { "permission": "yolo", "work": "plan" },
            "terminal": [action.clone()],
        });
        let (backend, leftover) = split_terminal(&mixed);
        let backend = backend.unwrap();
        assert_eq!(backend["model"]["id"], "kimi-code/k3");
        assert_eq!(backend["modes"], serde_json::json!({ "permission": "yolo" }));
        assert!(backend.get("terminal").is_none());
        let leftover = leftover.unwrap();
        assert_eq!(leftover["label"], "计划：开");
        assert_eq!(leftover["modes"], serde_json::json!({ "work": "plan" }));
        assert_eq!(leftover["terminal"], serde_json::json!([action]));
        // 只有终端项：宿主不做，整条留给对话页。
        let only = serde_json::json!({ "label": "计划：开", "modes": { "work": "plan" }, "terminal": [action] });
        let (none, rest) = split_terminal(&only);
        assert!(none.is_none());
        assert_eq!(rest.unwrap()["modes"]["work"], "plan");
        // 没有终端项：原样执行，不留。
        let plain = serde_json::json!({ "label": "K3", "model": { "id": "kimi-code/k3" } });
        assert_eq!(split_terminal(&plain), (Some(plain.clone()), None));
    }

    /// 第四轮补：运行中排进来的终端项同一件事后选的替换先选的，宿主合并时不丢别的项。
    #[test]
    fn terminal_actions_merge_by_subject() {
        let first = serde_json::json!({ "label": "计划：开", "modes": { "work": "plan" }, "terminal": [
            { "kind": "mode", "dimension": "work", "value": "plan", "inputs": [{ "data": "/plan on", "submit": true }] },
            { "kind": "effort", "level": "high" }
        ] });
        let next = serde_json::json!({ "label": "计划：关", "modes": { "work": "default" }, "terminal": [
            { "kind": "mode", "dimension": "work", "value": "default", "inputs": [{ "data": "/plan off", "submit": true }] }
        ] });
        let merged = merge_queued(Some(&first), &next);
        let items = merged["terminal"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["kind"], "effort");
        assert_eq!(items[1]["value"], "default");
        assert_eq!(merged["modes"]["work"], "default");
    }

    /// 做空 R4：超时只算「空闲却切不了」，新入队 / 重新选择时不在计时；取消提醒带上要切的名字。
    #[test]
    fn queue_timeout_only_counts_blocked_idle_time() {
        let sid = 987_657;
        enqueue(sid, &serde_json::json!({ "label": "K2" }));
        assert!(queue().get(&sid).unwrap().blocked.is_none());
        mark_blocked(sid);
        let first = queue().get(&sid).unwrap().blocked.unwrap();
        mark_blocked(sid);
        assert_eq!(queue().get(&sid).unwrap().blocked, Some(first), "继续切不了时不重新起算");
        not_blocked(sid);
        assert!(queue().get(&sid).unwrap().blocked.is_none(), "会话在跑时清掉");
        mark_blocked(sid);
        enqueue(sid, &serde_json::json!({ "label": "K3" }));
        assert!(queue().get(&sid).unwrap().blocked.is_none(), "重新选择从头算");
        queue().remove(&sid);
        assert_eq!(expired_label("zh", "只读"), "切换到「只读」等太久，已取消");
        // 第四轮 L1：会话归档 / 删除时取消也提醒。
        assert_eq!(gone_label("zh", "只读"), "会话已归档或删除，切换到「只读」已取消");
        assert_eq!(expired_label("en", "Read only"), "Switching to \"Read only\" waited too long and was canceled");
    }

    /// 做空 N10：屏幕挂着审批 / 提问（blocked）时宿主不动，不靠 tab_class 把它归成 waiting 后再碰巧被挡。
    #[test]
    fn host_never_switches_while_screen_is_blocked() {
        assert!(screen_allows_switch(true, "waiting", false, Some("idle"), || false));
        assert!(!screen_allows_switch(true, "waiting", false, Some("blocked"), || false));
        assert!(!screen_allows_switch(true, "running", false, Some("working"), || false));
        assert!(!screen_allows_switch(true, "waiting", true, Some("idle"), || false));
        assert!(!screen_allows_switch(true, "waiting", false, Some("idle"), || true));
    }

    /// 只有会杀进程 / 要往输入框打字的路线才要求输入框是空的；Codex 经会话服务不碰输入框。
    #[test]
    fn only_restart_and_dialog_routes_need_an_empty_composer() {
        let none = HashMap::new();
        let kimi = meowo_agent::by_id("kimi").unwrap();
        let codex = meowo_agent::by_id("codex").unwrap();
        let opencode = meowo_agent::by_id("opencode").unwrap();
        assert!(needs_empty_composer(kimi, true, &none));
        assert!(needs_empty_composer(opencode, true, &none));
        assert!(!needs_empty_composer(codex, true, &none));
        let codex_mode = codex.launch_options().iter().find(|o| o.switch == meowo_agent::ModeSwitch::Server).unwrap();
        let picks = HashMap::from([(codex_mode.id.to_string(), codex_mode.choices[0].id.to_string())]);
        assert!(!needs_empty_composer(codex, false, &picks));
        let kimi_mode = kimi.launch_options().iter().find(|o| o.switch == meowo_agent::ModeSwitch::Restart).unwrap();
        let picks = HashMap::from([(kimi_mode.id.to_string(), kimi_mode.choices[0].id.to_string())]);
        assert!(needs_empty_composer(kimi, false, &picks));
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
