/// 五家的审批请求样本，卡片相关的测试共用。每份都注明出处，不凭空编。
///
/// 屏幕样本是终端画面的文本（识别层的输入），broker 样本是后端发给前端的 RuntimeRequest。

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
/// @ 0.29），与 terminalAttention.test.ts 里的既有样本同一份。
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

/// Gemini 的工具确认框。0.61 真机屏（`--approval-mode default` 下跑一条带重定向的 shell），
/// 与 terminalAttention.test.ts 里的既有样本同一份。
export const GEMINI_APPROVAL_SCREEN = [
  "╭────────────────────────────────────────────╮",
  "│ ? Shell  echo meowo-probe > probe-out.txt                         │",
  "│ ╭────────────────────────────────────────╮ │",
  "│ │ echo meowo-probe > probe-out.txt                             │ │",
  "│ ╰────────────────────────────────────────╯ │",
  "│ Allow execution of [Shell]?                │",
  "│ Redirection detected. To auto-accept, press Shift+Tab │",
  "│                                            │",
  "│ ● 1. Allow once                           │",
  "│   2. Allow for this session               │",
  "│   3. No, suggest changes (esc)            │",
  "╰────────────────────────────────────────────╯",
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
/// permission_hook_decides，载荷只有工具名与参数）；形状同上，不是真机抓包。
export const CODEX_BROKER_APPROVAL: RuntimeRequest = {
  sessionId: 7,
  requestId: "codex-req-1",
  provider: "codex",
  kind: "approval",
  status: "pending",
  toolName: "shell",
  description: null,
  input: JSON.stringify({ command: "git push origin main" }),
  permissionSuggestions: [],
  responseCapability: { type: "broker_live" },
};
