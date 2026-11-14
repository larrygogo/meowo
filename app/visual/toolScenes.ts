// 工具行与运行状态的场景：五家各一份时间线，桌面是 flow-tools-<家>-<状态>，手机是 mobile-tools-<家>-<状态>，
// 两边用同一份数据。
//
// 条目直接取五家的回放样本（src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/<家>.<会话>.items.json，
// 本机真实会话经后端完整解析的产物，与 views/chat/replay.test.tsx 读的是同一份）。每个场景只用**一场会话**的
// 片段、保持原顺序，这里不改内容，只做三件事：
//   1. 按状态截取一段：运行中、等待审批取「那条命令所在的一轮开头 → 那条命令」，去掉它的回执。
//   2. 把时间平移到冻结时刻附近。样本是长会话里裁出来的片段，两条之间可能隔着几小时，演示页的时钟又是冻结的，
//      原样用会拍出「工作了 196 分钟」。每条调用到回执的间隔保留原值，其余按 2 秒一条往后排，末条落在冻结时刻前 30 秒。
//   3. 样本里没有的单独补，并在下面写明出处：待办（五家样本都没有）、Codex 的回合出错（本机没有真实记录）、
//      claude 与 kimi 回合中途的那句话（样本里没有，按后端产出的形态）。
import type { ChatItem } from "../src/api";
import { chatUi } from "../src/test/agents";
import { cardSceneMock } from "./cardScenes";
import { FIXED_NOW } from "./fixtures";
import claudeMain from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/claude.152e4c8e.items.json";
import claudeError from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/claude.15a3c5a2.items.json";
import claudeAgent from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/claude.e903be8f.items.json";
import codexMain from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/codex.01a1cea4.items.json";
import geminiMain from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/gemini.1b135d85.items.json";
import geminiError from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/gemini.882f0730.items.json";
import kimiMain from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/kimi.658d0ccd.items.json";
import kimiError from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/kimi.aa711c07.items.json";
import opencodeMain from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.bzQ2Bs.items.json";
import opencodeError from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.u5cG8m.items.json";
import opencodeMidTurn from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.KbLGZo.items.json";

type Lang = "zh" | "en";
export type ToolProvider = "claude" | "codex" | "kimi" | "gemini" | "opencode";
export const TOOL_PROVIDERS: ToolProvider[] = ["claude", "codex", "kimi", "gemini", "opencode"];
/// all：一场会话的片段，回合已结束。running：最后一条命令还没回执、会话在跑。waiting：同一段，停在审批上。
/// stale：同一段，但会话已经停了（没回执的调用应显示「已中断」，不再计时）。error：以回合出错收尾。
/// todo：整份样本前插一条待办调用。
/// midturn：运行中又发了一条——命令还在跑，后面跟着回合中途送进去的那句话（做空 ③ 的场景）。
/// agents：子任务块（claude、kimi、codex）。agents-running：子任务刚派出、还在跑（codex）。
export type ToolState = "all" | "running" | "waiting" | "stale" | "error" | "todo" | "midturn" | "agents" | "agents-running";

const items = (json: unknown) => json as ChatItem[];
type Sample = {
  /** 主样本：一场会话的原顺序片段。 */
  main: ChatItem[];
  /** 运行中、等待审批用的那条命令（摘要里含这段文字）；场景取「它所在的那一轮开头 → 它」，不带回执。 */
  live: string;
  /** 以回合出错收尾的那场会话；codex 没有，见 CODEX_TURN_ERROR。 */
  error: ChatItem[] | null;
  /** 子任务块所在的会话片段。gemini、opencode 本机没有派过子任务。 */
  agents: ChatItem[] | null;
};
const SAMPLES: Record<ToolProvider, Sample> = {
  claude: { main: items(claudeMain), live: "gh issue view", error: items(claudeError), agents: items(claudeAgent) },
  codex: { main: items(codexMain), live: "Get-Content app/src-tauri/CLAUDE.md", error: null, agents: items(codexMain) },
  kimi: { main: items(kimiMain), live: "cargo test", error: items(kimiError), agents: items(kimiMain) },
  gemini: { main: items(geminiMain), live: "npm test", error: items(geminiError), agents: null },
  opencode: { main: items(opencodeMain), live: "node --version", error: items(opencodeError), agents: null },
};

/// 样本所在的工作目录（脱敏后的路径）：落在它下面的路径显示成相对路径，与真实会话一致。
const CWD: Record<ToolProvider, string> = {
  claude: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  codex: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  kimi: "C:\\Users\\user\\Desktop\\workspace\\meowo-1114-0850",
  gemini: "C:\\Users\\user\\Desktop\\workspace\\meowo",
  opencode: "C:\\Users\\user\\Desktop\\workspace\\meowo",
};

/// 运行中的那一段：从那条命令所在的那一轮开头（往回数最近的一条用户消息；没有就从头）到那条命令，
/// 它的回执不带。
function liveSlice(sample: Sample): ChatItem[] {
  const at = sample.main.findIndex((item) => item.type === "tool_use" && item.summary.includes(sample.live));
  let from = at;
  while (from > 0 && sample.main[from].type !== "user_text") from -= 1;
  return sample.main.slice(from, at + 1);
}

/// 回合中途送进去的那句话，各按自己记录的形态：claude 的 queued_command、kimi 的 origin.inTurn 由后端标成
/// interjected；codex 的写法与回合开头那条相同，turn_id 也相同（本机 15 次都是）。
/// claude、kimi 的样本里没有这样一句，文字是这里写的；codex 用样本里真实的那一句（会话 01a1cea4）。
function midTurnMessage(provider: ToolProvider, before: ChatItem[]): ChatItem {
  const base = { type: "user_text", id: `visual-mid-${provider}`, timestamp: null, text: "顺便把日志也看一下" };
  if (provider === "codex") {
    const opener = [...before].reverse().find((item) => item.type === "user_text" && !!item.turn_id);
    const real = SAMPLES.codex.main.find((item) => item.type === "user_text" && item.text.includes("其他的provider的兼容都很差"));
    return { ...base, text: real?.type === "user_text" ? real.text : base.text, turn_id: opener?.type === "user_text" ? opener.turn_id : undefined } as ChatItem;
  }
  return { ...base, interjected: true } as ChatItem;
}

/// Codex 的回合出错：本机 98 份 rollout 里没有一条带错收尾的回合（回放样本的缺口），这一条按
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

function restamp(source: ChatItem[]): ChatItem[] {
  const startOf = new Map<string, number>();
  for (const item of source) if (item.type === "tool_use") startOf.set(item.id, Date.parse(item.timestamp ?? ""));
  const offsets: number[] = [];
  let cursor = 0;
  const newStart = new Map<string, number>();
  for (const item of source) {
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
  return source.map((item, index) => ({ ...item, timestamp: new Date(base + offsets[index]).toISOString() }) as ChatItem);
}

export function toolSceneItems(provider: ToolProvider, state: ToolState): ChatItem[] {
  const sample = SAMPLES[provider];
  const all = sample.main;
  if (state === "all") return restamp(all);
  if (state === "todo") {
    const at = all.findIndex((item) => item.type === "tool_use");
    const call = { type: "tool_use", id: `visual-todo-${provider}`, timestamp: all[at].timestamp, kind: "todo", detail: null, ...TODO_CALL[provider] } as unknown as ChatItem;
    const receipt = { type: "tool_result", id: `visual-todo-r-${provider}`, timestamp: all[at].timestamp, tool_use_id: `visual-todo-${provider}`, text: "", is_error: false } as unknown as ChatItem;
    return restamp([...all.slice(0, at), call, receipt, ...all.slice(at)]);
  }
  if (state === "error") {
    if (sample.error) return restamp(sample.error);
    // codex：真实样本里「继续」那一轮的开头（到第一段正文为止）之后接那条按单测形态写的出错记录。
    const from = all.findIndex((item) => item.type === "user_text" && item.text === "继续");
    const upto = all.findIndex((item, index) => index > from && item.type === "assistant_text");
    return restamp([...all.slice(from, upto + 1), CODEX_TURN_ERROR]);
  }
  if (state === "midturn") {
    // opencode：真实的一场（KbLGZo）——ping 还在跑时用户又发了一句，记录里它排在跑完的 ping 之后。
    // gemini 的调用连同结果一起落盘，不存在「命令在跑、后面又来了一条用户消息」的时间线，退回运行中那一段。
    if (provider === "opencode") {
      const real = items(opencodeMidTurn);
      return restamp(real.slice(0, real.findIndex((item) => item.type === "user_text" && item.text.includes("QUEUED-OK")) + 1));
    }
    const live = liveSlice(sample);
    return restamp(provider === "gemini" ? live : [...live, midTurnMessage(provider, live)]);
  }
  if (state === "agents" || state === "agents-running") {
    const source = sample.agents ?? all;
    if (provider !== "codex") return restamp(source);
    // codex：会话 01a1cea4 里「- 完整闭环」那一轮——派出两个子代理，各自跑完，之后其中一个被叫停。
    const from = source.findIndex((item) => item.type === "user_text" && item.text === "- 完整闭环");
    const until = state === "agents-running"
      // 两个都刚开始（started 已到，completed 还没到）。
      ? source.findIndex((item) => item.type === "tool_result" && item.subagent?.task_id === "/root/lead_design_two" && item.subagent.running === 1) + 1
      : source.findIndex((item) => item.type === "user_text" && item.text === "继续");
    return restamp(source.slice(from, until));
  }
  return restamp(liveSlice(sample));
}

export function parseToolScene(name: string): { provider: ToolProvider; state: ToolState } | null {
  const hit = /^(claude|codex|kimi|gemini|opencode)-(all|running|waiting|stale|error|todo|midturn|agents-running|agents)$/.exec(name);
  return hit ? { provider: hit[1] as ToolProvider, state: hit[2] as ToolState } : null;
}

/// 工具行场景的 IPC 返回值；返回 undefined 表示这条不归它管。name 认不出时整体不接管。
export function toolSceneMock(name: string, lang: Lang, chat: (cmd: string) => unknown): (cmd: string, args: unknown) => unknown {
  const scene = parseToolScene(name);
  if (!scene) return () => undefined;
  const { provider, state } = scene;
  const timeline = toolSceneItems(provider, state);
  // 等待审批：审批请求沿用卡片场景那一份（五家各自的真实样本），这里只换时间线。
  const card = state === "waiting" ? cardSceneMock(`${provider}-approval`, lang, chat) : null;
  return (cmd, args) => {
    if (cmd === "get_chat_history") {
      const base = (card?.(cmd, args) ?? chat(cmd)) as Record<string, unknown>;
      const live = state === "running" || state === "waiting" || state === "midturn" || state === "agents-running";
      return {
        ...base, provider, cwd: CWD[provider], ptyManaged: true, items: timeline, todos: [], model: null,
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
      const pending = [...timeline].reverse().find((item) => item.type === "tool_use");
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
