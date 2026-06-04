use cc_store::{TodoInput, TodoStatus};
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
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
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

    /// 取 Bash 工具的 command 字段（用于「当前动作」显示）。
    pub fn bash_command(&self) -> Option<String> {
        self.tool_input
            .as_ref()?
            .get("command")?
            .as_str()
            .map(|s| s.to_string())
    }
}
