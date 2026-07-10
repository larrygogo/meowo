//! Agent（CLI 提供方）的**运行时行为**抽象：进程名、resume/launch argv、Stop 正文、上下文占用、
//! 重命名回写、安装脚本。身份与静态声明（变体/路径/hooks 规格/鉴权）住在 `meowo_agent` 插件层。
//!
//! 于是同一个 agent 目前被两张注册表描述——本表与 `meowo_agent::all()`，靠下方配对测试钉在一起。
//! 这是 Phase 2 要消除的重复：本 trait 将折进 `meowo_agent::AgentPlugin` 的能力槽，本文件随之消失。
//! 见 `docs/architecture/agent-plugin.md`。

use crate::hook::HookEvent;
use meowo_agent::AgentId;

/// Stop 时要落库的输出：最近一条 AI 正文 + 模型展示名（kimi 走 transcript，一次读出两者）。
#[derive(Debug, Default, PartialEq)]
pub struct StopOutputs {
    pub last_ai: Option<String>,
    pub model: Option<String>,
}

/// 会话上下文占用快照。kimi/codex 从会话日志读；claude 走 statusline，返回 None。
#[derive(Debug, Default, PartialEq)]
pub struct ContextUsage {
    /// 已用百分比（0–100，已 clamp）。
    pub used_pct: i64,
    /// 上下文窗口大小（token）。
    pub window: i64,
}

pub trait Agent: Sync {
    /// agent 身份（与 DB sessions.provider / 前端 provider key 同值）。取自 `meowo_agent` 注册表，
    /// 本表的每个 id 都必须在那里注册过——由下方 registry 配对测试守住。
    fn id(&self) -> AgentId;
    /// 会话本体的进程名白名单（basename，小写）。owner_pid 上溯 + meowo-app 判活共用。
    fn process_names(&self) -> &'static [&'static str];
    /// Stop 时取最近 AI 正文 + 模型（claude 用 hook 携带的，kimi 读 wire.jsonl 一次出两者）。
    fn stop_outputs(&self, ev: &HookEvent) -> StopOutputs;
    /// 从会话日志读最近一次上下文占用。claude 返回 None（走 statusline）；
    /// kimi 读 wire.jsonl 的 usage.record，codex 读 rollout 的 token_count（各自覆写）。
    fn read_context(&self, _ev: &HookEvent) -> Option<ContextUsage> {
        None
    }
    /// 是否由 transcript 解析标题（claude 是；kimi 否，靠首条 prompt 命名）。
    fn resolves_transcript_title(&self) -> bool;
    /// 该 agent 是否把任务标题写进终端标签页标题。claude 写 → meowo-app 可按标题精确切到对应 WT 标签；
    /// codex/kimi 不写（标签是默认目录名/命令名）→ 按任务标题找标签会错抓同名无关标签，meowo-app 应改走
    /// 窗口级定位（按 root_pid 祖先/进程组找宿主窗口置前，不强选标签）。
    fn sets_terminal_tab_title(&self) -> bool;
    /// meowo-reporter 是否应在 hook 时往本标签 ConPTY 写 session_id token（让 meowo-app 能按 token 精确切到
    /// 该标签，解决同窗口同目录两会话标签同名分不清）。claude=false（自己写任务名，meowo-app 按任务名匹配）；
    /// kimi=true（不写标题且不抢 → 由我们补 token，已验证可精确切标签）；codex=false（持续 SetWindowTitle
    /// 抢标题、无法绕过，写了也被盖，详见其 impl）。见 crate::tabtitle。
    fn writes_tab_token(&self) -> bool {
        false
    }
    /// 恢复断开会话的命令 argv（[可执行名, 参数...]）。如 ["claude","--resume",id] / ["kimi","-r",id]。
    fn resume_args(&self, session_id: &str) -> Vec<String>;
    /// 裸启动一个全新会话的命令 argv（[可执行名, 参数...]），不含 resume/id。
    /// 如 ["claude"] / [kimi_exe()] / codex 启动前缀。看板「新建会话」用。
    fn launch_args(&self) -> Vec<String>;
    /// 该 agent 的可执行是否装在本机（决定各处是否列出/可选它）。
    fn is_installed(&self) -> bool;
    /// 官方一句话安装命令串（None = 无一键方案）。`windows` 决定返回 PowerShell 还是 curl 版。
    /// 命令是受信硬编码串，调用方在终端里跑（Windows powershell -Command / macOS do script）。
    fn install_script(&self, windows: bool) -> Option<String>;
    /// 把重命名同步到该 agent 自己的持久层，使 agent 自身的会话列表/恢复(resume)列表也显示新名字：
    /// claude 往 transcript 追加 custom-title 记录；kimi 改写 session state.json 的 title+isCustomTitle。
    /// 返回是否成功落地（失败不阻断调用方更新 DB 标题）。cwd 仅 claude 用于定位 transcript。
    fn write_rename(&self, session_id: &str, cwd: Option<&str>, title: &str) -> bool;
    /// 该 agent 的 transcript 规格：提供「定位 + 标题解析 + 增量分析」。claude 返回 ClaudeTranscript；
    /// codex/kimi 暂无（None）——它们的标题走首条 prompt、预览/模型走 stop_outputs，不读 transcript 分析。
    fn transcript(&self) -> Option<&'static dyn meowo_store::TranscriptSpec> {
        None
    }
}

struct ClaudeAgent;
impl Agent for ClaudeAgent {
    fn id(&self) -> AgentId {
        meowo_agent::id::CLAUDE
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
        // 用 claude 可执行绝对路径（spawned 终端 PATH 未必含 claude），与 launch_args 同源。
        let mut argv = crate::claude::claude_launch_argv();
        argv.push("--resume".into());
        argv.push(session_id.into());
        argv
    }
    fn launch_args(&self) -> Vec<String> {
        // 绝对路径优先：meowo-app 拉起的终端继承 app 启动时的 PATH 快照，未必含刚装好的 claude
        // （native installer 只改持久 PATH）。裸名会让 wt/powershell 报 0x80070002。
        crate::claude::claude_launch_argv()
    }
    fn is_installed(&self) -> bool {
        // 与 launch_args 同源：杜绝「检测说已安装、启动却找不到文件」。
        crate::claude::claude_installed()
    }
    fn install_script(&self, windows: bool) -> Option<String> {
        Some(if windows {
            "irm https://claude.ai/install.ps1 | iex".into()
        } else {
            "curl -fsSL https://claude.ai/install.sh | bash".into()
        })
    }
    fn write_rename(&self, session_id: &str, cwd: Option<&str>, title: &str) -> bool {
        write_claude_custom_title(session_id, cwd, title)
    }
    fn transcript(&self) -> Option<&'static dyn meowo_store::TranscriptSpec> {
        Some(&meowo_store::CLAUDE_TRANSCRIPT)
    }
}

struct KimiAgent;
impl Agent for KimiAgent {
    fn id(&self) -> AgentId {
        meowo_agent::id::KIMI
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
    fn read_context(&self, ev: &HookEvent) -> Option<ContextUsage> {
        crate::kimi::read_context(&ev.session_id)
    }
    fn resolves_transcript_title(&self) -> bool {
        false
    }
    fn sets_terminal_tab_title(&self) -> bool {
        false
    }
    fn writes_tab_token(&self) -> bool {
        // kimi 不写标签标题、也不抢 → 由 meowo-reporter 在 hook 时补 session_id token。
        true
    }
    fn resume_args(&self, session_id: &str) -> Vec<String> {
        // 用 kimi 可执行绝对路径（spawned 终端 PATH 未必含 kimi）。
        let mut argv = crate::kimi::kimi_launch_argv();
        argv.push("-r".into());
        argv.push(session_id.into());
        argv
    }
    fn launch_args(&self) -> Vec<String> {
        // 绝对路径优先（spawned 终端 PATH 未必含 kimi），与 resume_args 同源。
        crate::kimi::kimi_launch_argv()
    }
    fn is_installed(&self) -> bool {
        let bin = if cfg!(windows) { "kimi.exe" } else { "kimi" };
        crate::kimi::kimi_installed() || exe_on_path(bin)
    }
    fn install_script(&self, windows: bool) -> Option<String> {
        // 装当前 Node 版 Kimi Code（装到 ~/.kimi-code/bin/kimi.exe，与 kimi_installed 检测一致）。
        // 注意路径里的 `/kimi-code/`——不带它的 code.kimi.com/install.ps1 装的是旧 Python `kimi-cli`
        // （落到 ~/.local/bin/kimi-cli.exe，检测不到）。
        Some(if windows {
            "irm https://code.kimi.com/kimi-code/install.ps1 | iex".into()
        } else {
            "curl -LsSf https://code.kimi.com/kimi-code/install.sh | bash".into()
        })
    }
    fn write_rename(&self, session_id: &str, _cwd: Option<&str>, title: &str) -> bool {
        crate::kimi::set_custom_title(session_id, title)
    }
}

struct CodexAgent;
impl Agent for CodexAgent {
    fn id(&self) -> AgentId {
        meowo_agent::id::CODEX
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
    fn read_context(&self, ev: &HookEvent) -> Option<ContextUsage> {
        crate::codex::read_context(ev.transcript_path.as_deref(), &ev.session_id)
    }
    fn resolves_transcript_title(&self) -> bool {
        // 标题靠首条 prompt 命名：rollout 首条 user 文本被 AGENTS.md/指令包裹，不适合解析。
        false
    }
    fn sets_terminal_tab_title(&self) -> bool {
        // codex 不写「任务标题」式标签名（meowo-app 无法按任务名匹配）→ 改由下面 writes_tab_token 补 token。
        false
    }
    fn writes_tab_token(&self) -> bool {
        // 暂关：codex 持续用 SetWindowTitle 管理标签标题(spinner+project，如 "⠹ larry")，会盖掉我们写的
        // 任何 token，且无 session_id 组件、无禁用开关可绕过(实测 0.142.3=当前最新发布版)。其源码里
        // 「tui.terminal_title=[] 关闭标题管理」只在未发布主干，已发布版 [] 反而 clear 成终端默认(路径)。
        // 故 codex 的精确切标签暂不可达，meowo-app 走窗口级兜底。待 codex 发布 [] 禁用后，置 true 即与 kimi 同。
        false
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
    fn launch_args(&self) -> Vec<String> {
        // 与 resume_args 同款可执行解析，仅去掉 `resume <id>`：进入 codex TUI 新会话。
        crate::codex::codex_launch_prefix().unwrap_or_else(|| vec!["codex".into()])
    }
    fn is_installed(&self) -> bool {
        let bin = if cfg!(windows) { "codex.exe" } else { "codex" };
        crate::codex::codex_launch_prefix().is_some() || exe_on_path(bin)
    }
    fn install_script(&self, windows: bool) -> Option<String> {
        Some(if windows {
            "irm https://chatgpt.com/codex/install.ps1 | iex".into()
        } else {
            "curl -fsSL https://chatgpt.com/codex/install.sh | sh".into()
        })
    }
    fn write_rename(&self, _session_id: &str, _cwd: Option<&str>, _title: &str) -> bool {
        // codex 自定义标题走 app-server JSON-RPC（成本高），暂不回写；重命名仅改 Meowo DB。
        false
    }
}

/// claude：往会话 transcript 追加一条 custom-title 记录（与 Claude Code `/rename` 写入格式一致），
/// 使 `claude --resume` 列表与贴纸都显示新名。定位失败/打开失败/写失败返回 false。
/// session_id 已由命令层校验为安全形态（无路径分隔符/穿越），此处直接拼路径。
fn write_claude_custom_title(session_id: &str, cwd: Option<&str>, title: &str) -> bool {
    use std::io::Write;
    let Some(path) = meowo_store::title::resolve_cwd(cwd, session_id)
        .and_then(|c| meowo_store::title::reconstruct_transcript_path(&c, session_id))
        .filter(|p| p.exists())
        .or_else(|| meowo_store::title::find_transcript_by_session(session_id))
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
    // 先缓冲成完整一行再单次 write：该 transcript 同时被运行中的 claude 进程追加，
    // writeln!(f, "{record}") 会经 Display 拆成多次小块写，与对方的追加交错时两边的行都会被撕裂成非法 JSON；
    // append 模式下单次 write 在同一文件系统上是原子追加，消除交错窗口。
    let mut line = record.to_string();
    line.push('\n');
    f.write_all(line.as_bytes()).is_ok()
}

static CLAUDE: ClaudeAgent = ClaudeAgent;
static KIMI: KimiAgent = KimiAgent;
static CODEX: CodexAgent = CodexAgent;
static ALL: &[&dyn Agent] = &[&CLAUDE, &KIMI, &CODEX];

/// 按 agent 身份取实现。入参恒来自 `meowo_agent` 注册表，故必然命中——registry 配对测试守住这点，
/// find 失败时回退 claude 兜底（不 panic：reporter 是 hook 子进程，绝不阻塞 agent）。
pub fn for_agent(id: AgentId) -> &'static dyn Agent {
    ALL.iter().copied().find(|a| a.id() == id).unwrap_or(&CLAUDE)
}

/// DB 列 / 命令行的 provider 串 → agent 实现。**未知 id 返回 `None`，绝不冒名成 claude**
/// （旧 `ProviderKey::parse` 正是这么把未知 agent 的会话按 claude 去 resume / 读 transcript 的）。
/// `None`/空串 → 默认 agent（老会话没写过 provider 列）。
pub fn resolve(provider: Option<&str>) -> Option<&'static dyn Agent> {
    meowo_agent::resolve(provider).map(|p| for_agent(p.id()))
}

/// 所有已知 agent（供 meowo-app 收集全部进程名判活）。
pub fn all() -> &'static [&'static dyn Agent] {
    ALL
}

/// 可执行 `name`（Windows 传含 .exe 的名）是否能在 PATH 各目录找到。纯查文件存在，不 spawn。
pub fn exe_on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
    })
}

/// 进程名（可含路径、大小写不敏感）是否属于任一已知 agent 本体——取 basename **精确**比对。
/// owner_pid 上溯与 meowo-app 判活/清理共用此函数，杜绝子串误匹配（如名字恰好含 kimi 的无关进程）。
pub fn is_agent_process(name: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    ALL.iter().any(|a| a.process_names().contains(&base.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_agent_returns_matching_agent() {
        for id in [meowo_agent::id::CLAUDE, meowo_agent::id::KIMI, meowo_agent::id::CODEX] {
            assert_eq!(for_agent(id).id(), id);
        }
    }

    #[test]
    fn claude_read_context_defaults_none() {
        let ev = HookEvent::parse(r#"{"hook_event_name":"Stop","session_id":"x"}"#).unwrap();
        assert!(for_agent(meowo_agent::id::CLAUDE).read_context(&ev).is_none());
    }

    /// 未知 provider 串不得被冒名成 claude——否则一个本版本尚不认识的 agent，其会话会被按 claude
    /// 去 resume（拉起错误的 CLI）、读 transcript（读错文件）。空/缺省才走默认。
    #[test]
    fn resolve_rejects_unknown_provider_instead_of_falling_back() {
        assert_eq!(resolve(Some("kimi")).map(|a| a.id()), Some(meowo_agent::id::KIMI));
        assert_eq!(resolve(None).map(|a| a.id()), Some(meowo_agent::id::CLAUDE));
        assert_eq!(resolve(Some("")).map(|a| a.id()), Some(meowo_agent::id::CLAUDE));
        assert!(resolve(Some("gemini")).is_none());
        assert!(resolve(Some("zzz")).is_none());
    }

    /// 两张注册表（本 crate 的 `Agent` 与 `meowo_agent` 的 `AgentPlugin`）逐一配对。
    /// Phase 2 会把它们合并成一张，届时本测试连同 `Agent` trait 一起消失。
    #[test]
    fn every_plugin_has_agent_and_vice_versa() {
        for p in meowo_agent::all() {
            assert!(ALL.iter().any(|a| a.id() == p.id()), "插件 {} 无对应 Agent 实现", p.id());
        }
        for a in ALL {
            assert!(meowo_agent::by_id(a.id().as_str()).is_some(), "Agent {} 未在插件注册表登记", a.id());
        }
        assert_eq!(ALL.len(), meowo_agent::all().len());
    }

    /// 默认 agent 的 id 必须与 DB schema 的 `DEFAULT 'claude'` 字面量一致。本 crate 同时依赖
    /// `meowo-agent` 与 `meowo-store`，是唯一能做这个配对断言的地方。
    #[test]
    fn default_agent_id_matches_db_default_provider() {
        assert_eq!(meowo_agent::DEFAULT_ID.as_str(), meowo_store::DEFAULT_PROVIDER);
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
        // claude 首元素是可执行(绝对路径或回退裸名)，参数固定 --resume <id>。
        // 不写死裸名：装了 claude 的机器上这里应是绝对路径——终端继承的 PATH 快照未必含它。
        let claude = for_agent(meowo_agent::id::CLAUDE).resume_args("ID");
        assert_eq!(&claude[1..], ["--resume".to_string(), "ID".to_string()]);
        assert!(claude[0].to_ascii_lowercase().contains("claude"));
        // codex：末两位固定 `resume <id>`；首元素是 node(走包装) 或回退裸名 codex；某元素含 "codex"。
        let codex = for_agent(meowo_agent::id::CODEX).resume_args("ID");
        assert_eq!(codex[codex.len() - 2..], ["resume".to_string(), "ID".to_string()]);
        assert!(codex.iter().any(|a| a.to_ascii_lowercase().contains("codex")));
        // kimi 首元素是可执行(绝对路径或回退裸名)，参数固定 -r <id>。
        let kimi = for_agent(meowo_agent::id::KIMI).resume_args("ID");
        assert_eq!(&kimi[1..], ["-r".to_string(), "ID".to_string()]);
        assert!(kimi[0].to_ascii_lowercase().contains("kimi"));
    }

    #[test]
    fn launch_args_per_provider() {
        // claude：单元素可执行(绝对路径或回退裸名)，无 resume/id。
        let claude = for_agent(meowo_agent::id::CLAUDE).launch_args();
        assert_eq!(claude.len(), 1);
        assert!(claude[0].to_ascii_lowercase().contains("claude"));
        // codex：不含 resume/id；末元素不是 "resume"；某元素含 "codex"。
        let codex = for_agent(meowo_agent::id::CODEX).launch_args();
        assert!(codex.iter().all(|a| a != "resume"));
        assert!(codex.iter().any(|a| a.to_ascii_lowercase().contains("codex")));
        // kimi：单元素可执行（绝对路径或回退裸名），无参数。
        let kimi = for_agent(meowo_agent::id::KIMI).launch_args();
        assert_eq!(kimi.len(), 1);
        assert!(kimi[0].to_ascii_lowercase().contains("kimi"));
    }

    #[test]
    fn is_installed_reflects_executable_presence() {
        // 在临时 PATH 下放一个假 claude 可执行，claude 应判已装；清空 PATH 后应判未装。
        let dir = std::env::temp_dir().join(format!("cckb-agent-inst-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(if cfg!(windows) { "claude.exe" } else { "claude" });
        std::fs::write(&exe, b"").unwrap();
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", &dir);
        assert!(exe_on_path(if cfg!(windows) { "claude.exe" } else { "claude" }));
        std::env::set_var("PATH", ""); // 空 PATH
        assert!(!exe_on_path(if cfg!(windows) { "claude.exe" } else { "claude" }));
        if let Some(p) = saved { std::env::set_var("PATH", p); }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
