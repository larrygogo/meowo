/// 用户发出的消息从「写进终端」到「被 Agent 读到」的全过程。
///
/// 为什么单独成模块（对照 Kimi 的 kimi.prompt 仓库）：此前空闲态的乐观回显和运行中的
/// 排队回执是 ChatWindow 里两份各自对账的 state，同一套「包含 + 水位线」规则写了两遍，
/// 拿不准时直接删掉——用户看到的就是「我的消息不见了」。这里一条消息一个对象，状态在
/// 它身上推进；对账是纯函数，可脱离五千行的组件单测；按会话分仓，切走再切回不丢。
///
/// 证据只来自 CLI 自己的记录（PTY 写入没有回执，拿不到 Kimi 那样的 promptId）：transcript 里的
/// 用户消息、claude 的入队行（queue-operation enqueue）、UserPromptSubmit hook 记下的原话。
/// 拿不准时进 uncertain 并照实显示，而不是悄悄消失。
///
/// 送达状态误判的共同根因（leader 报过 5 次：11-01 两次、11-09 两次、11-11，调研 real-usage #1）：
/// 状态由 Meowo 自己的记账推着走，CLI 的记录只在少数条件下才被采信。每次实拍都是记账的
/// 一处盲区，此前逐处补洞：
/// - 记账绑在会话编号上：写入途中临时编号换正式编号，「发送中」等不到结算（11-09 07:27）。
///   transcript 里那条话早已以「回合中插入」落盘，只因发送中的回执不参与对账而被无视。
/// - 记账绑在加载窗口上：切走再回来首读只给最新一屏（11-01），新会话第一句话被挤出尾部
///   （11-09 08:27）。窗口里看不见不等于 CLI 没收到。
/// - 文本必须原样出现：CLI 会改写落盘正文（斜杠命令包成 <command-name>、! 包成 <bash-input>、
///   贴图插 [Image #N]），改写过的永远对不上。
/// - hook 的 UserPromptSubmit 是「CLI 收下了」最直接的回执，却只在没有 transcript 时才认。
///   transcript 不落盘的版本（CC 2.1.218 resume 静默不写，memory cc-resume-writer-bug）每条都报未确认。
/// 现在的规则：CLI 的任何一种记录都优先于本地记账（发送中也照样对账，迟到的「写入完成」
/// 不能把已确认的消息拨回去）；文本按 CLI 的改写规则归一后再比；看不见的窗口不下「未确认」
/// 的结论；hook 回执单独成一档 received。只有 CLI 侧确实没有任何记录时才 uncertain，
/// 并给出读终端画面得到的原因与就地出口（见 useOutgoingUpkeep）。

import { createContext, useSyncExternalStore } from "react";
import type { ChatItem } from "../../api";
import type { Attachment } from "./drafts";

/// echo = 空闲时发出，先以用户气泡挂在对话末尾；queued = 运行中发出，进排队回执。
export type OutgoingKind = "echo" | "queued";
/// sending：点了发送、还在往终端写（先上屏再发，像聊天软件那样，用户要求）。
/// failed：没写进去——气泡留着并给「重发 / 放回输入框」，不悄悄吞掉用户打的字。
/// pending：已写进终端，还没在对话记录里见到。
/// accepted：CLI 回了送达回执（它正忙，把这条收进了自己的队列，对照 Kimi 的 prompt:accepted），
/// 还没轮到处理。有了这个回执就不会再判「未确认送达」——消息确实进了 CLI。
/// inserted：queued 已被读到（短暂停留，让用户看见这一下状态变化再收起）。
/// received：echo 已有 UserPromptSubmit 回执（CLI 收下并开工），只是正文还没落进 transcript。
/// 不标任何提示，也不会再转 uncertain：CLI 确实收到了。
/// uncertain：echo 超时仍无证据——可能根本没进 CLI 的输入框，照实标出来。
export type OutgoingPhase = "sending" | "failed" | "pending" | "accepted" | "received" | "inserted" | "uncertain";

export type OutgoingAnchor = {
  /// 入队时 transcript 窗口的最后一条：它之后的条目才算新证据。
  lastItemId: string | null;
  /// 入队时窗口里最晚的落盘时间（宿主机时钟，毫秒）。锚点不在窗口里时按它划线：发出后切走、
  /// 回来时首读只给最新一屏，Agent 这期间干的活一多，锚点和这条话都早已不在尾部 12 条里，
  /// 原先的兜底永远对不上，气泡一直挂着「未确认送达」（用户实拍）。和条目时间同一个时钟，
  /// 不受手机与电脑时钟偏差影响。
  lastItemAt: number | null;
  /// 入队时尾部 12 条里的 user_text：锚点被翻页挤出窗口时的兜底水位线。
  priorItemIds: ReadonlySet<string>;
  /// 入队时 hook 记的最近用户输入：只在没有 transcript 时拿来比对。
  priorUserText: string | null;
  /// 新建会话页发出的第一句话：会话在它之前一条记录都没有，记录里的用户消息全是它之后的。
  /// 空锚点原本退回「尾部 12 条」，agent 一跑十几步命令，第一句就被挤出尾部，回显永远对不上，
  /// 气泡挂成「未确认送达」叠在真消息下面（用户实拍 2026-11-09）。
  fresh?: boolean;
};

export type OutgoingPrompt = {
  id: number;
  kind: OutgoingKind;
  /// 显示文本；纯附件消息是占位文案（它永不落盘，不能拿来做包含匹配）。
  text: string;
  attachmentOnly: boolean;
  at: number;
  phase: OutgoingPhase;
  settledAt: number | null;
  anchor: OutgoingAnchor;
  /// 发出时的原始草稿（正文 + 附件），失败后重发 / 放回输入框用。
  draft?: { prompt: string; attachments: readonly Attachment[] };
  /// 认领到的送达回执 id：一条回执只确认一条消息（连发两句「继续」各认各的）。
  ackId?: string;
};

export type PromptAck = Extract<ChatItem, { type: "prompt_queued" }>;

export type OutgoingEvidence = {
  items: readonly ChatItem[];
  /// CLI 的送达回执（目前只有 claude 记入队动作）。没有这类证据的 provider 传空。
  acks?: readonly PromptAck[];
  lastUserText: string | null | undefined;
  ended: boolean;
  now: number;
};

/// echo 等多久仍无证据就算「未确认」。CLI 正常提交后立刻落盘，15s 足够宽。
export const ECHO_CONFIRM_MS = 15_000;
/// 「已插入」停留多久后收起。
export const INSERTED_LINGER_MS = 2_500;
/// 「发送中」最多挂多久：发送链路里最长的一段等待是第一句话的 60s 上限（ChatWindow deliver），
/// 写入的 promise 再不落定就是卡死了。转 uncertain 给出口，不让气泡永远转圈（11-09 07:27 实拍）。
export const SENDING_STALL_MS = 90_000;

/// 比对用的归一形态：按 CLI 落盘时的改写规则还原，再去掉全部空白。
/// - 斜杠命令：`<command-name>/model</command-name>…<command-args>fable</command-args>` 还原成
///   `/model fable`（本机 transcript 实拍 2.1.3xx；command-name 与 command-message 先后两种都有）。
/// - bash 模式：`<bash-input> ssh-copy-id …</bash-input>` 还原成 `!ssh-copy-id …`（11-09 实拍）。
/// - 贴图：CLI 在正文前插 `[Image #N]`（11-09 08:27 实拍「[Image #1] [Image #2] [Image #3]1. 新建会话…」）。
/// 空白整个去掉：CLI 会裁首尾，换行在 PTY 里是 CRLF，`! cmd` 落盘成 `<bash-input> cmd`。
/// 误配的代价由水位线兜住（只认发送之后的记录）。
export function promptKey(text: string): string {
  let body = text;
  const command = /<command-name>\s*([^<]*?)\s*<\/command-name>/.exec(body);
  if (command) {
    const args = /<command-args>([\s\S]*?)<\/command-args>/.exec(body)?.[1] ?? "";
    const name = command[1].startsWith("/") ? command[1] : `/${command[1]}`;
    body = `${name} ${args}`;
  }
  const bash = /^\s*<bash-input>([\s\S]*?)<\/bash-input>/.exec(body);
  if (bash) body = `!${bash[1]}`;
  return body.replace(/\[Image #\d+\]/g, "").replace(/\s+/g, "");
}

const itemTime = (item: ChatItem): number | null => {
  const at = item.timestamp ? Date.parse(item.timestamp) : NaN;
  return Number.isFinite(at) ? at : null;
};

export function anchorFrom(items: readonly ChatItem[], lastUserText: string | null | undefined): OutgoingAnchor {
  let lastItemAt: number | null = null;
  for (const item of items) {
    const at = itemTime(item);
    if (at != null && (lastItemAt == null || at > lastItemAt)) lastItemAt = at;
  }
  return {
    lastItemId: items.length > 0 ? items[items.length - 1].id : null,
    lastItemAt,
    priorItemIds: new Set(items.slice(-12).filter((item) => item.type === "user_text").map((item) => item.id)),
    priorUserText: lastUserText ?? null,
  };
}

/// 新会话第一句话的锚点：见 OutgoingAnchor.fresh。
export const freshAnchor = (): OutgoingAnchor => ({ ...anchorFrom([], null), fresh: true });

type UserTextItem = Extract<ChatItem, { type: "user_text" }>;

/// 入队之后才出现的用户消息。锚点还在窗口里就取它之后的全部；被挤出去了按入队时的
/// 落盘时间划线，晚于它的才算；连时间都没有才退回「尾部 12 条里不在快照中的」——
/// 短插话（ok/继续）不能子串命中上一回合的旧消息。
function newUserTexts(items: readonly ChatItem[], anchor: OutgoingAnchor): UserTextItem[] {
  const isNew = (item: ChatItem): item is UserTextItem => item.type === "user_text" && !anchor.priorItemIds.has(item.id);
  if (anchor.fresh) return items.filter(isNew);
  const at = anchor.lastItemId == null ? -1 : items.findIndex((item) => item.id === anchor.lastItemId);
  if (at >= 0) return items.slice(at + 1).filter(isNew);
  const tail = new Set(items.slice(-12));
  const since = anchor.lastItemAt;
  return items.filter((item): item is UserTextItem => {
    if (!isNew(item)) return false;
    const stamp = since == null ? null : itemTime(item);
    return stamp == null || since == null ? tail.has(item) : stamp > since;
  });
}

/// 加载窗口看不见这条话：窗口里最早的记录也晚于发送时的划线，说明首读（切走再回来、文件
/// 重写后的整读）只给了发送之后的一屏，这条话即使落盘了也不在窗口里。CLI 在划线之后写了
/// 记录，就是它收到了输入、开了工（11-01 实拍：切走期间 agent 干了几十步）。不能在看不见
/// 的地方下「未确认」的结论。只在条目带时间戳时成立。
function windowStartsAfter(items: readonly ChatItem[], since: number | null): boolean {
  if (since == null) return false;
  let earliest: number | null = null;
  for (const item of items) {
    const at = itemTime(item);
    if (at != null && (earliest == null || at < earliest)) earliest = at;
  }
  return earliest != null && earliest > since;
}

/// hook 记的最近一句（UserPromptSubmit 原话）属于哪条消息。它只记最近一句，一次只能确认一条：
/// 原文完全相同的优先，其次是前缀（Meowo 在末尾追加了附件指令）；发送前就是这句的不算。
/// hook 原话不经 CLI 改写，所以按前缀比，比落盘正文的「包含」严。
function hookReceiptTarget(prompts: readonly OutgoingPrompt[], lastUserText: string | null | undefined): number | null {
  if (lastUserText == null) return null;
  const hookKey = promptKey(lastUserText);
  if (!hookKey) return null;
  const open = prompts.filter((prompt) => (prompt.phase === "sending" || prompt.phase === "pending" || prompt.phase === "uncertain")
    && hookKey !== promptKey(prompt.anchor.priorUserText ?? ""));
  const exact = open.find((prompt) => !prompt.attachmentOnly && promptKey(prompt.text) === hookKey);
  const prefix = exact ?? open.find((prompt) => prompt.attachmentOnly || hookKey.startsWith(promptKey(prompt.text)));
  return prefix?.id ?? null;
}

/// 按证据推进每条消息的状态。纯函数；无变化时原样返回同一个数组（调用方据此短路）。
///
/// 一条落盘消息只结算一条回执：连发两句相同的「ok」，第一句落盘不能把两张回执一起收走。
/// 落盘文本会带附件指令和 CLI 的改写（见 promptKey），所以比对用归一后的「包含」，
/// 误配的代价由水位线兜住。lastUserText（UserPromptSubmit hook 记的原话）是「CLI 收下了」
/// 的回执：没有 transcript 时它就是全部证据；有 transcript 时只推进到 received / accepted，
/// 不当作已读到。claude 在插话入队瞬间就触发它，正文要到下一个工具边界才落盘。
///
/// 发送中的也参与对账：写入完成的通知可能迟到或丢（11-09 07:27 编号搬家），CLI 的记录先到
/// 就按记录走。发送失败的不参与：那是用户还没处理的字，记录里出现同样的话也不是它。
export function reconcileOutgoing(
  prompts: readonly OutgoingPrompt[],
  evidence: OutgoingEvidence,
): readonly OutgoingPrompt[] {
  if (prompts.length === 0) return prompts;
  // 会话结束：已写进终端的回执没了下文，收起。还在发送的（发送会先把会话拉起来）和发送
  // 失败的留着——前者马上有结果，后者是用户还没处理的字。
  if (evidence.ended) {
    const kept = prompts.filter((prompt) => prompt.phase === "sending" || prompt.phase === "failed");
    return kept.length === prompts.length ? prompts : kept;
  }
  const used = new Set<string>();
  // 已被认领的回执跨轮次保留在消息身上，这里先全部记为占用。
  const usedAcks = new Set(prompts.flatMap((prompt) => (prompt.ackId ? [prompt.ackId] : [])));
  const hookIsEvidence = evidence.items.length === 0;
  const hookTarget = hookReceiptTarget(prompts, evidence.lastUserText);
  let changed = false;
  const next: OutgoingPrompt[] = [];
  for (const prompt of prompts) {
    if (prompt.phase === "failed") { next.push(prompt); continue; }
    if (prompt.phase === "inserted") {
      if (evidence.now - (prompt.settledAt ?? evidence.now) > INSERTED_LINGER_MS) { changed = true; continue; }
      next.push(prompt);
      continue;
    }
    const candidates = newUserTexts(evidence.items, prompt.anchor).filter((item) => !used.has(item.id));
    const needle = promptKey(prompt.text);
    const match = prompt.attachmentOnly
      ? candidates[0]
      : candidates.find((item) => promptKey(item.text).includes(needle));
    const hookHit = prompt.id === hookTarget;
    const hookMatch = hookIsEvidence && hookHit;
    // 未确认的回显：后面又有新消息落盘，说明这条的去向已经被后续事实盖过，收起。
    // 新会话的第一句话：记录里出现任何用户消息就说明它已提交（会话从它开始），不必等超时；
    // 第一句被翻页挤出加载窗口时文本对不上，也照样收起，但不占用候选（留给后发的消息认领）。
    // received 同理：CLI 已收下，落盘的正文却对不上，那条就是它被改写后的样子。
    const overtaken = (prompt.phase === "uncertain" || prompt.phase === "received" || !!prompt.anchor.fresh) && candidates.length > 0;
    // 加载窗口整个落在发送之后：看不见这条话，但 CLI 在那之后开了工（见 windowStartsAfter）。
    const unseen = prompt.phase !== "sending" && !prompt.anchor.fresh
      && windowStartsAfter(evidence.items, prompt.anchor.lastItemAt);
    if (match || hookMatch || overtaken || unseen) {
      if (match) used.add(match.id);
      changed = true;
      if (prompt.kind === "queued") next.push({ ...prompt, phase: "inserted", settledAt: evidence.now });
      continue;
    }
    // 送达回执：发出之后（按宿主机时钟晚于发送时窗口里最晚的一条）入队、正文对得上的那条。
    if (prompt.phase === "pending" || prompt.phase === "uncertain" || prompt.phase === "sending" || prompt.phase === "received") {
      const since = prompt.anchor.lastItemAt;
      const ack = (evidence.acks ?? []).find((entry) => {
        if (usedAcks.has(entry.id)) return false;
        const stamp = itemTime(entry);
        if (since != null && (stamp == null || stamp <= since)) return false;
        return prompt.attachmentOnly || promptKey(entry.text).includes(needle);
      });
      if (ack) {
        usedAcks.add(ack.id);
        changed = true;
        next.push({ ...prompt, phase: "accepted", ackId: ack.id });
        continue;
      }
    }
    // hook 回执：CLI 收下了这句话。排队的标「已收到」，回显不再提示（它已在处理）。
    if (hookHit) {
      changed = true;
      next.push({ ...prompt, phase: prompt.kind === "queued" ? "accepted" : "received" });
      continue;
    }
    if (prompt.kind === "echo" && prompt.phase === "pending" && evidence.now - prompt.at > ECHO_CONFIRM_MS) {
      changed = true;
      next.push({ ...prompt, phase: "uncertain" });
      continue;
    }
    if (prompt.phase === "sending" && evidence.now - prompt.at > SENDING_STALL_MS) {
      changed = true;
      next.push({ ...prompt, phase: "uncertain" });
      continue;
    }
    next.push(prompt);
  }
  return changed ? next : prompts;
}

const EMPTY: readonly OutgoingPrompt[] = [];

/// 发送失败的回执按会话存进 localStorage：刷新页面、手机页被系统回收后，失败的那条和
/// 它的原文（含附件路径）还在，回来照样能重发或放回输入框（对照微信保留发送失败的消息）。
/// 只存 failed：发送中的那条页面没了就不知道到底写没写进去，存下来反而可能重复发送。
const FAILED_KEY_PREFIX = "meowo-outgoing-failed:";
type StoredFailed = Pick<OutgoingPrompt, "kind" | "text" | "attachmentOnly" | "at" | "draft">;

function readFailed(sessionId: number): StoredFailed[] {
  // 负数临时 id 每次启动都从 -1 往下重新数：存过的失败回执会串进重启后同号的新会话（做空复现）。
  if (sessionId < 0) return [];
  try {
    const raw = localStorage.getItem(FAILED_KEY_PREFIX + sessionId);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((entry): entry is StoredFailed => !!entry && typeof entry === "object"
      && (entry.kind === "echo" || entry.kind === "queued") && typeof entry.text === "string");
  } catch {
    return [];
  }
}

function writeFailed(sessionId: number, list: readonly OutgoingPrompt[]) {
  if (sessionId < 0) return; // 同 readFailed：临时 id 不落盘，认领搬到真 id 后再存
  try {
    const failed: StoredFailed[] = list.filter((prompt) => prompt.phase === "failed")
      .map(({ kind, text, attachmentOnly, at, draft }) => ({ kind, text, attachmentOnly, at, draft }));
    if (failed.length === 0) localStorage.removeItem(FAILED_KEY_PREFIX + sessionId);
    else localStorage.setItem(FAILED_KEY_PREFIX + sessionId, JSON.stringify(failed));
  } catch {
    // 隐私模式/配额满：退回只在内存里留着。
  }
}

/// 失败回执不参与对账，水位线用不上；恢复时给一个空的。
const EMPTY_ANCHOR: OutgoingAnchor = { lastItemId: null, lastItemAt: null, priorItemIds: new Set(), priorUserText: null };

/// 本地记账（写入完成 / 写入失败 / 替用户按回车）推进回执。CLI 的记录已经把它推到
/// received / accepted 之后，迟到的「写入完成」不能把它拨回 pending 重新计时，「写入失败」也
/// 不能把 CLI 已收下的话标成失败（证据优先于记账，见文件头）。
function advance(prompt: OutgoingPrompt, patch: Partial<Pick<OutgoingPrompt, "phase" | "at">>): OutgoingPrompt {
  if (patch.phase === "pending" && prompt.phase !== "sending" && prompt.phase !== "uncertain") return prompt;
  if (patch.phase === "failed" && prompt.phase !== "sending") return prompt;
  return { ...prompt, ...patch };
}

export function createOutgoingStore() {
  const bySession = new Map<number, readonly OutgoingPrompt[]>();
  const listeners = new Set<() => void>();
  // 已从 localStorage 读过失败回执的会话：每个会话只在第一次用到时读一次。
  const hydrated = new Set<number>();
  let nextId = 0;
  const hydrate = (sessionId: number) => {
    if (hydrated.has(sessionId)) return;
    hydrated.add(sessionId);
    const restored = readFailed(sessionId).map((entry): OutgoingPrompt => {
      nextId += 1;
      return { ...entry, id: nextId, phase: "failed", settledAt: null, anchor: EMPTY_ANCHOR };
    });
    // 不通知订阅者：可能正处在 useSyncExternalStore 的取快照里，只把数据就位。
    if (restored.length > 0) bySession.set(sessionId, [...restored, ...(bySession.get(sessionId) ?? EMPTY)]);
  };
  const put = (sessionId: number, list: readonly OutgoingPrompt[]) => {
    const before = bySession.get(sessionId) ?? EMPTY;
    if (list.length === 0) bySession.delete(sessionId);
    else bySession.set(sessionId, list);
    // 失败回执有增减才写盘：发送中 → 已写入这类高频推进不碰 localStorage。
    const failedCount = (items: readonly OutgoingPrompt[]) => items.filter((prompt) => prompt.phase === "failed").length;
    if (failedCount(before) !== failedCount(list) || before.some((prompt) => prompt.phase === "failed" && !list.includes(prompt))) {
      writeFailed(sessionId, list);
    }
    listeners.forEach((listener) => listener());
  };
  return {
    get: (sessionId: number): readonly OutgoingPrompt[] => {
      hydrate(sessionId);
      return bySession.get(sessionId) ?? EMPTY;
    },
    add(sessionId: number, draft: { kind: OutgoingKind; text: string; attachmentOnly: boolean; anchor: OutgoingAnchor; at?: number; phase?: OutgoingPhase; draft?: OutgoingPrompt["draft"] }): number {
      hydrate(sessionId);
      nextId += 1;
      const prompt: OutgoingPrompt = {
        id: nextId,
        kind: draft.kind,
        text: draft.text,
        attachmentOnly: draft.attachmentOnly,
        at: draft.at ?? Date.now(),
        phase: draft.phase ?? "pending",
        settledAt: null,
        anchor: draft.anchor,
        draft: draft.draft,
      };
      put(sessionId, [...(bySession.get(sessionId) ?? EMPTY), prompt]);
      return prompt.id;
    },
    /// 推进一条回执（发送中 → 已写入 / 失败）。切走后才落定也照样更新：仓库按会话分仓。
    update(sessionId: number, id: number, patch: Partial<Pick<OutgoingPrompt, "phase" | "at">>) {
      const list = bySession.get(sessionId);
      if (!list?.some((prompt) => prompt.id === id)) return;
      put(sessionId, list.map((prompt) => (prompt.id === id ? advance(prompt, patch) : prompt)));
    },
    /// 临时负 id 认领成真 id：同一个 PTY 换了编号，回执跟着搬过去。新建会话页的第一句话
    /// 在认领前就上屏了，不搬的话认领那一刻气泡消失，等 transcript 落盘才又冒出来。
    move(from: number, to: number) {
      const list = bySession.get(from);
      if (!list || from === to) return;
      bySession.delete(from);
      put(to, [...(bySession.get(to) ?? EMPTY), ...list]);
    },
    /// 按回执编号推进或撤掉（patch 为 null），不管它此刻挂在哪个会话下：编号全局唯一，
    /// 认领搬家后按会话找会落空。
    settle(id: number, patch: Partial<Pick<OutgoingPrompt, "phase" | "at">> | null) {
      for (const [sessionId, list] of bySession) {
        if (!list.some((prompt) => prompt.id === id)) continue;
        put(sessionId, patch
          ? list.map((prompt) => (prompt.id === id ? advance(prompt, patch) : prompt))
          : list.filter((prompt) => prompt.id !== id));
        return;
      }
    },
    /// 只撤界面上的回执：消息已经写进终端，CLI 照样会处理它。
    dismiss(sessionId: number, id: number) {
      const list = bySession.get(sessionId);
      if (!list?.some((prompt) => prompt.id === id)) return;
      put(sessionId, list.filter((prompt) => prompt.id !== id));
    },
    reconcile(sessionId: number, evidence: OutgoingEvidence) {
      const list = bySession.get(sessionId);
      if (!list) return;
      const next = reconcileOutgoing(list, evidence);
      if (next !== list) put(sessionId, next);
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    /// 测试隔离用：仓库是模块级的，跨用例会残留。
    reset() {
      bySession.clear();
      hydrated.clear();
      // 编号不清零：上一个用例没跑完的异步流程会按编号结算回执（settle），清零后撞号会
      // 改到下一个用例的回执上。编号只要唯一，从几开始无所谓。
      listeners.forEach((listener) => listener());
    },
  };
}

/// 每个窗口一份（对话窗是独立 WebView，不跨窗口共享）。
export const outgoingStore = createOutgoingStore();

export function useOutgoingPrompts(sessionId: number): readonly OutgoingPrompt[] {
  return useSyncExternalStore(outgoingStore.subscribe, () => outgoingStore.get(sessionId));
}

/// 回显在对话流里的条目 id。只进显示层，对账一律读真实 transcript。
export const echoItemId = (id: number) => `echo-${id}`;

/// 显示层条目 id → 回显状态。Message 据此给「未确认送达」的气泡挂标注；对话窗之外为空。
export const EchoPhaseContext = createContext<ReadonlyMap<string, OutgoingPhase>>(new Map());

/// 回显条目 id → 回执 id（echoItemId 的逆）。不是回显条目返回 null。
export const outgoingIdOf = (itemId: string): number | null => {
  const match = /^echo-(\d+)$/.exec(itemId);
  return match ? Number(match[1]) : null;
};

/// 发送失败 / 未确认送达的气泡上的出口（对话窗提供；对话窗之外为 null，不渲染按钮）。
/// stuck = 读终端画面确认「这段话还停在 CLI 输入框里」的回显，给「提交」而不是「重发」。
export type EchoActions = {
  retry: (id: number) => void;
  restore: (id: number) => void;
  submit: (id: number) => void;
  stuck: ReadonlySet<number>;
};
export const EchoActionsContext = createContext<EchoActions | null>(null);
