// 卡片场景：五家的审批卡、提问卡、提示卡，以及过期、已取消、多选项、长命令、长摘要这些状态。
// 桌面 harness 的场景名是 flow-card-<名字>，手机是 mobile-card-<名字>，两边用同一份数据。
// 屏幕样本与 broker 请求直接用测试里那份（src/test/requestSamples.ts，各自注明了出处），
// 能力表用测试夹具的 chatUi(provider)（与后端各插件声明同源），所以这里拍到的卡片与真实会话走的是同一条识别链。
import { chatUi } from "../src/test/agents";
import {
  CLAUDE_APPROVAL_SCREEN,
  CLAUDE_BROKER_APPROVAL,
  CODEX_BROKER_APPROVAL,
  GEMINI_APPROVAL_SCREEN,
  KIMI_APPROVAL_SCREEN,
  OPENCODE_ALWAYS_CONFIRM_SCREEN,
  OPENCODE_PERMISSION_SCREEN,
} from "../src/test/requestSamples";
import { CHAT_SESSION_ID, questionInput } from "./fixtures";

type Lang = "zh" | "en";
type Scene = {
  provider: "claude" | "codex" | "kimi" | "gemini" | "opencode";
  /// 托管终端的屏幕文本（屏幕通道）。
  screen?: string;
  /// broker 挂起的审批。
  approval?: Record<string, unknown>;
  /// broker 挂起的提问（AskUserQuestion 的参数，JSON 字符串）。
  question?: (lang: Lang) => string;
  /// 会话记录里报的待处理交互。
  pendingReview?: "approval" | "question" | "plan" | null;
  /// 回合是否在跑（审批都发生在回合中途；信任页、长会话确认在回合之外）。
  running?: boolean;
  /// broker 审批出现几次轮询之后，后端改报这个终态（卡片收走，提示条说明）。
  endsAs?: "cancelled" | "expired";
};

const RULE = " " + "─".repeat(60);
const kimiForm = (...body: string[]) => [RULE, "  question", ...body, RULE, " Ask When Needed  K3 thinking: high"].join("\n");

/// 四十行的长命令：一条 heredoc 写文件再跑测试。看「展开全部」、卡内滚动与输入框是否被挤。
const LONG_COMMAND = [
  "cat > scripts/migrate-login-schema.mjs <<'EOF'",
  ...Array.from({ length: 34 }, (_, i) => `  await step${String(i + 1).padStart(2, "0")}(db, { table: "login_rules", dryRun: false, batch: ${200 + i * 25} });`),
  "EOF",
  "node scripts/migrate-login-schema.mjs --env staging --confirm && bun run test -- src/schemas/login.test.ts src/components/LoginForm.test.tsx",
].join("\n");

const LONG_SUMMARY = {
  zh: "把登录、注册、找回密码三张表单的校验规则搬到 src/schemas/login.ts，删除三处内联判断，补齐邮箱大小写、加号别名、全角空格、密码长度上下限、常见弱密码这几类边界的单测，然后在预发环境对最近三十天的真实提交回放一遍，确认没有此前能通过、现在被拦下的输入；回放脚本会读取只读副本，不写任何业务表，跑完自动清理临时目录。",
  en: "Move the validation rules for sign-in, sign-up and password reset into src/schemas/login.ts, drop the three inline checks, add unit tests for email casing, plus aliases, full-width spaces, password length bounds and common weak passwords, then replay the last thirty days of real submissions on staging to confirm nothing that used to pass is now rejected; the replay reads a read-only replica, writes no business table and cleans its temp directory afterwards.",
};

export const CARD_SCENES: Record<string, Scene> = {
  // ── 审批卡：五家各一张，claude 另有屏幕兜底的一张 ──
  "claude-approval": { provider: "claude", approval: CLAUDE_BROKER_APPROVAL, pendingReview: "approval", running: true },
  "codex-approval": { provider: "codex", approval: CODEX_BROKER_APPROVAL, pendingReview: "approval", running: true },
  "kimi-approval": { provider: "kimi", screen: KIMI_APPROVAL_SCREEN, running: true },
  "gemini-approval": { provider: "gemini", screen: GEMINI_APPROVAL_SCREEN, running: true },
  "opencode-approval": { provider: "opencode", screen: OPENCODE_PERMISSION_SCREEN, running: true },
  // 多选项：claude 2.1.325 的屏幕审批框有两个持久放行项。
  "claude-screen": { provider: "claude", screen: CLAUDE_APPROVAL_SCREEN, running: true },
  // ── 状态 ──
  // 过期不单设场景：拍 claude-approval 时把假时钟推过 300 秒，倒计时归零即过期。
  // 已取消：先给出审批，随后后端报这条请求已取消（回合被中断），卡片收走、提示条说明原因。
  "cancelled": { provider: "codex", approval: CODEX_BROKER_APPROVAL, endsAs: "cancelled", pendingReview: "approval", running: true },
  "long-command": { provider: "codex", approval: { ...CODEX_BROKER_APPROVAL, requestId: "codex-long", input: JSON.stringify({ command: LONG_COMMAND }) }, pendingReview: "approval", running: true },
  "long-summary": { provider: "claude", approval: { ...CLAUDE_BROKER_APPROVAL, requestId: "claude-long-summary", description: "__LONG_SUMMARY__" }, pendingReview: "approval", running: true },
  // ── 提问卡：有提问能力的三家（codex、opencode 没有提问工具） ──
  "claude-question": { provider: "claude", question: questionInput, pendingReview: "question", running: true },
  "kimi-question": { provider: "kimi", question: questionInput, pendingReview: "question", running: true },
  "kimi-form": { provider: "kimi", running: true, screen: kimiForm(
    "  (✓) 颜色   水果    Submit",
    "  ? 喜欢哪些水果？",
    "   [ ] 苹果",
    "   [✓] 香蕉",
    "   [ ] 橙子",
    "   [ ] Other",
    "   ↑↓ select  1-4 / ↵ toggle  ←/→/tab switch  esc cancel",
  ) },
  // gemini 0.65 ask_user 的多选页（npm bundle 源码取证，与 terminalAttention.test.ts 的样本同一份）。
  "gemini-form": { provider: "gemini", running: true, screen: [
    "⠏ Waiting for user confirmation...",
    "╭──────────────────────────────────────────────────────╮",
    "│ Answer Questions                                     │",
    "│──────────────────────────────────────────────────────│",
    "│ ← ✓ Database │ □ Features │ ≡ Review →               │",
    "│ Which features do you want?                          │",
    "│ (Select all that apply)                              │",
    "│                                                      │",
    "│ ● 1. [x] Auth                                        │",
    "│   2. [ ] Billing                                     │",
    "│   3. [ ] All of the above                            │",
    "│         Select all options                           │",
    "│   4. [ ] Enter a custom value                        │",
    "│        Done                                          │",
    "│         Finish selection                             │",
    "│                                                      │",
    "│ Enter to select · ←/→ to switch questions · Esc to cancel │",
    "╰──────────────────────────────────────────────────────╯",
  ].join("\n") },
  "claude-selector": { provider: "claude", pendingReview: "question", running: true, screen: [
    "Which validation library should we use?", "", "❯ 1. Zod", "     TypeScript-first, already in the project", "  2. Yup", "     Larger bundle",
    "  3. Type something.", "  4. Chat about this", "", "Enter to select · ↑/↓ to navigate · Esc to cancel",
  ].join("\n") },
  // ── 提示卡 ──
  "gemini-limit": { provider: "gemini", running: true, screen: [
    " > say hi",
    "╭──────────────────────────────────────────────╮",
    "│                                              │",
    "│ Usage limit reached for gemini-3.8-flash.    │",
    "│ /stats model for usage details               │",
    "│ /model to switch models.                     │",
    "│                                              │",
    "│ ● 1. Keep trying                             │",
    "│   2. Stop                                    │",
    "╰──────────────────────────────────────────────╯",
  ].join("\n") },
  "opencode-confirm": { provider: "opencode", screen: OPENCODE_ALWAYS_CONFIRM_SCREEN, running: true },
  "claude-plan": { provider: "claude", running: true, screen: [
    "Claude has written up a plan and is ready to execute. Would you like to proceed?", "",
    "❯ 1. Yes, and bypass permissions", "  2. Yes, manually approve edits", "  3. Tell Claude what to change",
  ].join("\n") },
  "codex-trust": { provider: "codex", screen: "Do you trust the contents of this directory?\n\n  C:\\dev\\web-app\n\n> 1. Yes, continue\n  2. No, quit" },
};

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

/// 卡片场景的 IPC 返回值；返回 undefined 表示这条不归它管。name 不在表里时整体不接管。
export function cardSceneMock(name: string, lang: Lang, chat: (cmd: string) => unknown): (cmd: string, args: unknown) => unknown {
  const scene = CARD_SCENES[name];
  if (!scene) return () => undefined;
  const withSession = (request: Record<string, unknown>) => ({
    ...request,
    sessionId: CHAT_SESSION_ID,
    description: request.description === "__LONG_SUMMARY__" ? (lang === "zh" ? LONG_SUMMARY.zh : LONG_SUMMARY.en) : request.description,
  });
  let interactionCalls = 0;
  return (cmd, args) => {
    if (cmd === "agent_chat_ui") return (args as { provider?: string })?.provider === scene.provider ? chatUi(scene.provider) : null;
    if (cmd === "get_chat_history") {
      const history = chat(cmd) as Record<string, unknown>;
      // 请求了结之后会话记录也不再报待处理（真实后端同样如此），否则会留下一张「正在读取选项」的降级卡。
      const ended = !!scene.endsAs && interactionCalls > 3;
      return { ...history, provider: scene.provider, ptyManaged: true, status: scene.running && !ended ? "running" : "waiting", pendingReview: ended ? null : scene.pendingReview ?? null };
    }
    if (cmd === "pending_interaction") {
      const base = (chat(cmd) as { approval: Record<string, unknown> }).approval;
      if (scene.endsAs && scene.approval && ++interactionCalls > 3) {
        return { approval: null, question: null, runtimeRequests: [{ ...withSession(scene.approval), status: scene.endsAs, responseCapability: { type: "not_resumable", reason: scene.endsAs } }] };
      }
      return {
        approval: scene.approval ? withSession(scene.approval) : null,
        question: scene.question ? { ...base, sessionId: CHAT_SESSION_ID, provider: scene.provider, requestId: `card-question-${name}`, toolName: "AskUserQuestion", description: null, input: scene.question(lang), answerable: true } : null,
      };
    }
    if (cmd === "managed_terminal_snapshot") {
      const text = (scene.screen ?? "").split("\n").join("\r\n");
      return { sessionId: CHAT_SESSION_ID, active: true, managed: true, data: b64(text), startOffset: 0, endOffset: new TextEncoder().encode(text).length,
        exited: false, exitCode: null, exitTail: null, cols: 120, rows: 40, modes: [], externalViewers: false, exitUnexpected: false };
    }
    return undefined;
  };
}
