// 视觉回归 harness 共用的假数据：在演示用的那段假会话（src/demo/chatData.ts）上加长内容、
// 补「更改」面板要的 git 接口。只给 app/visual/ 下的截图页用，不进产品构建。
import { CHAT_SESSION_ID } from "../src/demo/chatData";

/// 与 src/demo/*Embed.tsx 同一个冻结时刻，时间戳在两次拍摄间不漂。
export const FIXED_NOW = 1_780_000_000_000;

const fence = "```";

/// 长内容：超长路径/链接、宽表格、代码块、长单词、引用、列表，外加一条长输出工具卡和一条报错工具卡。
/// 插在演示会话最后一条 git push（审批卡对应的那条）之前，审批卡仍在最底下。
export function longItems(lang: "zh" | "en", ts: string): unknown[] {
  const zh = lang === "zh";
  const user = zh
    ? "请看一下 C:\\Users\\someone\\Desktop\\workspace\\very-long-project-name\\packages\\frontend\\src\\components\\authentication\\forms\\LoginFormWithValidation.tsx 这个文件，还有这个链接 https://github.com/example-org/example-repository/blob/main/packages/frontend/src/components/LoginForm.tsx#L120-L180 里的写法。"
    : "Please look at C:\\Users\\someone\\Desktop\\workspace\\very-long-project-name\\packages\\frontend\\src\\components\\authentication\\forms\\LoginFormWithValidation.tsx and the approach in https://github.com/example-org/example-repository/blob/main/packages/frontend/src/components/LoginForm.tsx#L120-L180.";
  const para = zh
    ? "这是一段很长的段落，用来检查消息列宽在超宽窗口下是否限制在可读宽度内，而不是把一行拉得很长导致阅读困难。".repeat(4)
    : "This is a long paragraph used to check that the message column keeps a readable width on very wide windows instead of stretching a line across the whole screen. ".repeat(3);
  const md = [
    zh ? "下面是改动说明：" : "Here is a summary of the changes:",
    "",
    zh ? "| 文件 | 改动前 | 改动后 | 说明 |" : "| File | Before | After | Notes |",
    "|---|---|---|---|",
    zh ? "| src/components/authentication/forms/LoginForm.tsx | 182 行 | 120 行 | 删掉重复校验 |" : "| src/components/authentication/forms/LoginForm.tsx | 182 lines | 120 lines | Removed duplicate checks |",
    zh ? "| src/schemas/login.ts | 0 | 64 | 新增 |" : "| src/schemas/login.ts | 0 | 64 | New |",
    "",
    fence + "ts",
    "export const loginSchema = z.object({ email: z.string().email({ message: 'Please enter a valid email address' }), password: z.string().min(8).max(128) });",
    "export type LoginInput = z.infer<typeof loginSchema>;",
    fence,
    "",
    (zh ? "长 inline code：" : "Long inline code: ") + "`C:/Users/someone/Desktop/workspace/very-long-project-name/packages/frontend/src/components/LoginForm.tsx`",
    "",
    "Pneumonoultramicroscopicsilicovolcanoconiosis_Pneumonoultramicroscopicsilicovolcanoconiosis_Pneumonoultramicroscopicsilicovolcanoconiosis",
    "",
    zh ? "> 引用一段比较长的话，看看引用块在窄窗下如何换行，以及左边框是否和正文对齐。" : "> A fairly long quote to see how the blockquote wraps in a narrow window and whether its left border lines up with the text.",
    "",
    zh ? "1. 第一项\n2. 第二项，带嵌套列表：\n   - 嵌套 A\n   - 嵌套 B" : "1. First item\n2. Second item, with a nested list:\n   - Nested A\n   - Nested B",
  ].join("\n");
  const testOut = Array.from({ length: 40 }, (_, i) => ` ✓ src/components/LoginForm.test.tsx > case ${i} validates the login form input correctly`).join("\n");
  return [
    { type: "user_text", id: "vx-u1", timestamp: ts, text: user },
    { type: "assistant_text", id: "vx-a1", timestamp: ts, text: md },
    { type: "tool_use", id: "vx-t1", timestamp: ts, name: "Bash", summary: "cd packages/frontend && npx vitest run src/components/LoginForm.test.tsx --reporter=verbose" },
    { type: "tool_result", id: "vx-r1", timestamp: ts, tool_use_id: "vx-t1", text: testOut, is_error: false },
    { type: "tool_use", id: "vx-t2", timestamp: ts, name: "Read", summary: "packages/frontend/src/components/authentication/forms/LoginFormWithValidation.tsx" },
    { type: "tool_result", id: "vx-r2", timestamp: ts, tool_use_id: "vx-t2", text: "error: ENOENT no such file or directory", is_error: true },
    { type: "assistant_text", id: "vx-a2", timestamp: ts, text: para },
  ];
}

/// 把长内容插进 get_chat_history 的结果（插在最后一条 tool_use 之前）。
export function withLongItems(history: unknown, lang: "zh" | "en"): unknown {
  const h = history as { items: unknown[] };
  const ts = new Date(FIXED_NOW).toISOString();
  const items = [...h.items];
  items.splice(items.length - 1, 0, ...longItems(lang, ts));
  return { ...h, items };
}

/// 「更改」面板：一个仓库、三个工作区改动 + 一个「本分支已提交」文件，带 PR 状态（git_forge_status）。
export function scmMock(cmd: string): unknown {
  if (cmd === "git_changes_overview") {
    return [{
      root: "C:/dev/web-app", name: "web-app", isCwdRepo: true,
      summary: {
        gitAvailable: true, isRepo: true, branch: "refactor/login-schema", ahead: 2, behind: 0,
        files: [
          { path: "src/schemas/login.ts", status: "U", additions: 18, deletions: 0 },
          { path: "src/components/LoginForm.tsx", status: "M", additions: 6, deletions: 21 },
          { path: "src/components/LoginForm.test.tsx", status: "M", additions: 9, deletions: 2 },
        ],
        upstream: "origin/refactor/login-schema", unpushed: 2, pushTarget: "origin/refactor/login-schema",
      },
      base: "abc123", baseLabel: "main",
      committed: [{ path: "README.md", status: "M", additions: 3, deletions: 1 }],
      detailed: true,
    }];
  }
  if (cmd === "git_forge_status") {
    return {
      gh: "ok", ghTooOld: false, repoUrl: "https://github.com/example/web-app", compareUrl: null,
      mergeMethods: ["squash", "merge"], lastMergeMethod: "squash",
      pr: {
        number: 162, state: "open", url: "https://github.com/example/web-app/pull/162", title: "重构登录页表单校验",
        baseRef: "main", headRef: "refactor/login-schema", headOid: "deadbeef", mergeable: "MERGEABLE", mergeState: "CLEAN",
        checks: [{ name: "CI / test (windows)", state: "pass", url: null }, { name: "CI / clippy", state: "pending", url: null }],
        checksSummary: "pending",
      },
      prDefaults: { title: "重构登录页表单校验", body: "", base: "main", draft: false },
      repoChoices: [], repoRemote: "origin", repoNeedsChoice: false,
    };
  }
  return undefined;
}

export { CHAT_SESSION_ID };

/// 截图页就绪信号：shoot.mjs 在推进假时钟时轮询它。
export function markReady(): void {
  (window as unknown as { __visualReady: boolean }).__visualReady = true;
}
