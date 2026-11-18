import { useCallback, useEffect, useMemo, useState } from "react";
import { promptQueueList, type PromptQueueChangedDto, type QueuedPromptDto } from "../../api";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { remoteUi } from "../../remoteMode";

export function usePromptQueue(sessionId: number) {
  const [items, setItems] = useState<readonly QueuedPromptDto[]>([]);
  const upsert = useCallback((item: QueuedPromptDto) => {
    setItems((current) => [...current.filter((entry) => entry.id !== item.id), item].sort((a, b) => a.id - b.id));
  }, []);
  const refresh = useCallback(async () => {
    const next = await promptQueueList(sessionId);
    // 新手机页面可能连着旧宿主；不支持队列时保持空表，不把整页打挂。
    if (!Array.isArray(next)) return;
    setItems((current) => JSON.stringify(current) === JSON.stringify(next) ? current : next);
  }, [sessionId]);

  useEffect(() => {
    setItems([]);
    void refresh().catch(() => {});
  }, [refresh]);

  useTauriEvent<PromptQueueChangedDto>("prompt-queue-changed", (event) => {
    if (event.payload.sessionId === sessionId) void refresh().catch(() => {});
  });

  useEffect(() => {
    if (!remoteUi()) return;
    const timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void refresh().catch(() => {});
    }, 2_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  // 只交出属于当前会话的条目。换会话那一帧 items 还是上一个会话的（清空在 effect 里，晚一拍），
  // 旧会话的 refresh 也可能在切走后才回来；此时 ChatWindow 的投递 effect 按新会话的 sessionId
  // 认领旧会话的消息、发进新会话——用户报「排队的消息会串到别的会话里面去发送」（2026-11-18）。
  const own = useMemo(
    () => items.every((item) => item.sessionId === sessionId) ? items : items.filter((item) => item.sessionId === sessionId),
    [items, sessionId],
  );
  return { items: own, refresh, upsert };
}
