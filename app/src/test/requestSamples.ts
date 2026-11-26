/// 五家的审批请求样本，卡片相关的测试共用。每份都注明出处，不凭空编。
///
/// 屏幕样本是终端画面的文本（识别层的输入），broker 样本是后端发给前端的 RuntimeRequest。
///
/// 这里有几份不是真机抓的（kimi 的屏幕取自源码，claude、codex 的 broker 请求只有字段形状）。
/// 真机抓的那一套在 fixtures/approval-frames.json（tests/probe_approval.rs，2026-11-14：claude、codex 的
/// broker 请求，claude、kimi、opencode 的屏幕；antigravity 的屏幕取自 2026-11-26 屏幕取证），由 views/chat/approvalFrames.test.tsx 回放。
/// 两套并存：这里的样本带着各条用例要的特定内容（长命令、危险命令、多条持久放行），真机那套管
/// 「CLI 现在实际长什么样」。

import type { RuntimeRequest } from "../api";

/// Claude Code 2.1.325 的权限框。本机真机取证 2026-11-14（tests/probe_tui_script.rs，
/// `--permission-mode default` 下让它跑 `curl -s https://example.com -o out.html`）。
export const CLAUDE_APPROVAL_SCREEN = [
  "❯ Run this bash command exactly: curl -s https://example.com -o out.html",
  "  Downloading example.com homepage to out.html",
  "  ⎿  $ curl -s https://example.com -o out.html",
  "────────────────────────────────────────────────────────────",
  " Bash command",
  " Tip: auto mode handles these prompts for you — choose \"switch to auto mode\" below",
  " Download example.com homepage to out.html",
  "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
  " curl -s https://example.com -o out.html",
  "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
  " This command requires approval",
  " Do you want to proceed?",
  " ❯ 1. Yes",
  "   2. Yes, and don’t ask again for: curl *",
  "   3. Yes, and switch to auto mode · auto mode handles these prompts for you",
  "   4. No",
  " Esc to cancel · Tab to amend",
].join("\n");

/// Kimi 的审批面板。官方源码取证（apps/kimi-code/src/tui/components/dialogs/approval-panel.ts
/// @ 0.29），与 terminalAttention.test.ts 里的既有样本同一份。2.6.1 真机的面板在标题与命令之间
/// 多一行 `cwd: <工作目录>`，见 approval-frames.json 的 kimi 帧。
export const KIMI_APPROVAL_SCREEN = [
  "────────────────────────",
  "  ▶ Run this command?",
  "",
  "  $ echo meowo-approval-probe-1234",
  "  Run the probe command",
  "",
  "  ▶ 1. Approve once",
  "    2. Approve for this session",
  "    3. Reject",
  "    4. Reject with feedback",
  "",
  "  ↑/↓ select · 1/2/3/4 choose · ↵ confirm",
  "────────────────────────",
].join("\n");

/// Antigravity CLI 1.5.18 的命令审批面板。真机屏幕取证 2026-11-26（接入 antigravity 的 Lead 任务，
/// research-screen-work/run2/05_approval_cmd.txt，plan 模式下让它跑 `Get-ChildItem -Force`），裁到面板附近；
/// 与 fixtures/approval-frames.json 的 antigravity 帧同一屏。
export const ANTIGRAVITY_APPROVAL_SCREEN = [
  "Command",
  "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "",
  "Requesting permission for:",
  "   Get-ChildItem -Force",
  "",
  "Run this command?",
  "> 1. Yes, run command",
  "  2. Yes, and always allow in this conversation for commands that start with 'Get-ChildItem -Force'",
  "  3. Yes, and always allow for commands that start with 'Get-ChildItem -Force' (Persist to settings.json)",
  "  4. No, cancel",
  "",
  "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
  "esc to cancel                                                                                plan · Gemini 4 Neon · high",
].join("\n");

/// 同一个审批面板在窄终端里（Meowo dev 实例对话页的托管终端，约 100 列，2026-11-26 实拍）：长选项
/// 折到下一行，续行顶格。「本会话」与「记住」两项的区别（in this conversation / Persist to）可能正好
/// 在折下去的那半句里。
export const ANTIGRAVITY_APPROVAL_SCREEN_NARROW = [
  "Command",
  "────────────────────────────────────────────────────────────────────────────────────────────────────",
  "",
  "Requesting permission for:",
  "   Get-Date -Format yyyy-MM-dd",
  "",
  "Run this command?",
  "> 1. Yes, run command",
  "  2. Yes, and always allow in this conversation for commands that start with 'Get-Date -Format",
  "yyyy-MM-dd'",
  "  3. Yes, and always allow for commands that start with 'Get-Date -Format yyyy-MM-dd' (Persist to",
  "settings.json)",
  "  4. No, cancel",
  "",
  "  ↑/↓ Navigate · tab Amend · ctrl+g edit/expand command",
  "esc to cancel                                                        accept-edits · Gemini 4 Neon · high",
].join("\n");

/// OpenCode 1.19.5 的权限框。本机真机取证 2026-11-14（tests/probe_tui_script.rs，
/// 让它读工作区外的 C:/Windows/win.ini）。
export const OPENCODE_PERMISSION_SCREEN = [
  "  ┃",
  "  ┃  △ Permission required",
  "  ┃    ← Access external directory C:\\Windows",
  "  ┃",
  "  ┃  Patterns",
  "  ┃",
  "  ┃  - C:\\Windows\\*",
  "  ┃",
  "  ┃",
  "  ┃   Allow once   Allow always   Reject                                 ctrl+f fullscreen  ⇆ select  enter confirm",
  "  ┃",
].join("\n");

/// OpenCode 选 Allow always 后的确认页。同一次真机取证（权限框上发 `→ 回车` 之后的画面）。
export const OPENCODE_ALWAYS_CONFIRM_SCREEN = [
  "  ┃",
  "  ┃  △ Always allow",
  "  ┃",
  "  ┃  This will allow the following patterns until OpenCode is restarted",
  "  ┃",
  "  ┃  - C:\\Windows\\*",
  "  ┃",
  "  ┃",
  "  ┃   Confirm   Cancel                                                                      ⇆ select  enter confirm",
  "  ┃",
].join("\n");

/// Claude 的 broker 审批。字段形状取自 reporter 的既有夹具（meowo-reporter/tests/parse_test.rs 的
/// PermissionRequest：`permission_suggestions` 带 addRules / localSettings），不是真机抓包。
export const CLAUDE_BROKER_APPROVAL: RuntimeRequest = {
  sessionId: 7,
  requestId: "claude-req-1",
  provider: "claude",
  kind: "approval",
  status: "pending",
  toolName: "Bash",
  description: null,
  input: JSON.stringify({ command: "cargo test", description: "运行测试" }),
  permissionSuggestions: [{
    type: "addRules",
    behavior: "allow",
    destination: "localSettings",
    rules: [{ toolName: "Bash", ruleContent: "cargo test" }],
  }],
  responseCapability: { type: "broker_live" },
};

/// Codex 的 broker 审批。Codex 的 PermissionRequest hook 不带持久放行建议（插件声明
/// permission_hook_decides，载荷只有工具名与参数）；形状同上，不是真机抓包。工具名按 0.167.2
/// 真机抓到的写（`Bash`，见 approval-frames.json 的 codex 帧；此前凭印象写成 `shell`）。
export const CODEX_BROKER_APPROVAL: RuntimeRequest = {
  sessionId: 7,
  requestId: "codex-req-1",
  provider: "codex",
  kind: "approval",
  status: "pending",
  toolName: "Bash",
  description: null,
  input: JSON.stringify({ command: "git push origin main" }),
  permissionSuggestions: [],
  responseCapability: { type: "broker_live" },
};
