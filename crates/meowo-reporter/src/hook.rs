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
    #[serde(default, alias = "assistant_message")]
    pub last_assistant_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTodo {
    content: String,
    #[serde(default)]
    status: String,
}

impl HookEvent {
    pub fn parse(s: &str) -> Result<HookEvent, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// 从 tool_input.todos 提取 TodoInput 列表（非 TodoWrite 或无 todos 时返回空）。
    pub fn todo_items(&self) -> Vec<TodoInput> {
        let Some(input) = &self.tool_input else { return Vec::new() };
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
