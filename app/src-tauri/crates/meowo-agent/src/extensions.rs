//! 设置 › 扩展页（对照 Kimi 的「插件」页）：各 agent 在**用户级**装了哪些技能、MCP 服务、
//! 自定义命令、钩子、插件。只读——改别家工具的配置文件有写坏的风险，开关留到后面。
//!
//! 每家的位置由各插件的 [`crate::registry::AgentPlugin::extensions`] 声明，这里只放通用的
//! 扫描助手。位置都核对过对方自己的代码（2026-11）：Claude/Codex 取自 Kimi Code 内置的
//! 「从 Claude/Codex 迁移」技能文档，Gemini 取自其 bundle 的 `Storage.getUser*Dir`，opencode
//! 取自其二进制里的 glob（`{skill,skills}/**/SKILL.md` 等），Kimi 取自同一份迁移文档。
//!
//! **隐私**：只取名字、描述、连接方式、开关。MCP 的命令参数、环境变量、URL 一律不读出
//! ——这些里面常带 token，而这份清单会显示在界面上。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 一类扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub enum ExtensionKind {
    Skill,
    Mcp,
    Command,
    /// 自定义子代理（Claude `agents/`、opencode `agent/`）。
    Agent,
    Hook,
    Plugin,
}

/// 一条扩展。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ExtensionItemDto {
    pub name: String,
    pub description: Option<String>,
    /// 相对所在组 `source` 的路径（技能 = `<目录>/SKILL.md`），前端「打开文件」用。
    /// 写在配置文件里的条目（MCP、钩子）没有自己的文件，为 None。
    pub rel: Option<String>,
    /// 短补充：MCP 的连接方式（`stdio` / `http` / `sse`）、插件的来源与版本、钩子的条数。
    pub detail: Option<String>,
    /// None = 没有开关的概念；Some(false) = 配置里关着。
    pub enabled: Option<bool>,
    /// Meowo 自己装的（状态上报钩子、opencode 的上报插件）：前端单独标出，免得被当成第三方关掉。
    pub managed: bool,
}

/// 同一来源（一个目录或一个配置文件）的一组扩展。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ExtensionGroupDto {
    pub kind: ExtensionKind,
    /// 来源的绝对路径（目录或配置文件），前端「打开所在文件夹」用。
    pub source: String,
    /// 好几家共用的目录（`~/.agents/skills`）：前端注明「共享」，同一份东西别让人以为装了好几份。
    pub shared: bool,
    pub items: Vec<ExtensionItemDto>,
}

/// 一个 agent 的扩展清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct AgentExtensionsDto {
    pub provider: String,
    pub display_name: String,
    /// 数据目录（`~/.claude` 等），前端「打开配置目录」用。
    pub data_dir: String,
    pub groups: Vec<ExtensionGroupDto>,
}

/// 一组目录条目最多列多少条：防有人把整个仓库软链进 skills。
const MAX_ITEMS: usize = 300;
/// 描述最长多少字：卡片里一行显示，再长也读不完。
const MAX_DESC: usize = 200;
/// SKILL.md / 命令文件只读开头这么多字节：描述在 frontmatter 里，不用整篇读。
const HEAD_BYTES: usize = 8 * 1024;

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn item(name: String) -> ExtensionItemDto {
    ExtensionItemDto {
        name,
        description: None,
        rel: None,
        detail: None,
        enabled: None,
        managed: false,
    }
}

fn clip(text: &str) -> Option<String> {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return None;
    }
    if flat.chars().count() <= MAX_DESC {
        return Some(flat);
    }
    Some(format!(
        "{}…",
        flat.chars().take(MAX_DESC).collect::<String>()
    ))
}

/// 文件开头一段（UTF-8 有损）。读不了返回空串。
fn read_head(path: &Path) -> String {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(HEAD_BYTES as u64).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// YAML frontmatter 里某个顶层字段的值（技能和命令的 `name` / `description`）。
/// 只认最常见的写法：单行值（可带引号），或 `|` / `>` 开头的多行块。不引 YAML 库：
/// 只为取一行描述，写错的 frontmatter 也不该让整页读不出来。
pub(crate) fn frontmatter_field(text: &str, key: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let body: Vec<&str> = lines.take_while(|line| line.trim_end() != "---").collect();
    let prefix = format!("{key}:");
    let at = body.iter().position(|line| line.starts_with(&prefix))?;
    let value = body[at][prefix.len()..].trim();
    if value.starts_with('|') || value.starts_with('>') {
        let block: Vec<&str> = body[at + 1..]
            .iter()
            .take_while(|line| line.is_empty() || line.starts_with(' ') || line.starts_with('\t'))
            .map(|line| line.trim())
            .collect();
        return clip(&block.join(" "));
    }
    let unquoted = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    clip(unquoted)
}

/// 没有 frontmatter 描述时，取正文第一行非空文字（去掉标题的 #）。Claude 的命令就是这么显示的。
fn first_line(text: &str) -> Option<String> {
    let text = text.trim_start_matches('\u{feff}');
    let body = if text.starts_with("---") {
        // 跳过 frontmatter：第二个 --- 之后才是正文。
        text.splitn(3, "---").nth(2).unwrap_or("")
    } else {
        text
    };
    body.lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .find(|line| !line.is_empty())
        .and_then(clip)
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    paths
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

/// 技能目录：每个子目录里有 `SKILL.md` 就是一个技能（Claude、Codex、Kimi、Gemini、opencode
/// 都只认这种目录形态，散放的 `.md` 不算）。点开头的目录跳过（Codex 的 `.system` 是它自带的）。
pub fn skills_group(dir: &Path, shared: bool) -> Option<ExtensionGroupDto> {
    if !dir.is_dir() {
        return None;
    }
    let mut items = Vec::new();
    for path in sorted_entries(dir) {
        if items.len() >= MAX_ITEMS {
            break;
        }
        if hidden(&path) || !path.is_dir() {
            continue;
        }
        let skill = path.join("SKILL.md");
        if !skill.is_file() {
            continue;
        }
        let dir_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let head = read_head(&skill);
        let mut entry = item(frontmatter_field(&head, "name").unwrap_or_else(|| dir_name.clone()));
        entry.description = frontmatter_field(&head, "description");
        entry.rel = Some(format!("{dir_name}/SKILL.md"));
        items.push(entry);
    }
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Skill,
        source: path_text(dir),
        shared,
        items,
    })
}

/// Markdown 文件目录（Claude 的 `commands/` `agents/`、Codex 的 `prompts/`、opencode 的
/// `command/` `agent/`）：每个 `.md` 一条，可嵌一两层子目录。名字用去掉扩展名的文件名。
pub fn markdown_group(dir: &Path, kind: ExtensionKind) -> Option<ExtensionGroupDto> {
    files_group(dir, kind, "md", |path, rel| {
        let head = read_head(path);
        let stem = Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut entry = item(frontmatter_field(&head, "name").unwrap_or(stem));
        entry.description = frontmatter_field(&head, "description").or_else(|| first_line(&head));
        entry
    })
}

/// Gemini 的自定义命令：`commands/**/*.toml`，子目录用 `:` 拼成命名空间（`git/commit.toml`
/// → `/git:commit`），描述在 `description` 字段。
pub fn gemini_commands_group(dir: &Path) -> Option<ExtensionGroupDto> {
    files_group(dir, ExtensionKind::Command, "toml", |path, rel| {
        let name = rel.trim_end_matches(".toml").replace(['/', '\\'], ":");
        let mut entry = item(name);
        entry.description = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
            .and_then(|doc| {
                doc.get("description")
                    .and_then(|v| v.as_str())
                    .and_then(clip)
            });
        entry
    })
}

/// 按扩展名收一个目录下的文件（最多下钻两层子目录）。
fn files_group(
    dir: &Path,
    kind: ExtensionKind,
    ext: &str,
    describe: impl Fn(&Path, &str) -> ExtensionItemDto,
) -> Option<ExtensionGroupDto> {
    if !dir.is_dir() {
        return None;
    }
    let mut found = Vec::new();
    collect_files(dir, dir, ext, 0, &mut found);
    let items = found
        .into_iter()
        .take(MAX_ITEMS)
        .map(|(path, rel)| {
            let mut entry = describe(&path, &rel);
            entry.rel = Some(rel);
            entry
        })
        .collect();
    Some(ExtensionGroupDto {
        kind,
        source: path_text(dir),
        shared: false,
        items,
    })
}

fn collect_files(
    root: &Path,
    dir: &Path,
    ext: &str,
    depth: usize,
    out: &mut Vec<(PathBuf, String)>,
) {
    for path in sorted_entries(dir) {
        if out.len() >= MAX_ITEMS || hidden(&path) {
            continue;
        }
        if path.is_dir() {
            if depth < 2 {
                collect_files(root, &path, ext, depth + 1, out);
            }
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((path, rel));
        }
    }
}

/// 一个 MCP 服务的连接方式：显式 `type` 优先，否则有 `command` 是本地进程（stdio），
/// 有 `url` / `httpUrl` / `serverUrl` 是远程（http）。opencode 写 `local` / `remote`。
fn mcp_transport(explicit: Option<&str>, has_command: bool, has_url: bool) -> Option<String> {
    let mapped = explicit.map(|t| match t {
        "local" => "stdio",
        "remote" | "streamable-http" | "streamableHttp" => "http",
        other => other,
    });
    mapped
        .map(str::to_string)
        .or_else(|| has_command.then(|| "stdio".to_string()))
        .or_else(|| has_url.then(|| "http".to_string()))
}

/// JSON 里的 MCP 表（Claude `mcpServers`、Gemini `mcpServers`、Kimi `mcp.json`、opencode `mcp`）。
/// 关闭标记三家写法不同：`disabled: true`（Claude/Gemini）、`enabled: false`（opencode）。
pub fn mcp_group_from_json(
    source: &Path,
    servers: Option<&serde_json::Value>,
) -> Option<ExtensionGroupDto> {
    let map = servers?.as_object()?;
    let items = map
        .iter()
        .take(MAX_ITEMS)
        .map(|(name, spec)| {
            let mut entry = item(name.clone());
            let has_url = ["url", "httpUrl", "serverUrl"]
                .iter()
                .any(|k| spec.get(k).is_some());
            entry.detail = mcp_transport(
                spec.get("type").and_then(|t| t.as_str()),
                spec.get("command").is_some(),
                has_url,
            );
            entry.enabled = match (
                spec.get("disabled").and_then(|v| v.as_bool()),
                spec.get("enabled").and_then(|v| v.as_bool()),
            ) {
                (Some(disabled), _) => Some(!disabled),
                (None, Some(enabled)) => Some(enabled),
                (None, None) => None,
            };
            entry
        })
        .collect();
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Mcp,
        source: path_text(source),
        shared: false,
        items,
    })
}

/// Codex `config.toml` 的 `[mcp_servers.<name>]`。关闭标记是 `enabled = false`。
pub fn mcp_group_from_codex_toml(config: &Path) -> Option<ExtensionGroupDto> {
    let doc = std::fs::read_to_string(config)
        .ok()?
        .parse::<toml_edit::DocumentMut>()
        .ok()?;
    let table = doc.get("mcp_servers")?.as_table_like()?;
    let items = table
        .iter()
        .take(MAX_ITEMS)
        .map(|(name, spec)| {
            let mut entry = item(name.to_string());
            let spec = spec.as_table_like();
            let has = |key: &str| spec.is_some_and(|t| t.get(key).is_some());
            entry.detail = mcp_transport(None, has("command"), has("url"));
            entry.enabled = spec
                .and_then(|t| t.get("enabled"))
                .and_then(|v| v.as_bool());
            entry
        })
        .collect();
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Mcp,
        source: path_text(config),
        shared: false,
        items,
    })
}

/// 命令是不是 Meowo 自己挂的状态上报（各家接线写进去的都是 meowo-reporter 的路径）。
fn is_meowo_command(command: &str) -> bool {
    command.to_ascii_lowercase().contains("meowo-reporter")
}

/// 钩子按「事件 × 是不是 Meowo 的」汇总成条：一个事件下挂几条命令只计数，不列命令本身
/// （命令行里可能带 token，而且一长串路径读了也没用）。
fn hook_items(entries: Vec<(String, String)>) -> Vec<ExtensionItemDto> {
    let mut counts: Vec<(String, bool, u32)> = Vec::new();
    for (event, command) in entries {
        let managed = is_meowo_command(&command);
        match counts
            .iter_mut()
            .find(|(e, m, _)| *e == event && *m == managed)
        {
            Some(slot) => slot.2 += 1,
            None => counts.push((event, managed, 1)),
        }
    }
    counts
        .into_iter()
        .map(|(event, managed, count)| {
            let mut entry = item(event);
            entry.detail = Some(count.to_string());
            entry.managed = managed;
            entry
        })
        .collect()
}

/// Claude / Codex / Gemini 同一种钩子写法：`hooks: { 事件: [ { matcher, hooks: [ { command } ] } ] }`。
pub fn hooks_group_from_json(
    source: &Path,
    hooks: Option<&serde_json::Value>,
) -> Option<ExtensionGroupDto> {
    let map = hooks?.as_object()?;
    let mut entries = Vec::new();
    for (event, groups) in map {
        for group in groups.as_array().into_iter().flatten() {
            for hook in group
                .get("hooks")
                .and_then(|h| h.as_array())
                .into_iter()
                .flatten()
            {
                let command = hook.get("command").and_then(|c| c.as_str()).unwrap_or("");
                entries.push((event.clone(), command.to_string()));
            }
        }
    }
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Hook,
        source: path_text(source),
        shared: false,
        items: hook_items(entries),
    })
}

/// Kimi Code `config.toml` 的 `[[hooks]]`：每条一个 `event` + `command`。
pub fn hooks_group_from_kimi_toml(config: &Path) -> Option<ExtensionGroupDto> {
    let doc = std::fs::read_to_string(config)
        .ok()?
        .parse::<toml_edit::DocumentMut>()
        .ok()?;
    let hooks = doc.get("hooks")?;
    let mut entries = Vec::new();
    let mut push = |table: &dyn toml_edit::TableLike| {
        let event = table.get("event").and_then(|v| v.as_str()).unwrap_or("?");
        let command = table.get("command").and_then(|v| v.as_str()).unwrap_or("");
        entries.push((event.to_string(), command.to_string()));
    };
    if let Some(array) = hooks.as_array_of_tables() {
        for table in array.iter() {
            push(table);
        }
    } else if let Some(array) = hooks.as_array() {
        for value in array.iter() {
            if let Some(table) = value.as_inline_table() {
                push(table);
            }
        }
    }
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Hook,
        source: path_text(config),
        shared: false,
        items: hook_items(entries),
    })
}

/// 读 JSON 配置（容忍 BOM 与 jsonc 的注释、尾逗号）。读不了返回 None。
pub fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&strip_jsonc(text)).ok())
}

/// 去掉 jsonc 的 `//` `/* */` 注释和对象/数组末尾的多余逗号（opencode 的 `opencode.jsonc`）。
/// 字符串里的同形字符原样保留。
pub(crate) fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut in_string = false;
    while at < chars.len() {
        let c = chars[at];
        if in_string {
            out.push(c);
            if c == '\\' && at + 1 < chars.len() {
                out.push(chars[at + 1]);
                at += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            at += 1;
            continue;
        }
        match (c, chars.get(at + 1)) {
            ('"', _) => {
                in_string = true;
                out.push(c);
                at += 1;
            }
            ('/', Some('/')) => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            ('/', Some('*')) => {
                at += 2;
                while at + 1 < chars.len() && !(chars[at] == '*' && chars[at + 1] == '/') {
                    at += 1;
                }
                at += 2;
            }
            (',', _) => {
                // 尾逗号：后面跳过空白就是 } 或 ]，丢掉这个逗号。
                let mut look = at + 1;
                while look < chars.len() && chars[look].is_whitespace() {
                    look += 1;
                }
                if !matches!(chars.get(look), Some('}') | Some(']')) {
                    out.push(c);
                }
                at += 1;
            }
            _ => {
                out.push(c);
                at += 1;
            }
        }
    }
    out
}

/// 几家共用的用户技能目录 `~/.agents/skills`（Codex、Kimi、Gemini、opencode 都读）。
pub fn shared_agent_skills() -> Option<ExtensionGroupDto> {
    let home = crate::home_dir()?;
    skills_group(&home.join(".agents").join("skills"), true)
}

/// 把 Claude 的已装插件（`plugins/installed_plugins.json`）和启用表（`settings.json` 的
/// `enabledPlugins`）合成一组。名字形如 `rust-analyzer-lsp@claude-plugins-official`：
/// @ 前是插件名，后是来源市场。
pub fn claude_plugins_group(
    data_dir: &Path,
    settings: Option<&serde_json::Value>,
) -> Option<ExtensionGroupDto> {
    let plugins_dir = data_dir.join("plugins");
    let installed = read_json(&plugins_dir.join("installed_plugins.json"))?;
    let map = installed.get("plugins")?.as_object()?;
    let enabled = settings.and_then(|s| s.get("enabledPlugins"));
    let items = map
        .iter()
        .take(MAX_ITEMS)
        .map(|(key, installs)| {
            let (name, market) = key.split_once('@').unwrap_or((key.as_str(), ""));
            let mut entry = item(name.to_string());
            let install = installs.as_array().and_then(|list| list.first());
            let version = install
                .and_then(|i| i.get("version"))
                .and_then(|v| v.as_str());
            entry.detail = match (market.is_empty(), version) {
                (false, Some(v)) => Some(format!("{market} · {v}")),
                (false, None) => Some(market.to_string()),
                (true, Some(v)) => Some(v.to_string()),
                (true, None) => None,
            };
            // 只认用户级启用表里明写的值。项目级安装（带 projectPath）的开关记在那个项目的
            // 设置里，这里查不到；当成「关」会误导，宁可不显示开关。
            entry.enabled = enabled.and_then(|e| e.get(key)).and_then(|v| v.as_bool());
            entry.rel = install
                .and_then(|i| i.get("installPath"))
                .and_then(|p| p.as_str())
                .and_then(|p| {
                    Path::new(p)
                        .strip_prefix(&plugins_dir)
                        .ok()
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                });
            entry
        })
        .collect();
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Plugin,
        source: path_text(&plugins_dir),
        shared: false,
        items,
    })
}

/// Gemini 的扩展：`extensions/<目录>/gemini-extension.json` 里的 name / version / description。
pub fn gemini_extensions_group(dir: &Path) -> Option<ExtensionGroupDto> {
    if !dir.is_dir() {
        return None;
    }
    let items = sorted_entries(dir)
        .into_iter()
        .filter(|path| path.is_dir() && !hidden(path))
        .filter_map(|path| {
            let manifest = path.join("gemini-extension.json");
            let json = read_json(&manifest)?;
            let dir_name = path.file_name()?.to_string_lossy().into_owned();
            let mut entry = item(
                json.get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| dir_name.clone()),
            );
            entry.description = json
                .get("description")
                .and_then(|d| d.as_str())
                .and_then(clip);
            entry.detail = json
                .get("version")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            entry.rel = Some(format!("{dir_name}/gemini-extension.json"));
            Some(entry)
        })
        .take(MAX_ITEMS)
        .collect();
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Plugin,
        source: path_text(dir),
        shared: false,
        items,
    })
}

/// opencode 的本地插件：`plugin(s)/*.ts|js`。Meowo 的上报插件也装在这里，标成 managed。
pub fn opencode_plugins_group(dir: &Path) -> Option<ExtensionGroupDto> {
    if !dir.is_dir() {
        return None;
    }
    let items = sorted_entries(dir)
        .into_iter()
        .filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("ts") | Some("js")
                )
        })
        .take(MAX_ITEMS)
        .filter_map(|path| {
            let file = path.file_name()?.to_string_lossy().into_owned();
            let mut entry = item(path.file_stem()?.to_string_lossy().into_owned());
            entry.managed = file.starts_with("meowo-reporter");
            entry.rel = Some(file);
            Some(entry)
        })
        .collect();
    Some(ExtensionGroupDto {
        kind: ExtensionKind::Plugin,
        source: path_text(dir),
        shared: false,
        items,
    })
}

/// 第一个存在的目录（opencode 单复数两种目录名都认：`skill/` 或 `skills/`）。
pub fn first_dir(base: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| base.join(n)).find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meowo-ext-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn frontmatter_reads_single_line_quoted_and_block_values() {
        let text = "---\nname: lark-doc\ndescription: \"飞书云文档：读取和编辑\"\nother: |\n  第一行\n  第二行\n---\n正文";
        assert_eq!(frontmatter_field(text, "name").as_deref(), Some("lark-doc"));
        assert_eq!(
            frontmatter_field(text, "description").as_deref(),
            Some("飞书云文档：读取和编辑")
        );
        assert_eq!(
            frontmatter_field(text, "other").as_deref(),
            Some("第一行 第二行")
        );
        assert_eq!(frontmatter_field("没有 frontmatter", "name"), None);
        assert_eq!(
            first_line("---\na: b\n---\n\n# 标题\n内容"),
            Some("标题".to_string())
        );
    }

    #[test]
    fn skills_group_needs_skill_md_and_skips_hidden_dirs() {
        let root = temp();
        std::fs::create_dir_all(root.join("good")).unwrap();
        std::fs::write(
            root.join("good/SKILL.md"),
            "---\nname: Good\ndescription: does things\n---\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("no-skill-md")).unwrap();
        std::fs::create_dir_all(root.join(".system/inner")).unwrap();
        std::fs::write(root.join(".system/SKILL.md"), "---\nname: sys\n---\n").unwrap();
        std::fs::write(root.join("loose.md"), "散放的不算").unwrap();
        let group = skills_group(&root, false).unwrap();
        assert_eq!(group.items.len(), 1);
        assert_eq!(group.items[0].name, "Good");
        assert_eq!(group.items[0].description.as_deref(), Some("does things"));
        assert_eq!(group.items[0].rel.as_deref(), Some("good/SKILL.md"));
        assert!(skills_group(&root.join("missing"), false).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_from_json_never_exposes_command_env_or_url() {
        let servers = serde_json::json!({
            "local": { "command": "npx", "args": ["secret-arg"], "env": { "TOKEN": "sk-123" } },
            "remote": { "url": "https://x.example/?key=abc", "disabled": true },
            "oc": { "type": "remote", "url": "https://y", "enabled": false },
        });
        let group = mcp_group_from_json(Path::new("/c.json"), Some(&servers)).unwrap();
        let text = serde_json::to_string(&group).unwrap();
        assert!(
            !text.contains("sk-123") && !text.contains("secret-arg") && !text.contains("key=abc")
        );
        let by = |n: &str| group.items.iter().find(|i| i.name == n).unwrap().clone();
        assert_eq!(by("local").detail.as_deref(), Some("stdio"));
        assert_eq!(by("local").enabled, None);
        assert_eq!(by("remote").detail.as_deref(), Some("http"));
        assert_eq!(by("remote").enabled, Some(false));
        assert_eq!(by("oc").detail.as_deref(), Some("http"));
        assert_eq!(by("oc").enabled, Some(false));
    }

    #[test]
    fn codex_toml_mcp_and_kimi_toml_hooks() {
        let root = temp();
        let codex = root.join("config.toml");
        std::fs::write(&codex, "model = \"x\"\n[mcp_servers.docs]\nurl = \"https://d\"\n[mcp_servers.cu]\ncommand = \"cu.exe\"\nenabled = false\n").unwrap();
        let group = mcp_group_from_codex_toml(&codex).unwrap();
        assert_eq!(group.items.len(), 2);
        assert_eq!(group.items[0].name, "docs");
        assert_eq!(group.items[0].detail.as_deref(), Some("http"));
        assert_eq!(group.items[1].enabled, Some(false));

        let kimi = root.join("kimi.toml");
        std::fs::write(&kimi, "[[hooks]]\nevent = \"Stop\"\ncommand = \"C:/x/meowo-reporter.exe kimi\"\n[[hooks]]\nevent = \"Stop\"\ncommand = \"my-script\"\n").unwrap();
        let hooks = hooks_group_from_kimi_toml(&kimi).unwrap();
        assert_eq!(hooks.items.len(), 2);
        assert!(hooks.items.iter().any(|i| i.managed && i.name == "Stop"));
        assert!(hooks
            .items
            .iter()
            .any(|i| !i.managed && i.name == "Stop" && i.detail.as_deref() == Some("1")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn json_hooks_split_meowo_and_user_commands_and_hide_command_text() {
        let hooks = serde_json::json!({
            "Stop": [ { "hooks": [
                { "type": "command", "command": "\"C:\\\\meowo\\\\meowo-reporter.exe\" claude" },
                { "type": "command", "command": "notify --token abc" },
            ] } ],
            "PreToolUse": [ { "matcher": "Bash", "hooks": [ { "type": "command", "command": "guard.sh" } ] } ],
        });
        let group = hooks_group_from_json(Path::new("/s.json"), Some(&hooks)).unwrap();
        assert!(!serde_json::to_string(&group).unwrap().contains("abc"));
        assert_eq!(group.items.iter().filter(|i| i.managed).count(), 1);
        assert_eq!(group.items.iter().filter(|i| !i.managed).count(), 2);
    }

    #[test]
    fn strip_jsonc_drops_comments_and_trailing_commas_but_keeps_strings() {
        let text = "{\n  // 注释\n  \"url\": \"https://a//b\", /* 块 */\n  \"mcp\": { \"x\": { \"type\": \"local\", }, },\n}";
        let value: serde_json::Value = serde_json::from_str(&strip_jsonc(text)).unwrap();
        assert_eq!(value["url"], "https://a//b");
        assert_eq!(value["mcp"]["x"]["type"], "local");
    }

    #[test]
    fn markdown_and_gemini_command_groups() {
        let root = temp();
        std::fs::create_dir_all(root.join("commands/git")).unwrap();
        std::fs::write(
            root.join("commands/review.md"),
            "---\ndescription: 审一遍改动\n---\n正文",
        )
        .unwrap();
        std::fs::write(root.join("commands/git/commit.md"), "# 写提交说明\n").unwrap();
        let md = markdown_group(&root.join("commands"), ExtensionKind::Command).unwrap();
        let names: Vec<_> = md
            .items
            .iter()
            .map(|i| (i.name.as_str(), i.description.as_deref()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("commit", Some("写提交说明")),
                ("review", Some("审一遍改动"))
            ]
        );

        std::fs::create_dir_all(root.join("gcmd/git")).unwrap();
        std::fs::write(
            root.join("gcmd/git/commit.toml"),
            "description = \"提交\"\nprompt = \"x\"\n",
        )
        .unwrap();
        let gemini = gemini_commands_group(&root.join("gcmd")).unwrap();
        assert_eq!(gemini.items[0].name, "git:commit");
        assert_eq!(gemini.items[0].description.as_deref(), Some("提交"));
        let _ = std::fs::remove_dir_all(root);
    }
}
