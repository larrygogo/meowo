import { useState } from "react";
import { promptQueueCancel, promptQueueEdit, type QueueDraftDto, type QueuedPromptDto } from "../../api";
import { useT } from "../../i18n";
import type { Attachment } from "./drafts";
import { busyProps } from "../../busyButton";

export function QueuedPrompts({
  sessionId,
  items,
  busy,
  onDeliver,
  onRetry,
  onError,
}: {
  sessionId: number;
  items: readonly QueuedPromptDto[];
  busy: boolean;
  onDeliver: (item: QueuedPromptDto) => void;
  onRetry: (item: QueuedPromptDto) => void;
  onError: (error: unknown) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<QueueDraftDto | null>(null);
  if (items.length === 0) return null;

  const startEdit = (item: QueuedPromptDto) => {
    setEditing(item.id);
    setDraft({ prompt: item.draft.prompt, attachments: [...item.draft.attachments] });
  };
  const save = async () => {
    if (editing == null || !draft) return;
    try {
      if (await promptQueueEdit(sessionId, editing, draft)) setEditing(null);
      else onError("queue/not_editable");
    } catch (error) {
      onError(error);
    }
  };

  return <section className="chat-queued-stack chat-prompt-queue" aria-label={t.chat.queueLabel}>
    {items.map((item) => <div key={item.id} className={"chat-queued-turn" + (item.state === "failed" ? " is-failed" : "")}>
      {editing === item.id && draft ? <>
        <textarea
          className="chat-queue-edit"
          value={draft.prompt}
          aria-label={t.chat.queueEditLabel}
          autoFocus
          onChange={(event) => setDraft({ ...draft, prompt: event.target.value })}
          onKeyDown={(event) => {
            if (event.key === "Escape") setEditing(null);
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) void save();
          }}
        />
        {draft.attachments.length > 0 && <div className="chat-queue-attachments">{draft.attachments.map((file) => <span key={file.path}>
          {file.name}
          <button type="button" aria-label={t.chat.queueRemoveAttachment} onClick={() => setDraft({
            ...draft,
            attachments: draft.attachments.filter((entry) => entry.path !== file.path),
          })}>×</button>
        </span>)}</div>}
        <div className="chat-queue-actions">
          <button type="button" className="chat-user-action" onClick={() => void save()}>{t.chat.queueSave}</button>
          <button type="button" className="chat-user-action" onClick={() => setEditing(null)}>{t.chat.queueEditCancel}</button>
        </div>
      </> : <>
        <div className="chat-queued-bubble">{item.draft.prompt || t.chat.queuedAttachmentOnly}</div>
        {item.draft.attachments.length > 0 && <div className="chat-queue-attachment-count">{t.chat.queueAttachmentCount(item.draft.attachments.length)}</div>}
        <div className="chat-queue-actions">
          {item.state === "held" && <>
            <button type="button" className="chat-user-action" {...busyProps(busy, () => onDeliver(item))}>{t.chat.queueDeliver}</button>
            <button type="button" className="chat-user-action" {...busyProps(busy, () => startEdit(item))}>{t.chat.queueEdit}</button>
            <button type="button" className="chat-user-action" {...busyProps(busy, () => void promptQueueCancel(sessionId, item.id).then((cancelled) => {
              if (!cancelled) onError("queue/not_cancellable");
            }).catch(onError))}>{t.chat.queueCancel}</button>
          </>}
          {item.state === "delivering" && <span className="chat-queued-tag" role="status">{t.chat.queueDelivering}</span>}
          {item.state === "failed" && <>
            <span className="chat-queued-tag is-warn" role="alert">{t.chat.queueFailed}</span>
            <button type="button" className="chat-user-action" {...busyProps(busy, () => onRetry(item))}>{t.chat.echoRetry}</button>
          </>}
        </div>
      </>}
    </div>)}
  </section>;
}

export function queueAttachments(draft: QueueDraftDto): Attachment[] {
  return draft.attachments.map((file) => ({ ...file }));
}
