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

export function ManagedDeliveryStatus({ deliveries, engineLive, onRestore, onRetry, onDismiss }: {
  deliveries: ManagedDeliveryDto[]; engineLive?: boolean; onRestore: (delivery: ManagedDeliveryDto) => void; onRetry: (delivery: ManagedDeliveryDto) => void; onDismiss: (delivery: ManagedDeliveryDto) => void;
}) {
  const t = useT();
  return <div className="chat-queued-list">
    {deliveries.map((delivery) => <div className="chat-queued-item" key={delivery.deliveryId}>
      <div>{delivery.origin === "lead" ? t.chat.engineLeadContinuation : delivery.state === "consumed" ? "" : delivery.text || t.chat.queuedAttachmentOnly}</div>
      <span>{t.chat.engineDeliveryState(delivery.state)}</span>
      {delivery.lastError && <span className="is-error">{formatBackendError(delivery.lastError, t.locale)}</span>}
      {(["uncertain", "failed", "consumed"].includes(delivery.state)) && <>
        {delivery.origin !== "lead" && <button type="button" className="sbtn" onClick={() => onRestore(delivery)}>{t.chat.engineRestoreDraft}</button>}
        <button type="button" className="sbtn" onClick={() => onRetry(delivery)}>{engineLive ? t.chat.engineStopRetry : t.chat.engineRetryDelivery}</button>
        <button type="button" className="sbtn" onClick={() => onDismiss(delivery)}>{engineLive ? t.chat.engineStopDismiss : t.chat.engineDismissDelivery}</button>
      </>}
    </div>)}
  </div>;
}
