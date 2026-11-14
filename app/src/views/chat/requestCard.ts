/// 请求卡的视图模型：把两条通道（broker 挂起的请求、屏幕上认出的请求）归一成卡片认的那一份。
///
/// 为什么要有这一层：此前 broker 审批一张卡（BrokerApprovalCard）、屏幕审批一张卡（写在
/// ChatWindow 里）、认不出文法的再落一张通用卡，三处各定各的标题、徽章、按钮和键盘。同一件事
/// 换一家 Provider 就换一副样子（走查 输入 A3、B3、B13）。现在差异全压在这里：
/// - 这是什么事（种类）由后端声明：broker 请求的 `kind`，屏幕规则的 `AttentionKind`；
/// - 能做什么（允许一次、拒绝、持久放行、说明并拒绝、其他选项、仅收起）：broker 请求看通道，
///   屏幕请求看每个选项的角色。角色由插件随识别规则声明（后端 `OptionRole`），这里不认任何一家
///   的选项文案（此前用正则认 `Approve for this session`、`switch to auto mode`，适配知识落在了界面层）。
/// 卡片组件只拿模型渲染，不看 Provider 名字，也不看规则 id。

import type { RuntimeRequest } from "../../api";
import type { useT } from "../../i18n";
import type { AttentionKind, OptionRole, TerminalAttention, TerminalAttentionOption, TypedAnswer } from "../../terminalAttention";
import { isRiskyCommand } from "./ApprovalCard";
import { commandOfInput } from "./approvalScope";
import { approvalSuggestionLabel, approvalSuggestionTip } from "./BrokerApprovalCard";
import {
  arrowPanelApprovalDetails,
  framedBoxApprovalDetails,
  optionRowApprovalDetails,
  proceedBoxApprovalDetails,
} from "./terminalApprovalDetails";

type Dict = ReturnType<typeof useT>;

/// 卡片的种类。标题与徽章只由它决定。
/// approval 审批、plan 计划批准、question 提问、choice CLI 要用户选一项、
/// trust 信任页、startup 登录与凭据类启动提示。
export type CardKind = "approval" | "plan" | "question" | "choice" | "trust" | "startup";

/// 屏幕规则的种类 → 卡片种类。菜单（刚发出菜单命令后认出的选项）也是「等你选择」。
export function cardKindOfAttention(kind: AttentionKind): CardKind {
  return kind === "menu" ? "choice" : kind;
}

/// 卡片上的请求状态。pending 等用户处理；expired 超过时限（broker 请求 300 秒），已交回终端。
/// 没有「已取消」这一态：请求在别处了结（终端里答了、回合被中断）时卡片直接收走，原因写在
/// 提示条上（见 approvalEndedNotice），不留一张点不了的卡。
export type CardState = "pending" | "expired";

export function cardStateOf(request: Pick<RuntimeRequest, "status">, timedOut = false): CardState {
  return request.status === "expired" || timedOut ? "expired" : "pending";
}

/// 审批在卡片之外了结后的说明。后端给了终态就照实说（过期、已取消），没给就用分不清原因的那一句。
export function approvalEndedNotice(status: RuntimeRequest["status"] | undefined, t: Dict): string {
  if (status === "expired") return t.chat.approvalExpiredNotice;
  if (status === "cancelled") return t.chat.approvalCancelledNotice;
  return t.chat.approvalClearedNotice;
}

/// 这条提示是不是「审批在卡片之外了结」的那几句说明之一。它解释的是上一张卡去哪了：
/// 新的卡片到来、或会话的进程换了代之后就不再成立，调用方据此把它清掉。
export function isApprovalEndedNotice(text: string, t: Dict): boolean {
  return text !== "" && [t.chat.approvalExpiredNotice, t.chat.approvalCancelledNotice, t.chat.approvalClearedNotice].includes(text);
}

/// 标题：种类 → 唯一的一句话。
export function cardTitle(kind: CardKind, t: Dict): string {
  switch (kind) {
    case "approval": return t.chat.approvalTitle;
    case "plan": return t.chat.planTitle;
    case "question": return t.chat.questionTitle;
    case "choice": return t.chat.choiceTitle;
    case "trust": return t.chat.trustPromptTitle;
    case "startup": return t.chat.terminalPromptTitle;
  }
}

/// 徽章：过期是一个词，各种卡通用；待处理时按种类说在等什么。
/// `countdown` 只有 broker 请求有（300 秒时限），接在待处理的词后面。
export function cardBadge(kind: CardKind, state: CardState, t: Dict, countdown: string | null = null): string {
  if (state === "expired") return t.chat.cardExpired;
  const pending = (() => {
    switch (kind) {
      case "approval":
      case "plan":
        return t.chat.approvalPending;
      case "question":
        return t.chat.questionPending;
      case "choice":
        return t.chat.choicePending;
      case "trust":
      case "startup":
        return t.chat.confirmPending;
    }
  })();
  return countdown ? `${pending} · ${countdown}` : pending;
}

/// 审批卡上的一个决定。取值与 broker 的应答词一致，屏幕通道在适配层换成按键：
/// `allow_once` 允许一次、`deny` 拒绝、`deny:<理由>` 说明并拒绝、`suggestion:<n>` 第 n 个持久放行、
/// `option:<n>` 第 n 个其他选项（只有屏幕通道有）。
export type ApprovalChoice = string;

export type ApprovalCardModel = {
  requestId: string;
  channel: "broker" | "terminal";
  state: CardState;
  /// 现在能不能在卡片上作答：broker 还握着请求，或终端可写。外部终端里跑的会话为 false。
  live: boolean;
  /// 请求工具；认不出时为空，卡片不显示这一行。
  tool: string;
  /// 影响范围标签的判定输入（broker 是原始参数，屏幕是命令原文）。
  scopeInput: string;
  description: string;
  /// 终端里的问句原文（屏幕通道才有）。
  question: string;
  /// 详情区：认出是一条命令就标「命令」，否则标「完整参数」。
  detail: { label: "command" | "input"; text: string } | null;
  /// 破坏性命令：「允许一次」换成警示色。
  risky: boolean;
  /// 能力：有就出按钮，没有就不出。按钮的名称与顺序由卡片定，五家相同。
  actions: {
    allowOnce: boolean;
    deny: boolean;
    /// 说明并拒绝：理由能送回 Agent 才有。broker 通道经 hook 送回；屏幕通道要这一屏上有一个
    /// 声明了打字流程的拒绝项（真机走通过的家才声明，见后端 `TypedInput`）。
    denyWithReason: boolean;
    /// 持久放行，下标 n 对应决定 `suggestion:<n>`。
    persistent: { label: string; tip: string }[];
    /// 认不出角色的选项，下标 n 对应决定 `option:<n>`。按钮上是终端原文，不带数字快捷键，
    /// 不冒充允许或拒绝（gemini 的 `Modify with external editor`、kimi 计划审阅里计划自带的选项）。
    others: { label: string; tip: string }[];
    /// 仅收起：屏幕识别是启发式的，认错或过期的卡要有不发按键的出口；
    /// broker 请求收起等于让 Agent 干等到超时，不给。
    collapse: boolean;
  };
};

function brokerApprovalModel(request: RuntimeRequest, t: Dict, timedOut: boolean): ApprovalCardModel {
  const command = commandOfInput(request.input);
  const state = cardStateOf(request, timedOut);
  return {
    requestId: request.requestId,
    channel: "broker",
    state,
    live: request.responseCapability.type === "broker_live",
    tool: request.toolName,
    scopeInput: request.input,
    description: request.description ?? "",
    question: "",
    detail: request.input ? { label: command !== null ? "command" : "input", text: command ?? request.input } : null,
    risky: isRiskyCommand(request.input),
    actions: {
      allowOnce: true,
      deny: true,
      denyWithReason: true,
      // `?? []`：类型上字段恒在，旧后端与新前端错配时负载可能缺它，一个可选按钮组不值得白屏。
      persistent: (request.permissionSuggestions ?? []).map((suggestion, index) => ({
        label: approvalSuggestionLabel(suggestion, index, t),
        tip: approvalSuggestionTip(suggestion, index, t),
      })),
      others: [],
      collapse: false,
    },
  };
}

/// 屏幕审批框里的选项按角色归位。角色是识别层照插件声明的选项规则定好的（见 terminalAttention
/// 的 assignRoles），这里只是分组，不看文案。
///
/// 没有顶替：这一屏上没有 allow_once 角色的选项，`allowOnce` 就是 null，卡上不出「允许一次」。
/// 此前认不出时拿「第一个不是拒绝的选项」顶上，只认出 `Allow always / Reject` 两项时，
/// 「允许一次」发出去的是 Allow always（做空审查 2026-11-14）。
export type TerminalApprovalOptions = {
  allowOnce: TerminalAttentionOption | null;
  deny: TerminalAttentionOption | null;
  /// 能带说明的拒绝项与它的打字流程：专门的「拒绝并说明」项，或可以追加说明的拒绝项。
  denyWithReason: (TerminalAttentionOption & { typed: TypedAnswer }) | null;
  persistent: TerminalAttentionOption[];
  others: TerminalAttentionOption[];
};

type AttentionOptions = Pick<TerminalAttention, "options" | "unparsedKeys">;

export function terminalApprovalOptions(attention: AttentionOptions): TerminalApprovalOptions {
  const options = attention.options ?? [];
  const first = (role: OptionRole) => options.find((option) => option.role === role) ?? null;
  const typedDeny = options.find((option): option is TerminalAttentionOption & { typed: TypedAnswer } => (
    (option.role === "deny_with_feedback" || option.role === "deny") && option.typed !== undefined
  )) ?? null;
  return {
    allowOnce: first("allow_once"),
    deny: first("deny"),
    denyWithReason: typedDeny,
    persistent: options.filter((option) => option.role === "allow_persistent"),
    // 没带角色的（旧后端没有下发选项规则）也归这里：宁可列成普通选项，也不猜它是允许还是拒绝。
    others: options.filter((option) => option.role === "other" || option.role === undefined),
  };
}

/// 持久放行按钮的名称，由插件声明的范围定：只记本会话的说「本会话内允许」，写进规则的说
/// 「允许并记住」（带上放行的对象），切自动模式的说「允许并切到自动模式」；没声明范围的照搬终端原文，
/// 不替它编一个范围。悬停提示一律是终端里的原文。
function terminalPersistentLabel(option: TerminalAttentionOption, t: Dict): { label: string; tip: string } {
  const raw = option.label.trim();
  switch (option.scope) {
    case "session": return { label: t.chat.allowSession, tip: raw };
    case "remember": return { label: option.target ? `${t.chat.allowRemember} · ${option.target}` : t.chat.allowRemember, tip: raw };
    case "auto_mode": return { label: t.chat.allowAutoMode, tip: raw };
    default: return plainOptionLabel(option);
  }
}

/// 普通选项按钮：终端原文，太长就截断（全文在悬停提示里）。
function plainOptionLabel(option: TerminalAttentionOption): { label: string; tip: string } {
  const raw = option.label.trim();
  return { label: raw.length > 28 ? raw.slice(0, 27) + "…" : raw, tip: raw };
}

/// 选项一个都没认出来时仍然成立的按键：只有插件声明过的才有（`unparsed_keys`，都是真机验证过、
/// 与焦点位置无关或焦点位置确定的键）。没声明的不猜，卡上只留「仅收起」，去终端处理。
function unparsedKey(attention: AttentionOptions, role: OptionRole): string | null {
  if ((attention.options?.length ?? 0) > 0) return null;
  return attention.unparsedKeys?.find((entry) => entry.role === role)?.input ?? null;
}

function terminalApprovalDetails(attention: TerminalAttention) {
  switch (attention.details) {
    case "proceed_box": return { ...proceedBoxApprovalDetails(attention.text), isCommand: true };
    case "arrow_panel": {
      const details = arrowPanelApprovalDetails(attention.text);
      return { ...details, isCommand: details.tool === "Bash" };
    }
    case "framed_box": {
      const details = framedBoxApprovalDetails(attention.text);
      return { ...details, isCommand: /shell|bash/i.test(details.tool) };
    }
    case "option_row": return { ...optionRowApprovalDetails(attention.text), isCommand: false };
    default: return { tool: "", command: attention.text, description: "", question: "", isCommand: false };
  }
}

function terminalApprovalModel(request: RuntimeRequest, attention: TerminalAttention, t: Dict): ApprovalCardModel {
  const details = terminalApprovalDetails(attention);
  const options = terminalApprovalOptions(attention);
  return {
    requestId: request.requestId,
    channel: "terminal",
    state: cardStateOf(request),
    live: request.responseCapability.type === "terminal_live",
    tool: details.tool,
    scopeInput: details.command,
    description: details.description,
    question: details.question,
    detail: details.command ? { label: details.isCommand ? "command" : "input", text: details.command } : null,
    risky: !!details.command && isRiskyCommand(details.command),
    actions: {
      allowOnce: options.allowOnce !== null || unparsedKey(attention, "allow_once") !== null,
      deny: options.deny !== null || unparsedKey(attention, "deny") !== null,
      denyWithReason: options.denyWithReason !== null,
      persistent: options.persistent.map((option) => terminalPersistentLabel(option, t)),
      others: options.others.map(plainOptionLabel),
      collapse: true,
    },
  };
}

/// 审批请求 → 卡片模型。屏幕请求带 `terminal` 载荷（识别结果原样），其余是 broker 请求。
export function approvalCardModel(request: RuntimeRequest, t: Dict, timedOut = false): ApprovalCardModel {
  const attention = request.terminal as TerminalAttention | undefined;
  return attention ? terminalApprovalModel(request, attention, t) : brokerApprovalModel(request, t, timedOut);
}

/// 屏幕通道把决定换成要写进终端的按键；这个决定在屏幕上没有对应项时返回 null。
/// 「说明并拒绝」不在这里：它不是一串按键，而是分步的打字流程（见 terminalFeedbackFor）。
export function terminalInputForChoice(attention: AttentionOptions, choice: ApprovalChoice): string | null {
  const options = terminalApprovalOptions(attention);
  if (choice === "allow_once") return options.allowOnce?.input ?? unparsedKey(attention, "allow_once");
  if (choice === "deny") return options.deny?.input ?? unparsedKey(attention, "deny");
  const indexed = /^(suggestion|option):(\d+)$/.exec(choice);
  if (!indexed) return null;
  const group = indexed[1] === "suggestion" ? options.persistent : options.others;
  return group[Number(indexed[2])]?.input || null;
}

/// 「说明并拒绝」在屏幕通道上的打字流程；这一屏上没有能带说明的拒绝项时返回 null。
export function terminalFeedbackFor(attention: AttentionOptions): TypedAnswer | null {
  return terminalApprovalOptions(attention).denyWithReason?.typed ?? null;
}

/// 决定里的拒绝理由（`deny:<理由>`）；不是带理由的拒绝时返回 null。
export function denyReasonOf(choice: ApprovalChoice): string | null {
  return choice.startsWith("deny:") ? choice.slice("deny:".length) : null;
}

/// 这个决定是不是持久放行（屏幕通道据此记一笔「等二次确认页」，见 terminalFollowUp）。
export function isPersistentChoice(choice: ApprovalChoice): boolean {
  return choice.startsWith("suggestion:");
}
