// 输入框上方的「N 条改动评论 / 清空 / 插入输入框」。
//
// walkthrough P-7（2026-11-12，wk3-review-clear）：「清空」紧挨着主按钮，误点一下全部评论就没了，没有确认也
// 不能撤销，焦点还掉到 body。现在清空后原位换成「已清空 N 条 · 撤销」，焦点落在「撤销」上；撤销把那批评论
// 原样放回（id 不变，diff 里的行标记跟着回来）。撤销条停到用户关掉、又写了新评论、或换了会话（key 按会话）。
// 不弹确认：破坏性操作「确认或可撤销二选一，优先可撤销」（第三轮设计准则）。
import { useEffect, useRef, useState } from "react";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icMessage } from "../../icons/glyphs.chat";
import { icClose } from "../../icons/glyphs";
import type { ReviewComment } from "./diffComments";

export function ReviewBar({ comments, onClear, onRestore, onInsert, onDismissFocus }: {
  comments: readonly ReviewComment[];
  onClear: () => void;
  onRestore: (list: readonly ReviewComment[]) => void;
  onInsert: () => void;
  /** 撤销条点 × 关掉后焦点去哪（对话页给输入框）。不给的话整条消失、焦点掉到 body（做空第一轮 #3，2026-11-12）。 */
  onDismissFocus?: () => void;
}) {
  const t = useT();
  const [cleared, setCleared] = useState<readonly ReviewComment[] | null>(null);
  const undoRef = useRef<HTMLButtonElement>(null);
  const insertRef = useRef<HTMLButtonElement>(null);
  const restored = useRef(false);
  // 清空后又写了新评论：撤销条让位（再撤销会把新写的那条一起覆盖掉）。
  useEffect(() => {
    if (comments.length > 0) setCleared(null);
  }, [comments.length]);
  useEffect(() => {
    if (cleared) undoRef.current?.focus();
  }, [cleared]);
  useEffect(() => {
    if (restored.current && comments.length > 0) {
      restored.current = false;
      insertRef.current?.focus();
    }
  }, [comments.length]);

  if (comments.length === 0 && cleared) {
    return (
      <div className="chat-review-bar" role="status" data-testid="review-cleared">
        <Icon glyph={icMessage} size={14} />
        <span className="chat-review-bar-label">{t.chat.reviewCommentsCleared(cleared.length)}</span>
        <button type="button" ref={undoRef} data-testid="review-undo" onClick={() => {
          restored.current = true;
          onRestore(cleared);
        }}>{t.chat.reviewCommentsUndo}</button>
        <button type="button" className="chat-review-bar-close" aria-label={t.chat.scmDismiss} data-tip={t.chat.scmDismiss} onClick={() => {
          // 先把焦点交出去再卸载：条一消失，拿着焦点的 × 跟着没了。
          onDismissFocus?.();
          setCleared(null);
        }}>
          <Icon glyph={icClose} size={12} />
        </button>
      </div>
    );
  }
  if (comments.length === 0) return null;
  return (
    <div className="chat-review-bar">
      <Icon glyph={icMessage} size={14} />
      <span className="chat-review-bar-label">{t.chat.reviewComments(comments.length)}</span>
      <button type="button" onClick={() => {
        setCleared(comments);
        onClear();
      }}>{t.chat.reviewCommentsClear}</button>
      <button type="button" ref={insertRef} className="btn-primary" onClick={onInsert}>{t.chat.reviewCommentsInsert}</button>
    </div>
  );
}
