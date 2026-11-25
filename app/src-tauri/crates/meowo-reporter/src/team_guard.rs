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
//!
//! 能力边界：Bash/PowerShell 靠命令文本判断，是尽力而为、不是沙箱。常见的写法（重定向、sed -i、cp/rm、
//! 格式化与 --fix、解释器内联脚本、curl/gh 写请求）都拦，但任意程序在内部写文件、发请求是看不到的
//! （两轮做空审查 redteam-1/2 的结论）。Write/Edit 工具的路径判断是完整的。

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
    (r"\bgh\s+(issue|discussion|gist)\s+(create|comment|edit|close|reopen|delete|lock|transfer|pin)\b", "在 GitHub 上发内容或改状态"),
    (r"\bgh\s+(release|workflow\s+run|run\s+rerun|repo\s+(create|delete|edit))\b", "发版或触发 CI"),
    (r"\bgh\s+api\b[^\n]*(-X|--method)[\s=]*(POST|PUT|PATCH|DELETE)\b", "发 GitHub 写请求"),
    (r"\bgh\s+api\b[^\n]*\s(-f|-F|--field|--raw-field|--input)[\s=]", "发 GitHub 写请求"),
    (r"\bkubectl\b|\bhelm\s+(install|upgrade|uninstall|rollback)\b", "操作集群"),
    (r"\b(npm|pnpm|yarn|bun)\s+publish\b|\bcargo\s+publish\b|\beas\s+(submit|update)\b", "发布包"),
    (r"\blark-cli\b[^\n]*\+(messages-send|messages-reply)", "发聊天消息"),
];
/// 碰到受保护目录时，哪些命令算「改动」。只读命令（ls、cat、git log）放行。重定向 `>` 另外判断（只认引号外的），
/// 格式化和 lint 修复是开发自验时的常规动作，也会改文件（redteam-2 B03/B04/B08/B11）。
const MUTATING: &str = r"(?i)\b(rm|mv|cp|del|rmdir|mkdir|touch|move|copy|ln|chmod|truncate|unzip|sed\s+-i|perl\b[^\n;|]*\s-p?i|tee|Set-Content|Add-Content|Out-File|Remove-Item|Move-Item|Copy-Item|New-Item|Rename-Item|Expand-Archive|fmt|gofmt|dd\b[^\n;|]*\bof=|tar\b[^\n;|]*\s-?x|git\b[^\n;&|]*\b(commit|reset|checkout|switch|merge|rebase|stash|clean|add|restore|apply|am|mv|rm|init|clone|worktree|push|pull|cherry-pick|revert))\b|(^|\s)(--fix|--write|-delete)\b|(^|[\s;|&(])patch\s";
/// 隐含改当前目录的命令：命令里出现的路径是输入（补丁、归档），改的是 cwd（`git apply /tmp/x.patch`、
/// `cargo fmt`、`find . -delete`）。这类命令只看 cwd，命令里的路径在临时目录也不算「只写临时文件」。
const IMPLICIT_CWD: &str = r"(?i)\b(fmt|gofmt|unzip|git(\s+-[Cc]\s+\S+)*\s+(commit|reset|checkout|switch|merge|rebase|stash|clean|add|restore|apply|am|mv|rm|init|clone|pull|cherry-pick|revert))\b|\btar\b[^\n;|]*\s-?x|(^|\s)(--fix|--write|-delete)\b|(^|[\s;|&(])patch\s";
/// 解释器内联脚本（`python -c`、`node -e`）：里面写什么文件看不出来，一律当改动（redteam-2 B01/B02）。
const INLINE_SCRIPT: &str = r"(?i)\b(python3?|py|node|deno|bun|ruby|perl|php|pwsh|powershell)(\.exe)?\b[^\n;|]*\s-(c|e|Command)\b";

/// 个人层 `config.json` 里守卫关心的部分（其余字段给角色读，这里不管）。
#[derive(Default, Debug)]
pub struct Config {
    /// 受保护目录：目录名（可带 `*` 通配）或路径前缀（含 `/` 或 `\`），不区分大小写。
    pub protected: Vec<String>,
    /// 用户自己追加的禁止命令（正则），例如本机发消息的脚本。写错的正则跳过，不让守卫整体失效。
    pub blocked_commands: Vec<(String, String)>,
    /// config.json 在却读不出（写坏了、读不了）：受保护目录无从确认，团队角色的写入和改动命令一律拦。
    /// 当空配置放行的话，用户多打一个逗号受保护目录就静默失效（做空 redteam-1 G1-4）。
    pub broken: Option<String>,
    /// 个人层目录本身：守卫的配置在这里，团队角色不能改（redteam-1 G1-5）。
    pub personal_dir: Option<PathBuf>,
}

impl Config {
    pub fn parse(text: &str) -> Self {
        let v = match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
            Ok(v) if v.is_object() => v,
            Ok(_) => return Self { broken: Some("顶层不是 JSON 对象".into()), ..Self::default() },
            Err(e) => return Self { broken: Some(format!("不是合法的 JSON：{e}")), ..Self::default() },
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
        Self { protected, blocked_commands, broken: None, personal_dir: None }
    }

    /// 个人层 `<Meowo 数据目录>/team/config.json`。文件不存在 = 空配置（公开版默认没有受保护目录）；
    /// 文件在却读不出 = broken。
    pub fn load(personal_dir: Option<&Path>) -> Self {
        let Some(dir) = personal_dir else { return Self::default() };
        let mut config = match std::fs::read_to_string(dir.join("config.json")) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self { broken: Some(format!("读不出：{e}")), ..Self::default() },
        };
        config.personal_dir = Some(dir.to_path_buf());
        config
    }

    /// 给设置页用：个人层 config.json 写坏时说明原因，没坏返回 None。
    pub fn problem(personal_dir: &Path) -> Option<String> {
        Self::load(Some(personal_dir)).broken
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
    let cwd = data.get("cwd").and_then(Value::as_str);
    let reason = match tool {
        "Bash" | "PowerShell" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            check_command(&role, command, cwd, &config, temp_dirs)
        }
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let target = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"))
                .and_then(Value::as_str)
                .unwrap_or("");
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

fn check_command(role: &str, command: &str, cwd: Option<&str>, config: &Config, temp_dirs: &[PathBuf]) -> Option<String> {
    let short: String = command.chars().take(80).collect();
    for (pattern, what) in FORBIDDEN {
        if Regex::new(&format!("(?i){pattern}")).is_ok_and(|re| re.is_match(command)) {
            return Some(format!("{role} 不能{what}（命令：{short}）"));
        }
    }
    if outbound_request(command) {
        return Some(format!("{role} 不能向外部服务发写入请求，可能是在发消息或改远端数据（命令：{short}）"));
    }
    for (pattern, what) in &config.blocked_commands {
        if Regex::new(pattern).is_ok_and(|re| re.is_match(command)) {
            return Some(format!("{role} 不能{what}（个人层 blocked_commands；命令：{short}）"));
        }
    }
    if !mutating(command) {
        return None;
    }
    if let Some(problem) = &config.broken {
        return Some(format!("个人层 config.json {problem}，受保护目录无法确认，{role} 先不能执行改动类命令。请用户修好 config.json"));
    }
    let cwd_path = cwd.map(PathBuf::from);
    let targets = path_tokens(command, cwd_path.as_deref());
    // 命令里的路径都落在 Lead 目录或临时目录：改的不是源码，受保护仓库里往这两处落报告、测试日志也放行
    // （redteam-2 M17/M19）。
    let implicit = Regex::new(IMPLICIT_CWD).is_ok_and(|re| re.is_match(&unquoted(command)))
        || Regex::new(INLINE_SCRIPT).is_ok_and(|re| re.is_match(command));
    let only_scratch = !implicit && !targets.is_empty() && targets.iter().all(|p| lead_or_temp(p, temp_dirs));
    if let Some(hit) = protected_in_command(command, &config.protected) {
        return Some(format!("{role} 不能改动受保护目录（个人层 protected 里的「{hit}」）"));
    }
    // 子代理的 cwd 继承主会话：主会话开在受保护仓库里时，`git commit -am x`、`rm -rf src` 这类命令不带路径，
    // 只看命令文本会全部放行（做空 redteam-1 G1-1）。
    if let Some(hit) = cwd_path.as_deref().and_then(|p| protected_hit(p, &config.protected)) {
        if !only_scratch {
            return Some(format!("{role} 不能在受保护目录里执行改动类命令（个人层 protected 里的「{hit}」）"));
        }
    }
    if targets.iter().any(|p| guard_config(p, config.personal_dir.as_deref())) {
        return Some(format!("{role} 不能改动团队守卫的配置（个人层、Claude 配置目录里的 settings.json 和团队文件）"));
    }
    // 产品经理和审查员的写入范围不能只管 Write 工具：Bash 里的 `> file`、`sed -i`、`Set-Content` 一样能写源码
    // （redteam-1 G1-2）。改动类命令里出现的路径都要在 Lead 目录或临时目录；一个路径都没有时，看所在目录。
    if limited(role) {
        // 命令里的路径都要在 Lead/临时目录；隐含改 cwd 的命令、或者一个路径都没写的，cwd 本身也要在。
        let cwd_ok = cwd_path.as_deref().is_some_and(|p| lead_or_temp(p, temp_dirs));
        let targets_ok = targets.iter().all(|p| lead_or_temp(p, temp_dirs));
        let inside = targets_ok && (cwd_ok || (!implicit && !targets.is_empty()));
        if !inside {
            return Some(format!(
                "{role} 执行改动类命令时，命令里的路径都要在 Lead 目录（路径里有 .lead）或临时目录里；要落临时文件请整条命令都用临时目录的路径（命令：{short}）"
            ));
        }
    }
    None
}

/// 去掉引号里的内容：`grep "=>" src`、`git log --format="<%ae>"` 里的 `>` 不是重定向（redteam-2 M05~M07）。
fn unquoted(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None => out.push(c),
        }
    }
    out
}

/// 改动类命令。重定向只认引号外的，并先去掉不落盘的（`2>&1`、`>/dev/null`、`> $null`、`> NUL`），
/// 免得跑测试时「cargo test 2>&1 | tail」被当成写文件。解释器内联脚本看全文。
fn mutating(command: &str) -> bool {
    let harmless = Regex::new(r"(?i)\d*>>?\s*&\s*\d+|\*?\d*>>?\s*(/dev/null|\$null|nul\b)").expect("常量正则");
    let bare = unquoted(command);
    let cleaned = harmless.replace_all(&bare, " ");
    cleaned.contains('>')
        || Regex::new(MUTATING).is_ok_and(|re| re.is_match(&cleaned))
        || Regex::new(INLINE_SCRIPT).is_ok_and(|re| re.is_match(command))
}

/// 向外部服务发写入请求：curl/wget/PowerShell 带 POST/PUT/PATCH/DELETE 或请求体，脚本里的 requests.post。
/// 发聊天消息、webhook、改远端数据都走这条路，只拦 lark-cli 对不用它的人等于没拦（redteam-1 G1-3）。
/// 本机地址放行（跑本地服务的测试要用），只看请求的网址：头里写个 localhost 不算（redteam-2 B16）。
fn outbound_request(command: &str) -> bool {
    let tools = [
        r#"(?i)\bcurl(\.exe)?\b.*(-X|--request)[\s=]*['"]?(POST|PUT|PATCH|DELETE)\b"#,
        r#"(?i)\bcurl(\.exe)?\b.*\s(-d|--data[\w-]*|--json|-F|--form|-T|--upload-file)([\s=@'"{]|$)"#,
        r"(?i)\bwget\b.*--(post-data|post-file|method|body-data|body-file)",
        r#"(?i)\b(Invoke-RestMethod|Invoke-WebRequest|irm|iwr)\b.*(-Method\s+['"]?(Post|Put|Patch|Delete)\b|-Body\b)"#,
        r"(?i)\brequests\.(post|put|patch|delete)\s*\(",
    ];
    let url = Regex::new(r#"(?i)https?://[^\s'"]+"#).expect("常量正则");
    let local_host = Regex::new(r"(?i)^https?://(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0)([:/]|$)").expect("常量正则");
    let local_word = Regex::new(r"(?i)(localhost|127\.0\.0\.1|\[::1\])").expect("常量正则");
    // 只按命令分隔符切段，不按单个 `&` 切：网址查询串里的 `&` 会把 `-X POST` 切到另一段（redteam-2 B15）。
    let separators = Regex::new(r"\r?\n|;|&&|\|\||\|").expect("常量正则");
    let hit = separators.split(command).filter(|s| !s.trim().is_empty()).any(|segment| {
        if !tools.iter().any(|t| Regex::new(t).is_ok_and(|re| re.is_match(segment))) {
            return false;
        }
        let urls: Vec<&str> = url.find_iter(segment).map(|m| m.as_str()).collect();
        let local = if urls.is_empty() { local_word.is_match(segment) } else { urls.iter().all(|u| local_host.is_match(u)) };
        !local
    });
    hit
}

/// 用户目录：USERPROFILE 优先，其次 HOME（同 meowo_agent::home_dir）。
fn home() -> Option<String> {
    std::env::var("USERPROFILE").ok().filter(|v| !v.is_empty()).or_else(|| std::env::var("HOME").ok().filter(|v| !v.is_empty()))
}

/// 把 shell 里的路径写法换成真实路径：`~`、`$HOME`、`$env:USERPROFILE`、`%USERPROFILE%` 换成用户目录；
/// Windows 上 Git Bash 的 `/c/...` 换成 `C:/...`。以 `/` 开头的一律当绝对路径（Windows 上 `Path::is_relative`
/// 会把 `/tmp/x` 判成相对路径，拼上 cwd 就和临时目录对不上，redteam-2 M08~M10）。
fn shell_path(token: &str, cwd: Option<&Path>) -> PathBuf {
    let mut t = token.to_owned();
    if let Some(home) = home() {
        for var in ["$env:USERPROFILE", "${HOME}", "$HOME", "%USERPROFILE%", "$USERPROFILE"] {
            if let Some(rest) = strip_prefix_ci(&t, var) {
                t = format!("{home}{rest}");
                break;
            }
        }
        if t == "~" || t.starts_with("~/") || t.starts_with("~\\") {
            t = format!("{home}{}", &t[1..]);
        }
    }
    if cfg!(windows) {
        let bytes = t.as_bytes();
        if bytes.len() >= 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && (bytes.len() == 2 || bytes[2] == b'/') {
            t = format!("{}:/{}", (bytes[1] as char).to_ascii_uppercase(), t.get(3..).unwrap_or(""));
        }
    }
    let p = PathBuf::from(&t);
    match cwd {
        Some(cwd) if p.is_relative() && !t.starts_with('/') && !t.starts_with('\\') => cwd.join(p),
        _ => p,
    }
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// 命令里像路径的片段（带分隔符、以点或 `~` 开头、带用户目录变量，或「名字.扩展名」），相对路径按 cwd 解析。
/// 选项（`-x`）、网址、别的变量不算。
fn path_tokens(command: &str, cwd: Option<&Path>) -> Vec<PathBuf> {
    let file_like = Regex::new(r"^[\w.-]+\.[A-Za-z0-9]{1,8}$").expect("常量正则");
    // 重定向写在一起的（`2>out.log`、`*>x`、`>>a`）：去掉前面的流编号和箭头。
    let redirect = Regex::new(r"^\*?\d*>+").expect("常量正则");
    let home_var = Regex::new(r"(?i)^(\$env:USERPROFILE|\$\{?HOME\}?|%USERPROFILE%|\$USERPROFILE)").expect("常量正则");
    command
        .split(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '|' | '&' | '(' | ')' | '=' | ',' | '<'))
        .map(|t| redirect.replace(t, "").into_owned())
        .filter(|t| !t.is_empty() && !t.starts_with('-') && !t.contains("://"))
        .filter(|t| !t.starts_with('$') || home_var.is_match(t))
        .filter(|t| t.contains('/') || t.contains('\\') || t.starts_with('.') || t.starts_with('~') || home_var.is_match(t) || file_like.is_match(t))
        .map(|t| shell_path(&t, cwd))
        .collect()
}

/// 团队守卫自己的配置：个人层目录；Claude 配置目录（`.claude` 或 Meowo 账号目录 `profiles/claude/<账号>`）
/// 本身、它的 settings.json、托管的知识目录和团队角色/技能文件。角色改得了它们，就能自己删掉受保护列表、
/// 守卫 hook 或改掉审查员的定义（redteam-1 G1-5、redteam-2 B28）。只认 Claude 配置目录下的 `meowo-team`：
/// 别处恰好叫这个名字的目录（比如 `<仓库>-<用途>` 命名的 worktree）不算（redteam-2 M20）。
fn guard_config(path: &Path, personal_dir: Option<&Path>) -> bool {
    let full = norm(&path.to_string_lossy());
    if personal_dir.is_some_and(|dir| {
        let dir = norm(&dir.to_string_lossy());
        !dir.is_empty() && (full == dir || full.starts_with(&format!("{dir}/")))
    }) {
        return true;
    }
    let names: Vec<&str> = full.split('/').filter(|s| !s.is_empty()).collect();
    let inside = names.iter().rposition(|n| *n == ".claude").map(|i| &names[i + 1..]).or_else(|| {
        names
            .windows(2)
            .rposition(|w| w == ["profiles", "claude"])
            .filter(|i| names.len() > i + 2)
            .map(|i| &names[i + 3..])
    });
    let Some(rest) = inside else { return false };
    match rest {
        [] => true, // 配置目录本身（rm -rf ~/.claude、cd 进去再写）
        ["settings.json" | "settings.local.json"] => true,
        ["meowo-team", ..] => true,
        ["agents", file] => file.starts_with(ROLE_PREFIX),
        ["skills", dir, ..] => dir.starts_with(ROLE_PREFIX),
        _ => false,
    }
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
    if let Some(problem) = &config.broken {
        return Some(format!("个人层 config.json {problem}，受保护目录无法确认，{role} 先不能写文件。请用户修好 config.json"));
    }
    if guard_config(&path, config.personal_dir.as_deref()) {
        return Some(format!("{role} 不能改团队守卫的配置（{target}）"));
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
    // Git Bash 里的 /tmp 就是系统临时目录，但字符串对不上，单列一项。
    vec![std::env::temp_dir(), PathBuf::from("/tmp")]
}

#[cfg(test)]
mod tests;
