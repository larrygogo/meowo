// 工具行与运行状态的场景：五家各一份时间线，桌面是 flow-tools-<家>-<状态>，手机是 mobile-tools-<家>-<状态>，
// 两边用同一份数据。
//
// 条目直接取五家的回放样本（src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/<家>.items.json，
// 本机真实会话经后端完整解析的产物，与 views/chat/replay.test.tsx 读的是同一份），这里不改内容，只做三件事：
//   1. 按状态截取一段（见 CUTS）：运行中、等待审批取「到某条命令为止、去掉它的回执」的前缀。
//   2. 重排时间戳。样本是从多次会话里拼的，原始时间互相错开几天，直接用会拍出「工作了 17181 分钟」。
//      每条调用到回执的间隔保留原值，其余按 2 秒一条往后排，末条落在冻结时刻前 30 秒。
//   3. 样本里没有的两类单独补，并在下面写明出处：待办（五家样本都没有）、Codex 的回合出错（本机没有真实记录）。
import type { ChatItem } from "../src/api";
import { chatUi } from "../src/test/agents";
import { cardSceneMock } from "./cardScenes";
import { FIXED_NOW } from "./fixtures";
import claudeItems from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/claude.items.json";
import codexItems from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/codex.items.json";
import geminiItems from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/gemini.items.json";
import kimiItems from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/kimi.items.json";
import opencodeItems from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.items.json";

type Lang = "zh" | "en";
export type ToolProvider = "claude" | "codex" | "kimi" | "gemini" | "opencode";
export const TOOL_PROVIDERS: ToolProvider[] = ["claude", "codex", "kimi", "gemini", "opencode"];
/// all：整份样本，回合已结束。running：最后一条命令还没回执、会话在跑。waiting：同一段，停在审批上。
/// stale：同一段，但会话已经停了（没回执的调用应显示「已中断」，不再计时）。error：以回合出错收尾。
/// todo：整份样本前插一条待办调用。
export type ToolState = "all" | "running" | "waiting" | "stale" | "error" | "todo";

const SAMPLES: Record<ToolProvider, ChatItem[]> = {
  claude: claudeItems as unknown as ChatItem[],
  codex: codexItems as unknown as ChatItem[],
  kimi: kimiItems as unknown as ChatItem[],
  gemini: geminiItems as unknown as ChatItem[],
  opencode: opencodeItems as unknown as ChatItem[],
};

/// 样本所在的工作目录（脱敏后的路径）：落在它下面的路径显示成相对路径，与真实会话一致。
const CWD: Record<ToolProvider, string> = {
  claude: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  codex: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  kimi: "C:\\Users\\user\\Desktop\\workspace\\meowo-1114-0850",
  gemini: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  opencode: "C:\\Users\\user\\Desktop\\workspace\\meowo",
};

/// 各家样本的截取位置（条目下标，含两端）。live：到一条「运行」调用为止，它的回执不带。
/// error：以 turn_error 结尾的那一段。codex 的 error 为 null：样本里没有，见 CODEX_TURN_ERROR。
const CUTS: Record<ToolProvider, { live: [number, number]; error: [number, number] | null }> = {
  claude: { live: [0, 11], error: [0, 16] },
  codex: { live: [0, 19], error: null },
  kimi: { live: [0, 13], error: [0, 17] },
  gemini: { live: [0, 12], error: [21, 22] },
  opencode: { live: [5, 19], error: [21, 22] },
};

/// Codex 的回合出错：本机 68 份 rollout 里没有一条带错收尾的回合（回放样本的缺口），这一条按
/// plugins/codex/telemetry.rs 单测 task_complete_error_becomes_turn_error 的记录形态与它的解析结果写：
/// codex_error_info=unauthorized → 标签「需要重新登录」，正文是 error.message。
const CODEX_TURN_ERROR = { type: "turn_error", id: "codex-error-unit", timestamp: null, label: "需要重新登录", text: "Your access token could not be refreshed." } as unknown as ChatItem;

/// 待办调用：五家的回放样本都没有这一类。工具名与摘要按各家插件的规则写，不是真实会话记录：
/// claude TodoWrite 的摘要是整包入参的紧凑 JSON（transcript.rs tool_summary 的兜底）；codex update_plan 取
/// explanation（telemetry.rs）；kimi TodoList、gemini write_todos、opencode todowrite 都退回整包入参。
const TODO_CALL: Record<ToolProvider, { name: string; summary: string }> = {
  claude: { name: "TodoWrite", summary: '{"todos":[{"content":"对齐两边的对比维度","status":"in_progress","activeForm":"对齐对比维度"},{"content":"写结论","status":"pending","activeForm":"写结论"}]}' },
  codex: { name: "update_plan", summary: "先落盘关卡 B 的决定，再补探针" },
  kimi: { name: "TodoList", summary: '{"todos":[{"title":"读 RuntimeRequest 的现状","status":"done"},{"title":"写垂直切片方案","status":"in_progress"}]}' },
  gemini: { name: "write_todos", summary: '{"todos":[{"description":"读 NewSessionPanel","status":"completed"},{"description":"改样式","status":"in_progress"}]}' },
  opencode: { name: "todowrite", summary: '{"todos":[{"content":"run ping","status":"completed","priority":"high"},{"content":"say DONE1","status":"pending","priority":"medium"}]}' },
};

function restamp(items: ChatItem[]): ChatItem[] {
  const startOf = new Map<string, number>();
  for (const item of items) if (item.type === "tool_use") startOf.set(item.id, Date.parse(item.timestamp ?? ""));
  const offsets: number[] = [];
  let cursor = 0;
  const newStart = new Map<string, number>();
  for (const item of items) {
    if (item.type === "tool_result" && item.tool_use_id && newStart.has(item.tool_use_id)) {
      const began = startOf.get(item.tool_use_id) ?? NaN;
      const ended = Date.parse(item.timestamp ?? "");
      const took = Number.isFinite(began) && Number.isFinite(ended) ? Math.max(0, ended - began) : 0;
      cursor = Math.max(cursor, newStart.get(item.tool_use_id)! + took);
    } else {
      cursor += 2000;
      if (item.type === "tool_use") newStart.set(item.id, cursor);
    }
    offsets.push(cursor);
  }
  const base = FIXED_NOW - 30_000 - cursor;
  return items.map((item, index) => ({ ...item, timestamp: new Date(base + offsets[index]).toISOString() }) as ChatItem);
}

export function toolSceneItems(provider: ToolProvider, state: ToolState): ChatItem[] {
  const all = SAMPLES[provider];
  const cut = CUTS[provider];
  if (state === "all") return restamp(all);
  if (state === "todo") {
    const at = all.findIndex((item) => item.type === "tool_use");
    const call = { type: "tool_use", id: `visual-todo-${provider}`, timestamp: all[at].timestamp, kind: "todo", detail: null, ...TODO_CALL[provider] } as unknown as ChatItem;
    const receipt = { type: "tool_result", id: `visual-todo-r-${provider}`, timestamp: all[at].timestamp, tool_use_id: `visual-todo-${provider}`, text: "", is_error: false } as unknown as ChatItem;
    return restamp([...all.slice(0, at), call, receipt, ...all.slice(at)]);
  }
  if (state === "error") {
    if (cut.error) return restamp(all.slice(cut.error[0], cut.error[1] + 1));
    // codex：真实样本的前半段（到第一段正文为止）之后接那条按单测形态写的出错记录。
    const upto = all.findIndex((item) => item.type === "assistant_text");
    return restamp([...all.slice(0, upto + 1), CODEX_TURN_ERROR]);
  }
  return restamp(all.slice(cut.live[0], cut.live[1] + 1));
}

export function parseToolScene(name: string): { provider: ToolProvider; state: ToolState } | null {
  const hit = /^(claude|codex|kimi|gemini|opencode)-(all|running|waiting|stale|error|todo)$/.exec(name);
  return hit ? { provider: hit[1] as ToolProvider, state: hit[2] as ToolState } : null;
}

/// 工具行场景的 IPC 返回值；返回 undefined 表示这条不归它管。name 认不出时整体不接管。
export function toolSceneMock(name: string, lang: Lang, chat: (cmd: string) => unknown): (cmd: string, args: unknown) => unknown {
  const scene = parseToolScene(name);
  if (!scene) return () => undefined;
  const { provider, state } = scene;
  const items = toolSceneItems(provider, state);
  // 等待审批：审批请求沿用卡片场景那一份（五家各自的真实样本），这里只换时间线。
  const card = state === "waiting" ? cardSceneMock(`${provider}-approval`, lang, chat) : null;
  return (cmd, args) => {
    if (cmd === "get_chat_history") {
      const base = (card?.(cmd, args) ?? chat(cmd)) as Record<string, unknown>;
      const live = state === "running" || state === "waiting";
      return {
        ...base, provider, cwd: CWD[provider], ptyManaged: true, items, todos: [], model: null,
        status: live ? "running" : "waiting",
        pendingReview: state === "waiting" ? base.pendingReview ?? null : null,
        // 屏幕通道的审批（kimi、gemini、opencode）不经 broker，会话记录不报 pendingReview；真实后端此时由 PTY 的
        // 屏幕检测报 screenState=blocked（session_query.rs），会话状态同样落到「待处理」。
        screenState: state === "waiting" && !base.pendingReview ? "blocked" : null, screenAssumed: false,
        errored: state === "error", connected: true, lastUserText: null, lastAiText: null,
      };
    }
    const hit = card?.(cmd, args);
    if (cmd === "pending_interaction" && hit) {
      // broker 审批（claude、codex）的请求带命令本身：换成时间线上等着批准的那条命令，卡片与工具行说的才是
      // 同一件事。屏幕通道（kimi、gemini、opencode）的卡片内容来自终端画面样本，画面是整屏取证的，不改。
      const pending = [...items].reverse().find((item) => item.type === "tool_use");
      const command = pending?.type === "tool_use" && pending.detail?.kind === "command" ? pending.detail.command : null;
      const approval = (hit as { approval?: { input?: string } | null }).approval;
      if (command && approval?.input?.startsWith("{")) {
        return { ...(hit as object), approval: { ...approval, description: null, permissionSuggestions: [], input: JSON.stringify({ command }) } };
      }
    }
    if (hit !== undefined) return hit;
    if (cmd === "agent_chat_ui") return (args as { provider?: string })?.provider === provider ? chatUi(provider) : null;
    if (cmd === "pending_interaction") return { approval: null, question: null };
    return undefined;
  };
}
