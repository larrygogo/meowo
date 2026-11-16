import { useState, type MouseEvent } from "react";
import { Icon, type Glyph } from "../../icons/Icon";
import { icClose, icPencil, icRefreshCw } from "../../icons/glyphs";
import { icArrowUp, icPaperclip, icUndo } from "../../icons/glyphs.chat";
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

  // 排在最前、还没交出去的那条：它左边的「现在交给 Agent」常显，其余几条悬停或聚焦时才出现
  // （对照 Kimi Code 客户端的队列：只有队首有立即发送，整列不堆一排主按钮）。
  const firstHeld = items.find((item) => item.state === "held")?.id;
  const heldCount = items.filter((item) => item.state === "held").length;
  const tool = (label: string, glyph: Glyph, props: { "aria-disabled"?: true; onClick: (event: MouseEvent<HTMLButtonElement>) => void }, tip = label) => (
    <button type="button" className="chat-queue-tool" aria-label={label} data-tip={tip} {...props}>
      <Icon glyph={glyph} size={14} />
    </button>
  );

  return <section className="chat-queued-stack chat-prompt-queue" aria-label={t.chat.queueLabel}>
    {/* 只数还会自动发出的；投递失败的不会自己走，不算进「结束后发送」。 */}
    {heldCount > 0 && <div className="chat-queued-head"><span>{t.chat.queueHeldHead(heldCount)}</span></div>}
    {items.map((item) => <div key={item.id} className={"chat-queued-turn is-" + item.state}>
      {editing === item.id && draft ? <div className="chat-queue-editor">
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
          <button type="button" className="chat-user-action" onClick={() => setEditing(null)}>{t.chat.queueEditCancel}</button>
          <button type="button" className="chat-user-action" onClick={() => void save()}>{t.chat.queueSave}</button>
        </div>
      </div> : <>
        {item.state === "held" && <button
          type="button"
          className={"chat-queue-send" + (item.id === firstHeld ? " is-first" : "")}
          aria-label={t.chat.queueDeliver}
          data-tip={t.chat.queueDeliver}
          {...busyProps(busy, () => onDeliver(item))}
        ><Icon glyph={icArrowUp} size={14} /></button>}
        <div className="chat-queued-bubble chat-queue-bubble">
          <div className="chat-queue-text">
            {/* 点正文进编辑，与 Kimi 一致；读屏和键盘走旁边的编辑钮，这里不再做成按钮（免得两个「编辑」）。 */}
            <div className="chat-queue-body" onClick={item.state === "held" && !busy ? () => startEdit(item) : undefined}>
              {item.draft.prompt || t.chat.queuedAttachmentOnly}
            </div>
            {/* 状态写在气泡里（附件行同一位置），不挤在气泡左边把它压窄。 */}
            {item.state === "delivering" && <span className="chat-queued-tag" role="status"><span className="chat-model-spinner" aria-hidden="true" />{t.chat.queueDelivering}</span>}
            {item.state === "failed" && <span className="chat-queued-tag is-warn" role="alert">{t.chat.queueFailed}</span>}
            {item.draft.attachments.length > 0 && <span className="chat-queue-chip">
              <Icon glyph={icPaperclip} size={12} />{t.chat.queueAttachmentCount(item.draft.attachments.length)}
            </span>}
          </div>
          <span className="chat-queue-tools">
            {item.state === "held" && <>
              {tool(t.chat.queueEdit, icPencil, busyProps(busy, () => startEdit(item)))}
              {tool(t.chat.queueCancel, icClose, busyProps(busy, () => void promptQueueCancel(sessionId, item.id).then((cancelled) => {
                if (!cancelled) onError("queue/not_cancellable");
              }).catch(onError).finally(onChanged)))}
            </>}
            {item.state === "failed" && tool(t.chat.echoRetry, icRefreshCw, busyProps(busy, () => onRetry(item)))}
            {item.state !== "held" && <>
              {tool(t.chat.echoRestore, icUndo, busyProps(dismissing === item.id, () => void restore(item)))}
              {tool(t.chat.queuedDismiss, icClose, busyProps(dismissing === item.id, () => void dismiss(item)), t.chat.queuedDismissTip)}
            </>}
          </span>
        </div>
      </>}
    </div>)}
  </section>;
}

export function queueAttachments(draft: QueueDraftDto): Attachment[] {
  return draft.attachments.map((file) => ({ ...file }));
}
