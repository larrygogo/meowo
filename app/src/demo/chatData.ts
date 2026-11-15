// 对话窗演示（chatEmbed.tsx）的假会话：Claude 在重构登录页表单校验，读了文件、改了代码、跑过
// 测试，待办清单进行到一半，现在要跑 git push 等你批准。侧栏另外列两个会话（Kimi、Codex）。
import { makeSession } from "./data";
import type { DemoLang } from "./strings";

export const CHAT_SESSION_ID = 1;

const T = {
  zh: {
    title: "重构登录页表单校验",
    others: [
      { title: "修支付回调的重复扣款", project: "billing", provider: "kimi" as const },
      { title: "补齐导出接口的单测", project: "api", provider: "codex" as const },
    ],
    ask: "登录页的表单校验散在三个组件里，帮我收到一个 schema 里，测试要跟上。",
    a1: "我先看一下现有的表单组件和校验逻辑。",
    a3: "改完了，12 个测试全部通过。规则都收进了 `src/schemas/login.ts`，`LoginForm` 里的重复判断删掉了。接下来推到远端分支。",
    todos: ["梳理现有校验逻辑", "抽出 loginSchema", "组件改用 schema", "推送并开 PR"],
    approval: "推送到远端分支",
  },
  en: {
    title: "Refactor login form validation",
    others: [
      { title: "Fix duplicate charge in payment callback", project: "billing", provider: "kimi" as const },
      { title: "Add tests for the export API", project: "api", provider: "codex" as const },
    ],
    ask: "The login form validation is spread across three components. Move it into one schema and keep the tests passing.",
    a1: "Let me look at the current form components and their validation.",
    a3: "Done, all 12 tests pass. The rules now live in `src/schemas/login.ts` and the duplicate checks in `LoginForm` are gone. Next I'll push to the remote branch.",
    todos: ["Map the current validation", "Extract loginSchema", "Switch components to the schema", "Push and open a PR"],
    approval: "Push to the remote branch",
  },
};

export function chatMock(lang: DemoLang) {
  const t = T[lang];
  const main = makeSession({ title: t.title, project: "web-app", status: "running", provider: "claude", agoMin: 0, todoDone: 3, todoTotal: 4 });
  main.session.id = CHAT_SESSION_ID;
  main.pending_review = "approval";
  const others = t.others.map((o, i) => {
    const s = makeSession({ ...o, status: i === 0 ? "running" : "waiting", agoMin: 2 + i * 3 });
    s.session.id = 2 + i;
    return s;
  });
  const sessions = [main, ...others];
  const ts = new Date(Date.now()).toISOString();
  const items = [
    { type: "user_text", id: "u1", timestamp: ts, text: t.ask },
    { type: "assistant_text", id: "a1", timestamp: ts, text: t.a1 },
    { type: "tool_use", id: "t1", timestamp: ts, name: "Read", kind: "read", summary: "src/components/LoginForm.tsx" },
    { type: "tool_result", id: "r1", timestamp: ts, tool_use_id: "t1", text: "182 lines", is_error: false },
    { type: "tool_use", id: "t2", timestamp: ts, name: "Grep", kind: "search", summary: "validate|isValid" },
    { type: "tool_result", id: "r2", timestamp: ts, tool_use_id: "t2", text: "6 matches", is_error: false },
    { type: "tool_use", id: "t3", timestamp: ts, name: "Write", kind: "write", summary: "src/schemas/login.ts" },
    { type: "tool_result", id: "r3", timestamp: ts, tool_use_id: "t3", text: "ok", is_error: false },
    { type: "tool_use", id: "t4", timestamp: ts, name: "Edit", kind: "edit", summary: "src/components/LoginForm.tsx" },
    { type: "tool_result", id: "r4", timestamp: ts, tool_use_id: "t4", text: "ok", is_error: false },
    { type: "tool_use", id: "t5", timestamp: ts, name: "Bash", kind: "run", summary: "npm test -- login" },
    { type: "tool_result", id: "r5", timestamp: ts, tool_use_id: "t5", text: "Tests: 12 passed, 12 total", is_error: false },
    { type: "assistant_text", id: "a3", timestamp: ts, text: t.a3 },
    { type: "tool_use", id: "t6", timestamp: ts, name: "Bash", kind: "run", summary: "git push -u origin refactor/login-schema" },
  ];
  const todos = t.todos.map((content, i) => ({ content, status: i < 3 ? "completed" : "in_progress", stale: false }));

  return (cmd: string): unknown => {
    switch (cmd) {
      case "get_chat_history":
        return {
          sessionId: CHAT_SESSION_ID,
          ccSessionId: "demo-chat",
          title: t.title,
          status: "running",
          provider: "claude",
          cwd: "C:/dev/web-app",
          supported: true,
          items,
          offset: 1,
          reset: true,
          pendingReview: "approval",
          model: "Opus 5",
          agentModes: null,
          contextPct: 18,
          contextWindow: 1_000_000,
          currentActivity: null,
          connected: true,
          screenState: null,
          errored: false,
          ptyManaged: true,
          endable: true,
          background: false,
          archived: false,
          todos,
          hasMore: false,
          earliest: null,
          lastUserText: t.ask,
          lastAiText: t.a3,
          predecessorId: null,
          supersededBy: null,
        };
      case "pending_interaction":
        return {
          question: null,
          approval: {
            sessionId: CHAT_SESSION_ID,
            requestId: "demo-approval",
            provider: "claude",
            toolName: "Bash",
            description: t.approval,
            input: JSON.stringify({ command: "git push -u origin refactor/login-schema" }),
            permissionSuggestions: [],
            answerable: false,
          },
        };
      case "get_live_sessions_page":
        return sessions;
      // 下面这些在演示里没有意义，给空值让组件按「没有」渲染。
      case "managed_terminal_snapshot":
        return { sessionId: CHAT_SESSION_ID, active: true, managed: true, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null };
      case "recent_cwds":
      case "list_folder_editors":
        return [];
      default:
        return undefined;
    }
  };
}
