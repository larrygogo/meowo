import { useCallback, useEffect, useState } from "react";
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

  return { items, refresh, upsert };
}
