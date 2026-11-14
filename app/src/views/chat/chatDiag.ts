/**
 * 对话窗的运行日志（验收 19）：发送失败、未确认送达、切模型或切模式未确认、卡片 8 秒未认出，
 * 各写一行进 ~/.meowo/logs/meowo.log（经已有的 diag_log 命令，安装版落盘、dev 打在终端）。
 *
 * 为什么要有：这些事用户实际撞上过很多次，日志里却一条都没有，只能靠截图才知道发生过
 * （research-real-usage 第五节）。现在每一行都带 Provider、会话 id 和原因，事后 grep 得到。
 *
 * 不写什么：消息正文、终端画面上的文字、密钥。原因只用固定的代码或后端错误码；
 * 需要画面形状时（卡片没认出要看表单长什么样）只记去掉文字后的轮廓（maskScreenLine）。
 *
 * 手机远程页不写：远程桥不放行 diag_log（它往宿主磁盘写），调用在这里统一跳过。
 */
import { useEffect, useRef } from "react";
import { diagLog } from "../../api";
import { backendErrorCode } from "../../i18n/errors";
import type { Dict } from "../../i18n/zh";
import { remoteUi } from "../../remoteMode";
import type { OutgoingPhase, OutgoingPrompt } from "./outgoingPrompts";

export type ChatDiagKind =
  /** 消息没能写进终端。 */
  | "send_failed"
  /** 发送被拦下、一个字也没写进终端（终端输入框没收拾好、有卡片在等、会话在外部终端里…）。 */
  | "send_blocked"
  /** 撤回没成或没确认。 */
  | "undo_failed"
  /** 错误条上显示了一句失败说明，而它不属于上面任何一类（兜底，保证显示过的失败日志里都有）。 */
  | "error_shown"
  /** 写进去了，但没等到它出现在对话记录里。 */
  | "send_unconfirmed"
  /** 切模型、思考档或模式：命令发了，没等到 CLI 的确认。 */
  | "switch_unconfirmed"
  /** 切换明确没成（CLI 拒绝、后端报错、排队过期）。 */
  | "switch_failed"
  /** 提问卡或审批卡出现后，终端里对应的表单一直没被认出来。 */
  | "card_unrecognized";

export interface ChatDiagEvent {
  kind: ChatDiagKind;
  /** 原因：固定代码（如 echo_timeout）或后端错误码。不放消息正文。 */
  reason: string;
  /** 附加的键值（哪一栏、从哪个阶段来…）。值同样会过滤。 */
  detail?: Record<string, string | number | boolean | null | undefined>;
}

const SCOPE = "chat";
const MAX_VALUE = 120;
const SECRET = /\b(?:sk|pk|rk|ghp|gho|ghu|ghs|xox[abprs])[-_][A-Za-z0-9_-]{8,}|\bBearer\s+\S+|[A-Za-z0-9+/_=-]{32,}/gi;

/** 值过滤：压成一行、隐去像密钥的串、截到 120 字。 */
export function safeDiagValue(value: unknown): string {
  const flat = String(value ?? "").replace(/\s+/g, " ").trim().replace(SECRET, "[已隐去]");
  return flat.length > MAX_VALUE ? `${flat.slice(0, MAX_VALUE)}…` : flat;
}

/**
 * 终端画面的一行 → 只剩轮廓：字母和汉字换成 x，数字换成 9，标点、框线、空白原样。
 * 认卡片要看的是结构（`❯ 9. xxxx`、`[ ] xxx`、`│ … │`），不需要也不该留下具体文字。
 */
export function maskScreenLine(line: string): string {
  return line.replace(/\p{L}/gu, "x").replace(/\p{N}/gu, "9");
}

/** 一行日志的正文（不含 diag_log 自己补的时间戳与 `[ui:chat]` 前缀）。 */
export function chatDiagLine(sessionId: number, provider: string | null | undefined, event: ChatDiagEvent): string {
  const parts = [
    `kind=${event.kind}`,
    `provider=${safeDiagValue(provider || "unknown")}`,
    `session=${sessionId}`,
    `reason=${safeDiagValue(event.reason) || "unknown"}`,
  ];
  for (const [key, value] of Object.entries(event.detail ?? {})) {
    if (value === undefined || value === null || value === "") continue;
    parts.push(`${key}=${safeDiagValue(value)}`);
  }
  return parts.join(" · ");
}

// 会话 id → Provider。切换动作分散在几个模块里（useModelControls、useSessionSettings），它们手上
// 只有会话 id；对话窗在会话确定时登记一次，写日志时按 id 查。
const providers = new Map<number, string>();

/** 对话窗登记当前会话的 Provider（临时负 id 认领成真 id 后会再登记一次）。 */
export function bindChatDiagSession(sessionId: number, provider: string | null | undefined): void {
  if (provider) providers.set(sessionId, provider);
}

/** 写一行对话窗运行日志。尽力而为：失败静默，手机远程页不写。 */
export function logChatEvent(sessionId: number, event: ChatDiagEvent): void {
  if (remoteUi()) return;
  // 日志写不了不能影响功能（宿主命令不在、单测里 api 被整体替换时都会抛）。
  try { diagLog(SCOPE, chatDiagLine(sessionId, providers.get(sessionId), event)); } catch { /* 尽力而为 */ }
}

// 文案 → 它在词典里的键（`chat.undoBusy`）。每个词典只建一次。
const copyKeys = new WeakMap<Dict, Map<string, string>>();

/**
 * 错误条上显示的一句话 → 写日志用的固定代码：词典里的键、后端错误码，都对不上的记 `unkeyed`
 * （带参数拼出来的句子、后端没有结构化代码的原文）。日志里不落这句话本身：里面可能有路径、
 * 终端画面上的字。
 */
export function shownErrorReason(message: string, t: Dict): string {
  let keys = copyKeys.get(t);
  if (!keys) {
    keys = new Map();
    for (const group of ["chat", "newSession"] as const) {
      for (const [key, value] of Object.entries(t[group])) {
        if (typeof value === "string" && !keys.has(value)) keys.set(value, `${group}.${key}`);
      }
    }
    copyKeys.set(t, keys);
  }
  return keys.get(message) ?? backendErrorCode(message, t.locale) ?? "unkeyed";
}

/**
 * 错误条上显示了一句话：写一行兜底日志（做空⑧）。对话窗里「失败、未确认、被拦下」几十处都经
 * 同一个 setSendError 上屏，日志挂在这个口子上，新加一处错误不用记得再补一行。发送失败、切换未确认
 * 这些另有专门的一行（原因更细），这里会多出一行同一件事的 `error_shown`，宁可重复不可漏。
 */
export function logShownError(sessionId: number, message: string, t: Dict): void {
  if (!message) return;
  logChatEvent(sessionId, { kind: "error_shown", reason: shownErrorReason(message, t) });
}

/**
 * 发出的消息从上一次观察到这一次，哪些落进了「发送失败」「未确认送达」。
 * 原因由阶段变化推出：sending → failed 是没写进终端；sending → uncertain 是写入迟迟不返回；
 * 其余进入 uncertain 的是写进去了、等回显超时。重开窗口后从本地恢复的旧回执不算新事件：
 * 调用方把每个会话第一次看到的列表只当基线（见 useOutgoingDiag）。
 */
export function outgoingDiagEvents(
  prev: ReadonlyMap<number, OutgoingPhase>,
  list: readonly Pick<OutgoingPrompt, "id" | "phase" | "kind">[],
): ChatDiagEvent[] {
  const events: ChatDiagEvent[] = [];
  for (const prompt of list) {
    const before = prev.get(prompt.id);
    if (before === prompt.phase) continue;
    const from = before ?? "new";
    if (prompt.phase === "failed") {
      events.push({ kind: "send_failed", reason: "terminal_write_failed", detail: { from, queue: prompt.kind === "queued" } });
    } else if (prompt.phase === "uncertain") {
      events.push({
        kind: "send_unconfirmed",
        reason: before === "sending" ? "terminal_write_stalled" : "echo_timeout",
        detail: { from, queue: prompt.kind === "queued" },
      });
    }
  }
  return events;
}

/** 盯着当前会话发出的消息：有落进失败或未确认的就写日志。每个会话第一次看到的列表只作基线。 */
export function useOutgoingDiag(sessionId: number, provider: string | null | undefined, outgoing: readonly OutgoingPrompt[]): void {
  const seen = useRef<{ sessionId: number; phases: Map<number, OutgoingPhase> } | null>(null);
  useEffect(() => { bindChatDiagSession(sessionId, provider); }, [sessionId, provider]);
  useEffect(() => {
    const phases = new Map(outgoing.map((prompt) => [prompt.id, prompt.phase]));
    const prev = seen.current;
    seen.current = { sessionId, phases };
    if (!prev || prev.sessionId !== sessionId) return;
    for (const event of outgoingDiagEvents(prev.phases, outgoing)) logChatEvent(sessionId, event);
  }, [sessionId, outgoing]);
}
