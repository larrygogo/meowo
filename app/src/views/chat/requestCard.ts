/// 请求卡的视图模型：把两条通道（broker 挂起的请求、屏幕上认出的请求）归一成卡片认的那一份。
///
/// 为什么要有这一层：此前 broker 审批一张卡（BrokerApprovalCard）、屏幕审批一张卡（写在
/// ChatWindow 里）、认不出文法的再落一张通用卡，三处各定各的标题、徽章、按钮和键盘。同一件事
/// 换一家 Provider 就换一副样子（走查 输入 A3、B3、B13）。现在差异全压在这里：
/// - 这是什么事（种类）由后端声明：broker 请求的 `kind`，屏幕规则的 `AttentionKind`；
/// - 能做什么（允许一次、拒绝、持久放行、说明并拒绝、仅收起）由通道与屏幕选项算出能力；
/// 卡片组件只拿模型渲染，不看 Provider 名字，也不看规则 id。

import type { RuntimeRequest } from "../../api";
import type { useT } from "../../i18n";
import type { AttentionKind, TerminalAttention, TerminalAttentionOption } from "../../terminalAttention";
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
/// `allow_once` 允许一次、`deny` 拒绝、`deny:<理由>` 说明并拒绝、`suggestion:<n>` 第 n 个持久放行。
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
    /// 说明并拒绝：理由能送回 Agent 才有（broker 通道）。
    denyWithReason: boolean;
    /// 持久放行，下标 n 对应决定 `suggestion:<n>`。
    persistent: { label: string; tip: string }[];
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
      collapse: false,
    },
  };
}

/// 屏幕审批框里的选项按语义归位。各家的文案（真机或源码取证，见各插件）：
/// claude `Yes` / `Yes, and don't ask again for: …` / `No, and tell Claude what to do differently (esc)`；
/// kimi `Approve once` / `Approve for this session` / `Reject`；
/// gemini `Allow once` / `Allow for this session` / `No, suggest changes (esc)`；
/// opencode `Allow once` / `Allow always` / `Reject`。
export type TerminalApprovalOptions = {
  allowOnce: TerminalAttentionOption | null;
  deny: TerminalAttentionOption | null;
  persistent: TerminalAttentionOption[];
};

const DENY_EXACT = /^(?:no|reject|deny|cancel)$/i;
const DENY_PREFIX = /^(?:no|reject|deny)\b/i;
const ALLOW_ONCE = /^(?:yes|approve once|approve|allow once|allow)$/i;
/// 要现场打字的拒绝项（另有一个干脆的拒绝项时，它不算持久放行，也不出按钮）。
const DENY_FEEDBACK = /\bfeedback\b|\bwhat to do\b|\bsuggest changes\b|^revise$/i;

export function terminalApprovalOptions(attention: Pick<TerminalAttention, "options">): TerminalApprovalOptions {
  const options = attention.options ?? [];
  // 拒绝项两轮匹配：先找干脆的（kimi 的 Reject 不能被 Reject with feedback 抢注），
  // 找不到再按词首认（claude 的长文案拒绝项只有前缀能认）。
  const deny = options.find((option) => DENY_EXACT.test(option.label.trim()))
    ?? options.find((option) => DENY_PREFIX.test(option.label.trim()))
    ?? null;
  const allowOnce = options.find((option) => option !== deny && ALLOW_ONCE.test(option.label.trim()))
    ?? options.find((option) => option !== deny)
    ?? null;
  const persistent = options.filter((option) => option !== deny && option !== allowOnce
    && !DENY_PREFIX.test(option.label.trim()) && !DENY_FEEDBACK.test(option.label.trim()));
  return { allowOnce, deny, persistent };
}

/// 持久放行按钮的名称。只记本会话的说「本会话内允许」，写进规则的说「允许并记住」，
/// 认不出范围的照搬终端原文，不替它编一个范围。悬停提示一律是终端里的原文。
function terminalPersistentLabel(option: TerminalAttentionOption, attention: TerminalAttention, t: Dict): { label: string; tip: string } {
  const raw = option.label.trim();
  // kimi、gemini 的记住项原文就写着 this session；横排选项（opencode）的 Allow always 到进程
  // 重启前有效（确认页原文 `until OpenCode is restarted`），一个会话就是一个进程，同样按本会话说。
  if (/\bthis session\b/i.test(raw) || (attention.details === "option_row" && /\balways\b/i.test(raw))) {
    return { label: t.chat.allowSession, tip: raw };
  }
  if (/don['’]t ask again|\balways\b|\bfuture\b|\bremember\b/i.test(raw)) {
    const target = raw.match(/for:\s*(.+)$/i)?.[1];
    return { label: target ? `${t.chat.allowRemember} · ${target}` : t.chat.allowRemember, tip: raw };
  }
  // claude 2.1.325 的第三项 `Yes, and switch to auto mode · …`（真机取证 2026-11-14）。
  if (/\bswitch to auto mode\b/i.test(raw)) return { label: t.chat.allowAutoMode, tip: raw };
  // 认不出范围：照搬终端原文，太长就截断（全文在悬停提示里）。
  return { label: raw.length > 28 ? raw.slice(0, 27) + "…" : raw, tip: raw };
}

/// 选项一个都没认出来时，能不能按「回车 = 默认的允许一次、Esc = 拒绝」兜底。
/// 只对验证过这两键的文法兜底：横排选项（opencode 真机：回车选第一项、Esc 是 Reject）、
/// 框线对话框（gemini：拒绝项原文带 `(esc)`，焦点默认在第一项）、没有声明文法的规则（此前的
/// 通用卡就是这两键）。框式（claude）与面板式（kimi）认不出选项多半是面板滚出了屏幕，
/// 这时不猜按键，卡上只留「仅收起」，去终端处理。
function fallbackKeys(attention: Pick<TerminalAttention, "options" | "details">): boolean {
  if ((attention.options?.length ?? 0) > 0) return false;
  return attention.details === "option_row" || attention.details === "framed_box"
    || attention.details === undefined || attention.details === "none";
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
  const keysFallback = fallbackKeys(attention);
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
      allowOnce: keysFallback || options.allowOnce !== null,
      deny: keysFallback || options.deny !== null,
      denyWithReason: false,
      persistent: options.persistent.map((option) => terminalPersistentLabel(option, attention, t)),
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
export function terminalInputForChoice(attention: Pick<TerminalAttention, "options" | "details">, choice: ApprovalChoice): string | null {
  const unparsed = fallbackKeys(attention);
  const options = terminalApprovalOptions(attention);
  if (choice === "allow_once") return unparsed ? "\r" : options.allowOnce?.input ?? null;
  if (choice === "deny") return unparsed ? "\x1b" : options.deny?.input ?? null;
  const persistent = /^suggestion:(\d+)$/.exec(choice);
  if (persistent) return options.persistent[Number(persistent[1])]?.input ?? null;
  return null;
}

/// 这个决定是不是持久放行（屏幕通道据此记一笔「等二次确认页」，见 terminalFollowUp）。
export function isPersistentChoice(choice: ApprovalChoice): boolean {
  return choice.startsWith("suggestion:");
}
