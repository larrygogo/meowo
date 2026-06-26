//! Agent（CLI 提供方）抽象。把散落在 dispatch/proc/cc-app 的 provider 专属逻辑收成一处，
//! 加新 CLI 只需新增一个 `impl Agent` 并在 `ALL` 注册。cc-app 也依赖本 crate，故两个 Rust crate
//! 共用同一套进程名/resume 定义（单一事实源），消除「一处精确一处子串」的口径漂移。

use crate::hook::HookEvent;

/// Stop 时要落库的输出：最近一条 AI 正文 + 模型展示名（kimi 走 transcript，一次读出两者）。
#[derive(Debug, Default, PartialEq)]
pub struct StopOutputs {
    pub last_ai: Option<String>,
    pub model: Option<String>,
}

pub trait Agent: Sync {
    /// provider key（与 DB sessions.provider / 前端一致）。
    fn key(&self) -> &'static str;
    /// 会话本体的进程名白名单（basename，小写）。owner_pid 上溯 + cc-app 判活共用。
    fn process_names(&self) -> &'static [&'static str];
    /// Stop 时取最近 AI 正文 + 模型（claude 用 hook 携带的，kimi 读 wire.jsonl 一次出两者）。
    fn stop_outputs(&self, ev: &HookEvent) -> StopOutputs;
    /// 是否由 transcript 解析标题（claude 是；kimi 否，靠首条 prompt 命名）。
    fn resolves_transcript_title(&self) -> bool;
    /// 恢复断开会话的命令 argv（[可执行名, 参数...]）。如 ["claude","--resume",id] / ["kimi","-r",id]。
    fn resume_args(&self, session_id: &str) -> Vec<String>;
}

struct ClaudeAgent;
impl Agent for ClaudeAgent {
    fn key(&self) -> &'static str {
        "claude"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["claude", "claude.exe"]
    }
    fn stop_outputs(&self, ev: &HookEvent) -> StopOutputs {
        // Claude 的 Stop hook 直接带 AI 正文；模型走 statusline（不在此处）。
        StopOutputs { last_ai: ev.last_assistant_message.clone(), model: None }
    }
    fn resolves_transcript_title(&self) -> bool {
        true
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["claude".into(), "--resume".into(), session_id.into()]
    }
}

struct KimiAgent;
impl Agent for KimiAgent {
    fn key(&self) -> &'static str {
        "kimi"
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["kimi", "kimi.exe"]
    }
    fn stop_outputs(&self, ev: &HookEvent) -> StopOutputs {
        // kimi 的 Stop hook 不带正文/模型 → 从 wire.jsonl 一次读出两者（避免双读）。
        match crate::kimi::read_summary(&ev.session_id) {
            Some(s) => StopOutputs { last_ai: s.last_ai, model: s.model },
            None => StopOutputs::default(),
        }
    }
    fn resolves_transcript_title(&self) -> bool {
        false
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["kimi".into(), "-r".into(), session_id.into()]
    }
}

static CLAUDE: ClaudeAgent = ClaudeAgent;
static KIMI: KimiAgent = KimiAgent;
static ALL: &[&dyn Agent] = &[&CLAUDE, &KIMI];

/// 按 provider key 取 agent；未知/缺省回退 claude。
pub fn for_provider(key: &str) -> &'static dyn Agent {
    match key {
        "kimi" => &KIMI,
        _ => &CLAUDE,
    }
}

/// 所有已知 agent（供 cc-app 收集全部进程名判活）。
pub fn all() -> &'static [&'static dyn Agent] {
    ALL
}

/// 进程名（可含路径、大小写不敏感）是否属于任一已知 agent 本体——取 basename **精确**比对。
/// owner_pid 上溯与 cc-app 判活/清理共用此函数，杜绝子串误匹配（如名字恰好含 kimi 的无关进程）。
pub fn is_agent_process(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    ALL.iter().any(|a| a.process_names().contains(&base.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_provider_falls_back_to_claude() {
        assert_eq!(for_provider("kimi").key(), "kimi");
        assert_eq!(for_provider("claude").key(), "claude");
        assert_eq!(for_provider("unknown").key(), "claude");
        assert_eq!(for_provider("").key(), "claude");
    }

    #[test]
    fn is_agent_process_exact_basename_not_substring() {
        // 精确命中（含路径、大小写）。
        assert!(is_agent_process("claude.exe"));
        assert!(is_agent_process("kimi.exe"));
        assert!(is_agent_process("C:/x/Kimi.EXE"));
        assert!(is_agent_process("/usr/bin/claude"));
        // 子串不应误匹配（这正是修复点）。
        assert!(!is_agent_process("kimi-desktop"));
        assert!(!is_agent_process("kimichat.exe"));
        assert!(!is_agent_process("claude-helper.exe"));
        assert!(!is_agent_process("node"));
        assert!(!is_agent_process(""));
    }

    #[test]
    fn resume_args_per_provider() {
        assert_eq!(for_provider("claude").resume_args("ID"), vec!["claude", "--resume", "ID"]);
        assert_eq!(for_provider("kimi").resume_args("ID"), vec!["kimi", "-r", "ID"]);
    }
}
