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

/// ── 对话流各种状态（重设计时补的场景，scene=flow-*）────────────────────────────
/// 一段会话里把 harness 原先拍不到的条目都摆出来：长用户消息（渐隐）、跨会话消息、思考、
/// 带 diff 的编辑卡、带输出的命令卡、子任务、Markdown 全家桶、回合报错、压缩分隔条、排队消息。
export function flowStateItems(lang: "zh" | "en", ts: string): unknown[] {
  const zh = lang === "zh";
  const longAsk = (zh
    ? "把登录页的校验规则全部收进一个 schema。下面是我现在的想法，逐条看一下有没有问题：\n"
    : "Move every login validation rule into one schema. My current thinking, check each point:\n")
    + Array.from({ length: 14 }, (_, i) => (zh ? `${i + 1}. 第 ${i + 1} 条规则要保留原来的报错文案，并且补一条单测。` : `${i + 1}. Rule ${i + 1} keeps its original error copy and gets a unit test.`)).join("\n");
  const cross = "Another Claude session sent a message:\n<cross-session-message from=\"uds:pipe\" from-name=\"" + (zh ? "复核会话" : "Review session") + "\" from-mode=\"bypass\">"
    + (zh ? "复核完了：`login.ts` 第 12 行的正则漏了加号别名，其余没问题。" : "Review done: the regex on line 12 of `login.ts` misses plus aliases, the rest is fine.")
    + "</cross-session-message>";
  const md = [
    zh ? "## 改动小结" : "## Summary",
    "",
    zh ? "规则收进了 `src/schemas/login.ts`，细节见 [PR #162](https://github.com/example/web-app/pull/162)。" : "Rules now live in `src/schemas/login.ts`, see [PR #162](https://github.com/example/web-app/pull/162).",
    "",
    zh ? "### 还要注意" : "### Watch out",
    "",
    zh ? "- **密码长度**下限从 6 提到 8\n- 邮箱校验改用 `z.string().email()`\n- [ ] 文档还没更新\n- [x] 单测已补" : "- **Password length** minimum raised from 6 to 8\n- Email check now uses `z.string().email()`\n- [ ] Docs not updated yet\n- [x] Unit tests added",
    "",
    fence + "ts",
    "// 登录表单的唯一校验来源",
    "export const loginSchema = z.object({",
    "  email: z.string().email(),",
    "  password: z.string().min(8).max(128),",
    "  remember: z.boolean().default(false),",
    "});",
    "export type LoginInput = z.infer<typeof loginSchema>;",
    fence,
    "",
    zh ? "> 旧的 `isEmail` 工具函数可以删，没有别处引用。" : "> The old `isEmail` helper can go, nothing else imports it.",
    "",
    "---",
    "",
    zh ? "| 文件 | 增 | 删 |\n|---|---:|---:|\n| login.ts | 18 | 0 |\n| LoginForm.tsx | 6 | 21 |" : "| File | Added | Removed |\n|---|---:|---:|\n| login.ts | 18 | 0 |\n| LoginForm.tsx | 6 | 21 |",
  ].join("\n");
  return [
    { type: "user_text", id: "fs-u1", timestamp: ts, text: longAsk },
    { type: "reasoning", id: "fs-th", timestamp: ts, text: zh ? "先确认三个组件里各自的规则有没有冲突，再决定 schema 的字段。" : "First check whether the three components disagree, then decide the schema fields." },
    { type: "assistant_text", id: "fs-a1", timestamp: ts, text: zh ? "我先改 `LoginForm.tsx`，再跑一遍测试。" : "I'll edit `LoginForm.tsx` first, then run the tests." },
    { type: "tool_use", id: "fs-t1", timestamp: ts, name: "Edit", summary: "src/components/LoginForm.tsx",
      detail: { kind: "edit", path: "src/components/LoginForm.tsx", old: "const next: Errors = {};", new: "const result = validateLogin(values);", replace_all: false, truncated: false } },
    { type: "tool_result", id: "fs-r1", timestamp: ts, tool_use_id: "fs-t1", text: "ok", is_error: false,
      patch: [{ old_start: 5, new_start: 5, lines: [
        " const submit = (values: FormValues) => {",
        "-  const next: Errors = {};",
        "-  if (!values.email.trim()) next.email = \"required\";",
        "+  const result = validateLogin(values);",
        "+  if (!result.ok) return setErrors(result.errors);",
        "   onSubmit(result.data);",
        " };",
      ] }] },
    { type: "tool_use", id: "fs-t2", timestamp: ts, name: "Bash", summary: "FORCE_COLOR=0 npx vitest run src/components --reporter=dot | tee \"out.log\"",
      detail: { kind: "command", command: "FORCE_COLOR=0 npx vitest run src/components --reporter=dot | tee \"out.log\"", description: zh ? "跑组件测试" : "Run component tests" } },
    { type: "tool_result", id: "fs-r2", timestamp: ts, tool_use_id: "fs-t2", text: " ✓ LoginForm.test.tsx (12)\n ✓ schema.test.ts (8)\n\n Test Files  2 passed (2)\n      Tests  20 passed (20)\nwarning: 1 deprecated API", is_error: false },
    { type: "tool_use", id: "fs-t3", timestamp: ts, name: "Agent", summary: zh ? "复核 schema 的边界情况" : "Review schema edge cases",
      subagent: { description: zh ? "复核 schema 的边界情况" : "Review schema edge cases", agent_type: "Explore", count: 1 } },
    { type: "tool_result", id: "fs-r3", timestamp: ts, tool_use_id: "fs-t3", text: "done", is_error: false, subagent: { running: 0, completed: 1, failed: 0 } },
    { type: "assistant_text", id: "fs-a2", timestamp: ts, text: md },
    { type: "user_text", id: "fs-u2", timestamp: ts, text: cross },
    { type: "meta", id: "fs-m1", timestamp: ts, kind: "compact", compaction: { trigger: "auto", pre_tokens: 182000, summary: null } },
    { type: "user_text", id: "fs-u3", timestamp: ts, text: zh ? "顺手把文档也更新了。" : "Update the docs while you're at it." },
    { type: "tool_use", id: "fs-t4", timestamp: ts, name: "Agent", summary: zh ? "更新 README 与迁移说明" : "Update README and migration notes",
      subagent: { description: zh ? "更新 README 与迁移说明" : "Update README and migration notes", agent_type: "general-purpose", count: 2 } },
    { type: "turn_error", id: "fs-e1", timestamp: ts, label: zh ? "服务过载" : "Overloaded", text: "API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}" },
    { type: "prompt_queued", id: "fs-q1", timestamp: ts, text: zh ? "跑完之后再开一个 PR。" : "Open a PR after that finishes." },
  ];
}

/// AskUserQuestion 题面（scene=flow-question）：一道单选带说明、一道多选。
export function questionInput(lang: "zh" | "en"): string {
  const zh = lang === "zh";
  return JSON.stringify({ questions: [
    { header: zh ? "校验库" : "Library", question: zh ? "schema 用哪个校验库？" : "Which validation library for the schema?", multiSelect: false, options: [
      { label: "Zod", description: zh ? "项目里已经在用，类型推导最好" : "Already in the project, best type inference" },
      { label: "Valibot", description: zh ? "体积更小，但要新增依赖" : "Smaller, but a new dependency" },
      { label: "Yup", description: null },
    ] },
    { header: zh ? "范围" : "Scope", question: zh ? "哪些表单一起迁移？" : "Which forms should migrate too?", multiSelect: true, options: [
      { label: zh ? "注册" : "Sign-up", description: null },
      { label: zh ? "找回密码" : "Password reset", description: null },
    ] },
  ] });
}

/// ── 补看场景共用的假数据（qa-scenes.mjs 里的场景，桌面与手机 harness 都用）──────────────
/// @ 补全的候选（file_mention_candidates）：目录以 / 结尾，带一条超长路径看省略。
export const MENTION_CANDIDATES = [
  "src/", "src/schemas/login.ts", "src/components/LoginForm.tsx", "src/components/LoginForm.test.tsx",
  "packages/frontend/src/components/authentication/forms/LoginFormWithValidation.tsx", "README.md",
];

/// Lead 任务快照（lead_task_snapshot）：默认一张待作答的 B 类关卡，摘要够长（会出「展开」），三个选项带推荐；
/// kind=C 换成验收关卡（固定的「驳回 / 验收通过」两颗按钮）。SPEC 标题和进度给面板的任务名、最新进展用。
export function leadTask(lang: "zh" | "en", kind: "B" | "C" = "B"): unknown {
  const zh = lang === "zh";
  const verify = kind === "C";
  return {
    slug: "login-schema", phase: verify ? "verify" : "design", decisions: "",
    spec: zh ? "# 统一登录表单校验（login-schema）\n\n## 目的\n三个表单的校验规则对不上。" : "# Unify sign-in form validation (login-schema)\n\n## Goal\nThree forms drifted apart.",
    progress: zh
      ? `phase: ${verify ? "verify" : "design"}\n- 对比了三个表单的校验规则，差异记进 decisions.md\n- ${verify ? "做空第 2 轮：可以交付，两条低危已修" : "出了三个方案，做空在挑刺"}`
      : `phase: ${verify ? "verify" : "design"}\n- Compared the three forms' rules; diffs are in decisions.md\n- ${verify ? "Red team round 2: ready to ship, two low findings fixed" : "Three options drafted; the red team is poking holes"}`,
    checks: [
      { id: "c1", check: zh ? "12 个测试全部通过" : "All 12 tests pass", passes: true, waived: null, leader: false },
      { id: "c2", check: zh ? "PR 已开并通过 CI" : "PR opened and CI green", passes: false, waived: null, leader: true },
    ],
    gate: verify ? {
      id: "g2", kind: "C", status: "pending", answer: null, options: [],
      title: zh ? "登录校验统一完成" : "Sign-in validation unified",
      summary: zh
        ? "**怎么上手试**\n- 构建：分支 `refactor/login-schema`，重启 dev 实例\n- 打开登录页，邮箱填 `a@`，点登录\n- 应该看到：输入框下方提示「邮箱格式不对」，三个页面文案一致\n\n**结果**：12 个测试全过，三个表单共用 `src/schemas/login.ts`。"
        : "**How to try it**\n- Build: branch `refactor/login-schema`, restart the dev instance\n- Open sign-in, enter `a@`, press Sign in\n- Expect: \"Invalid email\" under the field, same copy on all three pages\n\n**Result**: all 12 tests pass; the three forms share `src/schemas/login.ts`.",
    } : {
      id: "g1", kind: "B", status: "pending", answer: null,
      title: zh ? "校验规则放前端还是后端" : "Validate on the client or the server",
      summary: zh
        ? "三个表单各写了一份校验，规则已经对不上。两种收法：\n\n- **前端 schema 复用**：规则收进 `src/schemas/login.ts`，三个表单共用，改动小、今天能完\n- **后端统一校验**：前端只做格式提示，以接口返回为准，更稳但要改接口\n- **两边都做**：前端先拦一遍，后端兜底\n\n我的建议是先做前端复用，后端兜底另开一个任务。\n\n影响面：登录、注册、找回密码三个页面；不动接口契约。"
        : "Three forms each carry their own validation and the rules have drifted. Options:\n\n- **Shared client schema**: move the rules into `src/schemas/login.ts`, small change, done today\n- **Server-side validation**: the client only hints, the API decides; sturdier but changes the API\n- **Both**: client first, server as the backstop\n\nI suggest the shared client schema now and a follow-up for the server.\n\nScope: sign-in, sign-up and password reset; the API contract stays.",
      options: [
        { id: "a", label: zh ? "前端 schema 复用" : "Shared client schema", detail: zh ? "改动小，今天能完" : "Small change, done today", recommended: true },
        { id: "b", label: zh ? "后端统一校验" : "Server-side validation", detail: zh ? "更稳，但要改接口" : "Sturdier, but changes the API", recommended: false },
        { id: "c", label: zh ? "两边都做" : "Both", detail: null, recommended: false },
      ],
    },
  };
}

/// 权限模式有可选项时（agent_chat_ui.mode_controls 带屏幕标记），点权限按钮才出菜单而不是直接轮换。
export const MODE_MARKERS = [
  { marker: "accept edits on", value: "acceptEdits" },
  { marker: "plan mode on", value: "plan" },
  { marker: "bypass permissions on", value: "bypassPermissions" },
];
