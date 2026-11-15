//! codex 0.14x 起的「code mode」：模型不再逐个调工具，而是写一段 JS 交给名为 `exec` 的
//! 工具去跑，真正的工具调用藏在脚本里（本机 68 份 rollout、4541 段脚本：`exec_command`
//! 4130 次、`apply_patch` 1573 次、`write_stdin` 403 次、`web__run` 198 次）：
//!
//! ```js
//! const r = await tools.exec_command({"cmd":"git status","workdir":"C:\\repo"});
//! text(r.output);
//! ```
//!
//! ```js
//! text(await tools.apply_patch("*** Begin Patch\n*** Update File: a.rs\n@@\n-old\n+new\n*** End Patch"));
//! text(await tools.exec_command({cmd:"cargo test",max_output_tokens:3000}));
//! ```
//!
//! 对话页此前把整段 JS 当命令显示。这里把脚本解开：按出现顺序找出每个 `tools.<名字>(实参)`，
//! 实参是 JSON、不带引号键的对象字面量、字符串字面量，或前面 `const x = "…"` 绑定过的变量。
//! 只做「认字面量」这一件事，不执行脚本；认不出的实参留 `Null`，调用方退回显示工具名。
//!
//! 回执一侧（`Script completed\nWall time …\nOutput:\n` 后面每个 `text()` 一段）也在这里拆。
//! 两侧都**只看本行**：rollout 是逐行独立解析的（首屏只读尾部、增量只读新行），不能靠
//! 「先见过调用」来理解回执。所以两侧用同一条约定对上号——第 i 个调用的 id 是
//! `<call_id>#<i>`，回执的第 i 段挂到 `<call_id>#<i>`。

use serde_json::{Map, Value};

use crate::transcript::{DiffHunk, PatchFile, PATCH_MAX_LINES, PATCH_MAX_LINE_CHARS};

/// 脚本里的一次工具调用。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ScriptCall {
    pub name: String,
    /// 第一个实参；认不出时为 `Null`。
    pub arg: Value,
}

/// 脚本解析的规模上限：防病态输入（脚本本身最长见过几十 KB）。
const MAX_CALLS: usize = 32;
const MAX_DEPTH: usize = 32;

/// 按出现顺序找出脚本里的全部 `tools.<名字>(…)` 调用。
pub(super) fn script_calls(src: &str) -> Vec<ScriptCall> {
    let chars: Vec<char> = src.chars().collect();
    let mut scan = Scan {
        chars: &chars,
        at: 0,
    };
    let mut bindings: Vec<(String, Value)> = Vec::new();
    let mut calls = Vec::new();
    while scan.at < chars.len() && calls.len() < MAX_CALLS {
        let c = chars[scan.at];
        // 注释与字符串整段跳过：里面出现的 `tools.x(` 不是调用。
        if c == '/' && scan.peek(1) == Some('/') {
            while scan.at < chars.len() && chars[scan.at] != '\n' {
                scan.at += 1;
            }
            continue;
        }
        if c == '/' && scan.peek(1) == Some('*') {
            scan.at += 2;
            while scan.at < chars.len() && !(chars[scan.at] == '*' && scan.peek(1) == Some('/')) {
                scan.at += 1;
            }
            scan.at = (scan.at + 2).min(chars.len());
            continue;
        }
        if matches!(c, '"' | '\'' | '`') {
            if scan.string().is_none() {
                scan.at += 1;
            }
            continue;
        }
        if !is_ident_start(c) || scan.prev_is_ident() {
            scan.at += 1;
            continue;
        }
        let word = scan.ident();
        match word.as_str() {
            // `const x = <字面量>`：记下来，后面 `tools.apply_patch(x)` 用得上。
            "const" | "let" | "var" => {
                let mark = scan.at;
                scan.skip_space();
                let name = scan.ident();
                scan.skip_space();
                if !name.is_empty() && scan.eat('=') && scan.peek(0) != Some('=') {
                    scan.skip_space();
                    let before = scan.at;
                    match scan.value(0) {
                        Some(value) => bindings.push((name, value)),
                        None => scan.at = before,
                    }
                } else {
                    scan.at = mark;
                }
            }
            "tools" if scan.peek(0) == Some('.') => {
                scan.at += 1;
                let name = scan.ident();
                scan.skip_space();
                if name.is_empty() || !scan.eat('(') {
                    continue;
                }
                scan.skip_space();
                let before = scan.at;
                let arg = scan.value(0).or_else(|| {
                    scan.at = before;
                    let var = scan.ident();
                    bindings
                        .iter()
                        .rev()
                        .find(|(name, _)| *name == var)
                        .map(|(_, value)| value.clone())
                });
                if arg.is_none() {
                    scan.at = before;
                }
                calls.push(ScriptCall {
                    name,
                    arg: arg.unwrap_or(Value::Null),
                });
            }
            _ => {}
        }
    }
    calls
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

struct Scan<'a> {
    chars: &'a [char],
    at: usize,
}

impl Scan<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn prev_is_ident(&self) -> bool {
        self.at > 0 && (is_ident_part(self.chars[self.at - 1]) || self.chars[self.at - 1] == '.')
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek(0) == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn ident(&mut self) -> String {
        let start = self.at;
        while self.peek(0).is_some_and(is_ident_part) {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    /// 字符串字面量（`"…"` / `'…'` / 不带插值的 `` `…` ``）。带 `${` 插值的模板串不认。
    fn string(&mut self) -> Option<String> {
        let quote = self.peek(0)?;
        if !matches!(quote, '"' | '\'' | '`') {
            return None;
        }
        let start = self.at;
        self.at += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek(0) else {
                self.at = start;
                return None;
            };
            self.at += 1;
            if c == quote {
                return Some(out);
            }
            if quote == '`' && c == '$' && self.peek(0) == Some('{') {
                self.at = start;
                return None;
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(escaped) = self.peek(0) else {
                self.at = start;
                return None;
            };
            self.at += 1;
            match escaped {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'v' => out.push('\u{b}'),
                '0' => out.push('\0'),
                '\n' => {}
                'x' => out.push(self.hex(2).unwrap_or('\u{fffd}')),
                'u' => {
                    let unit = if self.eat('{') {
                        let start = self.at;
                        while self.peek(0).is_some_and(|c| c != '}') {
                            self.at += 1;
                        }
                        let digits: String = self.chars[start..self.at].iter().collect();
                        self.at = (self.at + 1).min(self.chars.len());
                        u32::from_str_radix(&digits, 16).ok()
                    } else {
                        self.hex_unit(4)
                    };
                    // 代理对：高位后面紧跟 `\uDC00..DFFF` 时合成一个码点。
                    let code = match unit {
                        Some(high @ 0xD800..=0xDBFF)
                            if self.peek(0) == Some('\\') && self.peek(1) == Some('u') =>
                        {
                            let mark = self.at;
                            self.at += 2;
                            match self.hex_unit(4) {
                                Some(low @ 0xDC00..=0xDFFF) => {
                                    Some(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
                                }
                                _ => {
                                    self.at = mark;
                                    Some(high)
                                }
                            }
                        }
                        other => other,
                    };
                    out.push(code.and_then(char::from_u32).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            }
        }
    }

    fn hex_unit(&mut self, len: usize) -> Option<u32> {
        let digits: String = self.chars.get(self.at..self.at + len)?.iter().collect();
        let value = u32::from_str_radix(&digits, 16).ok()?;
        self.at += len;
        Some(value)
    }

    fn hex(&mut self, len: usize) -> Option<char> {
        self.hex_unit(len).and_then(char::from_u32)
    }

    /// JS 字面量 → JSON 值。认：字符串、数字、true/false/null/undefined、数组、对象
    /// （键可不带引号，允许尾逗号）。不是字面量返回 None，位置由调用方复原。
    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.skip_space();
        let c = self.peek(0)?;
        if matches!(c, '"' | '\'' | '`') {
            return self.string().map(Value::String);
        }
        if c == '{' {
            self.at += 1;
            let mut map = Map::new();
            loop {
                self.skip_space();
                if self.eat('}') {
                    return Some(Value::Object(map));
                }
                let key = match self.peek(0)? {
                    '"' | '\'' | '`' => self.string()?,
                    c if is_ident_start(c) => self.ident(),
                    c if c.is_ascii_digit() => {
                        let start = self.at;
                        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                            self.at += 1;
                        }
                        self.chars[start..self.at].iter().collect()
                    }
                    _ => return None,
                };
                self.skip_space();
                if !self.eat(':') {
                    return None;
                }
                let value = self.value(depth + 1)?;
                map.insert(key, value);
                self.skip_space();
                if !self.eat(',') {
                    self.skip_space();
                    return self.eat('}').then_some(Value::Object(map));
                }
            }
        }
        if c == '[' {
            self.at += 1;
            let mut list = Vec::new();
            loop {
                self.skip_space();
                if self.eat(']') {
                    return Some(Value::Array(list));
                }
                list.push(self.value(depth + 1)?);
                self.skip_space();
                if !self.eat(',') {
                    self.skip_space();
                    return self.eat(']').then_some(Value::Array(list));
                }
            }
        }
        if c.is_ascii_digit() || c == '-' || c == '.' {
            let start = self.at;
            self.at += 1;
            while self
                .peek(0)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
            {
                // `1e-3` 的符号只在指数后面才算数字的一部分。
                if matches!(self.peek(0), Some('+' | '-'))
                    && !matches!(self.chars[self.at - 1], 'e' | 'E')
                {
                    break;
                }
                self.at += 1;
            }
            let text: String = self.chars[start..self.at]
                .iter()
                .filter(|c| **c != '_')
                .collect();
            return serde_json::from_str::<Value>(&text)
                .ok()
                .filter(Value::is_number);
        }
        if is_ident_start(c) {
            let start = self.at;
            let word = self.ident();
            return match word.as_str() {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                "null" | "undefined" => Some(Value::Null),
                _ => {
                    self.at = start;
                    None
                }
            };
        }
        None
    }
}

/// 脚本回执里的一段：对应脚本里的一次 `text(…)`。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct OutputPart {
    pub text: String,
    pub exit_code: Option<i32>,
    pub is_error: bool,
    pub interrupted: bool,
}

/// 拆脚本回执。`output` 是 rollout 里 `custom_tool_call_output.output` 原值：
/// - 数组：第 0 项是脚本状态头（`Script completed|failed\nWall time …\nOutput:\n`），其后每项
///   是一次 `text()` 的输出（本机 4168 条 completed、65 条 failed 都是这个形态）。
/// - 字符串：脚本还没出结果就返回了——让到后台（`Script running with cell ID g9:1`，结果
///   之后由 `wait` 取）、被用户打断（`aborted by user after 20.9s`），或老版本的纯文本回执。
///
/// 每段正文若是命令结果的 JSON（`{"chunk_id":…,"exit_code":1,"output":"…"}`，脚本写的是
/// `text(await tools.exec_command(…))`），取出其中的 `output` 与退出码；老版本的
/// `shell_command` 返回的是带文本头的字符串（见 [`shell_result`]），同样取出退出码。脚本写
/// `text(r.output)` 时那段本来就是纯输出，没有退出码可取——那种由读取层按旁路的
/// CommandExecution 记录补（`transcript::ExitJoin`）。
pub(super) fn output_parts(output: &Value) -> Vec<OutputPart> {
    let segments: Vec<String> = match output {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str().or_else(|| {
                    item.get("text")
                        .or_else(|| item.get("input_text"))
                        .or_else(|| item.get("output_text"))
                        .and_then(Value::as_str)
                })
            })
            .map(str::to_string)
            .collect(),
        Value::String(text) => vec![text.clone()],
        Value::Null => Vec::new(),
        other => vec![other.to_string()],
    };
    let Some(first) = segments.first() else {
        return vec![OutputPart::default()];
    };
    let head = first.lines().next().unwrap_or("").trim();
    let failed = head.starts_with("Script failed");
    let headed = head.starts_with("Script ");
    if head.starts_with("aborted by user") {
        return vec![OutputPart {
            text: String::new(),
            interrupted: true,
            ..OutputPart::default()
        }];
    }
    // 状态头之后同一段里可能还跟着正文（字符串形态时头和正文在一起）。
    let mut bodies: Vec<String> = Vec::new();
    if headed {
        let rest = first
            .split_once("Output:\n")
            .map(|(_, rest)| rest)
            .unwrap_or("");
        if !rest.trim().is_empty() {
            bodies.push(rest.to_string());
        }
        bodies.extend(segments[1..].iter().cloned());
        // 让到后台跑的脚本没有任何输出：留下那句状态，说明结果要等后面的 wait。
        if bodies.is_empty() && head.starts_with("Script running") {
            bodies.push(head.to_string());
        }
    } else {
        bodies = segments;
    }
    if bodies.is_empty() {
        return vec![OutputPart {
            is_error: failed,
            ..OutputPart::default()
        }];
    }
    let last = bodies.len() - 1;
    bodies
        .into_iter()
        .enumerate()
        .map(|(index, body)| {
            let mut part = command_result(&body)
                .or_else(|| shell_result(&body))
                .unwrap_or(OutputPart {
                    text: body,
                    ..OutputPart::default()
                });
            // 脚本失败时，出错的是最后那一段（`Script error:\n…`）。
            if failed && index == last {
                part.is_error = true;
            }
            part
        })
        .collect()
}

/// 一段正文是不是命令结果的 JSON；是则取出输出与退出码。`apply_patch` 成功时是 `{}`。
/// `Promise.allSettled` 包过的（`{"status":"fulfilled","value":{…}}`）拆一层。
fn command_result(body: &str) -> Option<OutputPart> {
    let trimmed = body.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let value = match value.get("status").and_then(Value::as_str) {
        Some("fulfilled") => value.get("value")?.clone(),
        Some("rejected") => {
            return Some(OutputPart {
                text: value
                    .get("reason")
                    .map(|reason| {
                        reason
                            .as_str()
                            .map_or_else(|| reason.to_string(), str::to_string)
                    })
                    .unwrap_or_default(),
                is_error: true,
                ..OutputPart::default()
            })
        }
        _ => value,
    };
    let object = value.as_object()?;
    if object.is_empty() {
        return Some(OutputPart::default());
    }
    let output = object.get("output")?.as_str()?;
    let exit_code = object
        .get("exit_code")
        .and_then(Value::as_i64)
        .map(|code| code.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    Some(OutputPart {
        text: output.to_string(),
        exit_code,
        is_error: exit_code.is_some_and(|code| code != 0),
        interrupted: false,
    })
}

/// 老版本 `shell_command` 的返回值：一个带文本头的字符串
/// `Exit code: 0\nWall time: 0.4 seconds\nOutput:\n<输出>`。非零退出时工具直接抛错，脚本以
/// `Script failed` 收场，那一段多一行前缀 `Script error:\n`。本机 0.148 之前的 68 份 rollout
/// 里这种段 1623 条（退出码 0 的 1360 条、非零 263 条，2026-11-14）。
/// 只认这三行原样打头的：脚本自己拼的 `RESULT 1\nExit code: …` 之类不碰。
fn shell_result(body: &str) -> Option<OutputPart> {
    let rest = body.strip_prefix("Script error:\n").unwrap_or(body);
    let rest = rest.strip_prefix("Exit code: ")?;
    let (code, rest) = rest.split_once('\n')?;
    let exit_code = code.trim().parse::<i32>().ok()?;
    let (_, rest) = rest.strip_prefix("Wall time: ")?.split_once('\n')?;
    let text = rest
        .strip_prefix("Output:\n")
        .or_else(|| (rest == "Output:").then_some(""))?;
    Some(OutputPart {
        text: text.to_string(),
        exit_code: Some(exit_code),
        is_error: exit_code != 0,
        interrupted: false,
    })
}

/// 解析 codex 的补丁文本（`*** Begin Patch` … `*** End Patch`）成逐文件的改动。
/// 这种格式的 `@@` 不带行号（只带可选的定位上下文），所以各段起始行号记 0；
/// 新增文件整份都是 `+` 行，从第 1 行起。
pub(super) fn parse_patch(text: &str) -> Vec<PatchFile> {
    let mut files: Vec<PatchFile> = Vec::new();
    let mut kept = 0usize;
    for line in text.lines() {
        let header = |prefix: &str| line.strip_prefix(prefix).map(|path| path.trim().to_string());
        if let Some((op, path)) = header("*** Add File: ")
            .map(|path| ("add", path))
            .or_else(|| header("*** Update File: ").map(|path| ("update", path)))
            .or_else(|| header("*** Delete File: ").map(|path| ("delete", path)))
        {
            files.push(PatchFile {
                path,
                op: op.to_string(),
                move_to: None,
                hunks: if op == "add" {
                    vec![DiffHunk {
                        old_start: 0,
                        new_start: 1,
                        lines: Vec::new(),
                    }]
                } else {
                    Vec::new()
                },
                added: 0,
                removed: 0,
                truncated: false,
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(path) = header("*** Move to: ") {
            file.move_to = Some(path);
            continue;
        }
        if line.starts_with("*** ") {
            // `*** End Patch` / `*** End of File`。
            continue;
        }
        if line.starts_with("@@") {
            file.hunks.push(DiffHunk {
                old_start: 0,
                new_start: 0,
                lines: Vec::new(),
            });
            continue;
        }
        let mark = line.chars().next();
        match mark {
            Some('+') => file.added += 1,
            Some('-') => file.removed += 1,
            Some(' ') => {}
            _ => continue,
        }
        if file.hunks.is_empty() {
            file.hunks.push(DiffHunk {
                old_start: 0,
                new_start: 0,
                lines: Vec::new(),
            });
        }
        if kept >= PATCH_MAX_LINES {
            file.truncated = true;
            continue;
        }
        kept += 1;
        if let Some(hunk) = file.hunks.last_mut() {
            hunk.lines
                .push(line.chars().take(PATCH_MAX_LINE_CHARS).collect());
        }
    }
    for file in &mut files {
        file.hunks.retain(|hunk| !hunk.lines.is_empty());
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_argument_with_result_variable() {
        // 本机实拍（0.167.2）：实参是标准 JSON，结果先存变量再输出。
        let src = "const r = await tools.exec_command({\"cmd\":\"git status --short\",\"workdir\":\"C:\\\\repo\",\"yield_time_ms\":10000});\ntext(r.output);\n";
        assert_eq!(
            script_calls(src),
            vec![ScriptCall {
                name: "exec_command".into(),
                arg: json!({"cmd": "git status --short", "workdir": "C:\\repo", "yield_time_ms": 10000}),
            }]
        );
    }

    #[test]
    fn object_literal_with_bare_keys_and_several_calls() {
        // 本机实拍（0.162.1）：键不带引号，一段脚本连调几个工具。
        let src = "text(await tools.exec_command({cmd:\"rg -n 'tools.x(' src\",max_output_tokens:3000}));\ntext(await tools.web__run({open:[{ref_id:\"https://example.com/a\"}],search_query:[{q:'site:example.com hooks'}],response_length:\"short\"}));";
        let calls = script_calls(src);
        assert_eq!(calls.len(), 2, "字符串里的 tools.x( 不是调用");
        assert_eq!(calls[0].name, "exec_command");
        assert_eq!(calls[0].arg["cmd"], "rg -n 'tools.x(' src");
        assert_eq!(calls[1].name, "web__run");
        assert_eq!(calls[1].arg["open"][0]["ref_id"], "https://example.com/a");
        assert_eq!(calls[1].arg["search_query"][0]["q"], "site:example.com hooks");
    }

    #[test]
    fn patch_passed_through_a_variable() {
        let src = "const patch = \"*** Begin Patch\\n*** Add File: a.txt\\n+hi\\n*** End Patch\";\nconst r = await tools.apply_patch(patch);\ntext(typeof r === \"string\" ? r : JSON.stringify(r));\n";
        let calls = script_calls(src);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "apply_patch");
        assert_eq!(
            calls[0].arg,
            json!("*** Begin Patch\n*** Add File: a.txt\n+hi\n*** End Patch")
        );
    }

    #[test]
    fn unknown_argument_keeps_the_call_with_null() {
        let calls = script_calls("text(await tools.exec_command(buildArgs()));");
        assert_eq!(
            calls,
            vec![ScriptCall {
                name: "exec_command".into(),
                arg: Value::Null
            }]
        );
        // `tools` 的普通属性访问、别的对象上的同名方法都不是工具调用。
        assert!(script_calls("const names = Object.keys(tools); mytools.exec_command({});").is_empty());
    }

    #[test]
    fn output_parts_split_by_text_call_and_carry_exit_code() {
        let output = json!([
            {"type": "input_text", "text": "Script completed\nWall time 5.2 seconds\nOutput:\n"},
            {"type": "input_text", "text": "{\"chunk_id\":\"aad648\",\"wall_time_seconds\":1.8,\"exit_code\":1,\"original_token_count\":19,\"output\":\"error: no such file\\n\"}"},
            {"type": "input_text", "text": "Model Context Protocol | Docs"},
        ]);
        assert_eq!(
            output_parts(&output),
            vec![
                OutputPart {
                    text: "error: no such file\n".into(),
                    exit_code: Some(1),
                    is_error: true,
                    interrupted: false,
                },
                OutputPart {
                    text: "Model Context Protocol | Docs".into(),
                    ..OutputPart::default()
                },
            ]
        );
    }

    /// 老版本 `shell_command` 的回执（本机 0.14x rollout 实拍，输出截短）：成功时是带文本头的
    /// 字符串；非零退出时工具抛错，脚本以 `Script failed` 收场。两种都要取出退出码、去掉文本头。
    #[test]
    fn shell_command_text_header_yields_exit_code() {
        let ok = json!([
            {"type": "input_text", "text": "Script completed\nWall time 0.6 seconds\nOutput:\n"},
            {"type": "input_text", "text": "Exit code: 0\nWall time: 0.4 seconds\nOutput:\n M .gitignore\n M Cargo.lock\n"},
        ]);
        assert_eq!(
            output_parts(&ok),
            vec![OutputPart {
                text: " M .gitignore\n M Cargo.lock\n".into(),
                exit_code: Some(0),
                is_error: false,
                interrupted: false,
            }]
        );
        let failed = json!([
            {"type": "input_text", "text": "Script failed\nWall time 0.5 seconds\nOutput:\n"},
            {"type": "input_text", "text": "Script error:\nExit code: 1\nWall time: 0.3 seconds\nOutput:\nParserError: \r\nLine |\r\n"},
        ]);
        assert_eq!(
            output_parts(&failed),
            vec![OutputPart {
                text: "ParserError: \r\nLine |\r\n".into(),
                exit_code: Some(1),
                is_error: true,
                interrupted: false,
            }]
        );
        // 没有输出：文本头到 `Output:\n` 为止。
        let empty = json!([
            {"type": "input_text", "text": "Script completed\nWall time 0.6 seconds\nOutput:\n"},
            {"type": "input_text", "text": "Exit code: 1\nWall time: 0.4 seconds\nOutput:\n"},
        ]);
        assert_eq!(output_parts(&empty)[0].exit_code, Some(1));
        assert!(output_parts(&empty)[0].is_error);
        // 脚本自己拼的输出（实拍：`RESULT 1\nExit code: 0…`）不是这个文本头，原样保留、不取退出码；
        // 别的脚本错误（实拍：沙箱启动失败）也不是。
        for text in [
            "RESULT 1\nExit code: 0\nWall time: 6.3 seconds\nOutput:\n\nrunning 9 tests\n",
            "Script error:\nexecution error: Io(Custom { kind: Other, error: \"windows sandbox: orchestrator_helper_launch_failed\" })",
            "Exit code: abc\nWall time: 1 seconds\nOutput:\n",
        ] {
            let parts = output_parts(&json!([
                {"type": "input_text", "text": "Script completed\nWall time 0.6 seconds\nOutput:\n"},
                {"type": "input_text", "text": text},
            ]));
            assert_eq!(parts[0].text, text);
            assert_eq!(parts[0].exit_code, None);
        }
    }

    #[test]
    fn output_parts_cover_failure_background_abort_and_empty() {
        let failed = json!([
            {"type": "input_text", "text": "Script failed\nWall time 0.0 seconds\nOutput:\n"},
            {"type": "input_text", "text": "Script error:\napply_patch verification failed: invalid patch"},
        ]);
        let parts = output_parts(&failed);
        assert_eq!(parts.len(), 1);
        assert!(parts[0].is_error);
        assert!(parts[0].text.starts_with("Script error:"));

        let background = json!("Script running with cell ID g9:1\nWall time 0.0 seconds\nOutput:\n");
        assert_eq!(
            output_parts(&background),
            vec![OutputPart {
                text: "Script running with cell ID g9:1".into(),
                ..OutputPart::default()
            }]
        );

        let aborted = json!("aborted by user after 20.9s");
        let parts = output_parts(&aborted);
        assert!(parts[0].interrupted && !parts[0].is_error && parts[0].text.is_empty());

        // apply_patch 成功只回一个 `{}`：正文为空，不是失败。
        let patched = json!([
            {"type": "input_text", "text": "Script completed\nWall time 0.1 seconds\nOutput:\n"},
            {"type": "input_text", "text": "{}"},
        ]);
        assert_eq!(output_parts(&patched), vec![OutputPart::default()]);
    }

    #[test]
    fn parse_patch_reports_files_ops_and_line_counts() {
        let patch = "*** Begin Patch\n*** Add File: docs/new.md\n+# 标题\n+正文\n*** Update File: src/a.rs\n*** Move to: src/b.rs\n@@ fn main() {\n context\n-old\n+new\n+more\n@@\n-gone\n*** Delete File: src/old.rs\n*** End Patch";
        let files = parse_patch(patch);
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].op.as_str(), files[0].added, files[0].removed), ("add", 2, 0));
        assert_eq!(files[0].hunks[0].new_start, 1);
        assert_eq!(files[0].hunks[0].lines, vec!["+# 标题", "+正文"]);
        assert_eq!(files[1].path, "src/a.rs");
        assert_eq!(files[1].move_to.as_deref(), Some("src/b.rs"));
        assert_eq!((files[1].added, files[1].removed), (2, 2));
        assert_eq!(files[1].hunks.len(), 2);
        assert_eq!(files[1].hunks[0].lines, vec![" context", "-old", "+new", "+more"]);
        assert_eq!((files[1].hunks[0].old_start, files[1].hunks[0].new_start), (0, 0));
        assert_eq!(files[2].op, "delete");
        assert!(files[2].hunks.is_empty());
    }
}
