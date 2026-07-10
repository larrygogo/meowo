//! hooks 接线的**声明**与**纯函数实现**：一个 [`HookSpec`] 描述某个变体「hooks 写在哪个文件、
//! 什么格式、挂哪些事件、命令长什么样」；三个方法回答「确保接入 / 是否已接入 / 二进制在哪」。
//!
//! 全是纯函数（输入配置文本，输出新文本或判定），落盘由调用方负责——便于单测，也保证
//! 「解析失败绝不写」。带副作用的接线步骤（claude 的 statusline 脚本、codex 的 trusted_hash）
//! 不在这里：它们要 `fsutil`/`sha2`/`db_path`，由 meowo-app 的 `SetupBehavior` 承接。

use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

/// 接线失败的机器可读原因，回传前端以给出精准提示（如未登录 → 「请先登录」）。
/// 序列化为 kebab-case 字符串；`None` 表示成功或已是目标状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairReason {
    /// agent 数据目录不存在（视为未安装）。
    NotDetected,
    /// 承载 hooks 的配置文件尚未生成（如 kimi 的 config.toml 需先 `kimi login`）。
    NeedLogin,
    /// 找不到 meowo-reporter 二进制（既有 hooks 无有效路径且 app 同目录无 sidecar）。
    ReporterNotFound,
    /// 配置文件读取或解析失败（权限/编码/畸形），为保护用户文件放弃写入。
    ConfigUnreadable,
    /// 写入失败（目录不可写/磁盘满/杀软拦截）。
    WriteFailed,
}

/// `ensure_hooks` 的三态结果。`Changed` 携带待落盘的新文本。
#[derive(Debug, PartialEq, Eq)]
pub enum EnsureOutcome {
    Changed(String),
    Unchanged,
    /// 配置形态无法安全改写 → 放弃，绝不写坏用户文件。
    Abandon(RepairReason),
}

/// hooks 配置的落盘格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// kimi：`config.toml` 顶层 `[[hooks]]` array-of-tables。
    KimiToml,
    /// codex：`hooks.json` 的 `{"hooks": {"<Event>": [{"hooks":[{...}]}]}}`，条目**无** matcher。
    /// 顶层只允许 `hooks` 键（codex 侧 deny_unknown_fields）。
    CodexJson,
    /// claude：`settings.json` 的 `{"hooks": {"<Event>": [{"matcher":"...","hooks":[{...}]}]}}`。
    /// 与 `CodexJson` 同构，唯一差别是条目**带 `matcher`**——同一事件下按 matcher 区分多条，
    /// 与用户自有 hook（如 `PreToolUse:Bash` 预检）共存。顶层还承载 `statusLine` 等无关键，
    /// 一律原样保留。
    ClaudeJson,
}

/// 配置文件不存在时怎么办。声明式，避免每个 agent 的 `apply` 各写一遍 NotFound 分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingConfig {
    /// 从这段初始文本起（如 claude 的 `{}`、codex 的 `{"hooks":{}}`）。
    CreateFrom(&'static str),
    /// 不创建，直接失败（如 kimi 的 config.toml 需先 `kimi login`）。
    Fail(RepairReason),
}

/// hook command 的书写形态。三家各不相同，且**不能随意统一**：
/// 引号与否影响既有配置的幂等判定，`--provider` 与否影响认领规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// 可执行路径是否加双引号（claude/codex 加，kimi 不加——保持与各自现存配置一致）。
    pub quote_exe: bool,
    /// 是否追加 `--provider <id>`（codex/kimi 加；claude 靠 settings 里的位置区分，不带参）。
    pub with_provider: bool,
}

impl CommandSpec {
    /// 写出的 command 串。
    pub fn render(self, reporter: &str, agent_id: &str) -> String {
        let exe = if self.quote_exe { format!("\"{reporter}\"") } else { reporter.to_string() };
        if self.with_provider {
            format!("{exe} --provider {agent_id}")
        } else {
            exe
        }
    }

    /// 严格认领：可执行文件名恰为 meowo-reporter[.exe]（或历史遗留 cc-reporter[.exe]，以便升级时
    /// 替换旧路径），且余参**恰好**符合本形态。返回可执行路径。不裸 `contains`，不误伤用户 hook
    /// （如 `node tools/meowo-reporter-notify.js`）。
    pub fn claim(self, cmd: &str, agent_id: &str) -> Option<String> {
        let (path, args) = parse_hook_command(cmd)?;
        if !is_any_reporter(&path) {
            return None;
        }
        let args_ok = if self.with_provider {
            args == ["--provider", agent_id]
        } else {
            args.is_empty()
        };
        args_ok.then_some(path)
    }
}

/// 一个 hook 事件。`matcher` 仅 claude 有（同一事件下按 matcher 区分多条，与用户自有 hook 共存）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookEvent {
    pub name: &'static str,
    pub matcher: Option<&'static str>,
}

impl HookEvent {
    pub const fn plain(name: &'static str) -> Self {
        Self { name, matcher: None }
    }
    pub const fn matched(name: &'static str, matcher: &'static str) -> Self {
        Self { name, matcher: Some(matcher) }
    }
}

/// 某变体的 hooks 接线规格。加/改 agent 只写这张声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSpec {
    /// 配置文件相对 `data_dir` 的路径（`settings.json` / `hooks.json` / `config.toml`）。
    pub config_rel: &'static str,
    pub format: ConfigFormat,
    pub missing: MissingConfig,
    pub events: &'static [HookEvent],
    pub command: CommandSpec,
}

impl HookSpec {
    /// 幂等接入 meowo-reporter hooks。用户自有的 hook 条目一概不动。
    pub fn ensure_hooks(&self, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        match self.format {
            ConfigFormat::KimiToml => kimi_toml::ensure_hooks(self, cur_text, reporter, agent_id),
            ConfigFormat::CodexJson => codex_json::ensure_hooks(self, cur_text, reporter, agent_id),
            ConfigFormat::ClaudeJson => claude_json::ensure_hooks(self, cur_text, reporter, agent_id),
        }
    }

    /// 配置文本能否被本格式解析。区分「解析不了」与「解析得了但没挂 reporter」——前者是
    /// 暂时不可读/损坏，不该误报成「未接入」诱导用户去修复（修复也会因 Abandon 而拒写）。
    pub fn parses(&self, cur_text: &str) -> bool {
        match self.format {
            ConfigFormat::KimiToml => cur_text.parse::<DocumentMut>().is_ok(),
            ConfigFormat::CodexJson | ConfigFormat::ClaudeJson => json_common::parse(cur_text).is_some(),
        }
    }

    /// 新会话是否真能入库：**只看 SessionStart**，且必须是当前 meowo-reporter。仅在别的事件
    /// （如 Stop）挂了 reporter，不能保证新会话被记录，不应误判成已接入。
    pub fn has_reporter(&self, cur_text: &str, agent_id: &str) -> bool {
        self.claimed_at(cur_text, agent_id, Some(SESSION_START)).is_some_and(|p| is_current_reporter(&p))
    }

    /// 从既有配置里取出已认领的**当前** meowo-reporter 绝对路径，用于复用其位置（不限事件）。
    /// 历史 cc-reporter 不算：它已废弃，当成目标写回去 hooks 依旧失效。
    pub fn claimed_reporter(&self, cur_text: &str, agent_id: &str) -> Option<String> {
        self.claimed_at(cur_text, agent_id, None).filter(|p| is_current_reporter(p))
    }

    /// 认领到的 reporter 路径（可能是历史 cc-reporter）。`event=None` 表示不限事件。
    fn claimed_at(&self, cur_text: &str, agent_id: &str, event: Option<&str>) -> Option<String> {
        match self.format {
            ConfigFormat::KimiToml => kimi_toml::claimed_at(self, cur_text, agent_id, event),
            // claude/codex 的 hooks 树同构（差别只在写入时是否带 matcher），认领扫描共用一份。
            ConfigFormat::CodexJson | ConfigFormat::ClaudeJson => json_common::claimed_at(self, cur_text, agent_id, event),
        }
    }
}

/// 决定「新会话能否入库」的事件名。三家同名。
const SESSION_START: &str = "SessionStart";

// ═══ 命令行解析（各格式共用） ═══

/// 解析 hook command 为（可执行路径, 余参）。首 token 支持带双引号或裸路径。
pub fn parse_hook_command(cmd: &str) -> Option<(String, Vec<String>)> {
    let c = cmd.trim();
    let (path, rest) = if let Some(r) = c.strip_prefix('"') {
        let end = r.find('"')?;
        (r[..end].to_string(), r[end + 1..].trim())
    } else {
        match c.split_once(char::is_whitespace) {
            Some((p, r)) => (p.to_string(), r.trim()),
            None => (c.to_string(), ""),
        }
    };
    let args = rest.split_whitespace().map(str::to_string).collect();
    Some((path, args))
}

fn file_name_lower(path: &str) -> Option<String> {
    Some(std::path::Path::new(path).file_name()?.to_str()?.to_ascii_lowercase())
}

/// 是 meowo-reporter 或历史遗留的 cc-reporter。
fn is_any_reporter(path: &str) -> bool {
    file_name_lower(path).is_some_and(|n| {
        matches!(n.as_str(), "meowo-reporter" | "meowo-reporter.exe" | "cc-reporter" | "cc-reporter.exe")
    })
}

/// 是**当前** meowo-reporter（排除历史 cc-reporter）。
fn is_current_reporter(path: &str) -> bool {
    file_name_lower(path).is_some_and(|n| matches!(n.as_str(), "meowo-reporter" | "meowo-reporter.exe"))
}

// ═══ KimiToml ═══

mod kimi_toml {
    use super::*;

    pub fn ensure_hooks(spec: &HookSpec, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        // 解析失败绝不写（kimi 自身对坏文件同样拒写）。
        let Ok(mut doc) = cur_text.parse::<DocumentMut>() else {
            return EnsureOutcome::Abandon(RepairReason::ConfigUnreadable);
        };
        match merge(spec, &mut doc, reporter, agent_id) {
            Merge::Changed => EnsureOutcome::Changed(doc.to_string()),
            Merge::Unchanged => EnsureOutcome::Unchanged,
            Merge::Abandon => EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
        }
    }

    enum Merge {
        Changed,
        Unchanged,
        Abandon,
    }

    /// 幂等合并 `[[hooks]]`：逐事件找认领条目（event 相符 + command 可认领），路径不符则更新
    /// command；缺则追加 `{event, command, timeout=5}`。用户条目一概不动。
    ///
    /// 纪律（源码调研 kimi 0.20）：kimi 自身会全量重写此文件（注释全丢）——幂等判定只按
    /// (event, command) 内容匹配，绝不依赖注释标记。
    fn merge(spec: &HookSpec, doc: &mut DocumentMut, reporter: &str, agent_id: &str) -> Merge {
        let desired_cmd = spec.command.render(reporter, agent_id);
        let mut changed = false;

        if doc.get("hooks").and_then(|it| it.as_array_of_tables()).is_none() {
            // 不是 array-of-tables。可安全替换的只有两种：
            //   - 不存在：新版 kimi-code 默认无顶层 hooks 键。
            //   - `hooks = []`：旧 Python 版 kimi-cli 的默认**空内联数组**——语义等价于「无 hooks」，
            //     与 `[[hooks]]` array-of-tables 是同一 TOML 类型（空列表），替换无损，kimi 照常读取。
            // 非空内联数组 / 字符串等畸形形态则保守放弃：无法结构保持地转换，绝不写坏用户配置。
            let replaceable = match doc.get("hooks") {
                None => true,
                Some(it) => it.as_array().is_some_and(|a| a.is_empty()),
            };
            if !replaceable {
                return Merge::Abandon;
            }
            doc.remove("hooks");
            doc.insert("hooks", Item::ArrayOfTables(ArrayOfTables::new()));
            changed = true;
        }
        // 上面已确保 hooks 是 array-of-tables（否则已提前返回），故此处必不为 None——
        // 仍用 let-else 而非 expect/panic。
        let Some(arr) = doc["hooks"].as_array_of_tables_mut() else {
            return Merge::Abandon;
        };

        for ev in spec.events {
            let mut found = false;
            for t in arr.iter_mut() {
                if t.get("event").and_then(|v| v.as_str()) != Some(ev.name) {
                    continue;
                }
                let Some(path) = t
                    .get("command")
                    .and_then(|v| v.as_str())
                    .and_then(|c| spec.command.claim(c, agent_id))
                else {
                    continue; // 该事件上的用户自有 hook，不动
                };
                found = true;
                if path != reporter {
                    t.insert("command", value(desired_cmd.clone()));
                    changed = true;
                }
            }
            if !found {
                let mut t = Table::new();
                t.insert("event", value(ev.name));
                t.insert("command", value(desired_cmd.clone()));
                t.insert("timeout", value(5));
                arr.push(t);
                changed = true;
            }
        }
        if changed {
            Merge::Changed
        } else {
            Merge::Unchanged
        }
    }

    /// 认领到的 reporter 路径。解析失败 → None（调用方对「读不出来」另有 Unknown 通道）。
    pub fn claimed_at(spec: &HookSpec, cur_text: &str, agent_id: &str, event: Option<&str>) -> Option<String> {
        // 迭代器借用 doc；必须先算出 owned String 再让 doc 出作用域。
        let doc = cur_text.parse::<DocumentMut>().ok()?;
        let found = doc
            .get("hooks")
            .and_then(|it| it.as_array_of_tables())
            .into_iter()
            .flat_map(|a| a.iter())
            .filter(|t| match event {
                None => true,
                Some(ev) => t.get("event").and_then(|v| v.as_str()) == Some(ev),
            })
            .find_map(|t| t.get("command").and_then(|v| v.as_str()).and_then(|c| spec.command.claim(c, agent_id)));
        found
    }
}

// ═══ JSON 系（claude / codex）共用 ═══

/// `{"hooks": {"<Event>": [{..., "hooks":[{"command":...}]}]}}` 这棵树的解析与认领扫描。
/// claude 与 codex 的差别只在**写入**时条目是否带 `matcher`，读侧完全一致，故共用。
mod json_common {
    use super::*;
    use serde_json::Value;

    /// 解析 JSON 文本。容忍 UTF-8 BOM——Windows 上不少编辑器/PowerShell 写出的 JSON 带 BOM，
    /// serde_json 会直接报错，曾导致无感接线静默失败。
    pub fn parse(text: &str) -> Option<Value> {
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        v.is_object().then_some(v)
    }

    /// 序列化为落盘文本（pretty + 末尾换行，与两家既有文件风格一致）。
    pub fn render(root: &Value) -> EnsureOutcome {
        match serde_json::to_string_pretty(root) {
            Ok(s) => EnsureOutcome::Changed(format!("{s}\n")),
            Err(_) => EnsureOutcome::Abandon(RepairReason::WriteFailed),
        }
    }

    /// 取出已认领的 reporter 路径。`event=None` 不限事件。**不看 matcher**：认领只由 command
    /// 形态决定，与条目挂在哪个 matcher 下无关。
    pub fn claimed_at(spec: &HookSpec, cur_text: &str, agent_id: &str, event: Option<&str>) -> Option<String> {
        let root = parse(cur_text)?;
        let hooks = root.get("hooks")?.as_object()?;
        hooks
            .iter()
            .filter(|(ev, _)| event.is_none_or(|want| ev.as_str() == want))
            .flat_map(|(_, arr)| arr.as_array().into_iter().flatten())
            .flat_map(|entry| entry.get("hooks").and_then(|x| x.as_array()).into_iter().flatten())
            .find_map(|h| h.get("command").and_then(|c| c.as_str()).and_then(|c| spec.command.claim(c, agent_id)))
    }

    /// 确保顶层 `hooks` 是 object。键不存在 → 建空 object（`true`＝有改动）；
    /// 存在但非 object（用户手改坏形状）→ `None`，调用方 Abandon，绝不覆盖用户文件。
    pub fn ensure_hooks_object(root: &mut Value) -> Option<bool> {
        match root.get("hooks") {
            None => {
                root["hooks"] = serde_json::json!({});
                Some(true)
            }
            Some(h) if !h.is_object() => None,
            Some(_) => Some(false),
        }
    }
}

// ═══ ClaudeJson ═══

mod claude_json {
    use super::*;
    use serde_json::{json, Value};

    pub fn ensure_hooks(spec: &HookSpec, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        // 解析失败 / 顶层非对象 → 绝不覆盖用户文件（settings.json 还装着 statusLine 等用户配置）。
        let Some(mut root) = json_common::parse(cur_text) else {
            return EnsureOutcome::Abandon(RepairReason::ConfigUnreadable);
        };
        match merge(spec, &mut root, reporter, agent_id) {
            Merge::Abandon => EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
            Merge::Unchanged => EnsureOutcome::Unchanged,
            Merge::Changed => json_common::render(&root),
        }
    }

    enum Merge {
        Changed,
        Unchanged,
        Abandon,
    }

    /// 条目的 `matcher` 是否是我们要找的那个。`want=None` 时不作要求（claude 的事件表全带
    /// matcher，此分支仅为规格完整性）。
    fn matcher_is(entry: &Value, want: Option<&str>) -> bool {
        match want {
            None => true,
            Some(m) => entry.get("matcher").and_then(|x| x.as_str()) == Some(m),
        }
    }

    /// 幂等合并。与 codex 版的唯一差别：定位/追加均**按 matcher 区分**——同一事件下用户自有的
    /// 其他 matcher 条目（如 `PreToolUse:Bash` 预检）原封不动。
    fn merge(spec: &HookSpec, root: &mut Value, reporter: &str, agent_id: &str) -> Merge {
        let desired_cmd = spec.command.render(reporter, agent_id);
        // 旧实现在此直接 `json!({})` 覆盖，会写坏用户手改成非 object 的 hooks 键。
        let mut changed = match json_common::ensure_hooks_object(root) {
            Some(c) => c,
            None => return Merge::Abandon,
        };
        let Some(hooks) = root["hooks"].as_object_mut() else {
            return Merge::Abandon;
        };

        for ev in spec.events {
            let entry_val = hooks.entry(ev.name.to_string()).or_insert_with(|| json!([]));
            let Some(arr) = entry_val.as_array_mut() else {
                continue; // 事件值存在但非 array（畸形形状）：跳过该事件不动，不置空覆盖。
            };
            let mut found = false;
            for entry in arr.iter_mut() {
                if !matcher_is(entry, ev.matcher) {
                    continue; // 该 matcher 下是用户自有 hook，不动
                }
                let Some(hs) = entry.get_mut("hooks").and_then(|x| x.as_array_mut()) else {
                    continue;
                };
                for h in hs.iter_mut() {
                    let claimed = h.get("command").and_then(|c| c.as_str()).and_then(|c| spec.command.claim(c, agent_id));
                    if let Some(path) = claimed {
                        found = true;
                        if path != reporter {
                            h["command"] = json!(desired_cmd);
                            changed = true;
                        }
                    }
                }
            }
            if !found {
                let mut entry = json!({ "hooks": [{ "type": "command", "command": desired_cmd, "timeout": 5 }] });
                if let Some(m) = ev.matcher {
                    entry["matcher"] = json!(m);
                }
                arr.push(entry);
                changed = true;
            }
        }
        if changed {
            Merge::Changed
        } else {
            Merge::Unchanged
        }
    }
}

// ═══ CodexJson ═══

mod codex_json {
    use super::*;
    use serde_json::{json, Value};

    pub fn ensure_hooks(spec: &HookSpec, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        // 解析失败 / 顶层非对象 → 绝不覆盖用户文件。
        let Some(mut root) = json_common::parse(cur_text) else {
            return EnsureOutcome::Abandon(RepairReason::ConfigUnreadable);
        };
        match merge(spec, &mut root, reporter, agent_id) {
            Merge::Abandon => EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
            Merge::Unchanged => EnsureOutcome::Unchanged,
            Merge::Changed => json_common::render(&root),
        }
    }

    enum Merge {
        Changed,
        Unchanged,
        Abandon,
    }

    fn merge(spec: &HookSpec, root: &mut Value, reporter: &str, agent_id: &str) -> Merge {
        let desired_cmd = spec.command.render(reporter, agent_id);
        // 键不存在：hooks.json 整个文件本就可从空态建，与 kimi「config.toml 缺失即未登录」
        // 不同是有意的——此处不存在不代表用户手改过畸形内容。
        // 键存在但非 object（手改坏形状）：放弃不写，绝不覆盖用户文件。
        let mut changed = match json_common::ensure_hooks_object(root) {
            Some(c) => c,
            None => return Merge::Abandon,
        };
        let Some(hooks) = root["hooks"].as_object_mut() else {
            return Merge::Abandon;
        };

        for ev in spec.events {
            let entry_val = hooks.entry(ev.name.to_string()).or_insert_with(|| json!([]));
            let Some(arr) = entry_val.as_array_mut() else {
                continue; // 事件值存在但非 array（畸形形状）：跳过该事件不动，不置空覆盖。
            };
            let mut found = false;
            for entry in arr.iter_mut() {
                let Some(hs) = entry.get_mut("hooks").and_then(|x| x.as_array_mut()) else {
                    continue;
                };
                for h in hs.iter_mut() {
                    let claimed = h.get("command").and_then(|c| c.as_str()).and_then(|c| spec.command.claim(c, agent_id));
                    if let Some(path) = claimed {
                        found = true;
                        if path != reporter {
                            h["command"] = json!(desired_cmd);
                            changed = true;
                        }
                    }
                }
            }
            if !found {
                arr.push(json!({ "hooks": [{ "type": "command", "command": desired_cmd, "timeout": 5 }] }));
                changed = true;
            }
        }
        if changed {
            Merge::Changed
        } else {
            Merge::Unchanged
        }
    }

}

/// 解析 hooks.json / settings.json 文本（容忍 BOM，顶层须为对象）。供 meowo-app 的 codex
/// trusted_hash 步骤与 claude statusLine 改写复用，免得它们各自再实现一遍 BOM 容忍。
pub fn parse_json_config(text: &str) -> Option<serde_json::Value> {
    json_common::parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kimi 的接线规格（与 plugins/kimi.rs 同源，此处独立声明以免测试依赖插件表的演化）。
    const KIMI_CMD: CommandSpec = CommandSpec { quote_exe: false, with_provider: true };
    static KIMI_EVENTS: [HookEvent; 6] = [
        HookEvent::plain("SessionStart"),
        HookEvent::plain("UserPromptSubmit"),
        HookEvent::plain("PostToolUse"),
        HookEvent::plain("Stop"),
        HookEvent::plain("SessionEnd"),
        HookEvent::plain("PermissionRequest"),
    ];
    static KT: HookSpec = HookSpec {
        config_rel: "config.toml",
        format: ConfigFormat::KimiToml,
        missing: MissingConfig::Fail(RepairReason::NeedLogin),
        events: &KIMI_EVENTS,
        command: KIMI_CMD,
    };

    fn changed(text: &str, reporter: &str) -> String {
        match KT.ensure_hooks(text, reporter, "kimi") {
            EnsureOutcome::Changed(s) => s,
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    /// claude 的接线规格（与 plugins/claude.rs 同源，此处独立声明以免测试依赖插件表的演化）。
    const CLAUDE_CMD: CommandSpec = CommandSpec { quote_exe: true, with_provider: false };
    static CLAUDE_EVENTS: [HookEvent; 8] = [
        HookEvent::matched("SessionStart", "*"),
        HookEvent::matched("UserPromptSubmit", "*"),
        HookEvent::matched("PostToolUse", "*"),
        HookEvent::matched("Stop", "*"),
        HookEvent::matched("SessionEnd", "*"),
        HookEvent::matched("PermissionRequest", "*"),
        HookEvent::matched("PreToolUse", "AskUserQuestion"),
        HookEvent::matched("PreToolUse", "ExitPlanMode"),
    ];
    static CJ: HookSpec = HookSpec {
        config_rel: "settings.json",
        format: ConfigFormat::ClaudeJson,
        missing: MissingConfig::CreateFrom("{}"),
        events: &CLAUDE_EVENTS,
        command: CLAUDE_CMD,
    };

    /// 跑一次 claude 接线，要求有改动，返回解析后的 settings。
    fn claude_changed(text: &str, reporter: &str) -> serde_json::Value {
        match CJ.ensure_hooks(text, reporter, "claude") {
            EnsureOutcome::Changed(s) => serde_json::from_str(&s).expect("产物应为合法 JSON"),
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    // ── ClaudeJson ──

    #[test]
    fn claude_claim_never_touches_user_hooks() {
        // 我们写入的形态：带引号的单可执行路径，无参数。
        assert_eq!(CLAUDE_CMD.claim("\"C:/x/meowo-reporter.exe\"", "claude").as_deref(), Some("C:/x/meowo-reporter.exe"));
        assert_eq!(CLAUDE_CMD.claim("/usr/local/bin/meowo-reporter", "claude").as_deref(), Some("/usr/local/bin/meowo-reporter"));
        // 不能误伤用户自有 hook：带参数、是别的脚本、或只是路径里含子串。
        // `node tools/meowo-reporter-notify.js` 是这条纪律的原始反例，务必保住。
        assert_eq!(CLAUDE_CMD.claim("node tools/meowo-reporter-notify.js", "claude"), None);
        assert_eq!(CLAUDE_CMD.claim("\"C:/x/meowo-reporter.exe\" --flag", "claude"), None);
        assert_eq!(CLAUDE_CMD.claim("/opt/meowo-reporter/run.sh", "claude"), None);
        assert_eq!(CLAUDE_CMD.claim("meowo-reporter-wrapper", "claude"), None);
        assert_eq!(CLAUDE_CMD.claim("", "claude"), None);
    }

    #[test]
    fn claude_ensure_hooks_adds_all_specs_including_pretooluse_matchers() {
        let v = claude_changed("{}", "C:/x/meowo-reporter.exe");
        for e in ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "SessionEnd", "PermissionRequest"] {
            assert_eq!(v["hooks"][e][0]["matcher"], "*", "{e} matcher");
            assert_eq!(v["hooks"][e][0]["hooks"][0]["command"], "\"C:/x/meowo-reporter.exe\"");
        }
        // PreToolUse：两条，matcher 分别 AskUserQuestion / ExitPlanMode。
        let matchers: Vec<&str> =
            v["hooks"]["PreToolUse"].as_array().unwrap().iter().map(|e| e["matcher"].as_str().unwrap()).collect();
        assert!(matchers.contains(&"AskUserQuestion"));
        assert!(matchers.contains(&"ExitPlanMode"));
        // 幂等。
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(CJ.ensure_hooks(&text, "C:/x/meowo-reporter.exe", "claude"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn claude_ensure_hooks_preserves_user_pretooluse_bash() {
        // 用户自有 PreToolUse:Bash node 预检，不是 meowo-reporter。
        let src = r#"{"hooks":{"PreToolUse":[
            {"matcher":"Bash","hooks":[{"type":"command","command":"node \"x/pre-check.cjs\""}]}
        ]}}"#;
        let v = claude_changed(src, "C:/x/meowo-reporter.exe");
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        // 原 Bash 条目原封保留。
        let bash = pre.iter().find(|e| e["matcher"] == "Bash").unwrap();
        assert_eq!(bash["hooks"][0]["command"], "node \"x/pre-check.cjs\"");
        // 且新增了 AskUserQuestion / ExitPlanMode 两条 meowo-reporter。
        assert!(pre.iter().any(|e| e["matcher"] == "AskUserQuestion"));
        assert!(pre.iter().any(|e| e["matcher"] == "ExitPlanMode"));
    }

    #[test]
    fn claude_ensure_hooks_updates_changed_path_and_keeps_other_hooks() {
        // 同一 matcher 下：一个别的 hook + 一个旧路径的 meowo-reporter。
        let src = r#"{"hooks":{"SessionStart":[
            {"matcher":"*","hooks":[{"type":"command","command":"node other.js"}]},
            {"matcher":"*","hooks":[{"type":"command","command":"\"C:/old/meowo-reporter.exe\"","timeout":5}]}
        ]}}"#;
        let v = claude_changed(src, "C:/new/meowo-reporter.exe");
        assert_eq!(v["hooks"]["SessionStart"][0]["hooks"][0]["command"], "node other.js");
        assert_eq!(v["hooks"]["SessionStart"][1]["hooks"][0]["command"], "\"C:/new/meowo-reporter.exe\"");
        // 没有重复追加（该事件仍是 2 条）。
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn claude_ensure_hooks_claims_legacy_cc_reporter() {
        // 行为改进（与 codex 同）：SessionStart 上挂着废弃的 cc-reporter 时认领并更新，
        // 而非不认领、重复追加一条。
        let src = r#"{"hooks":{"SessionStart":[
            {"matcher":"*","hooks":[{"type":"command","command":"\"C:/old/cc-reporter.exe\"","timeout":5}]}
        ]}}"#;
        let v = claude_changed(src, "C:/new/meowo-reporter.exe");
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 1, "应认领而非追加");
        assert_eq!(v["hooks"]["SessionStart"][0]["hooks"][0]["command"], "\"C:/new/meowo-reporter.exe\"");
    }

    #[test]
    fn claude_abandons_when_hooks_key_is_not_object() {
        // 旧实现在此直接 `json!({})` 覆盖，会写坏用户文件。现在如实 Abandon。
        let out = CJ.ensure_hooks(r#"{"hooks":[1,2]}"#, "C:/x/meowo-reporter.exe", "claude");
        assert_eq!(out, EnsureOutcome::Abandon(RepairReason::ConfigUnreadable));
        // 顶层非对象、非法 JSON 同样放弃。
        assert!(matches!(CJ.ensure_hooks("[]", "r", "claude"), EnsureOutcome::Abandon(_)));
        assert!(matches!(CJ.ensure_hooks("{not json", "r", "claude"), EnsureOutcome::Abandon(_)));
    }

    #[test]
    fn claude_tolerates_utf8_bom_and_preserves_top_level_keys() {
        // Windows 编辑器/PowerShell 常写出带 BOM 的 JSON；顶层 statusLine 等无关键须原样保留。
        let src = "\u{feff}{\"statusLine\":{\"type\":\"command\",\"command\":\"hud\"},\"model\":\"opus\"}";
        let v = claude_changed(src, "C:/x/meowo-reporter.exe");
        assert_eq!(v["statusLine"]["command"], "hud");
        assert_eq!(v["model"], "opus");
        assert!(v["hooks"]["SessionStart"].is_array());
    }

    #[test]
    fn claude_has_reporter_requires_session_start_and_current_binary() {
        // 只在 Stop 挂了 meowo-reporter：不能保证新会话入库，不应判定为已接入。
        // claimed_reporter（广扫，供路径复用）仍会命中这条，语义刻意不同。
        let stop_only = r#"{"hooks":{"Stop":[{"matcher":"*","hooks":[
            {"type":"command","command":"\"C:/a/b/meowo-reporter.exe\"","timeout":5}]}]}}"#;
        assert!(!CJ.has_reporter(stop_only, "claude"));
        assert_eq!(CJ.claimed_reporter(stop_only, "claude").as_deref(), Some("C:/a/b/meowo-reporter.exe"));

        let session_start = r#"{"hooks":{"SessionStart":[{"matcher":"*","hooks":[
            {"type":"command","command":"\"C:/a/b/meowo-reporter.exe\"","timeout":5}]}]}}"#;
        assert!(CJ.has_reporter(session_start, "claude"));

        // 历史 cc-reporter 不算「已接入」，也不作为可复用路径（写回去 hooks 依旧失效）。
        let legacy = r#"{"hooks":{"SessionStart":[{"matcher":"*","hooks":[
            {"type":"command","command":"\"C:/a/b/cc-reporter.exe\"","timeout":5}]}]}}"#;
        assert!(!CJ.has_reporter(legacy, "claude"));
        assert_eq!(CJ.claimed_reporter(legacy, "claude"), None);

        assert!(!CJ.has_reporter("{}", "claude"));
    }

    #[test]
    fn claude_real_shape_user_settings_merge() {
        // 精确复刻真实 settings.json 结构：PreToolUse(node 预检) + 5 个 meowo-reporter 事件 + claude-hud statusLine。
        let ccr = "C:/Users/larry/workspace/meowo/target/release/meowo-reporter.exe";
        let src = format!(
            r#"{{"hooks":{{
                "PreToolUse":[{{"matcher":"Bash","hooks":[{{"type":"command","command":"node \"x/pre-commit-check.cjs\"","timeout":5000}}]}}],
                "SessionStart":[{{"matcher":"*","hooks":[{{"type":"command","command":"\"{ccr}\"","timeout":5}}]}}],
                "UserPromptSubmit":[{{"matcher":"*","hooks":[{{"type":"command","command":"\"{ccr}\"","timeout":5}}]}}],
                "PostToolUse":[{{"matcher":"*","hooks":[{{"type":"command","command":"\"{ccr}\"","timeout":5}}]}}],
                "Stop":[{{"matcher":"*","hooks":[{{"type":"command","command":"\"{ccr}\"","timeout":5}}]}}],
                "SessionEnd":[{{"matcher":"*","hooks":[{{"type":"command","command":"\"{ccr}\"","timeout":5}}]}}]
            }},"statusLine":{{"type":"command","command":"bash -c 'claude-hud stuff'"}}}}"#
        );
        // fixture 缺 PermissionRequest / PreToolUse(AskUserQuestion|ExitPlanMode) → 追加 3 条。
        let v = claude_changed(&src, ccr);
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "node \"x/pre-commit-check.cjs\"");
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(pre.iter().any(|e| e["matcher"] == "AskUserQuestion"));
        assert!(pre.iter().any(|e| e["matcher"] == "ExitPlanMode"));
        assert_eq!(v["hooks"]["PermissionRequest"][0]["matcher"], "*");
        // statusLine 原样保留（它的改写由 meowo-app 的 amend 负责，不在格式适配器里）。
        assert_eq!(v["statusLine"]["command"], "bash -c 'claude-hud stuff'");
        // 再跑一次：此时才幂等。
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(CJ.ensure_hooks(&text, ccr, "claude"), EnsureOutcome::Unchanged);
    }

    // ── CommandSpec ──

    #[test]
    fn render_covers_three_agent_shapes() {
        let claude = CommandSpec { quote_exe: true, with_provider: false };
        let codex = CommandSpec { quote_exe: true, with_provider: true };
        assert_eq!(claude.render("C:/x/meowo-reporter.exe", "claude"), "\"C:/x/meowo-reporter.exe\"");
        assert_eq!(codex.render("C:/x/meowo-reporter.exe", "codex"), "\"C:/x/meowo-reporter.exe\" --provider codex");
        assert_eq!(KIMI_CMD.render("C:/x/meowo-reporter.exe", "kimi"), "C:/x/meowo-reporter.exe --provider kimi");
    }

    #[test]
    fn claim_with_provider_is_strict() {
        // 认领：带引号/裸路径两种形态。
        assert_eq!(KIMI_CMD.claim("\"C:/x/meowo-reporter.exe\" --provider kimi", "kimi").as_deref(), Some("C:/x/meowo-reporter.exe"));
        assert_eq!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider kimi", "kimi").as_deref(), Some("C:/x/meowo-reporter.exe"));
        // 历史遗留 cc-reporter 也认领，便于升级时替换旧路径。
        assert_eq!(KIMI_CMD.claim("C:/x/cc-reporter.exe --provider kimi", "kimi").as_deref(), Some("C:/x/cc-reporter.exe"));
        // 拒绝：agent 不符 / 无参数 / 多余参数 / 别的可执行 / 子串陷阱。
        assert!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider codex", "kimi").is_none());
        assert!(KIMI_CMD.claim("\"C:/x/meowo-reporter.exe\"", "kimi").is_none());
        assert!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider kimi --v", "kimi").is_none());
        assert!(KIMI_CMD.claim("node meowo-reporter-notify.js --provider kimi", "kimi").is_none());
        assert!(KIMI_CMD.claim("C:/x/cc-reporter-not-us.exe --provider kimi", "kimi").is_none());
    }

    #[test]
    fn claim_bare_quoted_rejects_any_argument() {
        // claude 形态：单个（可带引号的）可执行路径，禁带参数。
        let bare = CommandSpec { quote_exe: true, with_provider: false };
        assert_eq!(bare.claim("\"C:/x/meowo-reporter.exe\"", "claude").as_deref(), Some("C:/x/meowo-reporter.exe"));
        assert_eq!(bare.claim("/usr/local/bin/meowo-reporter", "claude").as_deref(), Some("/usr/local/bin/meowo-reporter"));
        // 带参数 = 不是我们写的那条；用户自有 hook 一概不认领。
        assert!(bare.claim("\"C:/x/meowo-reporter.exe\" --flag", "claude").is_none());
        assert!(bare.claim("node tools/meowo-reporter-notify.js", "claude").is_none());
        assert!(bare.claim("/opt/meowo-reporter/run.sh", "claude").is_none());
        assert!(bare.claim("meowo-reporter-wrapper", "claude").is_none());
        assert!(bare.claim("", "claude").is_none());
    }

    // ── KimiToml::ensure_hooks ──

    #[test]
    fn adds_all_when_absent_and_preserves_content() {
        let src = "default_model = \"kimi-code/kimi-for-coding\"\n# 用户注释\n[loop_control]\nmax_steps_per_turn = 100\n";
        let out = changed(src, "C:/x/meowo-reporter.exe");
        assert!(out.contains("# 用户注释")); // 结构保持：注释仍在
        assert!(out.contains("max_steps_per_turn = 100"));
        for ev in KIMI_EVENTS {
            assert!(out.contains(&format!("event = \"{}\"", ev.name)), "{} 未写入", ev.name);
        }
        assert!(out.contains(r#"command = "C:/x/meowo-reporter.exe --provider kimi""#));
        assert_eq!(KT.ensure_hooks(&out, "C:/x/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn adopts_manual_and_updates_stale_path() {
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!("[[hooks]]\nevent = \"{}\"\ncommand = \"{dev} --provider kimi\"\ntimeout = 5\n\n", ev.name));
        }
        // 路径一致：无改动。
        assert_eq!(KT.ensure_hooks(&src, dev, "kimi"), EnsureOutcome::Unchanged);
        // 路径失效换 sidecar：6 条 command 全部更新，用户键 theme 不动。
        let out = changed(&src, "C:/app/meowo-reporter.exe");
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
    }

    #[test]
    fn updates_legacy_cc_reporter_paths() {
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!(
                "[[hooks]]\nevent = \"{}\"\ncommand = \"C:/x/cc-reporter.exe --provider kimi\"\ntimeout = 5\n\n",
                ev.name
            ));
        }
        // 接线前：SessionStart 上挂的是废弃的 cc-reporter → 不算已接入。
        assert!(!KT.has_reporter(&src, "kimi"));
        assert_eq!(KT.claimed_reporter(&src, "kimi"), None);

        let out = changed(&src, "C:/app/meowo-reporter.exe");
        assert_eq!(out.matches("cc-reporter").count(), 0);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
        assert!(KT.has_reporter(&out, "kimi"));
        assert_eq!(KT.claimed_reporter(&out, "kimi").as_deref(), Some("C:/app/meowo-reporter.exe"));
        assert_eq!(KT.ensure_hooks(&out, "C:/app/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn keeps_user_hook_entries() {
        let src = "[[hooks]]\nevent = \"Notification\"\ncommand = \"my-notify --ding\"\ntimeout = 3\n";
        let out = changed(src, "C:/x/meowo-reporter.exe");
        assert!(out.contains("my-notify --ding")); // 用户 hook 原样
        assert_eq!(out.matches("--provider kimi").count(), 6);
    }

    #[test]
    fn abandons_on_nonempty_or_malformed_hooks() {
        for src in ["hooks = [1, 2]\n", "hooks = \"oops\"\n"] {
            assert_eq!(
                KT.ensure_hooks(src, "C:/x/meowo-reporter.exe", "kimi"),
                EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
                "src={src:?}"
            );
        }
    }

    #[test]
    fn abandons_on_invalid_toml() {
        assert_eq!(
            KT.ensure_hooks("this is not = = toml\n", "C:/x/meowo-reporter.exe", "kimi"),
            EnsureOutcome::Abandon(RepairReason::ConfigUnreadable)
        );
    }

    #[test]
    fn replaces_empty_inline_array_from_legacy_kimi() {
        // 旧 Python 版 kimi-cli 的真实结构：顶层标量 + `hooks = []` 空内联数组 + 各 [section] 表。
        let src = "\
default_model = \"kimi-code/kimi-for-coding\"
theme = \"dark\"
hooks = []
merge_all_available_skills = true

[models.\"kimi-code/kimi-for-coding\"]
provider = \"managed:kimi-code\"

[providers.\"managed:kimi-code\"]
type = \"managed\"
api_key = \"secret-should-survive\"

[loop_control]
max_steps_per_turn = 100
";
        let out = changed(src, "C:/app/meowo-reporter.exe");
        // 输出必须仍是合法 TOML（array-of-tables 不能错位插到 section 里）。
        let reparsed: DocumentMut = out.parse().expect("产物应为合法 TOML");
        let arr = reparsed["hooks"].as_array_of_tables().expect("hooks 应为 array-of-tables");
        assert_eq!(arr.len(), 6);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        // 其余键与 [section] 表原样保留（含 api_key，绝不能丢）。
        assert_eq!(reparsed["default_model"].as_str(), Some("kimi-code/kimi-for-coding"));
        assert_eq!(reparsed["merge_all_available_skills"].as_bool(), Some(true));
        assert_eq!(reparsed["providers"]["managed:kimi-code"]["api_key"].as_str(), Some("secret-should-survive"));
        assert_eq!(reparsed["loop_control"]["max_steps_per_turn"].as_integer(), Some(100));
        assert_eq!(KT.ensure_hooks(&out, "C:/app/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    // ── has_reporter / claimed_reporter ──

    #[test]
    fn has_reporter_only_counts_session_start() {
        let session_start = "[[hooks]]\nevent = \"SessionStart\"\ncommand = \"/home/u/.local/meowo-reporter --provider kimi\"\ntimeout = 5\n";
        assert!(KT.has_reporter(session_start, "kimi"));
        assert!(!KT.has_reporter(session_start, "codex")); // agent 不符

        // 只在 Stop 挂了 reporter：不能保证新会话入库，不应判定为已接入。
        let stop_only = "[[hooks]]\nevent = \"Stop\"\ncommand = \"/home/u/.local/meowo-reporter --provider kimi\"\ntimeout = 5\n";
        assert!(!KT.has_reporter(stop_only, "kimi"));
        // 但接线时仍能从中取到 reporter 位置（不限事件）。
        assert_eq!(KT.claimed_reporter(stop_only, "kimi").as_deref(), Some("/home/u/.local/meowo-reporter"));

        // Stop 块在前、SessionStart 块在后：不串块。
        let both = format!("{stop_only}\n{session_start}");
        assert!(KT.has_reporter(&both, "kimi"));

        // 非 hooks 结构 / 用户自有命令 / 畸形 TOML → false。
        assert!(!KT.has_reporter("event = \"SessionStart\"\ncommand = \"node a.js\"\n", "kimi"));
        assert!(!KT.has_reporter("[[hooks]]\nevent = \"SessionStart\"\ncommand = \"node a.js\"\n", "kimi"));
        assert!(!KT.has_reporter("= = 非法 toml", "kimi"));
    }

    // ═══ CodexJson ═══

    mod codex {
        use super::super::*;
        use serde_json::{json, Value};

        /// codex 的接线规格。事件集 = dispatch 消化面 ∩ codex 0.142 支持面：无 SessionEnd
        /// （codex 不支持，会话收尾靠 Stop + liveness）；不配 PreToolUse（matcher 目标是 claude 专属工具）。
        static EVENTS: [HookEvent; 5] = [
            HookEvent::plain("SessionStart"),
            HookEvent::plain("UserPromptSubmit"),
            HookEvent::plain("PostToolUse"),
            HookEvent::plain("Stop"),
            HookEvent::plain("PermissionRequest"),
        ];
        static CJ: HookSpec = HookSpec {
            config_rel: "hooks.json",
            format: ConfigFormat::CodexJson,
            missing: MissingConfig::CreateFrom("{\"hooks\":{}}"),
            events: &EVENTS,
            command: CommandSpec { quote_exe: true, with_provider: true },
        };

        fn changed(v: &Value, reporter: &str) -> Value {
            match CJ.ensure_hooks(&v.to_string(), reporter, "codex") {
                EnsureOutcome::Changed(s) => serde_json::from_str(&s).unwrap(),
                other => panic!("期望 Changed，实得 {other:?}"),
            }
        }
        fn outcome(v: &Value, reporter: &str) -> EnsureOutcome {
            CJ.ensure_hooks(&v.to_string(), reporter, "codex")
        }

        #[test]
        fn adds_all_events_when_empty() {
            let out = changed(&json!({}), "C:/x/meowo-reporter.exe");
            for ev in EVENTS {
                let h = &out["hooks"][ev.name][0]["hooks"][0];
                assert_eq!(h["command"], "\"C:/x/meowo-reporter.exe\" --provider codex");
                assert_eq!(h["timeout"], 5);
            }
            assert_eq!(outcome(&out, "C:/x/meowo-reporter.exe"), EnsureOutcome::Unchanged);
        }

        #[test]
        fn adopts_manual_wiring_and_fills_missing() {
            // 复刻手工接线形态：裸路径命令、3 事件、Stop timeout=10。
            let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
            let entry = |t: u64| json!({ "hooks": [{ "type": "command", "command": format!("{dev} --provider codex"), "timeout": t }] });
            let v = json!({ "hooks": { "SessionStart": [entry(5)], "UserPromptSubmit": [entry(5)], "Stop": [entry(10)] }});
            let out = changed(&v, dev); // 补 PostToolUse/PermissionRequest → 有改动
            // 既有条目原样保留（裸路径不被改写为引号形态、timeout 10 不动）——幂等按解析后内容判定。
            assert_eq!(out["hooks"]["Stop"][0]["hooks"][0]["command"], format!("{dev} --provider codex"));
            assert_eq!(out["hooks"]["Stop"][0]["hooks"][0]["timeout"], 10);
            assert!(out["hooks"]["PostToolUse"][0]["hooks"][0]["command"].as_str().unwrap().contains("--provider codex"));
            assert!(out["hooks"]["PermissionRequest"].is_array());
            assert_eq!(outcome(&out, dev), EnsureOutcome::Unchanged);
        }

        #[test]
        fn updates_stale_path_keeps_user_hooks() {
            let v = json!({ "hooks": { "Stop": [
                { "hooks": [{ "type": "command", "command": "node my-notify.js" }] },
                { "hooks": [{ "type": "command", "command": "\"C:/old/meowo-reporter.exe\" --provider codex", "timeout": 5 }] }
            ]}});
            let out = changed(&v, "C:/new/meowo-reporter.exe");
            assert_eq!(out["hooks"]["Stop"][0]["hooks"][0]["command"], "node my-notify.js"); // 用户 hook 不动
            assert_eq!(out["hooks"]["Stop"][1]["hooks"][0]["command"], "\"C:/new/meowo-reporter.exe\" --provider codex");
            assert_eq!(out["hooks"]["Stop"].as_array().unwrap().len(), 2); // 不重复追加
        }

        #[test]
        fn abandons_when_hooks_key_is_non_object() {
            // 手改坏形状：既有实现会整体置 {}，无备份地清掉用户内容。必须放弃。
            // 旧实现返回「无改动」，会被上层当成「已是目标状态」而谎报成功；现在如实回传原因。
            assert_eq!(
                outcome(&json!({ "hooks": 5 }), "C:/x/meowo-reporter.exe"),
                EnsureOutcome::Abandon(RepairReason::ConfigUnreadable)
            );
        }

        #[test]
        fn abandons_on_invalid_or_non_object_json() {
            for src in ["{not json", "[1,2]", "\"scalar\""] {
                assert_eq!(
                    CJ.ensure_hooks(src, "C:/x/meowo-reporter.exe", "codex"),
                    EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
                    "src={src:?}"
                );
            }
        }

        #[test]
        fn tolerates_utf8_bom() {
            let out = CJ.ensure_hooks("\u{feff}{\"hooks\":{}}", "C:/x/meowo-reporter.exe", "codex");
            assert!(matches!(out, EnsureOutcome::Changed(_)), "带 BOM 的 JSON 应能解析");
        }

        #[test]
        fn skips_event_with_non_array_value() {
            // 某事件值为畸形形状（非 array）：该事件原样跳过不动，其余事件正常补齐。
            let out = changed(&json!({ "hooks": { "Stop": "oops" } }), "C:/x/meowo-reporter.exe");
            assert_eq!(out["hooks"]["Stop"], json!("oops"));
            for ev in EVENTS.iter().filter(|e| e.name != "Stop") {
                assert!(out["hooks"][ev.name][0]["hooks"][0]["command"].as_str().unwrap().contains("--provider codex"));
            }
        }

        #[test]
        fn has_reporter_only_counts_session_start() {
            let wired = changed(&json!({}), "C:/x/meowo-reporter.exe").to_string();
            assert!(CJ.has_reporter(&wired, "codex"));
            assert!(!CJ.has_reporter(&wired, "kimi")); // agent 不符
            assert_eq!(CJ.claimed_reporter(&wired, "codex").as_deref(), Some("C:/x/meowo-reporter.exe"));

            // 只在 Stop 挂了 reporter：不应判定为已接入；但仍能取到二进制位置。
            let stop_only = json!({ "hooks": { "Stop": [
                { "hooks": [{ "type": "command", "command": "\"C:/x/meowo-reporter.exe\" --provider codex", "timeout": 5 }] }
            ]}}).to_string();
            assert!(!CJ.has_reporter(&stop_only, "codex"));
            assert_eq!(CJ.claimed_reporter(&stop_only, "codex").as_deref(), Some("C:/x/meowo-reporter.exe"));

            // 废弃的 cc-reporter 挂在 SessionStart：认领得到（供替换），但不算已接入。
            let legacy = json!({ "hooks": { "SessionStart": [
                { "hooks": [{ "type": "command", "command": "\"C:/x/cc-reporter.exe\" --provider codex", "timeout": 5 }] }
            ]}}).to_string();
            assert!(!CJ.has_reporter(&legacy, "codex"));
            assert_eq!(CJ.claimed_reporter(&legacy, "codex"), None);
            // 接线时会被认领并更新为当前 reporter，而非重复追加。
            let fixed = changed(&serde_json::from_str::<Value>(&legacy).unwrap(), "C:/new/meowo-reporter.exe");
            assert_eq!(fixed["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
            assert!(CJ.has_reporter(&fixed.to_string(), "codex"));

            // 用户自有 hook / 无 hooks 键 → false。
            assert!(!CJ.has_reporter(&json!({ "hooks": { "SessionStart": [{ "hooks": [{ "command": "node a.js" }] }] }}).to_string(), "codex"));
            assert!(!CJ.has_reporter("{}", "codex"));
        }
    }
}
