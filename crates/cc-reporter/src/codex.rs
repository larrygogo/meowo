//! codex（OpenAI Codex CLI）会话解析。codex 的 hooks 与 claude 同款：Stop hook 直带
//! `last_assistant_message`（故最近 AI 正文走 hook payload，不在此读），标题靠首条 prompt 命名
//! （rollout 首条 user 文本被 AGENTS.md/指令包裹，不适合当标题）。唯一需从会话文件补的是【模型】
//! ——Stop hook 不携带模型，需读 rollout 的 `turn_context.model`。
//!
//! rollout：`{CODEX_HOME 或 ~/.codex}/sessions/<YYYY>/<MM>/<DD>/rollout-<ISO>-<session_uuid>.jsonl`，
//! 每行一个事件 `{type, payload}`。首行 `type=session_meta`；其后 `type=turn_context` 的
//! `payload.model` 即模型（如 "gpt-5.5"），通常在文件靠前（首回合）。

use std::path::{Path, PathBuf};

/// codex 数据根：`CODEX_HOME` 优先，否则 `~/.codex`。
pub fn codex_home() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("CODEX_HOME") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(PathBuf::from(home).join(".codex"))
}

/// codex 的启动前缀 argv（不含 `resume <id>`）。优先用户【实际在用的】 bun 全局 codex.exe
/// (`~/.bun/bin/codex.exe`)——用户多用 bun 装/更新，npm 那个常是过期副本(导致 resume 拉到旧版、
/// 每次提示更新)。其次退回 npm 的 node 包装(`node <codex.js>`)。再次官方独立安装的原生 codex.exe。
/// 都没有则 None(调用方回退裸名 codex)。
pub fn codex_launch_prefix() -> Option<Vec<String>> {
    let bin = if cfg!(windows) { "codex.exe" } else { "codex" };
    // 1) bun 全局 bin。
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let bun = PathBuf::from(&home).join(".bun").join("bin").join(bin);
        if bun.exists() {
            return Some(vec![bun.to_string_lossy().into_owned()]);
        }
    }
    // 2) npm 全局：node "<npm>/node_modules/@openai/codex/bin/codex.js"。
    if let Some(js) = codex_js() {
        return Some(vec!["node".into(), js]);
    }
    // 3) 官方独立安装(chatgpt.com/codex/install)：{CODEX_HOME}/packages/standalone/current/bin/codex(.exe)。
    //    `current` 是指向当前 release 的 junction/symlink，跨平台稳定；原生二进制可直接 resume。
    //    也是修「装完仍显示未安装」的关键：安装脚本只改持久 PATH，运行中的 cc-app 进程 PATH 是启动时旧快照、
    //    看不到新目录，故直查此固定路径——装完聚焦/刷新即显示已装，且恢复会话走绝对路径不依赖 PATH。
    if let Some(standalone) = codex_home() {
        let exe = standalone
            .join("packages")
            .join("standalone")
            .join("current")
            .join("bin")
            .join(bin);
        if exe.exists() {
            return Some(vec![exe.to_string_lossy().into_owned()]);
        }
    }
    None
}

/// codex npm 包的入口 `bin/codex.js` 绝对路径。npm 全局的 `codex` 是 shim，实为
/// `node "{npm}/node_modules/@openai/codex/bin/codex.js" <args>`；【必须】走 node 包装，直接拉原生
/// codex.exe 不会真正恢复会话(无 rollout/无 hook)。resume 用：拉起的终端 PATH 未必含 `codex`(裸名报
/// 0x80070002)，但 `node` 在系统 PATH，故用 `node <此路径>`。路径是固定相对结构(无 arch 变量)，
/// 不存在则返回 None(调用方回退裸名 codex)。
pub fn codex_js() -> Option<String> {
    for var in ["APPDATA", "USERPROFILE"] {
        if let Ok(v) = std::env::var(var) {
            let base = if var == "APPDATA" {
                PathBuf::from(v).join("npm")
            } else {
                PathBuf::from(v).join("AppData").join("Roaming").join("npm")
            };
            let js = base
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("bin")
                .join("codex.js");
            if js.exists() {
                return Some(js.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 在 `~/.codex/sessions` 下按 session_id 找 rollout 文件（文件名内嵌 uuid，以 `<uuid>.jsonl` 结尾）。
/// 递归 walk 年/月/日（限深，避免误入无关深目录）。仅作 transcript_path 缺失时的兜底。
fn find_rollout(session_id: &str) -> Option<PathBuf> {
    let sessions = codex_home()?.join("sessions");
    let suffix = format!("{session_id}.jsonl");
    walk_find(&sessions, &suffix, 6)
}

fn walk_find(dir: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            return Some(p);
        }
    }
    for d in subdirs {
        if let Some(f) = walk_find(&d, suffix, depth - 1) {
            return Some(f);
        }
    }
    None
}

/// 读文件前 max_lines 行为一个 String（模型在文件靠前，无需读全量）。
fn read_head_lines(path: &Path, max_lines: usize) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut out = String::new();
    for line in BufReader::new(f).lines().take(max_lines).map_while(Result::ok) {
        out.push_str(&line);
        out.push('\n');
    }
    Some(out)
}

/// 纯解析：从 rollout 文本取第一条 `turn_context` 的 `payload.model`。便于单测，不碰文件系统。
pub fn parse_model(content: &str) -> Option<String> {
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("turn_context") {
            if let Some(m) = v
                .get("payload")
                .and_then(|p| p.get("model"))
                .and_then(|m| m.as_str())
                .filter(|s| !s.is_empty())
            {
                return Some(m.to_string());
            }
        }
    }
    None
}

/// 取某 codex 会话的模型展示名：优先用 hook 给的 transcript_path，否则按 session_id 在 sessions 下找。
/// 读 rollout 前若干行解析 `turn_context.model`。定位/解析失败返回 None（卡片模型留空，不阻断）。
pub fn read_model(transcript_path: Option<&str>, session_id: &str) -> Option<String> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    // turn_context 在首回合、文件靠前；读前 200 行足够，避免长会话全量读。
    let head = read_head_lines(&path, 200)?;
    parse_model(&head)
}

/// 从 rollout 文本取**最后一条 info 非 null** 的 token_count 的 (input_tokens, model_context_window)。
/// codex 会话开头的 token_count `info` 为 null（只有 rate_limits），跳过。used 取 last_token_usage.input_tokens
/// （最近一次请求的 context 输入量，已含 cached_input_tokens）。
pub fn parse_context(content: &str) -> Option<(i64, i64)> {
    let mut last: Option<(i64, i64)> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = v.get("payload");
        if payload.and_then(|p| p.get("type")).and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.and_then(|p| p.get("info")).filter(|i| !i.is_null()) else {
            continue;
        };
        let used = info
            .get("last_token_usage")
            .and_then(|l| l.get("input_tokens"))
            .and_then(|x| x.as_i64());
        let window = info.get("model_context_window").and_then(|x| x.as_i64());
        if let (Some(u), Some(w)) = (used, window) {
            last = Some((u, w));
        }
    }
    last
}

/// 读文件尾部最多 max_bytes 字节为 lossy UTF-8（首个半截行交给 parse_context 跳过）。
fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(max_bytes))).ok()?;
    let mut buf = Vec::new();
    f.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// codex 会话最近上下文占用：定位 rollout（hook 的 transcript_path 优先，否则按 id 找），
/// 尾部读取最后一条 token_count。定位/解析失败返回 None。
pub fn read_context(transcript_path: Option<&str>, session_id: &str) -> Option<crate::agent::ContextUsage> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    const TAIL_BYTES: u64 = 256 * 1024;
    let text = read_tail(&path, TAIL_BYTES)?;
    let (used, window) = parse_context(&text)?;
    if window <= 0 {
        return None;
    }
    let pct = (used * 100 / window).clamp(0, 100);
    Some(crate::agent::ContextUsage { used_pct: pct, window })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_context_takes_last_nonnull_token_count() {
        let rollout = r#"
{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":7.0}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":6766,"cached_input_tokens":4480},"model_context_window":258400}}}
"#;
        // 跳过 info=null 那条；取最后一条 info 非 null 的：input_tokens=6766, window=258400。
        assert_eq!(parse_context(rollout), Some((6766, 258400)));
    }

    #[test]
    fn parse_context_none_when_no_token_count() {
        assert_eq!(parse_context(r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#), None);
    }

    #[test]
    fn parse_model_takes_first_turn_context() {
        let rollout = r#"
{"type":"session_meta","payload":{"id":"x","cwd":"/p","model_provider":"openai"}}
{"type":"turn_context","payload":{"model":"gpt-5.5","cwd":"/p","effort":"medium"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}
{"type":"turn_context","payload":{"model":"gpt-5.3-codex"}}
"#;
        assert_eq!(parse_model(rollout).as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn parse_model_none_when_absent() {
        let rollout = r#"{"type":"session_meta","payload":{"id":"x"}}
{"type":"turn_context","payload":{"cwd":"/p"}}"#;
        assert_eq!(parse_model(rollout), None);
        assert_eq!(parse_model(""), None);
    }
}
