/// 已发出消息的日常维护：证据一变就对账、每秒推进计时、未确认送达时读终端画面看它是不是
/// 卡在 CLI 输入框里，以及「提交」替用户按一次回车。
///
/// 从 ChatWindow 拆出：对账规则本身在 outgoingPrompts（纯函数、不碰终端），这里是把规则
/// 接到 React 生命周期和终端上的那一层。对照 Kimi 的 prompt 状态由一处推进、视图只订阅。

import { useEffect, useRef, useState } from "react";
import { managedTerminalScreen, writeManagedTerminal, type ChatItem, type ChatUi } from "../../api";
import { outgoingStore, type OutgoingPrompt, type PromptAck } from "./outgoingPrompts";
import { composerDraftText } from "./terminalSubmit";
import { undoneInputRows } from "./undoTerminal";

export function useOutgoingUpkeep({ sessionId, outgoing, loading, items, acks, lastUserText, ended, chatUi, isActive, running, onError }: {
  sessionId: number;
  outgoing: readonly OutgoingPrompt[];
  /// 首读还没回来：items 是空的，这时对账会把切走超过 15s 的回显先判成未确认、白闪一下。
  loading: boolean;
  items: readonly ChatItem[];
  acks: readonly PromptAck[];
  lastUserText: string | null | undefined;
  ended: boolean;
  chatUi: ChatUi | null;
  /// 用户还停在这个会话（读画面是异步的，回来时可能已经切走）。
  isActive: (sessionId: number) => boolean;
  /// 此刻 CLI 在跑：提交要用插件的排队键而不是回车。
  running: () => boolean;
  onError: (error: unknown) => void;
}): { stuck: ReadonlySet<number>; submit: (id: number) => void } {
  // 对账：证据（transcript / 送达回执 / hook 记的最近输入 / 会话结束）一变就推进状态，规则见
  // outgoingPrompts.reconcileOutgoing。计时器负责「证据永不到达」的那一侧——回显 15s
  // 转未确认、「已插入」停留后收起，那时 items 不再变化，光靠依赖项这一步永远轮不到。
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (loading) return;
    outgoingStore.reconcile(sessionId, { items, acks, lastUserText, ended, now: Date.now() });
  }, [sessionId, loading, items, acks, lastUserText, ended, tick]);
  useEffect(() => {
    if (outgoing.length === 0) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 1_000);
    return () => window.clearInterval(timer);
  }, [outgoing.length]);

  // 未确认送达的回显：读一次终端画面，看这段话是不是还停在 CLI 输入框里（没按下回车）。
  // 每条只读一次；认得出输入框的插件才判（claude 的 draft_stash、opencode 的撤回输入框规格），
  // 认不出一律不算卡住，退回「重发 / 放回输入框」。
  const [stuck, setStuck] = useState<ReadonlySet<number>>(() => new Set());
  const probedRef = useRef(new Set<number>());
  useEffect(() => {
    const stash = chatUi?.draft_stash;
    const clear = chatUi?.undo?.input_clear;
    if (!stash && !clear) return;
    for (const echo of outgoing) {
      if (echo.kind !== "echo" || echo.phase !== "uncertain" || echo.attachmentOnly || probedRef.current.has(echo.id)) continue;
      probedRef.current.add(echo.id);
      const sid = sessionId;
      void managedTerminalScreen(sid).catch(() => null).then((screen) => {
        const head = echo.text.split(/\r?\n/).map((line) => line.trim()).find(Boolean)?.slice(0, 12) ?? "";
        if (!head || !isActive(sid)) return;
        const held = stash
          ? (composerDraftText(screen, stash) ?? "").startsWith(head)
          : clear != null && (undoneInputRows(screen, clear, echo.text) ?? 0) > 0;
        if (held) setStuck((prev) => new Set(prev).add(echo.id));
      });
    }
    // isActive 是读 ref 的稳定判断，不进依赖。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [outgoing, chatUi, sessionId]);

  /// 「提交」：替用户在 CLI 输入框里按一次回车，回显转回「已写入」重新计时等落盘。
  const submit = (id: number) => {
    setStuck((prev) => { const next = new Set(prev); next.delete(id); return next; });
    const input = running() ? chatUi?.queue_input ?? "\r" : "\r";
    void writeManagedTerminal(sessionId, input)
      .then(() => outgoingStore.update(sessionId, id, { phase: "pending", at: Date.now() }))
      .catch(onError);
  };
  return { stuck, submit };
}
