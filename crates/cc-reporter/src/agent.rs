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
    /// 该 agent 是否把任务标题写进终端标签页标题。claude 写 → cc-app 可按标题精确切到对应 WT 标签；
    /// codex/kimi 不写（标签是默认目录名/命令名）→ 按任务标题找标签会错抓同名无关标签，cc-app 应改走
    /// 窗口级定位（按 root_pid 祖先/进程组找宿主窗口置前，不强选标签）。
    fn sets_terminal_tab_title(&self) -> bool;
    /// cc-reporter 是否应在 hook 时往本标签 ConPTY 写 session_id token（让 cc-app 能按 token 精确切到
    /// 该标签，解决同窗口同目录两会话标签同名分不清）。claude=false（自己写任务名，cc-app 按任务名匹配）；
    /// codex=false（其 spinner 持续抢标题，应走原生 tui.terminal_title 配 session_id）；kimi=true
    /// （不写标题且不抢 → 由我们补 token）。见 crate::tabtitle。
    fn writes_tab_token(&self) -> bool {
        false
    }
    /// 恢复断开会话的命令 argv（[可执行名, 参数...]）。如 ["claude","--resume",id] / ["kimi","-r",id]。
    fn resume_args(&self, session_id: &str) -> Vec<String>;
    /// 把重命名同步到该 agent 自己的持久层，使 agent 自身的会话列表/恢复(resume)列表也显示新名字：
    /// claude 往 transcript 追加 custom-title 记录；kimi 改写 session state.json 的 title+isCustomTitle。
    /// 返回是否成功落地（失败不阻断调用方更新 DB 标题）。cwd 仅 claude 用于定位 transcript。
    fn write_rename(&self, session_id: &str, cwd: Option<&str>, title: &str) -> bool;
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
    fn sets_terminal_tab_title(&self) -> bool {
        true
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        vec!["claude".into(), "--resume".into(), session_id.into()]
    }
    fn write_rename(&self, session_id: &str, cwd: Option<&str>, title: &str) -> bool {
        write_claude_custom_title(session_id, cwd, title)
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
    fn sets_terminal_tab_title(&self) -> bool {
        false
    }
    fn writes_tab_token(&self) -> bool {
        // kimi 不写标签标题、也不抢 → 由 cc-reporter 在 hook 时补 session_id token。
        true
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        // 用 kimi 可执行绝对路径（spawned 终端 PATH 未必含 kimi）。
        vec![crate::kimi::kimi_exe(), "-r".into(), session_id.into()]
    }
    fn write_rename(&self, session_id: &str, _cwd: Option<&str>, title: &str) -> bool {
        crate::kimi::set_custom_title(session_id, title)
    }
}

struct CodexAgent;
impl Agent for CodexAgent {
    fn key(&self) -> &'static str {
        "codex"
    }
    fn process_names(&self) -> &'static [&'static str] {
        // 会话本体是原生 codex 二进制；npm 包装时它由 node 启动但 hook 由 codex 自身触发，上溯命中
        // codex(.exe) 即可。不收 node.exe（过宽，会把任意 node 进程误判为 agent）。
        &["codex", "codex.exe"]
    }
    fn stop_outputs(&self, ev: &HookEvent) -> StopOutputs {
        // codex 的 Stop hook 直带 AI 正文（同 claude）；模型 Stop 不带，从 rollout 的 turn_context 补。
        StopOutputs {
            last_ai: ev.last_assistant_message.clone(),
            model: crate::codex::read_model(ev.transcript_path.as_deref(), &ev.session_id),
        }
    }
    fn resolves_transcript_title(&self) -> bool {
        // 标题靠首条 prompt 命名：rollout 首条 user 文本被 AGENTS.md/指令包裹，不适合解析。
        false
    }
    fn sets_terminal_tab_title(&self) -> bool {
        // codex 不改 WT 标签标题（标签是默认目录名，如 pwsh 的 "larry"）→ 走窗口级定位。
        false
    }
    fn writes_tab_token(&self) -> bool {
        // 实测 codex 默认不写 WT 标签标题(标签显示默认目录名 "larry"，未见 spinner 抢标题)
        // → 由 cc-reporter 在 hook 时补 session_id token，cc-app 即可精确切到 codex 标签。
        true
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        // 优先用户实际在用的 codex(bun 全局 codex.exe，常是更新后的版本)，否则 npm 的 node 包装；
        // 直接拉 npm 旧版会每次提示更新。解析失败回退裸名 codex。
        match crate::codex::codex_launch_prefix() {
            Some(mut argv) => {
                argv.push("resume".into());
                argv.push(session_id.into());
                argv
            }
            None => vec!["codex".into(), "resume".into(), session_id.into()],
        }
    }
    fn write_rename(&self, _session_id: &str, _cwd: Option<&str>, _title: &str) -> bool {
        // codex 自定义标题走 app-server JSON-RPC（成本高），暂不回写；重命名仅改 cc-kanban DB。
        false
    }
}

/// claude：往会话 transcript 追加一条 custom-title 记录（与 Claude Code `/rename` 写入格式一致），
/// 使 `claude --resume` 列表与贴纸都显示新名。定位失败/打开失败/写失败返回 false。
/// session_id 已由命令层校验为安全形态（无路径分隔符/穿越），此处直接拼路径。
fn write_claude_custom_title(session_id: &str, cwd: Option<&str>, title: &str) -> bool {
    use std::io::Write;
    let Some(path) = cc_store::title::resolve_cwd(cwd, session_id)
        .and_then(|c| cc_store::title::reconstruct_transcript_path(&c, session_id))
        .filter(|p| p.exists())
        .or_else(|| cc_store::title::find_transcript_by_session(session_id))
    else {
        return false;
    };
    let record = serde_json::json!({
        "type": "custom-title",
        "customTitle": title,
        "sessionId": session_id,
    });
    let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) else {
        return false;
    };
    writeln!(f, "{record}").is_ok()
}

static CLAUDE: ClaudeAgent = ClaudeAgent;
static KIMI: KimiAgent = KimiAgent;
static CODEX: CodexAgent = CodexAgent;
static ALL: &[&dyn Agent] = &[&CLAUDE, &KIMI, &CODEX];

/// 按 provider key 取 agent；未知/缺省回退 claude。
pub fn for_provider(key: &str) -> &'static dyn Agent {
    match key {
        "kimi" => &KIMI,
        "codex" => &CODEX,
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
        assert_eq!(for_provider("codex").key(), "codex");
        assert_eq!(for_provider("claude").key(), "claude");
        assert_eq!(for_provider("unknown").key(), "claude");
        assert_eq!(for_provider("").key(), "claude");
    }

    #[test]
    fn is_agent_process_exact_basename_not_substring() {
        // 精确命中（含路径、大小写）。
        assert!(is_agent_process("claude.exe"));
        assert!(is_agent_process("kimi.exe"));
        assert!(is_agent_process("codex.exe"));
        assert!(is_agent_process("C:/x/Kimi.EXE"));
        assert!(is_agent_process("/usr/bin/claude"));
        assert!(is_agent_process("C:/x/Codex.EXE"));
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
        // codex：末两位固定 `resume <id>`；首元素是 node(走包装) 或回退裸名 codex；某元素含 "codex"。
        let codex = for_provider("codex").resume_args("ID");
        assert_eq!(codex[codex.len() - 2..], ["resume".to_string(), "ID".to_string()]);
        assert!(codex.iter().any(|a| a.to_ascii_lowercase().contains("codex")));
        // kimi 首元素是可执行(绝对路径或回退裸名)，参数固定 -r <id>。
        let kimi = for_provider("kimi").resume_args("ID");
        assert_eq!(&kimi[1..], ["-r".to_string(), "ID".to_string()]);
        assert!(kimi[0].to_ascii_lowercase().contains("kimi"));
    }
}
