import { useRef, useState } from "react";
import type { ManagedDeliveryDto, ManagedInteractionDto } from "../../api";
import { managedConversationResolve } from "../../api";
import { formatBackendError } from "../../i18n/errors";
import { useT } from "../../i18n";
import { ApprovalCard } from "./ApprovalCard";

export function ManagedInteraction({ sessionId, interaction, onResolved }: {
  sessionId: number; interaction: ManagedInteractionDto; onResolved: () => void;
}) {
  const t = useT();
  const [answers, setAnswers] = useState<Record<string, string | string[]>>({});
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const complete = interaction.questions.every((question) => {
    const value = answers[question.id];
    return question.multiple ? Array.isArray(value) && value.length > 0 : typeof value === "string" && value.length > 0;
  });
  const resolve = async (optionId: string | null) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true); setError(null);
    try {
      // interactionId 与原生 optionId 原样回传；正文发送和审批作答使用不同命令。
      await managedConversationResolve(sessionId, interaction.interactionId, optionId, interaction.questions.length ? answers : null);
      onResolved();
    } catch (reason) { setError(formatBackendError(reason, t.locale)); }
    finally { pending.current = false; setBusy(false); }
  };
  return <ApprovalCard title={interaction.title} badge={interaction.questions.length ? t.chat.questionPending : t.chat.approvalPending}>
    {interaction.detail && <pre className="chat-approval-input">{interaction.detail}</pre>}
    {interaction.questions.map((question) => <fieldset key={question.id} disabled={busy}>
      <legend>{question.title}</legend>{question.text && <p>{question.text}</p>}
      {question.multiple ? question.options.map((option) => <label key={option.id}>
        <input type="checkbox" checked={Array.isArray(answers[question.id]) && (answers[question.id] as string[]).includes(option.id)} onChange={(event) => setAnswers((current) => {
          const values = Array.isArray(current[question.id]) ? current[question.id] as string[] : [];
          return { ...current, [question.id]: event.target.checked ? [...values, option.id] : values.filter((id) => id !== option.id) };
        })} />{option.label}
      </label>) : <select aria-label={question.title} value={typeof answers[question.id] === "string" ? answers[question.id] : ""} onChange={(event) => setAnswers((current) => ({ ...current, [question.id]: event.target.value }))}>
        <option value="">{t.chat.engineChooseAnswer}</option>
        {question.options.map((option) => <option value={option.id} key={option.id}>{option.label}</option>)}
      </select>}
    </fieldset>)}
    {error && <div className="chat-send-error" role="alert">{error}</div>}
    <div className="chat-approval-options">
      {interaction.choices.map((choice) => <button type="button" className="sbtn" key={choice.id} disabled={busy || (interaction.questions.length > 0 && choice.kind === "accept" && !complete)} onClick={() => void resolve(choice.id)}>{choice.label}</button>)}
      {interaction.choices.length === 0 && <button type="button" className="sbtn" disabled={busy} onClick={() => void resolve(null)}>{t.chat.engineDismissInteraction}</button>}
    </div>
  </ApprovalCard>;
}

/// 托管会话的投递回执，照原生排队区（chat-queued-*）的样子画在对话末尾：
/// - 已发出（accepted/consumed/completed）：消息已作为气泡出现在对话里，不再重复挂一条；
/// - 排队/送达中：淡一档的用户气泡 + 小灰字状态；
/// - 结果未确认/失败：警示色状态 + 原因（悬停看）+ 小号次要按钮。
/// 此前是一条裸文本 + 三个大按钮，正常处理中也常驻（leader 截图：「这个界面怎么这么丑」，2026-11-13）。
const SETTLED = new Set(["accepted", "consumed", "completed", "cancelled"]);
export function ManagedDeliveryStatus({ deliveries, engineLive, onRestore, onRetry, onDismiss }: {
  deliveries: ManagedDeliveryDto[]; engineLive?: boolean; onRestore: (delivery: ManagedDeliveryDto) => void; onRetry: (delivery: ManagedDeliveryDto) => void; onDismiss: (delivery: ManagedDeliveryDto) => void;
}) {
  const t = useT();
  const shown = deliveries.filter((delivery) => !SETTLED.has(delivery.state));
  if (shown.length === 0) return null;
  return <section className="chat-queued-stack" aria-label={t.chat.queueLabel}>
    {shown.map((delivery) => {
      const problem = delivery.state === "uncertain" || delivery.state === "failed";
      const reason = delivery.lastError ? formatBackendError(delivery.lastError, t.locale) : undefined;
      return <div className={"chat-queued-turn" + (problem ? " is-failed" : "")} key={delivery.deliveryId}>
        <span className={"chat-queued-tag" + (problem ? " is-warn" : "")} role={problem ? "alert" : "status"} data-tip={reason}>
          {delivery.state === "sending" && <span className="chat-model-spinner" aria-hidden="true" />}
          {problem && reason ? reason : t.chat.engineDeliveryState(delivery.state)}
        </span>
        {problem && <>
          <button type="button" className="chat-user-action" onClick={() => onRetry(delivery)}>{engineLive ? t.chat.engineStopRetry : t.chat.engineRetryDelivery}</button>
          {delivery.origin !== "lead" && <button type="button" className="chat-user-action" onClick={() => onRestore(delivery)}>{t.chat.engineRestoreDraft}</button>}
        </>}
        <div className="chat-queued-bubble">{delivery.origin === "lead" ? t.chat.engineLeadContinuation : delivery.text || t.chat.queuedAttachmentOnly}</div>
        {/* 忽略同原生排队区：气泡右侧的 ×，不再是第三颗文字按钮。 */}
        {problem && <button type="button" className="chat-queued-dismiss" aria-label={engineLive ? t.chat.engineStopDismiss : t.chat.engineDismissDelivery} data-tip={engineLive ? t.chat.engineStopDismiss : t.chat.engineDismissDelivery} onClick={() => onDismiss(delivery)}>×</button>}
      </div>;
    })}
  </section>;
}
