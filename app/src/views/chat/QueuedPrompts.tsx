import { useState } from "react";
import { promptQueueCancel, promptQueueEdit, promptQueueFinish, type QueueDraftDto, type QueuedPromptDto } from "../../api";
import { useT } from "../../i18n";
import type { Attachment } from "./drafts";
import { busyProps } from "../../busyButton";

export function QueuedPrompts({
  sessionId,
  items,
  busy,
  onDeliver,
  onRetry,
  onRestore,
  onChanged,
  onError,
}: {
  sessionId: number;
  items: readonly QueuedPromptDto[];
  busy: boolean;
  onDeliver: (item: QueuedPromptDto) => void;
  onRetry: (item: QueuedPromptDto) => void;
  onRestore: (item: QueuedPromptDto) => void;
  onChanged: () => void;
  onError: (error: unknown) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<QueueDraftDto | null>(null);
  const [dismissing, setDismissing] = useState<number | null>(null);
  if (items.length === 0) return null;

  const dismiss = async (item: QueuedPromptDto): Promise<boolean> => {
    if (dismissing != null) return false;
    setDismissing(item.id);
    try {
      // 卡在 delivering 时原发送 Promise 可能永不返回；这里只结束宿主记账，不冒充撤回 CLI 输入。
      if (item.state === "delivering") await promptQueueFinish(sessionId, item.id, false);
      const cancelled = await promptQueueCancel(sessionId, item.id);
      if (!cancelled) onError("queue/not_cancellable");
      return cancelled;
    } catch (error) {
      onError(error);
      return false;
    } finally {
      setDismissing(null);
      onChanged();
    }
  };
  const restore = async (item: QueuedPromptDto) => {
    // 只恢复草稿，不自动重发：原写入结果不明，重试必须由用户确认。
    if (await dismiss(item)) onRestore(item);
  };

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
            }).catch(onError).finally(onChanged))}>{t.chat.queueCancel}</button>
          </>}
          {item.state === "delivering" && <>
            <span className="chat-queued-tag" role="status">{t.chat.queueDelivering}</span>
            <button type="button" className="chat-user-action" {...busyProps(dismissing === item.id, () => void restore(item))}>{t.chat.echoRestore}</button>
            <button type="button" className="chat-user-action" data-tip={t.chat.queuedDismissTip} {...busyProps(dismissing === item.id, () => void dismiss(item))}>{t.chat.queuedDismiss}</button>
          </>}
          {item.state === "failed" && <>
            <span className="chat-queued-tag is-warn" role="alert">{t.chat.queueFailed}</span>
            <button type="button" className="chat-user-action" {...busyProps(busy, () => onRetry(item))}>{t.chat.echoRetry}</button>
            <button type="button" className="chat-user-action" {...busyProps(dismissing === item.id, () => void restore(item))}>{t.chat.echoRestore}</button>
            <button type="button" className="chat-user-action" data-tip={t.chat.queuedDismissTip} {...busyProps(dismissing === item.id, () => void dismiss(item))}>{t.chat.queuedDismiss}</button>
          </>}
        </div>
      </>}
    </div>)}
  </section>;
}

export function queueAttachments(draft: QueueDraftDto): Attachment[] {
  return draft.attachments.map((file) => ({ ...file }));
}
