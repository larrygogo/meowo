use meowo_store::{TodoInput, TodoStatus};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HookEvent {
    pub hook_event_name: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// 用户输入。Claude 为纯字符串；kimi-code 为内容块数组 `[{"type":"text","text":...}]`。
    /// 存成 Value 兼容两者（否则 kimi 的数组会让整个事件反序列化失败），取文本走 `prompt_text()`。
    #[serde(default)]
    pub prompt: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// Claude PermissionRequest 提供的“本次允许之外”的原生选项（例如写入项目/用户权限规则）。
    /// 其他 Agent 没有该字段时保持空列表。
    #[serde(default)]
    pub permission_suggestions: Vec<serde_json::Value>,
    /// 回合结束时 hook 携带的最近一条 AI 正文。各家字段名不同，靠 alias 收束到同一个字段：
    /// claude/codex 是 `last_assistant_message`，kimi 是 `assistant_message`，
    /// gemini 的 `AfterAgent` 叫 `prompt_response`。
    #[serde(default, alias = "assistant_message", alias = "prompt_response")]
    pub last_assistant_message: Option<String>,
}

/// 各家的字段名不同：claude 的 `TodoWrite` 用 `content`，kimi 的 `TodoList` 用 `title`。
/// 两者都只是「这条待办的文字」，用 alias 收进同一个字段，不必为此分叉解析。
#[derive(Debug, Deserialize)]
struct RawTodo {
    #[serde(alias = "title", alias = "subject", alias = "text")]
    content: String,
    #[serde(default)]
    status: String,
}

impl HookEvent {
    pub fn parse(s: &str) -> Result<HookEvent, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 投影成 agent 能力所需的那几个字段。能力层刻意不认识 `HookEvent` 本身——它依赖
    /// `meowo_store::TodoInput`，让插件层反向依赖 DB 层。
    pub fn agent_ctx(&self) -> meowo_agent::HookContext<'_> {
        meowo_agent::HookContext {
            session_id: &self.session_id,
            transcript_path: self.transcript_path.as_deref(),
            last_assistant_message: self.last_assistant_message.as_deref(),
        }
    }

    /// 从 tool_input.todos 提取 TodoInput 列表（非 TodoWrite 或无 todos 时返回空）。
    pub fn todo_items(&self) -> Vec<TodoInput> {
        let Some(input) = &self.tool_input else {
            return Vec::new();
        };
        let Some(arr) = input.get("todos").and_then(|v| v.as_array()) else {
            return Vec::new();
        };
        arr.iter()
            .filter_map(|v| serde_json::from_value::<RawTodo>(v.clone()).ok())
            .map(|t| TodoInput {
                content: t.content,
                status: TodoStatus::from_str(&t.status),
            })
            .collect()
    }

    /// 把用户输入规整成纯文本：Claude 的字符串原样；kimi 的内容块数组拼接各 text 块（忽略图片等非文本块）。
    pub fn prompt_text(&self) -> Option<String> {
        match self.prompt.as_ref()? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(arr) => {
                let s = arr
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
                (!s.is_empty()).then_some(s)
            }
            _ => None,
        }
    }

    /// 取 Bash 工具的 command 字段（用于「当前动作」显示）。
    pub fn bash_command(&self) -> Option<String> {
        self.tool_input
            .as_ref()?
            .get("command")?
            .as_str()
            .map(|s| s.to_string())
    }
}
