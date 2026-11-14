/// 屏幕上认出的两类非审批请求的卡片：提问卡、提示卡。从 ChatWindow 搬出。
/// 审批走 ApprovalRequestCard（与 broker 审批同一张卡），不在这里。
///
/// 标题与徽章只由种类决定（requestCard 的 cardTitle / cardBadge），不看规则 id：
/// 此前 kimi 的提问落在提示卡上，徽章写「待批准」；gemini 额度耗尽被说成「启动步骤」。

import type { ReactNode, RefObject } from "react";
import { useT } from "../../i18n";
import type { TerminalAttention, TerminalAttentionOption } from "../../terminalAttention";
import { ApprovalCard } from "./ApprovalCard";
import { cardBadge, cardKindOfAttention, cardTitle, type CardKind } from "./requestCard";

/// 屏幕上认出的提问表单：单选点了就答完，多选逐项勾选后提交。
/// claude 的编号选择器、gemini 的框线表单、kimi 的方括号表单都渲染这一张。
export function TerminalQuestionCard({ attention, kind, live, customText, returnFocusTo, onChoose, onCustomText, onCustomSubmit, onCollapse }: {
  attention: TerminalAttention;
  /** question 或 plan（计划模式的选择器由会话的待处理状态指明）。 */
  kind: Extract<CardKind, "question" | "plan">;
  /** 终端可写（外部终端里跑的会话为 false，卡片只读）。 */
  live: boolean;
  customText: string;
  returnFocusTo: RefObject<HTMLElement | null>;
  onChoose: (option: TerminalAttentionOption) => void;
  onCustomText: (text: string) => void;
  onCustomSubmit: (option: TerminalAttentionOption) => void;
  onCollapse: () => void;
}) {
  const t = useT();
  const options = attention.options ?? [];
  return (
    <ApprovalCard
      returnFocusTo={returnFocusTo}
      className="chat-screen-approval"
      title={cardTitle(kind, t)}
      badge={cardBadge(kind, "pending", t)}
      sideActions={<button type="button" className="chat-attention-dismiss is-inline" data-tip={t.chat.attentionDismissTip} onClick={onCollapse}>{t.chat.attentionDismiss}</button>}
      actions={<>
        {options.filter((option) => option.kind === "chat").map((option) => (
          <button type="button" disabled={!live || !option.input} key={`${option.position}:${option.label}`} onClick={() => onChoose(option)}>{t.chat.chatAboutThis}</button>
        ))}
        {options.filter((option) => option.kind === "submit").map((option) => (
          // kimi 多选页的这一项是 Tab 去下一页（标签 `Next ⇥`），还不是提交，按钮照实说「下一步」。
          <button type="button" disabled={!live || !option.input} className="btn-primary" key={`${option.position}:${option.label}`} onClick={() => onChoose(option)}>{/^next\b/i.test(option.label) ? t.chat.questionNext : t.chat.submitAnswer}</button>
        ))}
      </>}
    >
      {!live && <span>{t.chat.questionExternalHint}</span>}
      {attention.text && <span className="chat-approval-prewrap">{attention.text}</span>}
      <div className="chat-approval-options">
        {options.filter((option) => option.kind === "choice").map((option, index) => (
          <button type="button" disabled={!live || !option.input} className={option.selected ? "is-selected" : ""} key={`${option.position ?? index}:${option.label}`} onClick={() => onChoose(option)}>
            <i aria-hidden="true">{option.selected ? "✓" : ""}</i>
            <span><b>{option.label}</b>{option.description && <small>{option.description}</small>}</span>
          </button>
        ))}
      </div>
      {options.filter((option) => option.kind === "input").map((option) => (
        <div className="chat-approval-custom" key={option.input}>
          <input value={customText} onChange={(event) => onCustomText(event.target.value)} placeholder={t.chat.customAnswerPlaceholder}
            // 输入法合成中的回车是选词，不是提交。
            onKeyDown={(event) => { if (event.key === "Enter" && !event.nativeEvent.isComposing) onCustomSubmit(option); }} />
          <button type="button" disabled={!live || !customText.trim() || !option.input} onClick={() => onCustomSubmit(option)}>{t.chat.addCustomAnswer}</button>
        </div>
      ))}
    </ApprovalCard>
  );
}

/// 提示卡正文里要摆出来的终端原文。
/// - 没认出选项：整段原文（按钮只有确认与取消，用户得看原文才知道在确认什么）。
/// - 「等你选择」且认出了选项：选项之前的那几行题干。标题只说了「终端正在等待你的选择」，
///   选什么得看原文（额度用完了、长会话怎么恢复、始终允许的范围）。
/// - 其余（信任页、启动步骤、计划批准、菜单）：标题已经说清是什么事，只出选项按钮。
///
/// 原文里的对话框框线（左右的竖线、整行的横线、框里的空行）是排版不是内容，一律去掉：
/// gemini 的额度耗尽框带着框线进卡片，在手机上折行后把「1. Keep trying / 2. Stop」挤出了可视区，
/// 用户看得到「确认」却看不到确认的是哪一项（2026-11-14 截图）。
export function promptContext(attention: Pick<TerminalAttention, "text" | "options">, kind: CardKind): string {
  const options = attention.options ?? [];
  if (options.length > 0 && kind !== "choice") return "";
  const lines = attention.text.split("\n");
  const firstOption = options.length === 0 ? -1
    : lines.findIndex((line) => options.some((option) => option.label && line.includes(option.label)));
  return (firstOption >= 0 ? lines.slice(0, firstOption) : lines)
    // 行首行尾的竖线连同它旁边的一格空白去掉；行内的缩进留着（选项的对齐、光标标记靠它）。
    .map((line) => line.replace(/^\s*[│┃]\s?/, "").replace(/\s*[│┃]\s*$/, "").trimEnd())
    // 整行都是框线的不是内容。U+2500–U+257F 是制表符块。
    .filter((line) => line.trim() && !/^[\s─-╿|+-]+$/u.test(line))
    .join("\n");
}

/// 提示卡：CLI 自己要用户选一项或确认一步（计划批准、额度耗尽后怎么办、长会话怎么恢复、
/// 信任页、登录步骤、刚打开的菜单）。认出了选项就一项一个按钮，按钮上是终端原文；
/// 没认出选项时给「确认」（回车）和「取消」（Esc），并把终端原文摆出来。
export function TerminalPromptCard({ attention, live, returnFocusTo, onChoose, onKey, onCollapse, children }: {
  attention: TerminalAttention;
  live: boolean;
  returnFocusTo: RefObject<HTMLElement | null>;
  onChoose: (option: TerminalAttentionOption) => void;
  /** 没认出选项时的两颗键：回车确认、Esc 取消。 */
  onKey: (input: "\r" | "\x1b") => void;
  onCollapse: () => void;
  children?: ReactNode;
}) {
  const t = useT();
  const kind = cardKindOfAttention(attention.kind);
  const options = attention.options ?? [];
  const context = promptContext(attention, kind);
  return (
    <ApprovalCard
      returnFocusTo={returnFocusTo}
      className="chat-screen-approval"
      title={cardTitle(kind, t)}
      badge={cardBadge(kind, "pending", t)}
      sideActions={<button type="button" className="chat-attention-dismiss is-inline" data-tip={t.chat.attentionDismissTip} onClick={onCollapse}>{t.chat.attentionDismiss}</button>}
      actions={options.length === 0 && <>
        {/* 取消发 Esc：回合正在跑时会把它打断，与零副作用的「仅收起」是两回事，后果写在悬停提示里。 */}
        <button type="button" disabled={!live} data-tip={t.chat.terminalPromptCancelTip} onClick={() => onKey("\x1b")}>{t.chat.terminalPromptCancel}</button>
        <button type="button" disabled={!live} className="btn-primary" onClick={() => onKey("\r")}>{t.chat.terminalPromptConfirm}</button>
      </>}
    >
      {!live && <span>{t.chat.questionExternalHint}</span>}
      {options.length === 0 && <span>{t.chat.terminalPromptHelp}</span>}
      {context && <pre>{context}</pre>}
      {options.length > 0 && <div className="chat-approval-options">
        {options.map((option, index) => (
          <button type="button" disabled={!live} className={index === 0 ? "is-recommended" : ""} key={`${index}:${option.label}`} onClick={() => onChoose(option)}>
            <span><b>{option.label}</b></span>
          </button>
        ))}
      </div>}
      {children}
    </ApprovalCard>
  );
}
