//! 团队（设置 → Lead 工作流 → 团队）：把产品经理、设计师、开发、审查员四个角色，派活和成长两个技能，
//! 以及它们读的功底清单和模板，装进每个 Claude 配置目录；再挂一个团队守卫 hook（`meowo-reporter team guard`）。
//!
//! 三层（团队规则 R2）：
//! - 托管层：本模块装的文件，带 [`MANAGED_MARK`]，每次启动刷新、卸载只删它们；
//! - 个人层：`<Meowo 数据目录>/team`（品味清单路径、受保护目录、三档权限、成长记录），本模块**从不写**；
//! - 项目层：Lead 目录里的 product/rules/standards。
//!
//! 为什么装进 agents 目录、不做成 Claude Code 插件：插件只能靠命令行参数、环境变量或插件市场加载，
//! Meowo 管不到用户在别处开的会话；agents 目录和 Lead 的 `lead-redteam` 同一套「托管标记 + 启动刷新 +
//! 只删托管文件」。代价是名字不能带冒号（2.1.332 实测 agents 目录不加载 `team:pm`），所以用 `team-` 前缀。
//!
//! 只支持 Claude Code（团队规则 R12）：守卫靠载荷里的子代理身份认角色，只在 Claude Code 上实测过；
//! 装不上守卫的 CLI 不装角色。

use crate::lead::MANAGED_MARK;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// 托管文件：相对 Claude 配置目录的路径 + 正文（含占位符）。新增文件要加进这里，
/// `asset_list_matches_the_directory` 测试会核对它和 team_assets 目录一致。
macro_rules! assets {
    ($($path:literal),* $(,)?) => { &[$(($path, include_str!(concat!("team_assets/", $path)))),*] };
}
const ASSETS: &[(&str, &str)] = assets![
    "agents/team-pm.md",
    "agents/team-designer.md",
    "agents/team-developer.md",
    "agents/team-reviewer.md",
    "skills/team-staff/SKILL.md",
    "skills/team-learn/SKILL.md",
    "meowo-team/USAGE.md",
    "meowo-team/config.example.json",
    "meowo-team/knowledge/designer/craft.md",
    "meowo-team/knowledge/designer/excellence.md",
    "meowo-team/knowledge/designer/styles.md",
    "meowo-team/knowledge/developer/craft.md",
    "meowo-team/knowledge/developer/excellence.md",
    "meowo-team/knowledge/developer/playbook.md",
    "meowo-team/knowledge/pm/craft.md",
    "meowo-team/knowledge/reviewer/craft.md",
    "meowo-team/knowledge/reviewer/excellence.md",
    "meowo-team/knowledge/shared/README.md",
    "meowo-team/knowledge/shared/leader-standards.md",
    "meowo-team/knowledge/shared/raise-ceiling.md",
    "meowo-team/knowledge/shared/templates.md",
    "meowo-team/templates/bench.md",
    "meowo-team/templates/brief.md",
    "meowo-team/templates/contract.json",
    "meowo-team/templates/contract.md",
    "meowo-team/templates/product.md",
    "meowo-team/templates/rules.md",
];

/// 知识与模板目录（相对 Claude 配置目录）。里面的 JSON 模板没法带托管标记，归属靠这个目录里的
/// [`OWNER_FILE`]：它在，目录里列着的文件就是 Meowo 的。
const KNOWLEDGE_DIR: &str = "meowo-team";
const OWNER_FILE: &str = ".meowo-managed";
/// 守卫 hook 挂的工具：Bash/PowerShell 查对外命令，写文件类查写入范围。
const GUARD_MATCHER: &str = "Bash|PowerShell|Write|Edit|MultiEdit|NotebookEdit";

fn team_cmd(reporter: &str) -> String {
    format!("\"{reporter}\" team guard")
}

/// 是不是团队守卫的 hook 条目。不能和 Lead 的认法（`… lead …`）重叠：Lead 每次启动刷新都会删掉它认出的条目。
fn is_team_hook(cmd: &str) -> bool {
    let c = cmd.replace('\\', "/");
    c.contains("meowo-reporter") && c.trim_end().ends_with(" team guard")
}

/// 占位符换成本机绝对路径。一律用正斜杠：Windows 上照样能用，写进 JSON 模板也不用转义。
fn render(body: &str, claude_dir: &Path, personal: &Path) -> String {
    let slash = |p: &Path| p.to_string_lossy().replace('\\', "/");
    body.replace("{{TEAM_DIR}}", &slash(&claude_dir.join(KNOWLEDGE_DIR)))
        .replace("{{PERSONAL_DIR}}", &slash(personal))
}

fn owned_by_mark(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.contains(MANAGED_MARK))
}

fn knowledge_owned(claude_dir: &Path) -> bool {
    owned_by_mark(&claude_dir.join(KNOWLEDGE_DIR).join(OWNER_FILE))
}

/// 这个文件现在归谁：Meowo 的可以覆盖，用户的（存在却没有标记）不碰。
fn is_ours(claude_dir: &Path, relative: &str) -> bool {
    let path = claude_dir.join(relative);
    if !path.exists() {
        return true;
    }
    if relative.starts_with(&format!("{KNOWLEDGE_DIR}/")) {
        knowledge_owned(claude_dir)
    } else {
        owned_by_mark(&path)
    }
}

/// 知识目录被用户的同名目录占着：所有角色都要读它，这时整个目录不装。
fn knowledge_blocked(claude_dir: &Path) -> Option<PathBuf> {
    let knowledge = claude_dir.join(KNOWLEDGE_DIR);
    (knowledge.exists() && !knowledge_owned(claude_dir)).then_some(knowledge)
}

/// 挡住安装的用户文件（团队规则 R5：不覆盖、报出是哪个文件）。角色或技能被挡只跳过它自己，
/// 其他照装（R5 边角 2）；设置页据此列出没装上的文件。
pub(crate) fn blocked_files(claude_dir: &Path) -> Vec<PathBuf> {
    if let Some(dir) = knowledge_blocked(claude_dir) {
        return vec![dir];
    }
    ASSETS
        .iter()
        .filter(|(relative, _)| !is_ours(claude_dir, relative))
        .map(|(relative, _)| claude_dir.join(relative))
        .collect()
}

/// 本机另一份名叫 team 的 Claude Code 插件（用户级环境变量 CLAUDE_CODE_PLUGIN_DIRS 加载的）。
/// 两份同时加载会出现两套产品经理（团队规则 R14）：检测到就不装，提示先去掉，不替用户改环境变量。
pub(crate) fn conflicting_plugin() -> Option<PathBuf> {
    plugin_dirs()
        .into_iter()
        .find(|dir| plugin_name(dir).as_deref() == Some("team"))
}

fn plugin_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok()?;
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    v.get("name")?.as_str().map(str::to_owned)
}

/// 进程环境变量之外，Windows 上再读一次注册表里的用户级值：用户刚用 setx 改过，
/// 已经在跑的 Meowo 进程看到的还是旧值（设置页让用户「先去掉再勾选」，不能要求先重启 Meowo）。
fn plugin_dirs() -> Vec<PathBuf> {
    let process: Vec<String> = std::env::var("CLAUDE_CODE_PLUGIN_DIRS").ok().into_iter().collect();
    let values = registry_plugin_dirs().unwrap_or(process);
    let sep = if cfg!(windows) { ';' } else { ':' };
    values
        .iter()
        .flat_map(|v| v.split(sep))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Windows 用户级注册表里的值；读得到就以它为准（用户已经去掉了，就不能再拿进程里的旧值去挡）。
/// 验收台沙箱（debug 构建 + MEOWO_DEV_CLAUDE_HOME，见 lead::claude_dir）只看进程环境变量，不读用户真实的注册表。
#[cfg(windows)]
fn registry_plugin_dirs() -> Option<Vec<String>> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let sandbox = cfg!(debug_assertions) && std::env::var_os("MEOWO_DEV_CLAUDE_HOME").is_some_and(|v| !v.is_empty());
    if sandbox {
        return None;
    }
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment").ok()?;
    Some(key.get_value::<String, _>("CLAUDE_CODE_PLUGIN_DIRS").ok().into_iter().collect())
}

/// 其他平台的用户级环境变量没有可靠的「最新值」来源，只看进程环境变量。
#[cfg(not(windows))]
fn registry_plugin_dirs() -> Option<Vec<String>> {
    None
}

/// 装着 = 知识目录归 Meowo。某个角色被用户同名文件挡住时也算装着，没装上的文件另外列出。
pub(crate) fn installed_in(claude_dir: &Path) -> bool {
    knowledge_owned(claude_dir)
}

/// 装进一个 Claude 配置目录。可重复执行：内容没变的文件不写，旧版本留下、这版不再有的托管文件删掉。
pub(crate) fn install_into(claude_dir: &Path, reporter: &str, personal: &Path) -> Result<(), String> {
    if !claude_dir.is_dir() {
        return Err("lead/claude_missing".into());
    }
    if let Some(path) = knowledge_blocked(claude_dir) {
        return Err(format!("team/user_file_exists：{}", path.display()));
    }
    // 先挂守卫，再放角色：settings.json 坏了时守卫挂不上，这时角色一个也不能落盘（团队规则 R12：
    // 没有守卫的角色等于没有权限边界；redteam-2 指出原先先写角色、最后才改 settings）。
    edit_settings(claude_dir, Some(reporter))?;
    let previous = owned_list(claude_dir);
    for (relative, body) in ASSETS {
        if !is_ours(claude_dir, relative) {
            continue; // 用户的同名角色或技能：不覆盖，只跳过这一个
        }
        let path = claude_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_if_changed(&path, &render(body, claude_dir, personal))?;
    }
    let owner = claude_dir.join(KNOWLEDGE_DIR).join(OWNER_FILE);
    write_if_changed(&owner, &owner_body())?;
    for stale in previous.iter().filter(|p| !ASSETS.iter().any(|(r, _)| r == p)) {
        remove_owned(claude_dir, stale);
    }
    Ok(())
}

/// 卸载：只删托管文件和团队守卫 hook，用户在同目录放的东西原样保留。
pub(crate) fn uninstall_from(claude_dir: &Path) -> Result<(), String> {
    let mut owned = owned_list(claude_dir);
    owned.extend(ASSETS.iter().map(|(r, _)| (*r).to_owned()));
    owned.sort();
    owned.dedup();
    for relative in &owned {
        remove_owned(claude_dir, relative);
    }
    let owner = claude_dir.join(KNOWLEDGE_DIR).join(OWNER_FILE);
    if owned_by_mark(&owner) {
        std::fs::remove_file(&owner).map_err(|e| e.to_string())?;
    }
    remove_empty_dirs(&claude_dir.join(KNOWLEDGE_DIR));
    edit_settings(claude_dir, None)
}

/// 归属文件：托管标记 + 这一版装了哪些文件（下一版据此删掉不再有的文件，卸载据此只删自己的）。
fn owner_body() -> String {
    let files: Vec<&str> = ASSETS.iter().map(|(r, _)| *r).collect();
    format!("{MANAGED_MARK}。这个目录和下面列出的文件由 Meowo 的「团队」开关管理，升级会覆盖，关掉开关会删除。\n要长期保留的改动写进个人层（Meowo 数据目录下的 team 文件夹）。\n\n{}\n", files.join("\n"))
}

fn owned_list(claude_dir: &Path) -> Vec<String> {
    let owner = claude_dir.join(KNOWLEDGE_DIR).join(OWNER_FILE);
    let Ok(text) = std::fs::read_to_string(&owner) else { return Vec::new() };
    if !text.contains(MANAGED_MARK) {
        return Vec::new();
    }
    text.lines()
        .map(str::trim)
        .filter(|l| safe_relative(l))
        .map(str::to_owned)
        .collect()
}

/// 归属文件是磁盘上的普通文本，删除前要确认路径落在我们管的三处里、不带 `..`。
fn safe_relative(relative: &str) -> bool {
    let p = Path::new(relative);
    let normal = !relative.is_empty() && p.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    let in_scope = relative.starts_with(&format!("{KNOWLEDGE_DIR}/"))
        || (relative.starts_with("agents/team-") && relative.ends_with(".md"))
        || (relative.starts_with("skills/team-") && relative.ends_with("/SKILL.md"));
    normal && in_scope
}

fn remove_owned(claude_dir: &Path, relative: &str) {
    if !safe_relative(relative) {
        return;
    }
    let path = claude_dir.join(relative);
    // 知识目录靠归属文件认；agents/skills 下的文件逐个看标记，用户去掉标记的就是他的了（R5 边角 3）。
    let ours = if relative.starts_with(&format!("{KNOWLEDGE_DIR}/")) {
        knowledge_owned(claude_dir)
    } else {
        owned_by_mark(&path)
    };
    if ours && path.is_file() {
        let _ = std::fs::remove_file(&path);
        if relative.starts_with("skills/") {
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir); // 空了才删得掉
            }
        }
    }
}

fn remove_empty_dirs(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            remove_empty_dirs(&entry.path());
        }
    }
    let _ = std::fs::remove_dir(dir);
}

fn write_if_changed(path: &Path, body: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|t| t == body) {
        return Ok(());
    }
    meowo_agent::fsutil::write_atomic(path, body).map_err(|e| e.to_string())
}

/// 守卫 hook：先删掉所有团队条目，装的时候再加一组。目标状态没变就不写 settings.json（理由同 Lead：
/// 每次启动刷新都改用户文件，会多出和 claude 并发写同一文件的窗口）。
fn edit_settings(claude_dir: &Path, reporter: Option<&str>) -> Result<(), String> {
    let path = claude_dir.join("settings.json");
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(text) if text.trim().is_empty() => json!({}),
        Ok(text) => match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
            Ok(v) if v.is_object() => v,
            _ => return Err("lead/settings_invalid".into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if reporter.is_none() {
                return Ok(());
            }
            json!({})
        }
        Err(e) => return Err(e.to_string()),
    };
    if settings.get("hooks").is_some_and(|h| !h.is_object()) {
        return Err("lead/settings_invalid".into());
    }
    let before = settings.clone();
    strip_team_hooks(&mut settings);
    if let Some(reporter) = reporter {
        if !settings["hooks"].is_object() {
            settings["hooks"] = json!({});
        }
        let group = json!({"matcher": GUARD_MATCHER, "hooks": [{"type": "command", "command": team_cmd(reporter), "timeout": 10}]});
        let mut groups = settings["hooks"]["PreToolUse"].as_array().cloned().unwrap_or_default();
        groups.push(group);
        settings["hooks"]["PreToolUse"] = Value::Array(groups);
    }
    if settings == before {
        return Ok(());
    }
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())? + "\n";
    if path.exists() {
        meowo_agent::wiring::backup_once(&path).map_err(|e| e.to_string())?;
    }
    meowo_agent::fsutil::write_atomic(&path, &text).map_err(|e| e.to_string())
}

fn strip_team_hooks(settings: &mut Value) {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else { return };
    for groups in hooks.values_mut() {
        if let Some(arr) = groups.as_array_mut() {
            for g in arr.iter_mut() {
                if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                    hs.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(is_team_hook));
                }
            }
            arr.retain(|g| g.get("hooks").and_then(Value::as_array).is_none_or(|hs| !hs.is_empty()));
        }
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
        }
    }
}

fn personal_dir() -> Result<PathBuf, String> {
    meowo_reporter::team_personal_dir().ok_or_else(|| "lead/claude_missing".into())
}

/// 开关落到每个目录（同 Lead 的 apply_to_dirs）：默认目录失败直接报，账号目录失败只记日志——
/// 报了错前端会保持旧的开关状态，可默认目录已经改了，开关就和实际相反。
fn apply_to_dirs(dirs: &[PathBuf], reporter: Option<&str>) -> Result<(), String> {
    let Some((home, profiles)) = dirs.split_first() else { return Ok(()) };
    let personal = personal_dir()?;
    match reporter {
        Some(r) => install_into(home, r, &personal)?,
        None => uninstall_from(home)?,
    }
    for dir in profiles {
        let result = match reporter {
            Some(r) => install_into(dir, r, &personal),
            None => uninstall_from(dir),
        };
        if let Err(e) = result {
            eprintln!("团队（{}）: {e}", dir.display());
        }
    }
    Ok(())
}

/// 启动刷新、新建账号时由 Lead 那边顺带调用：默认目录装着团队才动。
pub(crate) fn refresh_dirs(dirs: &[PathBuf], reporter: &str) {
    let Some(home) = dirs.first() else { return };
    if !installed_in(home) {
        return;
    }
    let Ok(personal) = personal_dir() else { return };
    for dir in dirs {
        if let Err(e) = install_into(dir, reporter, &personal) {
            eprintln!("团队启动时刷新失败（{}）: {e}", dir.display());
        }
    }
}

/// 关掉 Lead 时团队一起卸：团队是 Lead 的扩展（团队规则 R10），没有 Lead 的任务目录和关卡它跑不起来。
pub(crate) fn uninstall_dirs(dirs: &[PathBuf]) {
    for dir in dirs {
        if let Err(e) = uninstall_from(dir) {
            eprintln!("团队随 Lead 卸载失败（{}）: {e}", dir.display());
        }
    }
}

#[tauri::command]
pub(crate) async fn team_workflow_state() -> Result<meowo_protocol::ipc::TeamWorkflowStateDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = crate::lead::default_claude_dir();
        let installed = home.as_deref().is_some_and(installed_in);
        // 账号目录装失败只进日志（同 Lead，见 apply_to_dirs），所以设置页要从磁盘现状里把没装上的文件找出来，
        // 不然账号里有同名文件时开关显示装好、却没人知道哪个账号缺了角色（做空 redteam-1 I1）。
        let blocked = if installed {
            crate::lead::all_claude_dirs()
                .iter()
                .flat_map(|d| {
                    // 整个账号目录没装上（settings.json 坏了、没权限）：列出目录本身。
                    let mut files = blocked_files(d);
                    if files.is_empty() && !installed_in(d) {
                        files.push(d.clone());
                    }
                    files
                })
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        } else {
            Vec::new()
        };
        let personal = personal_dir().ok();
        Ok(meowo_protocol::ipc::TeamWorkflowStateDto {
            installed,
            lead_installed: home.as_deref().is_some_and(crate::lead::installed_in),
            conflict: conflicting_plugin().map(|p| p.to_string_lossy().into_owned()),
            personal_config_error: personal.as_deref().and_then(meowo_reporter::team_guard::Config::problem),
            personal_dir: personal.map(|p| p.to_string_lossy().into_owned()),
            blocked_files: blocked,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn set_team_workflow(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dirs = crate::lead::all_claude_dirs();
        let home = dirs.first().ok_or("lead/claude_missing")?;
        if !enabled {
            return apply_to_dirs(&dirs, None);
        }
        if !crate::lead::installed_in(home) {
            return Err("team/lead_required".into());
        }
        if let Some(dir) = conflicting_plugin() {
            return Err(format!("team/plugin_conflict：{}", dir.display()));
        }
        let reporter = crate::setup::sibling_reporter().ok_or("lead/reporter_missing")?;
        apply_to_dirs(&dirs, Some(&reporter))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
