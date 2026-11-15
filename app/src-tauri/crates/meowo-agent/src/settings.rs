//! 会话设置（模型 / 模式）的统一说法与切换机制声明。
//!
//! 起因（2026-11-14 leader 手机实拍「启动被卡住」）：四家原生 TUI 切模型原先是往终端敲 `/model`
//! 再识别弹出的菜单，CLI 一改画面或会话没起来就卡死。现在清单与切换方式都由插件按各家 CLI 自己的
//! 机制声明（配置文件、自带服务、启动参数），宿主按机制分发，不按 provider 身份分支。
//! 调研与实测证据：`.lead/20261114-tui-model-switch/research-*.md`。

use serde::Serialize;

/// 某类操作要不要问用户（统一说法的一个维度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum Ask {
    /// 每次都问。
    Always,
    /// 常规的自动做，危险或越界的才问（Kimi Ask When Needed、Codex 工作区内自动越界才问）。
    Risky,
    /// 不问，直接做。
    Never,
    /// 这一档根本不允许（只规划档改文件、Gemini plan 没有 shell）。
    Blocked,
    /// 不由这一栏决定，跟随另一栏的权限档（Kimi plan 只硬禁写文件、命令按权限档走，
    /// research-kimi-gemini §5；Codex 协作模式同理，research-codex-opencode §9）。
    Inherit,
}

/// 一档模式的统一说法：改文件 / 跑命令要不要问、是否只规划。各家原名不同、语义有错位
/// （Kimi `--yolo` 是「危险仍问」，Gemini `--yolo` 才是「都不问」），界面一律按这三维说。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ModeTraits {
    pub edits: Ask,
    pub commands: Ask,
    pub plan_only: bool,
}

impl ModeTraits {
    pub const fn new(edits: Ask, commands: Ask, plan_only: bool) -> Self {
        Self {
            edits,
            commands,
            plan_only,
        }
    }
}

/// 会话里切某一栏启动选项（模式）的机制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum ModeSwitch {
    /// 只能在新建会话时选（会话里没有确定的切换方式）。
    #[default]
    LaunchOnly,
    /// 会话里用同名维度的 [`crate::ModeControl`] 直达输入（Kimi `/plan on|off`、OpenCode Tab）。
    Control,
    /// 空闲时带参数重启接回同一会话（Kimi `-r <id> --yolo`、Gemini `--resume <id> --approval-mode`）。
    Restart,
    /// 经会话自带服务改设置（Codex app-server `thread/settings/update`），不碰终端。
    Server,
    /// 经会话服务打开 TUI 自己的选择对话框再输入名字回车（OpenCode `/tui/publish agent.list`，oc5/oc6
    /// 实测）；没有服务时退回同名维度 [`crate::ModeControl`] 的循环键。
    Dialog,
}

/// 会话里切模型的机制（插件级声明，宿主按它分发）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSwitch {
    /// 会话自带服务改设置（Codex `thread/settings/update {model, effort}`，下一回合生效，
    /// 草稿与待批审批都不受影响，research-codex-opencode §2/§4）。
    ServerSettings,
    /// 带 `<flag> <id>` 重启接回同一会话（Kimi `-r <id> -m <alias>`、Gemini `--resume <uuid> -m <id>`，
    /// research-kimi-gemini §2.3/§3.3 实测会话 id 与历史都保留、不改用户配置）。
    /// `unsent_resume_args`：会话还没发过消息时 CLI 不留会话文件、resume 会报错，改用这组参数接上
    /// 同一个 id（Gemini `--session-id <uuid>`，§3.3 E3-D）。
    RestartResume {
        flag: &'static str,
        unsent_resume_args: Option<&'static [&'static str]>,
    },
    /// 服务端打开 TUI 自己的模型对话框，再输入模型名回车（OpenCode `/tui/open-models`；上游没有
    /// 「设当前模型」的接口，§2 oc4 实测）。仍会写 PTY，必须走与 Lead 送达同一套守卫。
    TuiDialog,
}

/// 前端看到的切模型机制（[`ModelSwitch`] 去掉参数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum ModelSwitchKind {
    ServerSettings,
    RestartResume,
    TuiDialog,
}

/// 会话里切思考档（effort）的机制。前端按它决定思考档入口怎么切，不按 Provider 名字判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum EffortSwitchKind {
    /// 往终端打一条带档位的命令（插件声明了 [`crate::chat_ui::EffortControl`]，Claude / Kimi 的 `/effort <档>`）。
    Command,
    /// 随模型一起经会话服务下发（Codex `thread/settings/update {model, effort}`），不碰终端。
    /// 档位随模型变，由模型目录给（[`crate::ModelCatalogEntry::efforts`]）。
    ServerSettings,
}

impl ModelSwitch {
    /// 这条切模型的通路能不能同时带上思考档。只有会话服务的设置接口收 effort；
    /// 带参重启与 TUI 对话框两条路上游都没有对应参数。
    pub fn carries_effort(self) -> bool {
        matches!(self, Self::ServerSettings)
    }

    pub fn kind(self) -> ModelSwitchKind {
        match self {
            Self::ServerSettings => ModelSwitchKind::ServerSettings,
            Self::RestartResume { .. } => ModelSwitchKind::RestartResume,
            Self::TuiDialog => ModelSwitchKind::TuiDialog,
        }
    }
}

/// 新建会话页的默认值：CLI 自己配置里此刻的默认模型与各栏默认档（打开新建页时现读）。
/// `choices` 的值是启动选项里的 choice id；读不到的栏不出现，前端退回插件声明的 default。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct LaunchDefaults {
    pub model: Option<String>,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    pub choices: std::collections::BTreeMap<String, String>,
}

/// 运行时才知道的模式选项（OpenCode 用户自定义的 primary agent，来自 `opencode agent list`）。
/// 没有统一说法（权限规则由用户写），只给名字。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct DynamicModeChoice {
    pub option: String,
    pub id: String,
    pub label: String,
}

/// 模型 id 能不能进启动参数：**必须精确等于清单里的某个 id**——清单来自 CLI 配置或自带命令，这就是
/// 防注入的白名单；argv 逐参数传给 CreateProcessW / execve，不经 shell。字符上只再拦三样：控制字符
/// （会变成按键、截断参数）、`-` 开头（会被当成另一个参数）、双引号（Windows 上有的 CLI 是 `.cmd`
/// 包装，cmd 会拆坏引号，见 registry 的 McpAttach 注释）。不限 ASCII：leader 本机 Kimi 配置里就有
/// `反向推理/DeepSeek-V4.1-Flash-1017` 这样的别名（做空审查 H3）。
pub fn model_id_allowed(id: &str, catalog: &[crate::ModelCatalogEntry]) -> bool {
    let shape_ok = !id.trim().is_empty()
        && id.chars().count() <= 200
        && !id.starts_with('-')
        && !id.chars().any(|c| c.is_control() || c == '"');
    shape_ok && catalog.iter().any(|entry| entry.id == id)
}

/// 去掉 JSONC 的 `//` 与 `/* */` 注释（字符串里的不动），供读 opencode.jsonc 这类配置。
pub fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = '\0';
                for next in chars.by_ref() {
                    if prev == '*' && next == '/' {
                        break;
                    }
                    prev = next;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> crate::ModelCatalogEntry {
        crate::ModelCatalogEntry::new(id, id)
    }

    #[test]
    fn model_ids_must_come_from_the_catalog_and_look_like_ids() {
        let catalog = [entry("kimi-code/k3"), entry("gemini-2.5-pro"), entry("-rf")];
        assert!(model_id_allowed("kimi-code/k3", &catalog));
        assert!(model_id_allowed("gemini-2.5-pro", &catalog));
        assert!(!model_id_allowed("gpt-x", &catalog), "不在清单里");
        assert!(!model_id_allowed("-rf", &catalog), "像参数的不收");
        assert!(!model_id_allowed("a\"b", &[entry("a\"b")]));
        assert!(!model_id_allowed("a\rb", &[entry("a\rb")]));
        assert!(!model_id_allowed("a\u{1b}[A", &[entry("a\u{1b}[A")]));
    }

    /// 做空审查 H3：leader 本机 ~/.kimi-code/config.toml 里的两个自定义 provider 别名（第 175、182 行），
    /// 非 ASCII、清单里有，必须能用；空格也只是普通字符（argv 逐参数传）。
    #[test]
    fn non_ascii_aliases_from_the_users_config_are_allowed() {
        let catalog = [
            entry("反向推理/DeepSeek-V4.1-Flash-1017"),
            entry("反向推理/Qwen3.8-Flash-Next"),
            entry("lmstudio/qwen 3 8b"),
        ];
        assert!(model_id_allowed("反向推理/DeepSeek-V4.1-Flash-1017", &catalog));
        assert!(model_id_allowed("反向推理/Qwen3.8-Flash-Next", &catalog));
        assert!(model_id_allowed("lmstudio/qwen 3 8b", &catalog));
        assert!(!model_id_allowed("反向推理/别的", &catalog), "仍须在清单里");
    }

    #[test]
    fn jsonc_comments_are_removed_but_strings_kept() {
        let text = "{\n // 注释\n \"model\": \"a//b\", /* 块 */ \"x\": 1\n}";
        let value: serde_json::Value = serde_json::from_str(&strip_json_comments(text)).unwrap();
        assert_eq!(value["model"], "a//b");
        assert_eq!(value["x"], 1);
    }
}
