//! `team guard`（Claude 的 PreToolUse，挂 Bash|PowerShell|Write|Edit|NotebookEdit）：团队角色
//! （`team-pm`、`team-designer`、`team-developer`、`team-reviewer`）的权限边界。主会话和其他子代理不管。
//!
//! 为什么在钩子里拦、不靠角色文件的 frontmatter：Claude Code 2.1.332 实测（2026-11-25，`claude -p` +
//! agents 目录里的探针角色），角色 frontmatter 里写的 `hooks:` 不触发，`disallowedTools` 也不支持按命令
//! 前缀禁用（写 `Bash(git push:*)` 会把整个 Bash 拿掉）。settings.json 的钩子能看到载荷里的
//! `agent_type`（agents/*.md 的 name；主会话没有这个字段），在代码里按角色拦。
//!
//! 为什么不挂在 `lead` 子命令下：宿主的 Lead 安装按「meowo-reporter … lead …」认自己的 hook 条目，
//! 每次启动刷新都会删掉再重加；团队的条目要由团队开关自己管。
//!
//! 与 `lead guard` 的分工：那边只认 `lead-redteam`，这边只认 `team-` 开头的角色，两边不重叠。
//!
//! 拿不准时的方向和 `lead guard` 相反：载荷里有 `agent_id`（是子代理）却没有 `agent_type`，说明
//! Claude Code 的载荷格式变了、认不出是谁，按最严的团队角色处理——没有守卫的团队角色等于没有
//! 权限边界（团队规则 R12）。主会话（两个字段都没有）和解析不了的载荷照常放行，不能卡住正常会话。

use regex::Regex;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

pub const ROLE_PREFIX: &str = "team-";
/// 只能写 Lead 目录和临时目录的角色：产出是方案、规则表、审查报告，不该碰源码。
const WRITE_LIMITED: [&str; 2] = ["team-pm", "team-reviewer"];

/// 团队角色一律不能做的对外或不可逆动作：由主会话（技术负责人）按用户授权去做。
/// 匹配命令全文而不只看开头，`bash -c "git push"`、`cmd /c` 之类的包装也会被拦。
const FORBIDDEN: &[(&str, &str)] = &[
    (r"\bgit\b[^\n;&|]*\bpush\b", "推送"),
    (r"\bgh\s+pr\s+(create|merge|review|comment|edit|close|reopen|ready)\b", "开、改、合并或评论 PR"),
    (r"\bgh\s+(release|workflow\s+run|run\s+rerun|repo\s+(create|delete|edit))\b", "发版或触发 CI"),
    (r"\bgh\s+api\b[^\n]*(-X\s*|--method\s+)(POST|PUT|PATCH|DELETE)\b", "发 GitHub 写请求"),
    (r"\bgh\s+api\b[^\n]*\s(-f|-F|--field|--raw-field|--input)\s", "发 GitHub 写请求"),
    (r"\bkubectl\b|\bhelm\s+(install|upgrade|uninstall|rollback)\b", "操作集群"),
    (r"\b(npm|pnpm|yarn|bun)\s+publish\b|\bcargo\s+publish\b|\beas\s+(submit|update)\b", "发布包"),
    (r"\blark-cli\b[^\n]*\+(messages-send|messages-reply)", "发聊天消息"),
];
/// 碰到受保护目录时，哪些命令算「改动」。只读命令（ls、cat、git log）放行。
const MUTATING: &str = r"(?i)\b(rm|mv|cp|del|rmdir|move|copy|sed\s+-i|tee|Set-Content|Out-File|Remove-Item|Move-Item|Copy-Item|New-Item|git\b[^\n;&|]*\b(commit|reset|checkout|switch|merge|rebase|stash|clean|add|restore|apply|worktree|push|pull|cherry-pick|revert))\b|>";

/// 个人层 `config.json` 里守卫关心的部分（其余字段给角色读，这里不管）。
#[derive(Default, Debug)]
pub struct Config {
    /// 受保护目录：目录名（可带 `*` 通配）或路径前缀（含 `/` 或 `\`），不区分大小写。
    pub protected: Vec<String>,
    /// 用户自己追加的禁止命令（正则），例如本机发消息的脚本。写错的正则跳过，不让守卫整体失效。
    pub blocked_commands: Vec<(String, String)>,
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let Ok(v) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
            return Self::default();
        };
        let protected = v
            .get("protected")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
            .unwrap_or_default();
        let blocked_commands = v
            .get("blocked_commands")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let pattern = item.get("pattern")?.as_str()?.to_owned();
                        let what = item.get("what").and_then(Value::as_str).unwrap_or("执行这条命令").to_owned();
                        Some((pattern, what))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { protected, blocked_commands }
    }

    /// 个人层 `<Meowo 数据目录>/team/config.json`；读不到就是空配置（公开版默认没有受保护目录）。
    pub fn load(personal_dir: Option<&Path>) -> Self {
        personal_dir
            .and_then(|dir| std::fs::read_to_string(dir.join("config.json")).ok())
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }
}

/// 入口：返回 Some(stdout) 表示拒绝；None 放行。`temp_dirs` 是允许受限角色写入的临时目录。
pub fn check(raw: &str, config: impl FnOnce() -> Config, temp_dirs: &[PathBuf]) -> Option<String> {
    // 快速路径：主会话的每条命令都会走到这里，不是子代理就不解析 JSON。
    if !raw.contains("\"agent_id\"") && !raw.contains("\"agent_type\"") {
        return None;
    }
    let data: Value = serde_json::from_str(raw).ok()?;
    let role = role_of(&data)?;
    let config = config();
    let tool = data.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let input = data.get("tool_input").cloned().unwrap_or(Value::Null);
    let reason = match tool {
        "Bash" | "PowerShell" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            check_command(&role, command, &config)
        }
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let target = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let cwd = data.get("cwd").and_then(Value::as_str);
            check_write(&role, target, cwd, &config, temp_dirs)
        }
        _ => None,
    }?;
    Some(
        json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": format!("团队守卫：{reason}。需要的话写进交付，由主会话按用户授权去做。"),
        }})
        .to_string(),
    )
}

/// 团队角色返回角色名；认不出的子代理返回 `"未识别的子代理"`（按最严处理）；其他返回 None。
fn role_of(data: &Value) -> Option<String> {
    match data.get("agent_type").and_then(Value::as_str) {
        Some(t) if t.starts_with(ROLE_PREFIX) => Some(t.to_owned()),
        Some(_) => None,
        None if data.get("agent_id").and_then(Value::as_str).is_some_and(|id| !id.is_empty()) => {
            Some("未识别的子代理".to_owned())
        }
        None => None,
    }
}

fn limited(role: &str) -> bool {
    WRITE_LIMITED.contains(&role) || !role.starts_with(ROLE_PREFIX)
}

fn check_command(role: &str, command: &str, config: &Config) -> Option<String> {
    let short: String = command.chars().take(80).collect();
    for (pattern, what) in FORBIDDEN {
        if Regex::new(&format!("(?i){pattern}")).is_ok_and(|re| re.is_match(command)) {
            return Some(format!("{role} 不能{what}（命令：{short}）"));
        }
    }
    for (pattern, what) in &config.blocked_commands {
        if Regex::new(pattern).is_ok_and(|re| re.is_match(command)) {
            return Some(format!("{role} 不能{what}（个人层 blocked_commands；命令：{short}）"));
        }
    }
    if let Some(hit) = protected_in_command(command, &config.protected) {
        if Regex::new(MUTATING).is_ok_and(|re| re.is_match(command)) {
            return Some(format!("{role} 不能改动受保护目录（个人层 protected 里的「{hit}」）"));
        }
    }
    None
}

fn check_write(role: &str, target: &str, cwd: Option<&str>, config: &Config, temp_dirs: &[PathBuf]) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    let mut path = PathBuf::from(target);
    if path.is_relative() {
        if let Some(cwd) = cwd {
            path = Path::new(cwd).join(path);
        }
    }
    if let Some(hit) = protected_hit(&path, &config.protected) {
        return Some(format!("{role} 不能写受保护目录里的文件（{target}；个人层 protected 里的「{hit}」）"));
    }
    if limited(role) && !lead_or_temp(&path, temp_dirs) {
        return Some(format!(
            "{role} 只能把产出写在 Lead 目录（路径里有 .lead 的任务目录）或临时目录，不能写 {target}"
        ));
    }
    None
}

/// 路径里有 `.lead` 一段（Lead 目录在 Meowo 数据目录 `projects/<项目>/.lead/` 下，老项目在项目根），
/// 或在临时目录下。带 `..` 的一律不算：不能借 `.lead/../` 跳出去。
fn lead_or_temp(path: &Path, temp_dirs: &[PathBuf]) -> bool {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    if path.components().any(|c| matches!(c, Component::Normal(s) if s.eq_ignore_ascii_case(".lead"))) {
        return true;
    }
    let lower = norm(&path.to_string_lossy());
    temp_dirs.iter().any(|dir| {
        let dir = norm(&dir.to_string_lossy());
        !dir.is_empty() && (lower == dir || lower.starts_with(&format!("{dir}/")))
    })
}

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// 一项受保护规则命中这个路径吗：含分隔符的按路径前缀，否则按任意一级目录名（`*` 通配）。
fn protected_hit(path: &Path, protected: &[String]) -> Option<String> {
    let full = norm(&path.to_string_lossy());
    let names: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect();
    protected.iter().find(|rule| rule_matches(rule, &full, &names)).cloned()
}

fn rule_matches(rule: &str, full: &str, names: &[String]) -> bool {
    if rule.contains('/') || rule.contains('\\') {
        let prefix = norm(rule);
        return !prefix.is_empty() && (full == prefix || full.starts_with(&format!("{prefix}/")));
    }
    let rule = rule.to_lowercase();
    names.iter().any(|name| glob(&rule, name))
}

/// 命令文本里出现的受保护目录：把命令按空白、引号和分隔符切开，逐段按目录名或路径前缀比对。
fn protected_in_command(command: &str, protected: &[String]) -> Option<String> {
    if protected.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '|' | '&' | '(' | ')' | '=' | ','))
        .filter(|t| !t.is_empty())
        .collect();
    for token in tokens {
        let full = norm(token);
        let names: Vec<String> = full.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect();
        if let Some(rule) = protected.iter().find(|rule| rule_matches(rule, &full, &names)) {
            return Some(rule.clone());
        }
    }
    None
}

/// 只支持 `*`（任意多个字符）的通配，整段匹配。
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// 允许受限角色写入的临时目录：系统临时目录（Claude Code 的 scratchpad 也在它下面）。
pub fn default_temp_dirs() -> Vec<PathBuf> {
    vec![std::env::temp_dir()]
}

#[cfg(test)]
mod tests;
