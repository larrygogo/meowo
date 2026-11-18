import { useEffect, useRef, useState, type MouseEvent, type PointerEvent } from "react";
import { Icon, type Glyph } from "../../icons/Icon";
import { icClose, icPencil, icRefreshCw } from "../../icons/glyphs";
import { icArrowUp, icGrip, icPaperclip, icUndo } from "../../icons/glyphs.chat";
import { promptQueueCancel, promptQueueEdit, promptQueueFinish, promptQueueReorder, type QueueDraftDto, type QueuedPromptDto } from "../../api";
import { useT } from "../../i18n";
import type { Attachment } from "./drafts";
import { busyProps } from "../../busyButton";

/** 按乐观顺序重排「等待中」的几条：其余状态（投递中、失败）留在原位，只在 held 的格子里换人。 */
export function applyHeldOrder(items: readonly QueuedPromptDto[], order: readonly number[] | null): readonly QueuedPromptDto[] {
  if (!order) return items;
  const byId = new Map(items.map((item) => [item.id, item]));
  const queue = order.filter((id) => byId.get(id)?.state === "held");
  if (queue.length !== items.filter((item) => item.state === "held").length) return items;
  let next = 0;
  return items.map((item) => (item.state === "held" ? byId.get(queue[next++])! : item));
}

export function QueuedPrompts({
  sessionId,
  items: incoming,
  busy,
  onDeliver,
  onRetry,
  onRestore,
  onChanged,
  onError,
  deliverLabel,
}: {
  sessionId: number;
  items: readonly QueuedPromptDto[];
  busy: boolean;
  onDeliver: (item: QueuedPromptDto) => void;
  onRetry: (item: QueuedPromptDto) => void;
  onRestore: (item: QueuedPromptDto) => void;
  onChanged: () => void;
  onError: (error: unknown) => void;
  /** 队首发送钮的说法，按这家 CLI 收到运行中输入后怎么处理来定（下一步就读到 = 立即插入当前回合）。
   *  null = 这家运行中不收输入，交出去只会报错，不给这颗钮。缺省「现在交给 Agent」。 */
  deliverLabel?: string | null;
}) {
  const t = useT();
  const sendLabel = deliverLabel === undefined ? t.chat.queueDeliver : deliverLabel;
  // 拖拽排序（对照 Kimi Code 客户端的队列）：拖的是哪条、落在哪条的上半还是下半。
  const [dragging, setDragging] = useState<number | null>(null);
  const [drop, setDrop] = useState<{ id: number; after: boolean } | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState<QueueDraftDto | null>(null);
  const [dismissing, setDismissing] = useState<number | null>(null);
  const stackRef = useRef<HTMLElement>(null);
  const touch = useRef<{ pointerId: number; id: number; x: number; y: number; startY: number; moved: boolean; target: { id: number; after: boolean } | null } | null>(null);
  const frame = useRef<number | null>(null);
  const heldKey = incoming.filter((item) => item.state === "held").map((item) => item.id).join(",");
  // 落点吸附（U7，2026-11-18）：松手那一刻就按新顺序摆好，不等后端回执。此前松手后拖动态立刻清掉、
  // 列表却要等 promptQueueReorder 往返加 onChanged 重读才换位，中间一两帧条目弹回原处再跳到新位置。
  // 后端的新顺序一到（heldKey 变了）就以它为准；排序被拒或出错时撤回乐观顺序。
  const [optimistic, setOptimistic] = useState<number[] | null>(null);
  useEffect(() => { setOptimistic(null); }, [sessionId, heldKey]);
  const items = applyHeldOrder(incoming, optimistic);
  const cancelTouch = () => {
    touch.current = null;
    if (frame.current != null) cancelAnimationFrame(frame.current);
    frame.current = null;
  };
  useEffect(() => {
    cancelTouch();
    setDragging(null);
    setDrop(null);
    return cancelTouch;
  }, [sessionId, heldKey, busy]);
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
  // 只有还在等待的能排：正在投递、投递失败的留在原位（后端同样只认 held）。
  const heldIds = items.filter((item) => item.state === "held").map((item) => item.id);
  const reorder = async (ids: number[]) => {
    if (ids.every((id, index) => id === heldIds[index])) return;
    setOptimistic(ids);
    try {
      if (!await promptQueueReorder(sessionId, ids)) { setOptimistic(null); onError("queue/not_reorderable"); }
    } catch (error) {
      setOptimistic(null);
      onError(error);
    } finally {
      onChanged();
    }
  };
  const moveTo = (id: number, target: number, after: boolean) => {
    if (id === target) return;
    const rest = heldIds.filter((entry) => entry !== id);
    const at = rest.indexOf(target);
    if (at < 0) return;
    rest.splice(after ? at + 1 : at, 0, id);
    void reorder(rest);
  };
  const endDrag = () => { cancelTouch(); setDragging(null); setDrop(null); };
  const updateTouch = () => {
    const active = touch.current;
    const stack = stackRef.current;
    if (!active || !stack || !active.moved) return;
    let scroll: HTMLElement | null = stack.parentElement;
    while (scroll && !(scroll.scrollHeight > scroll.clientHeight && /auto|scroll/.test(getComputedStyle(scroll).overflowY))) scroll = scroll.parentElement;
    const bounds = stack.getBoundingClientRect();
    const viewport = scroll?.getBoundingClientRect();
    const top = Math.max(bounds.top, viewport?.top ?? 0);
    const bottom = Math.min(bounds.bottom, viewport?.bottom ?? window.innerHeight);
    const inside = active.x >= bounds.left && active.x <= bounds.right && active.y >= top && active.y <= bottom;
    if (scroll && inside) {
      const edge = 36;
      if (active.y < top + edge) scroll.scrollTop -= 8;
      else if (active.y > bottom - edge) scroll.scrollTop += 8;
    }
    let target: { id: number; after: boolean } | null = null;
    if (inside) {
      for (const row of stack.querySelectorAll<HTMLElement>('[data-queue-held="true"]')) {
        const box = row.getBoundingClientRect();
        target = { id: Number(row.dataset.queueId), after: active.y > box.top + box.height / 2 };
        if (!target.after) break;
      }
    }
    active.target = target;
    setDrop((old) => old?.id === target?.id && old?.after === target?.after ? old : target);
    frame.current = requestAnimationFrame(updateTouch);
  };
  // 鼠标、触摸、笔都走指针事件，不用 HTML 拖放：Windows 上对话窗为了接住文件拖入，在页面的渲染窗口上
  // 注册了自己的系统拖放目标（src-tauri/src/dropzone.rs），网页内部的 HTML 拖放跟着失效——dragover /
  // drop 根本不触发，桌面上拖了没反应（2026-11-18 用户报「排队消息的拖拽排序不生效」）。
  const touchStart = (event: PointerEvent<HTMLButtonElement>, id: number) => {
    if (busy || !event.isPrimary || (event.pointerType === "mouse" && event.button !== 0)) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    touch.current = { pointerId: event.pointerId, id, x: event.clientX, y: event.clientY, startY: event.clientY, moved: false, target: null };
  };
  const touchMove = (event: PointerEvent<HTMLButtonElement>) => {
    const active = touch.current;
    if (!active || active.pointerId !== event.pointerId) return;
    active.x = event.clientX;
    active.y = event.clientY;
    if (!active.moved && Math.abs(active.y - active.startY) >= 6) {
      active.moved = true;
      setDragging(active.id);
    }
    if (active.moved) {
      if (frame.current != null) cancelAnimationFrame(frame.current);
      updateTouch();
    }
  };
  const touchCancel = (event: PointerEvent<HTMLButtonElement>) => {
    if (touch.current && touch.current.pointerId === event.pointerId) endDrag();
  };
  const touchEnd = (event: PointerEvent<HTMLButtonElement>) => {
    const active = touch.current;
    if (!active || active.pointerId !== event.pointerId) return;
    active.x = event.clientX;
    active.y = event.clientY;
    if (frame.current != null) cancelAnimationFrame(frame.current);
    updateTouch();
    if (!busy && active.moved && active.target) moveTo(active.id, active.target.id, active.target.after);
    endDrag();
  };

  const tool = (label: string, glyph: Glyph, props: { "aria-disabled"?: true; onClick: (event: MouseEvent<HTMLButtonElement>) => void }, tip = label) => (
    <button type="button" className="chat-queue-tool" aria-label={label} data-tip={tip} {...props}>
      <Icon glyph={glyph} size={14} />
    </button>
  );

  return <section ref={stackRef} className="chat-queued-stack chat-prompt-queue" aria-label={t.chat.queueLabel}>
    {/* 只数还会自动发出的；投递失败的不会自己走，不算进「结束后发送」。 */}
    {heldCount > 0 && <div className="chat-queued-head"><span>{t.chat.queueHeldHead(heldCount)}</span></div>}
    {items.map((item) => <div
      key={item.id}
      data-queue-id={item.id}
      data-queue-held={item.state === "held"}
      className={"chat-queued-turn is-" + item.state
        + (dragging === item.id ? " is-dragging" : "")
        + (drop?.id === item.id && dragging !== item.id ? (drop.after ? " drop-after" : " drop-before") : "")}
    >
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
        {item.state === "held" && sendLabel && <button
          type="button"
          className={"chat-queue-send" + (item.id === firstHeld ? " is-first" : "")}
          aria-label={sendLabel}
          data-tip={sendLabel}
          {...busyProps(busy, () => onDeliver(item))}
        ><Icon glyph={icArrowUp} size={14} /></button>}
        <div className="chat-queued-bubble chat-queue-bubble">
          {/* 拖拽手柄（只有两条以上还在等时才有得排）。键盘：聚焦后上下方向键挪一格。 */}
          {item.state === "held" && heldIds.length > 1 && <button
            type="button"
            className="chat-queue-grip"
            aria-label={t.chat.queueDragHandle}
            data-tip={t.chat.queueDragHandle}
            onPointerDown={(event) => touchStart(event, item.id)}
            onPointerMove={touchMove}
            onPointerUp={touchEnd}
            onPointerCancel={touchCancel}
            onLostPointerCapture={touchCancel}
            onKeyDown={(event) => {
              if (busy || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
              event.preventDefault();
              const index = heldIds.indexOf(item.id);
              const target = heldIds[event.key === "ArrowUp" ? index - 1 : index + 1];
              if (target != null) moveTo(item.id, target, event.key === "ArrowDown");
            }}
          ><Icon glyph={icGrip} size={14} /></button>}
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
