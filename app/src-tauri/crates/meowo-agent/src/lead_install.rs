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
    pub version_family: (u32, u32),
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

pub fn version_matches(line: Option<&str>, family: (u32, u32)) -> bool {
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
        .is_some_and(|(major, minor, _)| (major, minor) == family)
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
        && matches!(args[1].as_str(), "session" | "claim" | "stop")
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
    let after = hook_edit(cap, spec, &before, reporter, true)?;
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
            let after = hook_edit(cap, spec, &before, "", false)?;
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
            hook_edit(cap, spec, &before, reporter, true).is_ok_and(|after| {
                after == before
                    && cap
                        .extra_config(installation, &before, &after, true)
                        .is_ok_and(|changes| changes.is_empty())
            })
        });
    let activation_issue = cap.activation_issue(installation);
    LeadInstallState {
        supported,
        installed: supported && assets_present && hooks_present && activation_issue.is_none(),
        assets_present,
        hooks_present,
        limitations: spec.limitations.iter().map(|text| (*text).into()).collect(),
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
        .any(|relative| managed_file(&installation.data_dir.join(relative)))
    {
        return true;
    }
    let Ok(Some(before)) = read(&installation.data_dir.join(spec.hook_rel)) else {
        return false;
    };
    match spec.format {
        LeadHookFormat::Plugin => before.contains(MANAGED_MARK),
        _ => hook_edit(cap, spec, &before, "", false).is_ok_and(|after| after != before),
    }
}

#[cfg(test)]
mod tests;
