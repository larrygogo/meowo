//! 对话页能力：由**安装实况**组装，而不是一张写死的表。
//!
//! 「agent 的能力由装的是什么决定」在工程上分三层，可动态化程度不同：
//!
//! 1. **真正可发现的**：CLI 版本（宿主探测 `--version`）、用户自定义斜杠命令（文件系统：
//!    claude 的 `commands/`、codex 的 `prompts/`、gemini 的 `commands/*.toml`、opencode 的
//!    `command/`，含项目级目录）。本模块负责这层——装了新命令，下一次询问就反映。
//! 2. **版本相关但查询不到的**：内置命令表、`/model` 是否接受内联参数。五家 CLI 都没有
//!    「自述能力」的接口（没有任何 `list-commands`），这份知识只能由插件整理；但**选表**由
//!    安装实况驱动——变体粒度走 `Variant`，未来真出现版本分叉时 [`ChatUiContext::version`]
//!    已经在场，插件加分支即可、不必改接口。
//! 3. **本质是 meowo 的知识**（resume 参数、接线格式、安装地址）：CLI 永远不会提供，留在插件层。
//!
//! 与 transcript 同理：本模块只**读**文件系统，不写盘、不联网、不 spawn（探测版本的子进程
//! 由宿主负责，经 [`ChatUiContext`] 传入）。

use crate::registry::ModelPreset;
use serde::Serialize;
use std::path::Path;

/// 多家 CLI 共用的启动目录信任标题。即使某个 provider 当前版本尚无此功能，继承这些足够
/// 具体的句子也不会把普通欢迎页误判为交互提示，并能自然覆盖未来新增的同类安全检查。
pub const COMMON_STARTUP_ATTENTION_MARKERS: &[&str] = &[
    // Claude Code、Gemini CLI
    "do you trust the files in this folder",
    // Codex CLI
    "do you trust the contents of this directory",
];

/// 组装对话页能力时的输入，由宿主（meowo-app）构造。
#[derive(Debug, Default, Clone, Copy)]
pub struct ChatUiContext<'a> {
    /// 会话工作目录——发现**项目级**自定义命令用。None = 只发现用户级。
    pub cwd: Option<&'a Path>,
    /// 宿主探测到的已装 CLI 版本（`--version` 首行，探测失败为 None）。
    /// 当前没有按版本分叉的表；它在场是为了真出现版本差异时插件能分支，而不必改接口。
    pub version: Option<&'a str>,
    /// Agent 自己的会话 id（不是 Meowo DB id）。有它时插件可从该会话的 runtime 元数据
    /// 发现真实能力，例如 Claude transcript 的 `skill_listing`。
    pub session_id: Option<&'a str>,
}

/// 一条斜杠命令来自哪里。前端可据此区分展示（内置/用户级/项目级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum SlashSource {
    Builtin,
    User,
    Project,
}

/// 运行中提交的消息何时被 agent 读到。GUI 的排队回执与输入框提示按它说话——
/// 一句统一的「本回合结束后处理」对边干边读的 claude 是错的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum QueueDelivery {
    /// 在下一个工具边界插进当前回合，不打断。
    NextStep,
    /// 排到本回合结束后再处理。
    TurnEnd,
}

/// 一条斜杠命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SlashCommand {
    /// 含前导 `/`；子目录命令含命名空间（`/frontend:component`）。
    pub name: String,
    /// 自定义命令从文件头取（md 的 frontmatter `description:` / toml 的顶层 `description`）；
    /// 内置命令为 None——描述文案是翻译资产，前端 i18n 按名取。
    pub description: Option<String>,
    pub source: SlashSource,
}

impl SlashCommand {
    fn builtin(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            source: SlashSource::Builtin,
        }
    }

    /// Agent 在当前会话启动时自述的命令/skill。归为 builtin 是因为它不是用户在
    /// commands 目录创建的文件；description 直接采用 Agent 自己给出的说明。
    pub(crate) fn runtime(name: String, description: Option<String>) -> Self {
        Self {
            name,
            description,
            source: SlashSource::Builtin,
        }
    }
}

/// 一次模式切换需要写入 PTY 的输入。斜杠命令要提交回车，快捷键则只写原始字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ModeInput {
    pub data: &'static str,
    pub submit: bool,
}

/// 可直接跳转到的模式值。一个选项允许由多次输入组成，例如 Kimi 回到 manual 需要同时
/// 关闭 `/yolo` 与 `/auto`，不能让前端理解 provider 私有语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ModeOption {
    pub value: &'static str,
    pub inputs: &'static [ModeInput],
}

/// 锚点项的角色。此前是裸 `&str`（"input"/"chat" 靠注释约定），升级为枚举后
/// 生成的 TS 类型自然收窄为 `"input" | "chat"`，与前端 terminalAttention 的
/// SelectorAnchor.kind 对齐由类型系统而非注释保证。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum SelectorAnchorKind {
    /// 自由输入项(如 Type something)。
    Input,
    /// 转聊天项(如 Chat about this)。
    Chat,
}

/// composer 草稿的「收起 → 提交后自动还原」能力(issue #72)。对话页往 PTY 写正文前按
/// `input` 收走用户在终端输入行里未提交的草稿,免得它拼进消息;CLI 在下一次提交后自己
/// 还原草稿。认输入框用同一份 [`ComposerSpec`] 的框体规则；判断全看**仿真后的整屏**
/// (增量字节不可靠:渲染器按字符差分重绘):
/// - 输入框空 = **不按键**。这一步不能省——键是切换语义,composer 空且已有暂存时按下 =
///   把暂存放回 composer。
/// - 非空才按;`stashed_marker` 是「当前有暂存」的常驻标记,按前有、按后消失即误放出了
///   用户手动暂存的内容,再按一次收回。
///
/// 取证见 tests/probe_draft_residual.rs。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ComposerStash {
    pub input: &'static str,
    pub stashed_marker: &'static str,
}

/// 输入框处在另一种输入模式（Gemini 的 shell 模式、vim NORMAL）：这时写进去的字不是普通消息
/// （会被当成 shell 命令 / vim 命令执行）。认出来之后先按 `exit_input` 退回普通模式，复核
/// 退出了才写正文；退不出就不写，并按 `id` 给出说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ComposerAltMode {
    /// 模式标识（`shell` / `vim_normal`），前端按它选说明文案。
    pub id: &'static str,
    /// `prompt_pattern`：匹配输入框首行（去掉框线之后）；`below_pattern`：匹配输入框底边之下的
    /// 任意一行（状态行）。二者任一命中即处在这个模式。
    pub prompt_pattern: Option<&'static str>,
    pub below_pattern: Option<&'static str>,
    pub exit_input: &'static str,
}

/// 数字选择器的锚点项:该 provider 的选择器里**固有出现**的选项文案(小写子串匹配)。
/// 识别层据此把「纯编号的连续 run」认定为选择器本体——会话正文里的普通编号列表没有
/// 这些文案。声明前必须有该 provider 的真机取证;空 = 该 agent 的纯编号菜单不做卡片化
/// (发送侧的未识别交互软拦兜底),宁可不弹卡也不把正文列表变成会打方向键的按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SelectorAnchor {
    pub marker: &'static str,
    pub kind: SelectorAnchorKind,
}

/// 命中提示后，**详情区**按哪种屏幕文法解析（工具名/命令段/问句怎么从文本里抠出来）。
/// 命名的是**屏幕格式**，不是 agent：新 agent 的审批框长得像哪种就声明哪种，
/// 都不像就 `None`——按钮照常可用，只是详情区不做结构化拆解。
/// 此前前端按 pattern id（`"kimi:command-approval"`）猜格式，是变相的身份分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum AttentionDetails {
    /// 不解析，整段命中文本原样展示。
    #[default]
    None,
    /// 「Do you want to proceed?」框式：工具头行 + 命令段 + 问句尾行（claude 真机取证）。
    /// 附带语义：「记住此决定」项是**持久**权限规则。
    ProceedBox,
    /// 「▶ 问题标题」面板式：▶ 标题行 + display 块（kimi 官方源码取证 approval-panel.ts）。
    /// 附带语义：「Approve for this session」只记本会话，不是持久规则。
    ArrowPanel,
    /// 圆角外框对话框式：`? 工具名  说明` 头行 + 内框正文 + 问句 + `│ ● 1. Allow once │`
    /// 编号选项（gemini 0.6x 真机取证 + 源码 ToolConfirmationMessage）。选项行带框线，
    /// 以 ● 焦点做相对移动；「Allow for this session」只记本会话。
    FramedBox,
    /// 方括号编号表单：`? 问题` + `[1] 选项`（单选，`→` 为光标）或 `[ ] 选项` / `[✓] 选项`
    /// （多选），数字键直接选中 / 勾选；末页是 `[1] Submit` / `[2] Cancel` 的确认页
    /// （kimi 2.4.1 AskUserQuestion 表单，2026-11-05 真机取证）。
    BracketForm,
    /// 圆角外框里的提问表单：`│ Answer Questions` 头行 + 问句 + `│ ● 1.  选项` 编号行
    /// （多选另有 `[ ]`/`[x]` 勾选框与无编号的 `Done` 行），末页 `Review your answers:`。
    /// 以 ● 焦点做相对移动，回车选中（gemini 0.65 AskUserDialog 源码取证，2026-11-12，未实拍）。
    /// 与 `FramedBox` 的区别在语义：这是提问，不是命令审批，前端出提问卡。
    FramedForm,
    /// 横排选项行：标题行之下是正文，末行是 `Allow once   Allow always   Reject` 这样用多个空格
    /// 隔开的选项，同行尾部带 `⇆ select  enter confirm` 按键说明。焦点只用底色表示，文本里
    /// 认不出，按「初始焦点在第一项」从左往右做相对移动（→ 下一项、回车选中），Esc 等于末项。
    /// opencode 1.19.2 真机屏幕 + 1.19.5 程序文件内嵌 JS 取证（权限框的 `left/right/return/escape`
    /// 绑定，2026-11-14）。
    OptionRow,
}

/// 屏幕上这条可操作提示**是什么事**。前端按它定卡片的标题、徽章和按钮形态，
/// 不再从 `id` 的后缀猜（`gemini:usage-limit` 曾因此被说成「启动步骤」、徽章写「待批准」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum AttentionKind {
    /// 工具或命令在等用户放行。
    Approval,
    /// 计划写好了，等用户批准后执行。
    Plan,
    /// Agent 向用户提问。
    Question,
    /// CLI 自己要用户选一项才能继续（额度耗尽后重试还是停下、长会话怎么恢复），
    /// 不是审批，也不是 Agent 的提问。
    Choice,
}

/// 屏幕上一个选项**起什么作用**。卡片只按它摆按钮，不看选项的文案。
///
/// 此前前端用正则认各家的英文选项文案（`Approve for this session`、`switch to auto mode`）来决定
/// 哪个是「允许一次」、哪个是「拒绝」，适配知识落在界面层；兜底还会出错：只认出
/// `Allow always / Reject` 两项时，「允许一次」发出去的是 Allow always（做空审查 2026-11-14）。
/// 现在由插件随规则声明（[`AttentionOptionRule`]），认不出的一律是 `Other`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum OptionRole {
    /// 只放行这一次。卡片上的「允许一次」只认它：请求里没有这个角色，就没有这颗按钮。
    AllowOnce,
    /// 放行并记住（范围见 [`PersistScope`]）。
    AllowPersistent,
    /// 拒绝这一次。
    Deny,
    /// 拒绝并附一句说明（说明要在终端里现场打字，流程见 [`TypedInput`]）。
    DenyWithFeedback,
    /// 提问表单里的自定义答案项（同样要现场打字）。
    CustomAnswer,
    /// 认不出作用的选项：卡片照终端原文列成普通按钮，不带数字快捷键，不冒充允许或拒绝。
    Other,
}

/// 持久放行记到哪里为止。按钮上的说法由它决定（本会话内允许 / 允许并记住 / 允许并切到自动模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub enum PersistScope {
    /// 只到这个会话（或这个 CLI 进程）结束。
    Session,
    /// 写进规则，以后的会话也生效。
    Remember,
    /// 放行并把会话切到自动模式（claude 2.1.325 的 `Yes, and switch to auto mode`）。
    AutoMode,
}

/// 要在终端里**现场打字**的选项怎么作答（「说明并拒绝」的说明、提问表单的自定义答案）。
///
/// 流程三步，每一步都不盲按：先让这一项进入等输入的状态，屏幕上认出 `ready_pattern` 才打字，
/// 打完再按 `submit_input`。认不出就停在第一步，不往终端里打字：审批框里数字键就是选项
/// （kimi 面板里 `1` 是 Approve once），没进输入状态时打进去的字可能变成放行。
///
/// **每家都要真机走通才能声明**（tests/probe_deny_feedback.rs、tests/probe_kimi_other.rs）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct TypedInput {
    /// 怎么让这一项进入等输入的状态。None = 按这一项自己的选中键就进（kimi 的数字键）；
    /// Some(key) = 先把焦点移到这一项（不回车），再按 key（claude 在「No」上按 Tab）。
    pub amend_input: Option<&'static str>,
    /// 进入等输入状态后屏幕上出现的那一行（JS 正则，前端以 `im` 编译）。认出才打字。
    pub ready_pattern: &'static str,
    /// 打完字后的提交键。
    pub submit_input: &'static str,
}

/// 一条「选项文案 → 角色」的规则。前端拿屏幕上解析出的每个选项的文字，按声明顺序逐条试，
/// 第一条命中的定角色；都不命中是 [`OptionRole::Other`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct AttentionOptionRule {
    /// JS 正则源码，对选项文字（去掉首尾空白）匹配，前端以 `i` 编译。
    /// 捕获组 1（如有）= 放行的对象（`don't ask again for: curl *` 里的 `curl *`），按钮上带出来。
    pub pattern: &'static str,
    pub role: OptionRole,
    /// 只对 `AllowPersistent` 有意义。None = 范围认不出，按钮照搬终端原文。
    pub scope: Option<PersistScope>,
    /// 这一项可以（或必须）带一句现场打的字。None = 不能，或还没真机验证过。
    /// `DenyWithFeedback`、`CustomAnswer` 没有它时卡片上不出入口（点了也完成不了）；
    /// `Deny` 带着它 = 拒绝项可以追加说明（claude 的 Tab to amend）。
    pub typed: Option<TypedInput>,
}

/// 选项行没解析出来时也成立的按键（只填真机验证过、与焦点位置无关或焦点位置确定的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct RoleKey {
    pub role: OptionRole,
    pub input: &'static str,
}

/// 屏幕上**可操作**提示的识别规格：审批框、长会话确认这类需要用户当场决定的整句提示。
///
/// 与 [`crate::screen::ScreenRule`] 的分工：那套只判**状态**（working/idle/blocked，只读）；
/// 这套要在屏幕上**定位**出一张可点选、会往 PTY 写按键的卡片，故必须给出位置。
///
/// 此前这些整句规则硬编码在前端 `terminalAttention.ts` 里、按 `provider ===` 分支，
/// 循 `selector_anchors` 的先例下放为插件声明——加 agent 只动 `plugins/`。
///
/// **声明前必须有该 agent 的真机取证**：误报会凭空弹出一张卡并锁住输入框，
/// 比不弹更糟。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct AttentionPattern {
    /// 规则标识（如 `"claude:command-approval"`），只用来去重、写日志和对上漂移检查的清单。
    /// 前端**不按它**决定卡片的标题、徽章或按钮：那些由 `kind` 与 `details` 决定。
    pub id: &'static str,
    /// JS 正则源码（不含分隔符与标志），**任一命中**即算命中该卡片。
    /// 前端统一以 `im`（+ `last` 时加 `g`）标志编译，故 `^`/`$` 按行匹配。
    /// 字面量里的正则元字符要自行转义（如 `proceed\\?`）。
    pub patterns: &'static [&'static str],
    /// 取**最后一次**匹配而非第一次。审批框必须为 true：TUI 重绘不清屏，上一屏的
    /// 审批框会残留在缓冲上方，取第一次会把用户看到的下方活动面板与上方残影搞混
    /// ——真实事故是「用户看着 A 批准了 B」。仅出现一次的提示（长会话确认）用 false。
    pub last: bool,
    /// 详情区的屏幕文法风格（见 [`AttentionDetails`]）。None = 不做结构化拆解。
    pub details: AttentionDetails,
    /// 这条提示是什么事（见 [`AttentionKind`]）。卡片的标题、徽章由它决定。
    pub kind: AttentionKind,
    /// 这一屏是另一条规则里某个选项的二次确认页时，填那条规则的 id（opencode 选 Allow always
    /// 后的「Always allow」确认页）。用户刚在 Meowo 的卡片上点了那个选项，Meowo 认出这一屏后
    /// 替他按下确认，对话页上一次点击就完成；用户自己在终端里走到这一屏的，照常出卡。
    pub confirms: Option<&'static str>,
    /// 选项文案 → 角色（见 [`AttentionOptionRule`]）。空 = 这条提示的选项不分角色，
    /// 卡片把它们原样列成普通按钮（额度耗尽后重试还是停下、长会话怎么恢复）。
    pub options: &'static [AttentionOptionRule],
    /// 选项行还没画出来、或滚出了屏幕时仍然成立的按键（见 [`RoleKey`]）。空 = 认不出选项就不猜按键，
    /// 卡上只有「仅收起」。
    pub unparsed_keys: &'static [RoleKey],
}

/// 屏幕回显标记：TUI 状态栏上代表某个模式值的稳定文案片段（如 claude 的 "plan mode on"）。
///
/// cycle 型控件是**盲切**：写入 cycle_input 后落到哪个值只有 CLI 自己知道，而 transcript
/// 只在活跃回合才记录模式——空闲时（恰是用户切权限模式的典型时机）切换在界面上毫无回显，
/// 用户只能认为按钮坏了。声明这张表后，宿主在写入后短暂轮询 PTY 屏幕，命中即更新显示。
/// 匹配为小写子串；文案取自 provider 文档承诺的指示文本，不是猜测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ModeScreenMarker {
    pub marker: &'static str,
    pub value: &'static str,
}

/// ChatWindow 中一个独立的模式维度。`cycle_input` 与 `options` 可任选其一或同时存在；
/// 只读维度无需声明 control，仍可由 transcript 状态展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ModeControl {
    pub dimension: &'static str,
    /// 菜单型模式必须等用户在 CLI 选择，不能把写入命令当成切换成功（Kimi Code 2.2 文档）。
    pub menu_command: Option<&'static str>,
    pub cycle_input: Option<&'static str>,
    pub options: &'static [ModeOption],
    /// 空表 = 该维度无屏幕回显能力，显示只随 transcript 状态走。
    pub screen_markers: &'static [ModeScreenMarker],
}

/// 思考程度（effort）的切换能力（对照 Kimi 模型菜单里的「思考 Low/High/Max」）。
/// 当前档位不在这里：它随会话、随模型变，由 statusline 上报（模型不支持时不报，前端据此不显示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct EffortControl {
    /// 带参切换的命令（claude `/effort high`，切换后屏幕回一行 `Set effort level to High…`）。
    pub command: &'static str,
    /// 可选档位（小写，即命令参数）。某些档位只有部分模型支持，选了不支持的 CLI 自己会拒，
    /// 前端照实报错，不在这里按模型猜。空表 = 档位随模型变，由模型目录给（kimi 的
    /// `support_efforts`）。
    pub levels: &'static [&'static str],
    /// 切换成功的屏幕确认行（捕获组 1 = 生效的档位）。只认命令之后的新输出。
    /// 两条正则都按**不区分大小写**匹配（前端带 i 标志编译，故不写内联 `(?i)`——JS 不认）。
    pub confirm_pattern: &'static str,
    /// CLI 拒绝切换的屏幕行（整行原样带给用户）。可带捕获组 1 = CLI 实际改成的档位：
    /// 组织/设置封顶时 claude 不拒绝而是降到上限（2.1.322 包内
    /// `Effort '${e}' exceeds the cap for ${u} …; set to '${o}' instead`），前端据此把显示值
    /// 落到实际档位，同时把原话告诉用户。
    pub reject_pattern: &'static str,
}

/// 带参切换命令的屏幕回话（`/model sonnet`）：确认行与拒绝行。
/// 两条都按**不区分大小写**匹配（同 [`EffortControl`]，不写内联 `(?i)`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct CommandReply {
    /// 切换成功的确认行。前端另外核对行内含所选模型的家族名（CLI 有降档路径，会确认成别的模型）。
    pub confirm_pattern: &'static str,
    /// CLI 拒绝或放弃切换的行（整行原样带给用户，不再只报「未确认」）。
    pub reject_pattern: &'static str,
}

/// 带参切换命令（`/model sonnet`、`/effort high`）在对话进行到一半时弹出的二次确认框。
///
/// claude 2.1.325 真机取证（tests/probe_claude_model.rs，2026-11-14）：会话里已经有过回合、提示缓存
/// 还热着时，`/model haiku` 不直接切，先弹：
/// ```text
///    Switch model?
///    Your next response will be slower and use more tokens
///    This conversation is cached for the current model. Switching to Haiku 5.5 means the full history gets re-read.
///    ❯ 1. Yes, switch to Haiku 5.5
///      2. No, go back
/// ```
/// 此前对话页不认识它：命令发出去后等不到确认行，报「未确认」，确认框留在终端里挡着输入框，
/// 之后发的消息被它吞掉（回合结束后自动执行的切换同样撞上，验收 6）。
///
/// 用户已经在对话页选定了要切到哪一个，所以认出这个框、且框里要切到的正是所选的那一个、
/// 焦点就在「Yes」那一行时，替他按下确认；对不上就不按，照旧报未确认并给看终端画面的出口。
/// 三条正则都按**不区分大小写**、逐行匹配（前端带 i 标志编译）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SwitchConfirm {
    /// 确认框的标题行（整行）。
    pub title_pattern: &'static str,
    /// 带着焦点的确认项那一行，捕获组 1 = 要切到的目标（模型显示名或思考档）。
    /// 前端核对它含所选的那一个才按键。
    pub accept_pattern: &'static str,
    /// 确认键（焦点已在确认项上时按）。
    pub accept_input: &'static str,
}

/// 撤回最近几轮对话的能力（对照 Kimi 桌面版用户消息上的「撤回」：这条及之后移出上下文，
/// 原文放回输入框）。轮数由前端按「非插话的用户消息」数——插件须保证 transcript 里插话标了
/// interjected，否则计数与 CLI 对不上。撤回是否生效以 transcript 为准（被撤的消息从对话里
/// 消失），不认屏幕。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct UndoControl {
    /// 撤回命令。
    pub command: &'static str,
    /// true = 命令带轮数参数（kimi `/undo 2`，一次撤到任意一条）；false = 一次只撤最近一轮
    /// （opencode `/undo` 不收参数），前端只给最后一条用户消息挂撤回。
    pub count_arg: bool,
    /// 撤回是否连带还原工作区文件（opencode 按快照回滚；kimi 不碰文件）。确认框据此说清后果。
    pub restores_files: bool,
    /// CLI 拒绝撤回的屏幕行（整行原样带给用户；前端带 i 标志编译）。None = 未取证。
    pub reject_pattern: Option<&'static str>,
    /// CLI 撤回后把原文放回**它自己的**输入框（opencode / claude）。原文已放进对话页输入框，
    /// 终端里那份不清，下一次发送会拼在新消息前面（真机复现过）。清它用 [`ComposerSpec`] 的
    /// `clear_inputs`：先在屏幕上认出输入框、确认里面是原文才按——清空键在空输入框上另有含义
    /// （opencode 的 Ctrl-C 空框是退出，claude 的双 Esc 空框是打开回退菜单）。按后复核。
    pub returns_input: bool,
    /// 撤回命令打开的是选择菜单（claude `/rewind`）时的操作规格；None = 命令直接生效。
    /// 菜单式撤回**不写 transcript**（claude 回退零写入，下一条消息才长出新分支），成败只能
    /// 看屏幕：原文回到 CLI 输入框即成，前端随之在本地收掉被撤的消息。
    pub menu: Option<UndoMenu>,
}

/// 菜单式撤回的操作规格（claude `/rewind`，2.1.314 真机取证，tests/probe_claude_rewind.rs）：
/// 列表从旧到新、光标停在最底的当前位置，按 `step_input` k 次落到倒数第 k 条用户消息；
/// 回车进确认页，确认页用 `quote_prefix` 引出所选消息全文，选项随有无代码改动变化，按
/// 文案找 `option_label` 那一项的数字。每一步都先核对屏幕（选中行、引文）与要撤的消息
/// 对得上才往下走，对不上按 `cancel_input` 退出。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct UndoMenu {
    /// 菜单标题行（整行，trim 后相等）：其下才是菜单区，区分历史区里同样带 `❯` 的用户消息。
    pub title: &'static str,
    /// 菜单就绪的标志行（trim 后相等）。
    pub ready_marker: &'static str,
    pub step_input: &'static str,
    /// 选中行前缀。
    pub selected_prefix: &'static str,
    pub enter_input: &'static str,
    pub quote_prefix: &'static str,
    pub option_label: &'static str,
    pub cancel_input: &'static str,
}

/// 侧问（对照 Kimi 的「侧边聊天」：基于当前会话上下文的只读问答，不进对话记录）：CLI 原生的
/// 旁路提问，`<command> <问题>` 提交后弹覆盖层作答，**答案不写 transcript**，只在屏幕上
/// （claude `/btw`，2.1.314 探针 probe_claude_rewind 实测）。前端从屏幕上把答案读下来显示在
/// 对话页，再按 `close_input` 收起覆盖层——不收的话覆盖层吞掉后续按键（`c` 复制、`f` 分叉）。
///
/// 覆盖层形态：`<command> <问题>` 一行、答案区、底部提示行。作答中提示行只有 `Esc to close`，
/// 答完变成 `↑/↓ to scroll · c to copy · f to fork · Esc to close`。答案超出一屏时答案区是
/// 滚动窗口，每按一次 `scroll_input` 下移几行，前端逐窗拼接。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SideQuestion {
    pub command: &'static str,
    /// 提示行的固定结尾（小写比较）：答案区到它的上一行为止。
    pub footer_marker: &'static str,
    /// 提示行含它（小写比较）= 已答完。
    pub ready_marker: &'static str,
    pub scroll_input: &'static str,
    pub close_input: &'static str,
}

/// 终端输入框的规格：**每家只有这一份**，认框、判空、收草稿、清原文、退出特殊输入模式全用它。
///
/// 此前有三份各认各的（草稿暂存用的只有 claude、撤回清原文用的只有 claude 与 opencode、
/// 发送前判空用的只有另外四家），每个功能只接其中一份：某家缺哪一份，对应功能就静默失效或
/// 退成「请到终端处理」（走查输入 A1、A4、B6）。现在发送前收草稿、撤回后清原文、切模型前搬
/// 草稿、未确认回显核对、子代理视图定位都读这一份；架构测试拦着不让第二份再长出来。
///
/// 认框只认真实取证过的框体；框体不完整一律按「认不出」，不猜。
///
/// 占位文字与内容相同的真实草稿怎么分（2026-11-14 五家真机取证，见各插件注释）：占位时硬件
/// 光标停在输入起点，真打出来的字光标在文末。读屏时带上光标位置即可分开；没有光标信息时
/// 退回只看文字（`empty_pattern`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ComposerSpec {
    /// Codex 用 prompt 与状态 footer 定界；有边框的 TUI 不启用这个布局。
    pub prompt_footer: bool,
    /// 捕获组 1 必须是输入起点之前的物理前缀；占位文字可能另有灰字缩进，不能反推光标。
    pub input_start_pattern: &'static str,
    pub top_pattern: &'static str,
    pub bottom_pattern: &'static str,
    /// 捕获组 1 为行内正文；必须锚定整行，不能把历史中的零散提示当成 composer。
    pub row_pattern: &'static str,
    pub prompt_prefix: &'static str,
    /// 有些 TUI 把模式与模型放在同一个框内，必须验证后再排除这些固定尾行。
    pub tail_rows: usize,
    pub tail_pattern: Option<&'static str>,
    pub empty_pattern: &'static str,
    /// 认不出输入框时对话页还发不发：false = 照发（claude 多年如此，另有提示识别、子代理视图等
    /// 守卫兜着）；true = 不发，错误条给看终端画面与「仍然发送」的出口。
    pub send_requires_recognition: bool,
    /// 收起草稿、提交后 CLI 自己还原（见 [`ComposerStash`]）。有它时发送前优先用它：草稿留在
    /// 终端里原样回来。None = 没有这种键，改用 `clear_inputs` 清空并把草稿搬进对话页输入框。
    pub stash: Option<ComposerStash>,
    /// 清空输入框的按键，逐个分开写（两个 Esc 合成一次写入会被当成 Alt+Esc）。**只在认出输入框
    /// 里有字时才按**：这些键在空输入框上另有含义（退出、打开回退菜单）。空表 = 未取证，不清。
    pub clear_inputs: &'static [&'static str],
    /// 一轮 `clear_inputs` 清不完时最多重复几轮，每轮后复核（gemini 用行删除键逐行清）。
    /// 1 = 一轮清空整段。
    pub clear_rounds: u8,
    /// 回合进行中按 `clear_inputs` 是否只清草稿、不打断回合（逐家真机取证）。false = 运行中不清，
    /// 错误条说明原因。
    pub clear_while_running: bool,
    /// 特殊输入模式（见 [`ComposerAltMode`]）。
    pub alt_modes: &'static [ComposerAltMode],
}

/// 子代理视图（claude 在提示框下方的子代理面板里「打开」某个子代理）：此时 composer 改发给
/// 那个子代理，对话页（代表主会话）写进 PTY 的话会被它收走，主会话一个字也收不到。前端
/// 发送前据此认出视图、按面板键选中主会话行并回车切回，确认回到主会话才写。
///
/// 屏幕形态（2.1.322 真机取证，tests/probe_claude_subagent_view.rs）：
/// - composer 上边框右侧挂子代理描述 `──── probe-sleeper ─`，空 composer 的占位是
///   `❯ Message @general-purpose…`（`placeholder_prefix`）；回到主会话后边框是整条横线、
///   空 composer 只剩 `❯`。
/// - composer 底边之下是子代理面板：`  ◯ main` / `❯ ● general-purpose  probe-sleeper  5s`。
///   行首 `panel_pointer` = 面板选中项，`panel_viewed_marker` = 当前在看的那一行。
///   在主会话时 main 行是 `●`，在看子代理时 main 行是 `◯`。
/// - 键：composer 里 ↓ 进面板并落在 main 行；面板里 ↑/↓ 移动；main 行上回车 = 退出视图；
///   打开子代理后面板保持焦点（指针在被看的那行）；Esc 取消选中、焦点回 composer。
///
/// 不用 Esc 直接退视图：视图里 composer 焦点下的 Esc 在被看的子代理还在跑时是「中断它的
/// 工作」，只有它停了才退出视图（2.1.322 源码），按错就把用户的子任务掐了。面板里的 Esc
/// 只取消选中（官方 sub-agents 文档「Observe and steer running forks」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentView {
    pub placeholder_prefix: &'static str,
    /// 输入框上边框挂着子代理描述时的形态，捕获组 1 = 描述（`──── probe-sleeper ─`）。输入框本身
    /// 用 [`ComposerSpec`] 认，这里只负责从上边框那一行取出描述。
    pub label_pattern: &'static str,
    pub panel_main_label: &'static str,
    pub panel_pointer: &'static str,
    pub panel_viewed_marker: &'static str,
    /// 从 composer 把焦点移进面板、在面板里下移。
    pub down_input: &'static str,
    pub up_input: &'static str,
    /// 打开选中行（选中主会话行 = 切回主会话）。
    pub open_input: &'static str,
    /// 面板有选中项时取消选中、焦点回 composer（面板没焦点时**不能**发：那是中断键）。
    pub leave_panel_input: &'static str,
}

/// 对话页能力总装的结果，原样下发前端。
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ChatUi {
    pub slash_commands: Vec<SlashCommand>,
    pub model_presets: Vec<ModelPreset>,
    /// 只有支持内联选择的 CLI 才声明；列举模型不代表能用同一协议选择模型。
    pub model_select_command: Option<&'static str>,
    /// `model_select_command` 之后 CLI 的回话（见 [`CommandReply`]）；None = 前端用通用确认文案兜底。
    pub model_select_reply: Option<CommandReply>,
    /// 带参切换命令在对话中途弹的二次确认框（见 [`SwitchConfirm`]）；None = 这家没有这种框。
    pub switch_confirm: Option<SwitchConfirm>,
    /// 打开「选模型」菜单的命令。预设为空但有它时，前端发这条命令并把 CLI 弹出的
    /// 交互菜单渲染成按钮——模型清单由 CLI 现给，不必宿主维护一份会过时的。
    pub model_menu_command: Option<&'static str>,
    /// 思考程度切换（见 [`EffortControl`]）；None = 不支持或未取证。
    pub effort: Option<EffortControl>,
    /// 思考档在会话里怎么切（见 [`crate::EffortSwitchKind`]）。None = 这家在会话里切不了，
    /// 前端不出思考档入口。与 `effort` 的区别：`effort` 只描述「打命令」这一种通路的细节，
    /// 这里是「有没有、走哪条路」的总开关，经会话服务切的（Codex）也在内。
    pub effort_switch: Option<crate::EffortSwitchKind>,
    /// 在对话页选模型会不会顺带改写用户 CLI 的默认模型（新会话也跟着变）。各家不同、界面要说
    /// （走查输入 C1）：true 时模型菜单里提醒一句。只对对话页自己走的那条通路声明。
    pub model_select_saves_default: bool,
    /// 在对话页切思考档会不会顺带改写用户 CLI 的默认配置。同上。
    pub effort_saves_default: bool,
    /// 撤回最近几轮（见 [`UndoControl`]）；None = 不支持或未取证。
    pub undo: Option<UndoControl>,
    /// 侧问（见 [`SideQuestion`]）；None = 不支持或未取证。
    pub side_question: Option<SideQuestion>,
    /// 能否分叉会话（插件声明了 [`crate::ForkMode`]）。分叉由后端 fork_session 完成，前端只管给不给入口。
    pub fork: bool,
    /// 在自己的终端里恢复本会话的命令前缀：裸可执行名 + resume 子命令（`["claude", "--resume"]`），
    /// 前端接上会话 id 供「复制恢复命令」（对照 Kimi 会话菜单的「复制 Session ID」：复制 id 几乎
    /// 都是为了到终端里接着跑，直接给命令省一步）。用裸名而非绝对路径：用户自己的终端有 PATH。
    /// 空 = 不支持恢复。
    pub resume_command: Vec<String>,
    /// Provider 声明的模式交互能力。模式的真实当前值由 transcript 增量独立提供。
    pub mode_controls: Vec<ModeControl>,
    /// 裸发送会弹出交互界面的内置命令（含 `model_menu_command`，总装时并入）。
    /// 前端对它们走菜单识别通道，而不是裸发后让用户对着空白对话页干等。
    pub menu_slash_commands: Vec<&'static str>,
    /// 框架默认值与 provider 补充的、启动/恢复期间必须由用户在终端里处理的提示文本片段。
    /// 宿主据此不能把
    /// “首屏已有输出”误当成 composer 已就绪，更不能把待发送消息写进交互式选择器。
    pub startup_attention_markers: Vec<&'static str>,
    /// 数字选择器锚点(见 [`SelectorAnchor`]),识别层的选择器文法由插件声明。
    pub selector_anchors: Vec<SelectorAnchor>,
    /// 可操作提示的识别规格(见 [`AttentionPattern`])。整句识别规则由插件声明，
    /// 前端不再按 provider 分支。
    pub attention_patterns: Vec<AttentionPattern>,
    /// 该 agent 的 TUI 在 PermissionRequest hook 阻塞期间会**并行**显示自己的权限选择框，
    /// 与 GUI 审批竞速——用户先在终端作答则 hook 结果被丢弃（claude 官方 hooks 文档明载，
    /// 实测同现）。true = 终端视图不挂「回对话页处理」横幅：授权框就在用户眼前，横幅反成
    /// 误导。false（默认）= 未取证，按「终端不显示」保守处理，横幅照挂。
    pub permission_prompt_races_hook: bool,
    /// 中断当前回合的按键序列(如 Esc)。运行中「强制插话」= 先写它停掉当前回合,
    /// CLI 随即处理排队消息/接受新输入。None = 该 agent 的中断键未经取证,GUI 不提供入口。
    pub interrupt_input: Option<&'static str>,
    /// 中断键要按几次(分次写入,间隔由前端定)。opencode 首按只提示 "esc again to interrupt",
    /// 第二下才停;两个 Esc 合成一次写入会被当成 Alt+Esc,一下都不算(真机取证见其插件)。
    pub interrupt_presses: u8,
    /// 运行中排队的提交键。Codex 的 Enter 不等于排队（官方键盘文档）；gemini 0.64 实测 Enter 即排队。
    pub queue_input: Option<&'static str>,
    /// 用 queue_input 提交的消息何时被读到。None = 未取证，GUI 只说「已交给 Agent」。
    pub queue_delivery: Option<QueueDelivery>,
    /// Ctrl/Shift+Enter 在 composer 插入换行的注入序列(如 claude 的 ESC+CR)。
    /// None = 未声明,前端保持终端原生语义(带修饰的 Enter 与裸 Enter 同码 `\r`)。
    pub newline_input: Option<&'static str>,
    /// 单行正文命中这条正则（JS 语法）时，也包成 bracketed paste 再写进终端；None = 单行照旧直接写。
    ///
    /// 多行正文本来就一律走 bracketed paste。单行直接写是为了兼容不开 bracketed paste 的旧 CLI，
    /// 但有的 TUI 会把「一次写进来的一串字」里的某些字符当成快捷键吃掉（gemini 0.68.0 吞半角问号，
    /// 见其插件注释）。命中的正文改走粘贴，TUI 按粘贴内容原样收下。
    pub single_line_paste_pattern: Option<&'static str>,
    /// 当前会话支持 runtime 命令自发现、但权威清单尚未写入 transcript。前端据此继续随
    /// transcript 增量重试；一旦为 false 就停止探测，避免稳态轮询反复扫描文件。
    pub runtime_commands_pending: bool,
    /// 附件可用该 CLI 的原生 `@路径` 提及注入（提交时由 CLI 原生附加文件内容,transcript
    /// 里也显示为原生提及）。false = 前端退回通用指令文本兜底。由插件按已探测版本声明；
    /// 探测不到版本一律 false——宁可多一段指令文本,不可发出对方不认的语法。
    pub attachment_mention: bool,
    /// 该 CLI 的 TUI 支持 Ctrl-V 原生粘贴剪贴板图片时,composer 里出现的占位符正则
    /// (claude `\[Image ?#\d`、kimi `\[image[:# ]`)。前端在「剪贴板里仍是刚粘贴的那张图」
    /// 时向 PTY 发粘贴键（见 `clipboard_paste_input`）触发原生图片附加,并用它从屏幕上
    /// 验证成功;None = 不支持,图片一律走指令文本兜底。
    pub clipboard_image_paste: Option<&'static str>,
    /// 触发原生图片粘贴的按键序列（claude 的 Ctrl-V = `\x16`；kimi 在 Windows 上是
    /// Alt+V = `\x1bv`，发 Ctrl-V 会被 composer 无视）。由插件声明，前端不自行猜键。
    pub clipboard_paste_input: Option<&'static str>,
    /// 终端界面里「运行中状态行」的匹配正则（claude 的 `✻ Composing… (41s · ↓ 427 tokens)`）：
    /// 捕获组 1 = 动词，2 = 括号统计（可缺）。对话页运行条据此照搬状态行内容；None = 不解析，
    /// 运行条退回本地计时。匹配的是对方界面文案，随版本改版会失效——规则须宁可漏认不可误认。
    pub status_line_pattern: Option<&'static str>,
    /// 终端界面里「请求失败、正在自动重试」那一行的匹配正则：捕获组 1 = 第几次，2 = 上限。
    /// 这类重试不落 transcript，屏幕是唯一证据；不认的话重试期间对话页像卡死。
    /// 同 status_line_pattern：对方界面文案，宁可漏认不可误认。None = 不解析。
    pub retry_line_pattern: Option<&'static str>,
    /// 终端输入框的规格（见 [`ComposerSpec`]）：认框、收草稿、清原文共用这一份。None = 这个
    /// 版本的输入框未取证，对话页不读也不动它。
    pub composer: Option<ComposerSpec>,
    /// 发送前切回主会话的能力(见 [`SubagentView`])。None = 该 agent 没有子代理视图或未取证。
    pub subagent_view: Option<SubagentView>,
    /// 会话里切模型的机制（见 [`crate::ModelSwitch`]）。None = 没有确定方式，前端退回终端菜单兜底。
    pub model_switch: Option<crate::ModelSwitchKind>,
    /// CLI 已起好、只是在等第一条消息才建会话的屏幕原文（见 `AgentPlugin::awaiting_first_message_markers`）。
    pub awaiting_first_message_markers: Vec<&'static str>,
    /// 探测到的 CLI 版本，原样回传（展示/排障）。
    pub version: Option<String>,
}

/// 自定义斜杠命令的发现规格——**这才是「命令由 agent 提供」字面成立的那部分**：
/// 用户在 agent 自己的目录里放了什么，补全就出什么。
#[derive(Debug, Clone, Copy)]
pub struct CustomCommandSpec {
    /// 用户级命令目录，相对该 agent 的**数据目录**（多账号时即 profile 的数据目录，天然隔离）。
    pub user_dir: Option<&'static str>,
    /// 项目级命令目录，相对会话 cwd。None = 该 agent 无项目级命令。
    pub project_dir: Option<&'static str>,
    /// 命令文件扩展名（不含点）。`md` 的描述取 frontmatter，`toml` 取顶层 `description` 键。
    pub ext: &'static str,
    /// 子目录的命名空间连接符（claude/gemini 都是 `:`：`commands/git/commit.*` → `/git:commit`）。
    /// None = **只收顶层文件**——用于嵌套语义未经验证的 agent，宁可少收也不编造名字。
    pub namespace_sep: Option<&'static str>,
}

/// 单个目录树的扫描上限。命令目录本该只有几十个文件；真被指到一棵大树（比如 cwd 误配到 home）
/// 时截断保护，宁可补全不满也不把 UI 查询变成全盘遍历。
const MAX_FILES: usize = 200;
const MAX_DEPTH: usize = 3;

impl CustomCommandSpec {
    /// 扫出全部自定义命令：用户级（agent 数据目录下）+ 项目级（cwd 下）。
    /// 目录不存在/不可读一律静默跳过——「没配过自定义命令」是常态，不是错误。
    pub fn discover(&self, data_dir: &Path, cwd: Option<&Path>) -> Vec<SlashCommand> {
        let mut out = Vec::new();
        if let Some(rel) = self.user_dir {
            self.scan(&crate::join_rel(data_dir, rel), SlashSource::User, &mut out);
        }
        if let (Some(rel), Some(cwd)) = (self.project_dir, cwd) {
            self.scan(&crate::join_rel(cwd, rel), SlashSource::Project, &mut out);
        }
        out
    }

    fn scan(&self, root: &Path, source: SlashSource, out: &mut Vec<SlashCommand>) {
        let mut budget = MAX_FILES;
        self.scan_dir(root, source, &mut Vec::new(), &mut budget, out);
    }

    fn scan_dir(
        &self,
        dir: &Path,
        source: SlashSource,
        prefix: &mut Vec<String>,
        budget: &mut usize,
        out: &mut Vec<SlashCommand>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        // 排序保证同一目录内容产出稳定的命令顺序（read_dir 的顺序是平台细节）。
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if *budget == 0 {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                if self.namespace_sep.is_some() && prefix.len() + 1 < MAX_DEPTH {
                    prefix.push(entry.file_name().to_string_lossy().into_owned());
                    self.scan_dir(&path, source, prefix, budget, out);
                    prefix.pop();
                }
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if path.extension().and_then(|e| e.to_str()) != Some(self.ext) {
                continue;
            }
            *budget -= 1;
            let name = match (self.namespace_sep, prefix.is_empty()) {
                (Some(sep), false) => format!("/{}{sep}{stem}", prefix.join(sep)),
                _ => format!("/{stem}"),
            };
            out.push(SlashCommand {
                name,
                description: read_description(&path, self.ext),
                source,
            });
        }
    }
}

/// 从命令文件头里取描述，纯 best-effort：取不到就 None，前端只是少一行说明文字。
fn read_description(path: &Path, ext: &str) -> Option<String> {
    // 描述在文件头部；整读上限防住把大文件塞进命令目录的意外。
    let text = read_head(path, 64 * 1024)?;
    let desc = match ext {
        "toml" => text
            .parse::<toml_edit::DocumentMut>()
            .ok()?
            .get("description")?
            .as_str()?
            .to_string(),
        // md：YAML frontmatter（首行 `---` 到下一个 `---`）里的 `description:`。
        _ => {
            let mut lines = text.lines();
            if lines.next()?.trim() != "---" {
                return None;
            }
            lines
                .take_while(|l| l.trim() != "---")
                .find_map(|l| l.strip_prefix("description:"))?
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        }
    };
    let desc = desc.trim();
    (!desc.is_empty()).then(|| desc.to_string())
}

fn read_head(path: &Path, max: u64) -> Option<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(max)
        .read_to_string(&mut buf)
        .ok()?;
    Some(buf)
}

/// 内置表 ∪ 发现的自定义命令：同名时**自定义覆盖内置**（与 CLI 自身的遮蔽语义一致，
/// 且自定义带着从文件里读出的描述）；自定义内部同名时**项目级覆盖用户级**（claude 的
/// 遮蔽语义：离会话 cwd 越近优先级越高）。最后按名排序供补全稳定展示。
pub(crate) fn merge_commands(builtins: &[&str], custom: Vec<SlashCommand>) -> Vec<SlashCommand> {
    let mut out: Vec<SlashCommand> = builtins
        .iter()
        .filter(|b| !custom.iter().any(|c| c.name == **b))
        .map(|b| SlashCommand::builtin(b))
        .collect();
    out.extend(custom);
    // discover 输出用户级在前；只按名字做稳定排序会把用户级留在前面，dedup 留首个便让
    // 用户级盖住项目级——与 claude 语义恰好相反。故同名时按遮蔽优先级再排一层。
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| shadow_rank(a.source).cmp(&shadow_rank(b.source)))
    });
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// 同名命令的遮蔽优先级：数值小者在排序中居前，从而在 `dedup` 中存活。
/// Builtin 实际到不了这一层（同名内置已在上方被过滤），列出只为覆盖全部变体。
fn shadow_rank(source: SlashSource) -> u8 {
    match source {
        SlashSource::Project => 0,
        SlashSource::User => 1,
        SlashSource::Builtin => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-chatui-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const MD_SPEC: CustomCommandSpec = CustomCommandSpec {
        user_dir: Some("commands"),
        project_dir: Some(".claude/commands"),
        ext: "md",
        namespace_sep: Some(":"),
    };

    #[test]
    fn discovers_user_and_project_commands_with_namespaces_and_descriptions() {
        let root = temp_root("md");
        let user = root.join("data").join("commands");
        fs::create_dir_all(user.join("git")).unwrap();
        fs::write(
            user.join("deploy.md"),
            "---\ndescription: \"部署到测试环境\"\n---\n正文",
        )
        .unwrap();
        // 子目录 → 命名空间；无 frontmatter → 无描述。
        fs::write(user.join("git").join("commit.md"), "正文而已").unwrap();
        // 扩展名不符的不收（README、临时文件混进目录是常态）。
        fs::write(user.join("README.txt"), "x").unwrap();
        let proj = root.join("repo").join(".claude").join("commands");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("review.md"), "---\ndescription: 走查清单\n---").unwrap();

        let got = MD_SPEC.discover(&root.join("data"), Some(&root.join("repo")));
        assert_eq!(
            got,
            vec![
                SlashCommand {
                    name: "/deploy".into(),
                    description: Some("部署到测试环境".into()),
                    source: SlashSource::User,
                },
                SlashCommand {
                    name: "/git:commit".into(),
                    description: None,
                    source: SlashSource::User,
                },
                SlashCommand {
                    name: "/review".into(),
                    description: Some("走查清单".into()),
                    source: SlashSource::Project,
                },
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn toml_description_and_flat_only_when_no_namespace_sep() {
        let root = temp_root("toml");
        let user = root.join("commands");
        fs::create_dir_all(user.join("nested")).unwrap();
        fs::write(
            user.join("commit.toml"),
            "description = \"生成提交信息\"\nprompt = \"...\"",
        )
        .unwrap();
        fs::write(user.join("nested").join("x.toml"), "prompt = \"...\"").unwrap();

        // namespace_sep = None：嵌套语义未验证的 agent 只收顶层，宁可少收也不编造名字。
        let flat = CustomCommandSpec {
            user_dir: Some("commands"),
            project_dir: None,
            ext: "toml",
            namespace_sep: None,
        };
        let got = flat.discover(&root, None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "/commit");
        assert_eq!(got[0].description.as_deref(), Some("生成提交信息"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dirs_are_silently_empty() {
        let root = temp_root("none");
        assert!(MD_SPEC.discover(&root.join("nope"), None).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_prefers_custom_over_builtin_and_sorts() {
        let custom = vec![SlashCommand {
            name: "/review".into(),
            description: Some("自定义走查".into()),
            source: SlashSource::Project,
        }];
        let merged = merge_commands(&["/review", "/clear"], custom);
        assert_eq!(
            merged.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["/clear", "/review"]
        );
        // 同名时自定义覆盖内置——与 CLI 自身的遮蔽语义一致。
        assert_eq!(merged[1].source, SlashSource::Project);
        assert_eq!(merged[1].description.as_deref(), Some("自定义走查"));
    }

    /// discover 输出用户级在前、项目级在后；合并时同名必须项目级胜出（claude 语义），
    /// 不能靠「稳定排序 + dedup 留首个」让用户级盖掉项目级（修复前正是如此）。
    #[test]
    fn project_command_shadows_same_named_user_command() {
        let custom = vec![
            SlashCommand {
                name: "/review".into(),
                description: Some("用户级走查".into()),
                source: SlashSource::User,
            },
            SlashCommand {
                name: "/review".into(),
                description: Some("项目级走查".into()),
                source: SlashSource::Project,
            },
            SlashCommand {
                name: "/deploy".into(),
                description: None,
                source: SlashSource::User,
            },
        ];
        let merged = merge_commands(&[], custom);
        assert_eq!(
            merged.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["/deploy", "/review"]
        );
        assert_eq!(merged[1].source, SlashSource::Project);
        assert_eq!(merged[1].description.as_deref(), Some("项目级走查"));
    }
}
