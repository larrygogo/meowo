//! Lead 工作流的宿主侧命令：读任务快照、回答关卡。
//!
//! 解析逻辑与 reporter 的 lead hook 共用一份（`meowo_reporter::lead`），宿主只做转接。
//! 两个命令都碰文件，按 CLAUDE.md 的线程纪律走 `async fn` + `spawn_blocking`。
//! 远程（手机）放行见 remote.rs 模块头的「例外（2026-11-09）」：只读 `.lead/active`
//! 指向的任务目录，目录本身还要先过 `ensure_session_dir`。

use meowo_protocol::ipc::{LeadGateAnswerDto, LeadTaskDto};

/// 会话 cwd 所在项目的 Lead 任务快照；没有任务返回 None（前端据此隐藏入口）。`session` 是对话窗
/// 当前会话的 cc_session_id：认领了的任务只给认领它的会话看（同项目的其它会话不该冒出它的入口）。
#[tauri::command]
pub(crate) async fn lead_task_snapshot(
    cwd: String,
    session: Option<String>,
) -> Result<Option<LeadTaskDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut task = meowo_reporter::lead::snapshot_for(&cwd, session.as_deref());
        if let Some(task) = task.as_mut() {
            if let Some(gate) = &task.gate {
                task.delivery =
                    crate::lead_conversation::delivery_status(&crate::db_path(), &cwd, &gate.id);
            }
        }
        task
    })
    .await
    .map_err(|e| e.to_string())
}

/// leader 回答关卡。核对关卡 id 与状态后写入 answered；同一答案重复提交视为成功。
///
/// 送达：会话活着时，agent 侧的 asyncRewake 后台 hook（`meowo-reporter lead wait`）看到 answered
/// 就唤醒模型。会话的 claude 进程已经不在（例如 Meowo 重启时托管终端随之退出），后台就没有人在等——
/// 这时由这里把会话以托管终端恢复（--resume），恢复后 SessionStart 上的 wait 发现已作答，立刻唤醒。
/// 恢复失败不影响作答本身：答复已落盘，用户下次打开会话时一样会被注入。
#[tauri::command]
pub(crate) async fn answer_lead_gate(
    app: tauri::AppHandle,
    cwd: String,
    gate_id: String,
    choice: String,
    note: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 按关卡 id 找任务：关卡可能挂在非 active 的旧任务里（做空审查 M4）
        let task = meowo_reporter::lead::task_of_gate(&cwd, &gate_id).ok_or("lead/gate_missing")?;
        use tauri::Manager;
        let db = app.state::<crate::AppState>().db_path.clone();
        let managed = crate::lead_conversation::answer_and_deliver(
            &db,
            &cwd,
            &gate_id,
            &LeadGateAnswerDto { choice, note },
            Some(app.clone()),
        )?;
        if !managed {
            if let Err(e) = resume_owner_if_gone(&app, &task) {
                eprintln!("Lead 关卡已作答，但恢复会话失败（答复已保存，打开会话时会送达）: {e}");
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 关卡归属会话的 claude 进程不在了 → 以托管终端恢复它。托管终端还开着或外部终端里仍在跑 → 不动。
#[tauri::command]
pub(crate) async fn retry_lead_gate_delivery(
    app: tauri::AppHandle,
    cwd: String,
    gate_id: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let db = app.state::<crate::AppState>().db_path.clone();
        crate::lead_conversation::retry(&db, &cwd, &gate_id, Some(app))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn resume_owner_if_gone(app: &tauri::AppHandle, task: &std::path::Path) -> Result<(), String> {
    use tauri::Manager;
    let owner = std::fs::read_to_string(task.join(".owner")).unwrap_or_default();
    let state = app.state::<crate::AppState>();
    let store = crate::open_store(&state.db_path)?;
    let Some(sid) = store
        .find_session_id_pub(owner.trim())
        .map_err(|e| e.to_string())?
    else {
        return Ok(()); // 找不到会话：交给用户打开时注入
    };
    if state.ptys.is_managed(sid) || crate::terminal::session_agent_alive(&store, sid)? {
        return Ok(()); // 进程还在，后台 wait 会唤醒
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let session = store.get_session(sid).map_err(|e| e.to_string())?;
        if !crate::session_command::is_safe_id(&session.cc_session_id) {
            return Err("无效 session_id".into());
        }
        let cwd = store.session_cwd(sid).map_err(|e| e.to_string())?;
        let provider = store.session_provider(sid).map_err(|e| e.to_string())?;
        crate::terminal::start_managed_resume_sized(
            app.clone(),
            state.ptys.clone(),
            sid,
            cwd,
            session.cc_session_id,
            provider,
            crate::pty::TerminalSize::new(120, 40),
            None,
        )
        .map(|_| ())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = sid;
        Ok(())
    }
}

// ---------------- 一键安装 / 卸载（设置 → Lead 工作流） ----------------

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const SKILL_MD: &str = include_str!("lead_assets/SKILL.md");
const REDTEAM_MD: &str = include_str!("lead_assets/lead-redteam.md");
/// 安装文件里的标记：卸载只删带它的文件，用户自己写的同名文件不碰。
const MANAGED_MARK: &str = "由 Meowo 安装和管理";
/// wait（asyncRewake）最多挂 7 天，hook timeout 设得更长一点，交给 reporter 自己按时退出。
const WAIT_TIMEOUT_SECS: u64 = 8 * 24 * 3600;

/// lead 的 hook 条目：`"<reporter>" lead <mode>`。
fn lead_cmd(reporter: &str, mode: &str) -> String {
    format!("\"{reporter}\" lead {mode}")
}

/// 是不是 lead 工作流的 hook：本版（reporter 的 lead 子命令）或初版（~/.claude/lead/hook.py）。
/// 后者由新版接管——两套同时跑会双重拦截。
fn is_lead_hook(cmd: &str) -> bool {
    let c = cmd.replace('\\', "/");
    (c.contains("meowo-reporter") && (c.contains(" lead ") || c.trim_end().ends_with(" lead")))
        || c.contains(".claude/lead/hook.py")
}

/// 从 settings 的 hooks 里删掉所有 lead 条目；删空的分组和事件也一并清掉。
fn strip_lead_hooks(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    for groups in hooks.values_mut() {
        if let Some(arr) = groups.as_array_mut() {
            for g in arr.iter_mut() {
                if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                    hs.retain(|h| {
                        !h.get("command")
                            .and_then(Value::as_str)
                            .is_some_and(is_lead_hook)
                    });
                }
            }
            arr.retain(|g| {
                g.get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|hs| !hs.is_empty())
            });
        }
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
        }
    }
}

fn push_group(settings: &mut Value, event: &str, group: Value) {
    if !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }
    let mut groups = settings["hooks"][event]
        .as_array()
        .cloned()
        .unwrap_or_default();
    groups.push(group);
    settings["hooks"][event] = Value::Array(groups);
}

fn add_lead_hooks(settings: &mut Value, reporter: &str) {
    let h =
        |mode: &str| json!({"type": "command", "command": lead_cmd(reporter, mode), "timeout": 10});
    let wait = json!({"type": "command", "command": lead_cmd(reporter, "wait"), "asyncRewake": true, "timeout": WAIT_TIMEOUT_SECS});
    // SessionStart：注入规格与进度；会话恢复时若关卡已作答，wait 立刻唤醒模型。
    push_group(
        settings,
        "SessionStart",
        json!({"hooks": [h("session"), wait.clone()]}),
    );
    push_group(
        settings,
        "PostToolUse",
        json!({"matcher": "Write|Edit|MultiEdit|Bash|PowerShell", "hooks": [h("claim")]}),
    );
    // Stop：实施阶段拦截；有挂起关卡时 wait 在后台等答复，答了就唤醒。
    push_group(settings, "Stop", json!({"hooks": [h("stop"), wait]}));
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        // 根节点必须是对象：数组之类的形态下面的 `settings["hooks"] = …` 会 panic（做空审查 L3）
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.is_object() => Ok(v),
            _ => Err("lead/settings_invalid".to_owned()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.to_string()),
    }
}

/// 写 ~/.claude/settings.json：这是用户自己的文件，claude 也在同时读写它。
/// 原子写（唯一临时名 + fsync + rename，崩溃/并发都不会留半截），首次覆盖前留原件备份。
fn write_settings(path: &Path, v: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n";
    if path.exists() {
        meowo_agent::wiring::backup_once(path).map_err(|e| e.to_string())?;
    }
    meowo_agent::fsutil::write_atomic(path, &text).map_err(|e| e.to_string())
}

fn managed_file(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.contains(MANAGED_MARK))
}

/// 安装到 claude 配置目录：skill、做空子代理、hook。可重复执行（先删旧条目再加）。
/// 同名文件若是用户自己写的（没有托管标记），不覆盖，报错让用户先处理。
pub(crate) fn install_into(
    claude_dir: &Path,
    reporter: &str,
    registry: Option<&Path>,
) -> Result<(), String> {
    if !claude_dir.is_dir() {
        return Err("lead/claude_missing".into()); // 没装 Claude Code 不凭空造目录
    }
    let skill = claude_dir.join("skills/lead/SKILL.md");
    let agent = claude_dir.join("agents/lead-redteam.md");
    // 初版（~/.claude/lead/hook.py 那套）留下的同名文件不带托管标记，但它就是要被接管的对象：
    // 先备份成 *.prototype-bak 再覆盖。不是初版的同名文件才算用户自己的，拒绝覆盖（做空审查 H3）。
    let prototype = claude_dir.join("lead/hook.py").exists();
    for p in [&skill, &agent] {
        if p.exists() && !managed_file(p) {
            if !prototype {
                return Err("lead/user_file_exists".into());
            }
            std::fs::copy(p, p.with_extension("md.prototype-bak")).map_err(|e| e.to_string())?;
        }
    }
    if prototype {
        if let Some(reg) = registry {
            migrate_prototype_registry(&claude_dir.join("lead/sessions.json"), reg);
        }
    }
    let settings_path = claude_dir.join("settings.json");
    let mut settings = read_settings(&settings_path)?;
    std::fs::create_dir_all(claude_dir.join("skills/lead")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(claude_dir.join("agents")).map_err(|e| e.to_string())?;
    write_if_changed(&skill, SKILL_MD)?;
    write_if_changed(&agent, REDTEAM_MD)?;
    // 每次启动 refresh_if_installed 都会走到这里：hook 已是目标状态就不重写 settings.json——
    // 否则每启动一次就改一次用户文件的 mtime、多一个与 claude 并发写同一文件的窗口。
    let before = settings.clone();
    strip_lead_hooks(&mut settings);
    add_lead_hooks(&mut settings, reporter);
    if settings == before {
        return Ok(());
    }
    write_settings(&settings_path, &settings)
}

/// 托管文件内容没变就不写（理由同 settings.json），变了走原子写：claude 可能正在读 skill。
fn write_if_changed(path: &Path, body: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|t| t == body) {
        return Ok(());
    }
    meowo_agent::fsutil::write_atomic(path, body).map_err(|e| e.to_string())
}

/// 初版的「会话 → 任务」登记表并进新登记表（新表已有的会话不动），进行中的任务不必重新认领。
fn migrate_prototype_registry(old: &Path, new: &Path) {
    let Ok(text) = std::fs::read_to_string(old) else {
        return;
    };
    let Ok(Value::Object(old_map)) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let mut map = match std::fs::read_to_string(new)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    for (k, v) in old_map {
        map.entry(k).or_insert(v);
    }
    // 原子写：这张登记表 reporter（lead hook，每次一个进程）随时在读写，直接 write 先截断，
    // 并发读到空表再写回就把别的会话的认领冲掉（同 meowo-reporter lead.rs 认领处的做法）。
    let _ = meowo_agent::fsutil::write_atomic(new, &Value::Object(map).to_string());
}

/// 卸载：只删带托管标记的文件与 lead hook 条目，其余原样保留。
pub(crate) fn uninstall_from(claude_dir: &Path) -> Result<(), String> {
    let skill = claude_dir.join("skills/lead/SKILL.md");
    if managed_file(&skill) {
        std::fs::remove_file(&skill).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir(claude_dir.join("skills/lead")); // 空了才删得掉
    }
    let agent = claude_dir.join("agents/lead-redteam.md");
    if managed_file(&agent) {
        std::fs::remove_file(&agent).map_err(|e| e.to_string())?;
    }
    let settings_path = claude_dir.join("settings.json");
    if settings_path.exists() {
        let mut settings = read_settings(&settings_path)?;
        let before = settings.clone();
        strip_lead_hooks(&mut settings);
        if settings != before {
            write_settings(&settings_path, &settings)?;
        }
    }
    Ok(())
}

pub(crate) fn installed_in(claude_dir: &Path) -> bool {
    managed_file(&claude_dir.join("skills/lead/SKILL.md"))
}

fn claude_dir() -> Option<PathBuf> {
    meowo_agent::home_dir().map(|h| h.join(".claude"))
}

/// reporter lead 子命令用的登记表（与 board.db 同目录，见 meowo-reporter main.rs）。
fn lead_registry() -> Option<PathBuf> {
    meowo_reporter::db_path().and_then(|p| p.parent().map(|d| d.join("lead-sessions.json")))
}

/// 启动时：装着 Lead 就重装一遍——reporter 路径变了（挪了 app、dev 与安装版混用）要跟着改，
/// 否则所有 lead hook 都会 127、拦截悄悄失效（做空审查 L2）；顺带把 skill 文本更新到本版本。
pub(crate) fn refresh_if_installed() {
    // 与 Meowo 自身 hook 同一道 dev 门：dev 构建一启动就把 lead hook 改指到 target/debug，
    // 挂着的 wait 会锁住那个 exe（cargo build 报 os error 5），cargo clean 后拦截与注入一起静默失效
    // （做空审查第二轮 M4）。
    if !crate::setup::auto_wire_enabled() {
        return;
    }
    let Some(reporter) = crate::setup::sibling_reporter() else {
        return;
    };
    if let Some(dir) = claude_dir().filter(|dir| installed_in(dir)) {
        if let Err(e) = install_into(&dir, &reporter, None) {
            eprintln!("Lead 工作流启动时刷新失败: {e}");
        }
    }
    let _guard = PROVIDER_INSTALL_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let settings = crate::settings::load_settings();
    for plugin in meowo_agent::all()
        .iter()
        .copied()
        .filter(|plugin| plugin.lead().is_some())
    {
        let cap = plugin.lead().expect("已由能力过滤");
        let mut installations = plugin.resolve().into_iter().collect::<Vec<_>>();
        for profile in settings
            .profiles
            .get(plugin.id().as_str())
            .into_iter()
            .flatten()
        {
            if let Ok((inst, _)) = known_provider_profile(plugin, Some(&profile.id)) {
                installations.push(inst);
            }
        }
        // 只有用户曾安装的独立目录才刷新，不把默认账号选择扩散到其他账号。
        for inst in installations
            .into_iter()
            .filter(|inst| meowo_agent::lead_install::managed_present(cap, inst))
        {
            let version = crate::probe_cli_version(plugin);
            if let Err(error) =
                meowo_agent::lead_install::install(cap, &inst, version.as_deref(), &reporter)
            {
                eprintln!("Lead 工作流刷新[{}]失败: {error}", plugin.id());
            }
        }
    }
}

/// 设置页读开关状态：以「是否真的装着」为准，不另存一份设置（两者不会不一致）。
#[tauri::command]
pub(crate) async fn lead_workflow_installed() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(claude_dir().is_some_and(|d| installed_in(&d))))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn set_lead_workflow(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = claude_dir().ok_or("lead/claude_missing")?;
        if enabled {
            let reporter = crate::setup::sibling_reporter().ok_or("lead/reporter_missing")?;
            install_into(&dir, &reporter, lead_registry().as_deref())
        } else {
            uninstall_from(&dir)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------- 插件声明的原生安装（旧 Claude 开关保持独立） ----------------

// 查询与改动共享锁，防止较早开始的查询覆盖安装成功后刚更新的运行时缓存。
static PROVIDER_INSTALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn provider_installation_state(
    plugin: &dyn meowo_agent::AgentPlugin,
    installation: Option<&meowo_agent::Installation>,
    profile: Option<(&str, &str)>,
    version: Option<&str>,
    reporter: Option<&str>,
) -> meowo_protocol::ipc::LeadProviderInstallationDto {
    use meowo_agent::lead_install;
    let mut dto = meowo_protocol::ipc::LeadProviderInstallationDto {
        provider: plugin.id().as_str().into(),
        profile_id: profile.map(|(id, _)| id.into()),
        profile_name: profile.map(|(_, name)| name.into()),
        name: plugin.display_name().into(),
        version: version.map(str::to_owned),
        configured: false,
        supported: false,
        managed: false,
        installed: false,
        assets_installed: false,
        hooks_installed: false,
        automatic_continue: false,
        reason: Some("lead/provider_missing".into()),
    };
    let Some(cap) = plugin.lead() else {
        dto.reason = Some("lead/provider_unsupported".into());
        return dto;
    };
    let Some(inst) = installation else { return dto };
    dto.configured = inst.data_dir.is_dir();
    let state = lead_install::inspect(cap, inst, version, reporter.unwrap_or(""));
    dto.supported = state.supported;
    dto.managed = lead_install::managed_present(cap, inst);
    dto.assets_installed = state.assets_present;
    dto.hooks_installed = state.hooks_present;
    dto.installed = state.installed;
    // 身份策略仅是准入条件；模型等待、答复消费及离线恢复仍需各自闭环证据，不能从磁盘安装推导。
    dto.reason = if !dto.configured {
        Some("lead/provider_missing".into())
    } else if reporter.is_none() {
        Some("lead/reporter_missing".into())
    } else {
        state.reason
    };
    dto
}

fn known_provider_profile(
    plugin: &dyn meowo_agent::AgentPlugin,
    profile_id: Option<&str>,
) -> Result<(meowo_agent::Installation, Option<String>), String> {
    match profile_id {
        None => plugin
            .resolve()
            .map(|inst| (inst, None))
            .ok_or_else(|| "lead/provider_missing".into()),
        Some(id) => {
            // 账号名来自 IPC；即使设置文件被手工编辑，也不允许它变成跨目录路径。
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            {
                return Err("lead/profile_missing".into());
            }
            let settings = crate::settings::load_settings();
            let profile = settings
                .profiles
                .get(plugin.id().as_str())
                .and_then(|profiles| profiles.iter().find(|profile| profile.id == id))
                .ok_or("lead/profile_missing")?;
            let root = crate::profile::profile_root(plugin.id().as_str(), id);
            if !root.is_dir() {
                return Err("lead/profile_missing".into());
            }
            plugin
                .installation_for_profile(&root)
                .map(|inst| (inst, Some(profile.name.clone())))
                .ok_or_else(|| "lead/profile_missing".into())
        }
    }
}

fn provider_installation_states_locked() -> Vec<meowo_protocol::ipc::LeadProviderInstallationDto> {
    let reporter = crate::setup::sibling_reporter();
    let settings = crate::settings::load_settings();
    let mut states = Vec::new();
    for plugin in meowo_agent::all()
        .iter()
        .copied()
        .filter(|plugin| plugin.lead().is_some())
    {
        let version = crate::probe_cli_version(plugin);
        let inst = plugin.resolve();
        states.push(provider_installation_state(
            plugin,
            inst.as_ref(),
            None,
            version.as_deref(),
            reporter.as_deref(),
        ));
        for profile in settings
            .profiles
            .get(plugin.id().as_str())
            .into_iter()
            .flatten()
        {
            let inst = known_provider_profile(plugin, Some(&profile.id))
                .ok()
                .map(|(inst, _)| inst);
            states.push(provider_installation_state(
                plugin,
                inst.as_ref(),
                Some((&profile.id, &profile.name)),
                version.as_deref(),
                reporter.as_deref(),
            ));
        }
    }
    states
}

pub(crate) fn provider_installation_states() -> Vec<meowo_protocol::ipc::LeadProviderInstallationDto>
{
    let _guard = PROVIDER_INSTALL_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let states = provider_installation_states_locked();
    crate::lead_runtime::replace_installations(&states);
    states
}

#[tauri::command]
pub(crate) async fn lead_provider_installations(
) -> Result<Vec<meowo_protocol::ipc::LeadProviderInstallationDto>, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(provider_installation_states()))
        .await
        .map_err(|_| "lead/worker_failed".to_owned())?
}

#[tauri::command]
pub(crate) async fn set_lead_provider_enabled(
    args: meowo_protocol::ipc::SetLeadProviderEnabledArgs,
) -> Result<meowo_protocol::ipc::LeadProviderInstallationDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = PROVIDER_INSTALL_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let plugin = meowo_agent::by_id(&args.provider).ok_or("lead/provider_unsupported")?;
        let cap = plugin.lead().ok_or("lead/provider_unsupported")?;
        let (inst, profile_name) = known_provider_profile(plugin, args.profile_id.as_deref())?;
        let reporter = crate::setup::sibling_reporter();
        let version = crate::probe_cli_version(plugin);
        let result = if args.enabled {
            meowo_agent::lead_install::install(
                cap,
                &inst,
                version.as_deref(),
                reporter.as_deref().ok_or("lead/reporter_missing")?,
            )
            .map(|_| ())
        } else {
            // 卸载不受未知版本、缺少 reporter 影响，只处理该账号拥有托管标记的文件。
            meowo_agent::lead_install::uninstall(cap, &inst)
        };
        let state = provider_installation_state(
            plugin,
            Some(&inst),
            args.profile_id.as_deref().zip(profile_name.as_deref()),
            version.as_deref(),
            reporter.as_deref(),
        );
        // 出错后也按磁盘现状收回能力，不能让部分回滚留下旧的运行时启用状态。
        crate::lead_runtime::update_installation(&state);
        result?;
        Ok(state)
    })
    .await
    .map_err(|_| "lead/worker_failed".to_owned())?
}

// ---------------- 关卡巡检：看板「待拍板」+ 通知 ----------------

/// 巡检间隔。关卡由 agent 写文件发起，没有推送通道；读的是最近 20 个项目里的几个小文件。
const GATE_POLL: std::time::Duration = std::time::Duration::from_secs(10);

/// 有关卡挂起的会话按任务归属映射；同一次巡检处理已回答的托管投递。
fn scan_gate_states(
    store: &meowo_store::Store,
    mut answered: impl FnMut(&Path),
) -> std::collections::HashSet<i64> {
    let mut out = std::collections::HashSet::new();
    let Ok(cwds) = store.recent_cwds(20) else {
        return out;
    };
    for cwd in cwds {
        // 项目里所有任务的关卡，不只 active 指向的那个（做空审查 M4）
        for (task, gate) in meowo_reporter::lead::project_gates(&cwd) {
            if gate.status == "answered" {
                answered(&task);
            }
            if gate.status != "pending" {
                continue;
            }
            let owner = std::fs::read_to_string(task.join(".owner")).unwrap_or_default();
            if let Ok(Some(sid)) = store.find_session_id_pub(owner.trim()) {
                out.insert(sid);
            }
        }
    }
    out
}

/// 归属会话已离线（进程退出、Meowo 重启后托管终端随之退出）时，看板标不上「待处理」，
/// watch.rs 的通知循环也会跳过它——leader 在手机上就完全不知道有关卡在等他（做空审查第二轮 H1）。
/// 这种情况直接推一次手机推送（Bark，用户配置过才会发），每个关卡只推一次。
fn notify_offline_gates(
    store: &meowo_store::Store,
    want: &std::collections::HashSet<i64>,
    pushed: &mut std::collections::HashSet<String>,
) {
    let settings = crate::settings::load_settings();
    let lang = crate::settings::ui_lang(&settings);
    let Ok(cwds) = store.recent_cwds(20) else {
        return;
    };
    for cwd in cwds {
        for (task, gate) in meowo_reporter::lead::project_gates(&cwd) {
            if gate.status != "pending" || pushed.contains(&gate.id) {
                continue;
            }
            let owner = std::fs::read_to_string(task.join(".owner")).unwrap_or_default();
            let Ok(Some(sid)) = store.find_session_id_pub(owner.trim()) else {
                continue;
            };
            // 在线的会话由看板标记 + watch.rs 的常规通知负责；只有标不上（离线）的才走这里。
            let marked = store.pending_review_of(sid).ok().flatten().as_deref() == Some("gate");
            if want.contains(&sid)
                && marked
                && crate::terminal::session_agent_alive(store, sid).unwrap_or(false)
            {
                continue;
            }
            pushed.insert(gate.id.clone());
            let project = std::path::Path::new(&cwd)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            crate::push::push_session(
                &settings,
                crate::push::PushMessage {
                    title: format!(
                        "{project} · {}",
                        crate::settings::tr(lang, "notify.pending.gate")
                    ),
                    body: gate.title.clone(),
                    session_id: sid,
                    urgent: true,
                },
            );
        }
    }
}

/// 关卡挂起 → 会话标成 pending_review=gate：看板/侧栏/托盘的「待处理」、桌面通知、手机推送都复用
/// 现有链路（watch.rs 按 pending_review 指纹发通知）。关卡答完或撤掉 → 清掉。reporter 处理 Stop 时会
/// 清 pending_review，所以每轮都按文件重新对齐，而不是只写一次。
pub(crate) fn spawn_gate_watcher(app: tauri::AppHandle) {
    use tauri::Manager;
    let spawned = std::thread::Builder::new()
        .name("lead-gates".into())
        .spawn(move || {
            refresh_if_installed();
            // dev 门只限制自动写配置；运行时缓存仍从实际文件建立，普通会话启动不再扫描。
            let _ = provider_installation_states();
            // 已经为哪些关卡推送过（每个关卡只推一次）。重启后会再推一次，可以接受。
            let mut pushed: std::collections::HashSet<String> = std::collections::HashSet::new();
            loop {
                std::thread::sleep(GATE_POLL);
                let db_path = app.state::<crate::AppState>().db_path.clone();
                let Ok(store) = crate::open_store(&db_path) else {
                    continue;
                };
                let want = scan_gate_states(&store, |task| {
                    crate::lead_conversation::reconcile_task(&db_path, task, Some(app.clone()));
                });
                let now = crate::now_ms();
                let mut changed = false;
                notify_offline_gates(&store, &want, &mut pushed);
                for sid in &want {
                    if store.pending_review_of(*sid).ok().flatten().is_none() {
                        // 只在空着时写：真有工具审批/提问在等时，不拿关卡盖掉它。会话已 ended 时写不进去
                        // （set_pending_review 的条件），以实际是否变成 gate 为准，避免每轮都刷看板（做空审查 M1）。
                        let _ =
                            store.set_pending_review(*sid, meowo_store::PendingReview::Gate, now);
                        changed |=
                            store.pending_review_of(*sid).ok().flatten().as_deref() == Some("gate");
                    }
                }
                // 以库为准清理：Meowo 重启后内存里没有上次标过谁，残留的 gate 标记也要清掉。
                for sid in store
                    .sessions_with_pending_review("gate")
                    .unwrap_or_default()
                {
                    if !want.contains(&sid) {
                        let _ = store.clear_pending_review_kind(
                            sid,
                            meowo_store::PendingReview::Gate,
                            now + 1,
                        );
                        changed |= store.pending_review_of(sid).ok().flatten().is_none();
                    }
                }
                if changed {
                    crate::watch::emit_board_changed(&app, "lead-gate");
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("Lead 关卡巡检线程启动失败: {error}");
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;

    /// 测试用临时目录（app crate 不引 tempfile：现有测试一律 temp_dir + 唯一名）。
    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p =
                std::env::temp_dir().join(format!("meowo-lead-install-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn settings_of(dir: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap()
    }

    fn lead_cmds(v: &Value) -> Vec<String> {
        let mut out = vec![];
        let Some(obj) = v["hooks"].as_object() else {
            return out;
        };
        for groups in obj.values() {
            for g in groups.as_array().into_iter().flatten() {
                for h in g["hooks"].as_array().into_iter().flatten() {
                    if let Some(c) = h["command"].as_str().filter(|c| is_lead_hook(c)) {
                        out.push(c.to_owned());
                    }
                }
            }
        }
        out
    }

    #[test]
    fn install_then_uninstall_restores_user_settings() {
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(dir.join("skills/mine")).unwrap();
        std::fs::write(dir.join("skills/mine/SKILL.md"), "用户自己的 skill").unwrap();
        let original = json!({
            "model": "opus",
            "hooks": {
                "Stop": [{"hooks": [{"type": "command", "command": "\"C:/m/meowo-reporter.exe\""}]}],
                "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "my-guard.sh"}]}]
            }
        });
        write_settings(&dir.join("settings.json"), &original).unwrap();

        install_into(&dir, "C:/m/meowo-reporter.exe", None).unwrap();
        assert!(installed_in(&dir));
        assert!(dir.join("agents/lead-redteam.md").exists());
        let v = settings_of(&dir);
        let cmds = lead_cmds(&v);
        for mode in ["session", "claim", "stop", "wait"] {
            assert!(
                cmds.iter().any(|c| c.ends_with(&format!(" lead {mode}"))),
                "{mode}: {cmds:?}"
            );
        }
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["Stop"][0], original["hooks"]["Stop"][0]);
        assert_eq!(v["hooks"]["PreToolUse"], original["hooks"]["PreToolUse"]);
        let waits: Vec<&Value> = v["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|g| g["hooks"].as_array().unwrap().iter())
            .filter(|h| h["command"].as_str().unwrap().ends_with(" lead wait"))
            .collect();
        assert_eq!(waits.len(), 1);
        assert_eq!(waits[0]["asyncRewake"], true);
        assert!(waits[0]["timeout"].as_u64().unwrap() > 7 * 24 * 3600);

        install_into(&dir, "C:/m/meowo-reporter.exe", None).unwrap();
        assert_eq!(
            lead_cmds(&settings_of(&dir)).len(),
            cmds.len(),
            "重复安装不重复追加"
        );

        uninstall_from(&dir).unwrap();
        assert_eq!(settings_of(&dir), original, "卸载后要与安装前 JSON 等价");
        assert!(!installed_in(&dir));
        assert!(!dir.join("skills/lead").exists() && !dir.join("agents/lead-redteam.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("skills/mine/SKILL.md")).unwrap(),
            "用户自己的 skill"
        );
    }

    /// 启动刷新（refresh_if_installed）每次都会走 install_into：内容没变就不能重写用户的
    /// settings.json（无谓改 mtime、和并行写 settings 的 claude 抢文件）；首次改写前要留原件备份。
    #[test]
    fn reinstall_with_same_content_does_not_rewrite_settings() {
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        let original = "{\n  \"model\": \"opus\"\n}\n";
        std::fs::write(&settings, original).unwrap();

        install_into(&dir, "C:/m/meowo-reporter.exe", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json.cckb-bak")).unwrap(),
            original
        );
        // 把 mtime 拨回很久以前：第二次只要发生了写入，mtime 必然变化，不受文件系统时间粒度影响。
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        let pin_mtime = |p: &Path| {
            std::fs::File::options()
                .write(true)
                .open(p)
                .unwrap()
                .set_modified(old)
                .unwrap()
        };
        let skill = dir.join("skills/lead/SKILL.md");
        let agent = dir.join("agents/lead-redteam.md");
        for p in [&settings, &skill, &agent] {
            pin_mtime(p);
        }
        let text = std::fs::read_to_string(&settings).unwrap();

        install_into(&dir, "C:/m/meowo-reporter.exe", None).unwrap();
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), text);
        for p in [&settings, &skill, &agent] {
            assert_eq!(
                std::fs::metadata(p).unwrap().modified().unwrap(),
                old,
                "{} 被重写",
                p.display()
            );
        }
    }

    #[test]
    fn meowo_startup_rewire_keeps_lead_hooks() {
        // Meowo 每次启动都按 HookSpec 整理自己的条目并删掉重复项。lead 条目带参数（`lead stop`），
        // 认领规则要求余参恰好为空，所以它们应被当作「别人的 hook」原样保留。这里用与 claude 插件
        // 相同的命令形态构造 spec（plugins/claude/mod.rs 的 HOOKS），防止将来改认领规则时误删。
        use meowo_agent::{
            CommandSpec, ConfigFormat, EnsureOutcome, HookEvent, HookSpec, MissingConfig,
        };
        static EV: [HookEvent; 3] = [
            HookEvent::matched("SessionStart", "*"),
            HookEvent::matched("PostToolUse", "*"),
            HookEvent::matched("Stop", "*"),
        ];
        static SPEC: HookSpec = HookSpec {
            config_rel: "settings.json",
            format: ConfigFormat::ClaudeJson,
            missing: MissingConfig::CreateFrom("{}"),
            events: &EV,
            command: CommandSpec {
                quote_exe: true,
                with_provider: false,
                ps_call_operator: false,
            },
        };
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let reporter = "C:/m/meowo-reporter.exe";
        install_into(&dir, reporter, None).unwrap();
        let before = lead_cmds(&settings_of(&dir));
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        let after_text = match SPEC.ensure_hooks(&text, reporter, "claude") {
            EnsureOutcome::Changed(t) => t,
            EnsureOutcome::Unchanged => text,
            EnsureOutcome::Abandon(r) => panic!("{r:?}"),
        };
        assert_eq!(
            lead_cmds(&serde_json::from_str(&after_text).unwrap()),
            before
        );
    }

    #[test]
    fn replaces_python_prototype_and_respects_user_files() {
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let proto = json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": "python \"C:/Users/x/.claude/lead/hook.py\" stop"}]}]}});
        write_settings(&dir.join("settings.json"), &proto).unwrap();
        install_into(&dir, "C:/m/meowo-reporter.exe", None).unwrap();
        assert!(
            !lead_cmds(&settings_of(&dir))
                .iter()
                .any(|c| c.contains("hook.py")),
            "初版 Python hook 要被接管"
        );
        assert_eq!(
            install_into(&tmp.path().join("nope"), "x", None),
            Err("lead/claude_missing".into())
        );
        let dir2 = tmp.path().join(".claude2");
        std::fs::create_dir_all(dir2.join("skills/lead")).unwrap();
        std::fs::write(dir2.join("skills/lead/SKILL.md"), "我自己的 lead").unwrap();
        assert_eq!(
            install_into(&dir2, "x", None),
            Err("lead/user_file_exists".into())
        );
        uninstall_from(&dir2).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir2.join("skills/lead/SKILL.md")).unwrap(),
            "我自己的 lead"
        );
    }

    #[test]
    fn takes_over_python_prototype_files_with_backup_and_migrates_registry() {
        // 做空审查 H3：作者机器上的真实情况——初版的 skill 和子代理文件都在、都没有托管标记，
        // 而且 ~/.claude/lead/hook.py 在。原来直接报 user_file_exists，接管落空。
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(dir.join("skills/lead")).unwrap();
        std::fs::create_dir_all(dir.join("agents")).unwrap();
        std::fs::create_dir_all(dir.join("lead")).unwrap();
        std::fs::write(dir.join("skills/lead/SKILL.md"), "初版 skill").unwrap();
        std::fs::write(dir.join("agents/lead-redteam.md"), "初版做空").unwrap();
        std::fs::write(dir.join("lead/hook.py"), "# 初版").unwrap();
        std::fs::write(
            dir.join("lead/sessions.json"),
            r#"{"old-sid":"C:/p/.lead/t"}"#,
        )
        .unwrap();
        let reg = tmp.path().join("lead-sessions.json");
        std::fs::write(&reg, r#"{"new-sid":"C:/q/.lead/u"}"#).unwrap();

        install_into(&dir, "C:/m/meowo-reporter.exe", Some(&reg)).unwrap();
        assert!(installed_in(&dir));
        assert_eq!(
            std::fs::read_to_string(dir.join("skills/lead/SKILL.md.prototype-bak")).unwrap(),
            "初版 skill"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("agents/lead-redteam.md.prototype-bak")).unwrap(),
            "初版做空"
        );
        let merged: Value = serde_json::from_str(&std::fs::read_to_string(&reg).unwrap()).unwrap();
        assert_eq!(
            merged,
            json!({"new-sid": "C:/q/.lead/u", "old-sid": "C:/p/.lead/t"})
        );
    }

    #[test]
    fn refuses_non_object_settings() {
        // 做空审查 L3：根节点是数组时原来会 panic
        let tmp = Tmp::new();
        let dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "[]").unwrap();
        assert_eq!(
            install_into(&dir, "x", None),
            Err("lead/settings_invalid".into())
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            "[]",
            "不改用户文件"
        );
    }
}

#[cfg(test)]
mod provider_install_tests {
    use super::*;
    use meowo_agent::AgentPlugin;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "meowo-provider-host-install-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn installation(
            &self,
            plugin: &dyn AgentPlugin,
            directory: &str,
        ) -> meowo_agent::Installation {
            let variant = &plugin.variants()[0];
            let data_dir = self.0.join(directory);
            std::fs::create_dir_all(&data_dir).unwrap();
            meowo_agent::Installation {
                id: plugin.id(),
                variant_tag: variant.tag,
                data_dir_from_env: false,
                data_dir,
                hooks: variant.hooks,
                auth: variant.auth,
                launch: None,
                launch_stem: variant.launch.stem,
                trust: variant.trust,
                profile: None,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn provider_states_keep_asset_install_and_automatic_continuation_separate() {
        let fixture = Fixture::new();
        let providers: [(&dyn AgentPlugin, &str); 4] = [
            (&meowo_agent::plugins::codex::Codex, "0.167.2"),
            (&meowo_agent::plugins::kimi::Kimi, "2.5.3"),
            (&meowo_agent::plugins::gemini::Gemini, "0.65.0"),
            (&meowo_agent::plugins::opencode::Opencode, "1.19.3"),
        ];
        for (plugin, version) in providers {
            let inst = fixture.installation(plugin, plugin.id().as_str());
            let cap = plugin.lead().unwrap();
            let before = provider_installation_state(
                plugin,
                Some(&inst),
                None,
                Some(version),
                Some("meowo-reporter.exe"),
            );
            assert!(before.configured && before.supported);
            assert!(!before.managed && !before.installed && !before.automatic_continue);
            meowo_agent::lead_install::install(cap, &inst, Some(version), "meowo-reporter.exe")
                .unwrap();
            let installed = provider_installation_state(
                plugin,
                Some(&inst),
                None,
                Some(version),
                Some("meowo-reporter.exe"),
            );
            assert!(
                installed.installed
                    && installed.managed
                    && installed.assets_installed
                    && installed.hooks_installed
            );
            assert!(!installed.automatic_continue);
            let unknown = provider_installation_state(
                plugin,
                Some(&inst),
                None,
                None,
                Some("meowo-reporter.exe"),
            );
            assert!(!unknown.supported && unknown.managed);
            assert_eq!(unknown.reason.as_deref(), Some("lead/version_unsupported"));
            meowo_agent::lead_install::uninstall(cap, &inst).unwrap();
            assert!(
                !provider_installation_state(
                    plugin,
                    Some(&inst),
                    None,
                    Some(version),
                    Some("meowo-reporter.exe")
                )
                .managed
            );
        }
    }

    #[test]
    fn installing_one_account_never_installs_or_copies_auth_into_another() {
        let fixture = Fixture::new();
        let plugin = &meowo_agent::plugins::codex::Codex;
        let cap = plugin.lead().unwrap();
        let default = fixture.installation(plugin, "default");
        let work = fixture.installation(plugin, "work");
        std::fs::write(
            default.data_dir.join("auth.json"),
            "test-only credential fixture",
        )
        .unwrap();
        meowo_agent::lead_install::install(cap, &default, Some("0.167.2"), "meowo-reporter.exe")
            .unwrap();
        let pending = provider_installation_state(
            plugin,
            Some(&work),
            Some(("work", "工作")),
            Some("0.167.2"),
            Some("meowo-reporter.exe"),
        );
        assert!(!pending.managed);
        assert_eq!(pending.profile_id.as_deref(), Some("work"));
        meowo_agent::lead_install::install(cap, &work, Some("0.167.2"), "meowo-reporter.exe")
            .unwrap();
        assert!(!work.data_dir.join("auth.json").exists());
        meowo_agent::lead_install::uninstall(cap, &work).unwrap();
        assert!(
            provider_installation_state(
                plugin,
                Some(&default),
                None,
                Some("0.167.2"),
                Some("meowo-reporter.exe")
            )
            .installed
        );
        assert_eq!(
            std::fs::read_to_string(default.data_dir.join("auth.json")).unwrap(),
            "test-only credential fixture"
        );
    }
}
