/// 有卡片待处理时输入框怎么办：五家、两条通道同一条规则。
///
/// 规则（2026-11-14 定）：
/// 1. 输入框永远不因卡片禁用。草稿照常可写、焦点不被踢走。
/// 2. 只要有请求待处理，发送就先拦住，并且事先说明（占位文字、发送键置灰带提示），
///    不再出现「能发、发了才报错」。
/// 3. 屏幕上认出的卡摆在眼前时（终端里正开着一个等按键的框），会往终端写按键的控件
///    （模式、模型、停止）照旧停用。这一条沿用现状，没有改：卡片被「仅收起」后、以及 broker
///    挂起的请求，这些控件由「此刻做不到」的统一策略管，不在这里。
///
/// 依据：
/// - 终端里的框开着时不接受新消息。Claude Code 2.1.325 真机（2026-11-14，probe_tui_script）：
///   权限框取代输入框，此时打进去的 `hello` 没有任何落点，Esc 之后输入框是空的，字丢了。
///   屏幕上认出的卡都是这种状态；Claude 的 broker 审批期间终端同样摆着权限框（hook 与权限框
///   并行）。往里写正文，文字会去过滤选项，回车会选中焦点项。所以发送必须拦。
/// - Codex 的 broker 审批期间终端不显示权限框，理论上还能排队一条消息。这里仍然拦：现状就是拦
///   （只是发了才说），五家同一条规则比多一个例外更好懂，也不让新消息挤进一个审批未决的回合。
/// - 此前屏幕通道把整个输入区设为 inert（不能打字），broker 通道完全不锁、发送后才报错
///   （走查 输入 B2）。统一成「不能打字」不行：审批卡的数字键只在焦点不在输入框时生效，
///   正在打字的人被踢出输入框后，下一个数字键就成了放行（UI 优化 c3/m11 的教训）。
///   统一成「能打字、先说明不能发」两头都安全。
///
/// 这里只回答「因为卡片要不要锁、怎么说」。其余输入框逻辑（草稿、排队、送达）不在这里。

import type { useT } from "../../i18n";

type Dict = ReturnType<typeof useT>;

export type CardLockInput = {
  /// 屏幕上认出的卡（审批框、提问表单、提示）。collapsed = 被「仅收起」，卡不在眼前但终端里还等着。
  terminalCard: { kind: "approval" | "question" | "prompt"; collapsed: boolean } | null;
  /// broker 挂起的审批。
  brokerApproval: boolean;
  /// broker 带来的提问（展开或收起）。
  brokerQuestion: boolean;
  /// 会话记录里报的待处理交互（拍板关卡不算，它不占终端）。有它而没有卡，说明题面还没认出来。
  pendingReview: "question" | "plan" | "approval" | null;
  /// 刚发出菜单命令、终端菜单还开着。
  terminalMenuOpen: boolean;
};

export type CardLockReason = "approval" | "question" | "prompt" | "menu";

export type CardLock = {
  /// 发送是否被拦。
  send: boolean;
  /// 往终端写按键的控件是否停用。
  controls: boolean;
  /// 在等什么。说明文字按它取，同一个原因只有一句话。
  reason: CardLockReason | null;
};

export function composerCardLock(input: CardLockInput): CardLock {
  const controls = input.terminalCard !== null && !input.terminalCard.collapsed;
  const reason: CardLockReason | null = input.terminalCard ? input.terminalCard.kind
    : input.brokerApproval ? "approval"
    : input.terminalMenuOpen ? "menu"
    : input.brokerQuestion || input.pendingReview === "question" ? "question"
    : input.pendingReview ? "approval"
    : null;
  return { send: reason !== null, controls, reason };
}

/// 屏幕上认出的请求在锁定规则里算哪一类：审批与计划批准是审批，提问是提问，其余是提示。
export function screenCardLockKind(kind: string): "approval" | "question" | "prompt" {
  return kind === "approval" || kind === "plan" ? "approval" : kind === "question" ? "question" : "prompt";
}

/// 发送途中才发现有卡在等（启动时认出了提示、读会话记录发现有待处理交互）：
/// 与卡片在场时同一条规则、同一句说明。
export function lockForScreenCard(card: { kind: string }): CardLock {
  return composerCardLock({
    terminalCard: { kind: screenCardLockKind(card.kind), collapsed: false },
    brokerApproval: false,
    brokerQuestion: false,
    pendingReview: null,
    terminalMenuOpen: false,
  });
}

export function lockForPendingReview(pendingReview: string): CardLock {
  return composerCardLock({
    terminalCard: null,
    brokerApproval: false,
    brokerQuestion: false,
    pendingReview: pendingReview === "question" ? "question" : pendingReview === "plan" ? "plan" : "approval",
    terminalMenuOpen: false,
  });
}

/// 锁定时的说明。占位文字、发送键提示、按回车后的提示用的都是这一句。
export function cardLockHint(lock: CardLock, t: Dict): string {
  switch (lock.reason) {
    case "approval": return t.chat.cardLockApproval;
    case "question": return t.chat.cardLockQuestion;
    case "prompt": return t.chat.cardLockPrompt;
    case "menu": return t.chat.cardLockMenu;
    default: return "";
  }
}
