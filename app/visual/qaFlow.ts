// 补看场景（scene=flow-<名字>，见 qa-scenes.mjs）：对话窗里要靠特定后端返回才出现的状态。
// 都挂真实 ChatWindow，这里只决定各条 IPC 返回什么、挂载前往本地状态里放什么。
// 出现条件逐条对过源码（ChatWindow.tsx / LeadPanel.tsx / WorktreeSetupBar.tsx / terminalAttention.ts）。
import { CHAT_SESSION_ID, FIXED_NOW, leadTask, MENTION_CANDIDATES, MODE_MARKERS } from "./fixtures";
import { cardSceneMock } from "./cardScenes";
import { toolSceneMock } from "./toolScenes";
import { outgoingStore } from "../src/views/chat/outgoingPrompts";
import { demoChatUi } from "../src/demo/mock";
import { panelMock } from "../src/demo/panelData";
import { makeSession } from "../src/demo/data";
import { savePanel } from "../src/views/chat/panelTabs";

type Lang = "zh" | "en";
type Mock = (cmd: string, args: unknown) => unknown;
type Internals = { runCallback?: (id: number, data: unknown) => void; convertFileSrc?: (path: string) => string };
const internals = () => (window as unknown as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__;

/// 一张假截图（现画的 PNG data URL）：图片缩略图、灯箱与截图标注用。标注器靠 createImageBitmap 读图，SVG 读不出来。
function makeShot(): string {
  const canvas = document.createElement("canvas");
  canvas.width = 960; canvas.height = 600;
  const g = canvas.getContext("2d");
  if (!g) return "";
  const box = (x: number, y: number, w: number, h: number, fill: string) => { g.fillStyle = fill; g.beginPath(); g.roundRect(x, y, w, h, 10); g.fill(); };
  g.fillStyle = "#20242b"; g.fillRect(0, 0, 960, 600);
  box(40, 40, 880, 64, "#2c323b"); box(40, 130, 420, 430, "#2c323b"); box(490, 130, 430, 200, "#3a2a2a"); box(490, 360, 430, 200, "#2c323b");
  g.fillStyle = "#ff8580"; g.font = "28px monospace"; g.fillText("TypeError: x is undefined", 520, 240);
  return canvas.toDataURL("image/png");
}
const SHOT = makeShot();

const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

/// 托管终端的屏幕文本：屏幕识别的五种交互卡各一段（terminalAttention.ts 按这些字样认）。
const SCREENS: Record<string, string[]> = {
  trust: [
    "Do you trust the files in this folder?", "", "C:\\dev\\web-app", "",
    "Claude Code may read, write, or execute files contained in this directory.", "",
    "❯ 1. Yes, proceed", "  2. No, exit", "", "Enter to confirm · Esc to exit",
  ],
  command: [
    "Bash command", "", "  git push -u origin refactor/login-schema", "  Push the branch to origin", "",
    "Do you want to proceed?", "❯ 1. Yes", "  2. Yes, and don't ask again for git push commands in C:\\dev\\web-app",
    "  3. No, and tell Claude what to do differently (esc)",
  ],
  plan: [
    "Here is the plan:", "", "  1. Move the rules into src/schemas/login.ts", "  2. Replace the three inline checks", "  3. Add unit tests", "",
    "Claude has written up a plan and is ready to execute. Would you like to proceed?", "",
    "❯ 1. Yes, and bypass permissions", "  2. Yes, manually approve edits", "  3. Tell Claude what to change",
  ],
  question: [
    "Which validation library should we use?", "", "❯ 1. Zod", "     TypeScript-first, already in the project", "  2. Yup", "     Larger bundle",
    "  3. Type something.", "  4. Chat about this", "", "Enter to select · ↑/↓ to navigate · Esc to cancel",
  ],
  prompt: ["Invalid API key · Please run /login to sign in", "", "Press Enter to continue"],
};

const ATTENTION_PATTERNS = [
  { id: "claude:command-approval", patterns: ["this command requires approval", "do you want to proceed\\?"], last: true, details: "proceed_box", kind: "approval", confirms: null },
  { id: "claude:plan-approval", patterns: ["written\\s+up\\s+a\\s+plan\\s+and\\s+is\\s+ready\\s+to\\s+execute"], last: true, details: "none", kind: "plan", confirms: null },
];

function worktreeSetup(state: string): unknown {
  return {
    taskId: "wt-1", path: "C:/dev/web-app", state, exitCode: state === "failed" ? 1 : null,
    logTail: state === "failed"
      ? "$ bun install\n+ 312 packages installed [4.21s]\n$ bun run build\nerror: Could not resolve \"./generated/contracts\"\n    at src/api.ts:3:28\nerror: script \"build\" exited with code 1"
      : "$ bun install\n+ 312 packages installed [4.21s]\n$ bun run build",
    logPath: "C:\\Users\\demo\\.meowo\\worktree-setup\\wt-1.log",
    startedAt: FIXED_NOW - 42_000, finishedAt: state === "failed" ? FIXED_NOW - 2_000 : null, reason: null,
    script: "bun install\nbun run build\ncp ../web-app/.env.local .env.local", configHash: "9f2c1a7e", configSource: "meowo", runner: "pwsh",
    setupEnv: [["NODE_OPTIONS", "--max-old-space-size=4096"]],
  };
}

/// 挂载 ChatWindow 之前要做的事：本地排队消息、图片地址换算。installMocks 之后调。
export function qaFlowBoot(scene: string, lang: Lang): void {
  const zh = lang === "zh";
  const it = internals();
  // mockIPC 不提供 convertFileSrc：用户消息里的图片引用会抛错。假截图映射到一张内嵌 SVG。
  if (it) it.convertFileSrc = (path) => (path.endsWith("shot.png") ? SHOT : path);
  // 输入框里带一张图片附件（草稿恢复）：缩略图上的铅笔进截图标注。
  if (scene === "flow-annotate") {
    localStorage.setItem(`meowo-chat-draft:${CHAT_SESSION_ID}`, JSON.stringify({ prompt: "", attachments: [{ path: "C:/tmp/shot.png", name: "shot.png", image: true }], at: 0 }));
  }
  // 右侧面板的几种页签：文件（点开 Markdown）、浏览器（各种提示条）、终端。
  const panelTabs: Record<string, unknown[]> = {
    "flow-md": [{ key: "files", type: "files" }],
    "flow-browser": [{ key: "b1", type: "browser", paneId: 7, url: "http://localhost:5173/", title: "Vite" }],
    "flow-browser-blank": [{ key: "b1", type: "browser", paneId: 7, url: "about:blank", title: "" }],
    "flow-shell": [{ key: "t1", type: "term", shellId: 5, cwd: "C:/dev/web-app" }],
  };
  if (panelTabs[scene]) {
    const tabs = [{ key: "diff", type: "diff" }, ...panelTabs[scene]] as never[];
    // ?max=1：面板最大化（同 desktop.tsx 的 panel-* 场景），Markdown 预览最大化靠它拍。
    const expanded = new URLSearchParams(location.search).get("max") === "1";
    savePanel(CHAT_SESSION_ID, { tabs, active: (tabs[1] as { key: string }).key, visible: true, expanded } as never);
  }
  if (scene === "flow-queued") {
    // 排队消息只来自本地的 outgoingStore（history 里的 prompt_queued 只是回执）：发送中、已排队、失败各一条。
    const anchor = { lastItemId: "t6", lastItemAt: null, priorItemIds: new Set<string>(), priorUserText: null };
    const add = (text: string, phase: "sending" | "pending" | "failed") =>
      outgoingStore.add(CHAT_SESSION_ID, { kind: "queued", text, attachmentOnly: false, phase, anchor } as never);
    add(zh ? "跑完之后再开一个 PR。" : "Open a PR after that finishes.", "pending");
    add(zh ? "顺便把 README 里登录那一节也更新一下，旧的截图可以先删掉，等新界面定了我再补一张；另外迁移说明里要写清楚密码长度下限变了。" : "Also update the sign-in section of the README; drop the old screenshot for now, I will add a new one once the UI settles. The migration notes must mention the new minimum password length.", "sending");
    add(zh ? "这条没发出去" : "This one did not go out", "failed");
  }
}

/// 场景对应的 IPC 返回值；返回 undefined 表示这条不归它管。
export function qaFlowMock(scene: string, lang: Lang, chat: (cmd: string) => unknown): Mock {
  const zh = lang === "zh";
  const ts = new Date(FIXED_NOW).toISOString();
  const kind = scene.slice("flow-".length);
  const screenKind = kind.startsWith("screen-") ? kind.slice("screen-".length) : null;
  let historyCalls = 0;
  const base = () => chat("get_chat_history") as Record<string, unknown> & { items: unknown[] };
  const calm = { pendingReview: null, status: "waiting" };
  const panel = panelMock();
  const termText = (lines: string[]) => {
    const text = lines.join("\r\n");
    return { sessionId: CHAT_SESSION_ID, managed: true, data: b64(text), startOffset: 0, endOffset: new TextEncoder().encode(text).length,
      exitTail: null, cols: 120, rows: 40, modes: [], externalViewers: false, exitUnexpected: false };
  };
  // 卡片场景（flow-card-<名字>）：五家的审批卡、提问卡、提示卡，数据在 cardScenes.ts，手机 harness 共用。
  // 工具行场景（flow-tools-<家>-<状态>）：五家回放样本的时间线，数据在 toolScenes.ts，手机 harness 共用。
  const card = kind.startsWith("card-") ? cardSceneMock(kind.slice("card-".length), lang, chat)
    : kind.startsWith("tools-") ? toolSceneMock(kind.slice("tools-".length), lang, chat) : null;
  return (cmd, args) => {
    const cardHit = card?.(cmd, args);
    if (cardHit !== undefined) return cardHit;
    if (cmd === "file_mention_candidates") return MENTION_CANDIDATES;
    // @ 补全：桌面走目录列举与项目搜索。
    if (cmd === "list_dir_entries") return panel(cmd, args);
    if (cmd === "search_project_files") return { files: MENTION_CANDIDATES.filter((f) => !f.endsWith("/")).map((relPath) => ({ relPath, isDir: false, nameMatch: true, lines: [], totalLineMatches: 0 })), truncated: false };
    // 模型市场入口要当前 agent 能导出对话、且另有装好的 agent 可切。
    if (cmd === "list_agents") return [
      { id: "claude", display_name: "Claude Code", installed: true, supports_chat_export: true },
      { id: "codex", display_name: "Codex", installed: true }, { id: "kimi", display_name: "Kimi Code", installed: true },
    ];
    if (cmd === "list_folder_editors") return [{ id: "vscode", name: "Visual Studio Code", icon: null }, { id: "zed", name: "Zed", icon: null }];
    if (cmd === "git_diff_summary" || cmd === "git_file_diff" || cmd === "list_file_openers") return panel(cmd, args);
    if (cmd === "git_pull_request") return { number: 162, state: "open", url: "https://github.com/example/web-app/pull/162", title: zh ? "重构登录页表单校验" : "Refactor login form validation" };
    if (cmd === "rename_session" && kind === "rename-error") throw new Error("db locked");
    // 侧栏：空态、首载失败、全文搜索命中、清理 worktree、工作区页签。
    if (cmd === "get_live_sessions_page" && kind === "side-empty") return [];
    if (cmd === "get_live_sessions_page" && kind === "side-error") throw new Error("offline");
    if (cmd === "search_chat_transcripts") return [
      { sessionId: 2, title: zh ? "修支付回调的重复扣款" : "Fix duplicate charge in payment callback", projectName: "billing", excerpt: zh ? "…回调重试时没带幂等键，同一笔订单被扣了两次，schema 里要加唯一约束…" : "…the retry carried no idempotency key, so the order was charged twice; the schema needs a unique constraint…" },
      { sessionId: 3, title: zh ? "补齐导出接口的单测" : "Add tests for the export API", projectName: "api", excerpt: zh ? "…导出 schema 的字段顺序要固定…" : "…the export schema must keep a stable field order…" },
    ];
    if (cmd === "list_cleanable_worktrees") return [];
    if (cmd === "recent_cwds") return ["C:\\Users\\demo\\workspace\\web-app", "C:\\Users\\demo\\workspace\\billing-service", "D:\\code\\infra\\a-very-long-monorepo-name\\packages\\export-api"];
    if (cmd === "get_session_admin_page") {
      const titles = zh ? ["重构登录页表单校验", "修支付回调的重复扣款", "补齐导出接口的单测，顺带把超长标题在这一行里的省略也验一下看看效果到底怎么样", "升级依赖", "排查夜间构建失败"] : ["Refactor login form validation", "Fix duplicate charge in payment callback", "Add tests for the export API and check how a very long title is truncated in this row", "Upgrade dependencies", "Investigate the nightly build failure"];
      const items = titles.map((title, i) => {
        const one = makeSession({ title, project: ["web-app", "billing", "api", "web-app", "infra"][i], provider: (["claude", "codex", "kimi", "claude", "gemini"] as const)[i], status: "waiting", agoMin: 5 + i * 47 });
        one.session.id = 100 + i;
        if (i > 2) { one.archived = true; one.archived_at = FIXED_NOW - i * 3_600_000; }
        return one;
      });
      return { items, total: 42 };
    }
    // 右侧面板：Markdown 预览、浏览器提示条、终端。
    if (cmd === "read_file_text") return { relPath: "README.md", truncated: false, binary: false, text: zh
      ? "# web-app\n\n登录、注册与找回密码的前端。\n\n## 开发\n\n```bash\nbun install\nbun run dev\n```\n\n- 校验规则在 `src/schemas/`\n- 组件在 `src/components/`\n\n> 改校验规则前先看 [迁移说明](docs/migration.md)。\n\n| 命令 | 作用 |\n|---|---|\n| `bun run test` | 跑单测 |\n| `bun run build` | 打包 |\n"
      : "# web-app\n\nFront end for sign-in, sign-up and password reset.\n\n## Development\n\n```bash\nbun install\nbun run dev\n```\n\n- Validation rules live in `src/schemas/`\n- Components live in `src/components/`\n\n> Read the [migration notes](docs/migration.md) before changing a rule.\n\n| Command | What it does |\n|---|---|\n| `bun run test` | Unit tests |\n| `bun run build` | Bundle |\n" };
    if (cmd === "browser_pane_list") return [{ id: 7, owner: CHAT_SESSION_ID, url: kind === "browser-blank" ? "about:blank" : "http://localhost:5173/", title: "Vite", loading: false, agentActive: kind === "browser", agentPaused: false, zoom: 1,
      downloads: kind === "browser" ? [{ id: 1, fileName: "report-2026-11.pdf", state: "done" }] : [], permission: kind === "browser" ? { id: 3, kind: "camera", host: "localhost:5173" } : null, blocked: null }];
    if (cmd === "shell_terminal_list") return [{ id: 5, owner: CHAT_SESSION_ID, title: "pwsh", exited: false }];
    if (cmd === "shell_terminal_attach") {
      const id = (args as { onEvent?: { id?: number } })?.onEvent?.id;
      if (id != null) window.setTimeout(() => internals()?.runCallback?.(id, { index: 0, message: { kind: "output", data: b64("PS C:\\dev\\web-app> bun run test\r\n\r\n ✓ src/components/LoginForm.test.tsx (12)\r\n ✓ src/schemas/login.test.ts (8)\r\n\r\n Test Files  2 passed (2)\r\n      Tests  20 passed (20)\r\n\r\nPS C:\\dev\\web-app> "), replay: false } }), 200);
      return 0;
    }
    // 托管终端页签：有画面、退出后的居中卡、没启动。
    if (cmd === "managed_terminal_snapshot" && kind.startsWith("term")) {
      const lines = ["╭──────────────────────────────────────────╮", "│ ✻ Welcome to Claude Code                 │", "╰──────────────────────────────────────────╯", "", "> Refactor the login form validation", "", "● I'll start by reading the three components. See https://docs.example.com/schemas for the conventions.", "", "  Read(src/components/LoginForm.tsx)", "  ⎿  Read 182 lines", "", "> "];
      if (kind === "term-exited") return { ...termText(lines), active: false, exited: true, exitCode: 1, exitTail: "error: API key expired", exitUnexpected: true };
      if (kind === "term-clean") return { ...termText(lines), active: false, exited: true, exitCode: 0 };
      if (kind === "term-idle") return { ...termText([]), active: false, exited: false, exitCode: null };
      return { ...termText(lines), active: true, exited: false, exitCode: null };
    }
    if (cmd === "open_attached_terminal" && kind === "term-error") throw new Error("terminal/spawn_failed: wt.exe not found");
    if (cmd === "agent_chat_ui" && (args as { provider?: string })?.provider === "claude") return patchChatUi(demoChatUi());
    if (cmd === "lead_task_snapshot" && (kind === "lead" || kind === "lead-panel")) return leadTask(lang);
    if (cmd === "lead_task_snapshot" && kind === "lead-c") return leadTask(lang, "C");
    if (cmd === "worktree_setup" && kind.startsWith("wt-")) return worktreeSetup(kind === "wt-failed" ? "failed" : kind === "wt-trust" ? "needs_trust" : "running");
    if (cmd === "pending_interaction") {
      if (kind === "term-approval") return chat(cmd);
      if (kind === "broker" || kind === "broker-danger") {
        const approval = (chat(cmd) as { approval: Record<string, unknown> }).approval;
        return { question: null, approval: {
          ...approval, requestId: "qa-broker",
          input: JSON.stringify({ command: kind === "broker-danger" ? "rm -rf ./dist ./node_modules && git clean -fdx" : "git push -u origin refactor/login-schema" }),
          description: kind === "broker-danger" ? (zh ? "清掉构建产物和依赖" : "Remove build output and dependencies") : approval.description,
          permissionSuggestions: [{ type: "addRules", rules: [{ toolName: "Bash", ruleContent: "git push:*" }], behavior: "allow", destination: "localSettings" }],
        } };
      }
      return { approval: null, question: null };
    }
    if (cmd === "managed_terminal_snapshot" && screenKind && SCREENS[screenKind]) {
      const text = SCREENS[screenKind].join("\r\n");
      return { sessionId: CHAT_SESSION_ID, active: true, managed: true, data: b64(text), startOffset: 0, endOffset: new TextEncoder().encode(text).length,
        exited: false, exitCode: null, exitTail: null, cols: 120, rows: 40, modes: [], externalViewers: false, exitUnexpected: false };
    }
    if (cmd === "get_chat_history") {
      const h = base();
      if (kind === "broker" || kind === "broker-danger" || kind === "term-approval") return { ...h, pendingReview: "approval" };
      if (kind === "sync") {
        // 成功过一次之后连续失败三次以上：对话流顶上出红色的同步中断横幅。
        if (historyCalls++ === 0) return { ...h, ...calm };
        throw new Error("ipc down");
      }
      if (kind === "error") throw new Error("ipc down");
      if (kind === "loading") return new Promise(() => {});
      if (kind === "unsupported") return { ...h, ...calm, supported: false, items: [] };
      if (kind === "earlier") return { ...h, ...calm, hasMore: true, earliest: 12 };
      if (kind === "handoff") return { ...h, ...calm, predecessorId: 99 };
      if (kind === "image") {
        const items = [...h.items];
        items.splice(1, 0, { type: "user_text", id: "qa-img", timestamp: ts, text: (zh ? "这个报错是什么意思？" : "What does this error mean?") + "\n[Image #1: source: C:\\Users\\demo\\shot.png]" });
        return { ...h, ...calm, items };
      }
      if (screenKind) return { ...h, status: "waiting", pendingReview: screenKind === "question" ? "question" : screenKind === "plan" ? "plan" : null };
      if (kind === "queued") return h.pendingReview ? { ...h, pendingReview: null } : h;
      return { ...h, ...calm };
    }
    // 拖入文件：后端的拖放事件登记监听时回放一条「进入」。
    if (cmd === "plugin:event|listen" && kind === "drop") {
      const a = args as { event?: string; handler?: number };
      if (a.event === "tauri://drag-enter" && a.handler != null) {
        window.setTimeout(() => internals()?.runCallback?.(a.handler!, { event: a.event, id: 1, payload: { paths: ["C:\\Users\\demo\\shot.png"], position: { x: 400, y: 300 } } }), 300);
      }
      return 1;
    }
    return undefined;
  };
}

/// agent_chat_ui 的补丁：在默认 mock 那份上加屏幕识别规则与权限模式可选项。
export function patchChatUi(ui: Record<string, unknown>): unknown {
  const u = ui as Record<string, unknown> & { mode_controls?: Array<Record<string, unknown>> };
  return {
    ...u,
    attention_patterns: ATTENTION_PATTERNS,
    // 模型菜单要有切换命令才点得开；思考档位段要 effort。
    model_select_command: "/model",
    effort: { command: "/effort", levels: ["low", "medium", "high", "max"], confirm_pattern: "effort", reject_pattern: "invalid" },
    mode_controls: (u.mode_controls ?? []).map((c) => ({ ...c, screen_markers: MODE_MARKERS })),
  };
}
