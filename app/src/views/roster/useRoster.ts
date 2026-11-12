// 名单的数据与租约：桌面主窗口（RosterShell）和手机页（PhoneRoster）共用，两端是同一张名单。
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { getLiveSessionsPage, registerApprovalConsumer, unregisterApprovalConsumer } from "../../api";
import { useBoardRefresh } from "../../hooks/useBoardRefresh";
import { useVisibleInterval } from "../../hooks/useVisibleInterval";
import { remoteUi } from "../../remoteMode";
import { noteSessionMeta } from "../chat/historyCache";
import { REMOTE_CONSUMER_HEARTBEAT_MS } from "../chat/useApprovalChannel";
import { useStarred } from "../sticker/helpers";
import type { Item } from "../sticker/types";

/** 名单一次取多少行。名单不翻页：超过这个数的老会话去「已完成 / 工作区」里找。 */
const ROSTER_LIMIT = 200;

/** 名单的行。sessions 为 null = 还没取到第一份。刷新走 board-changed（手机端是 3 秒一次的轮询）。 */
export function useRosterRows(): { sessions: Item[] | null; rows: readonly Item[] } {
  const [sessions, setSessions] = useState<Item[] | null>(null);
  const { starred } = useStarred();
  const load = useCallback(() => {
    // 手动顺序：没拖过就是创建时间倒序，任何事件都不改它（规格：顺序不随事件跳动）。
    getLiveSessionsPage("all", null, null, ROSTER_LIMIT, null, false, false, "manual")
      .then((page) => {
        // 记下名单已知的标题 / agent / 目录：点进去首读未回时，对话窗顶栏先用它（同侧栏的做法）。
        for (const row of page.items) {
          noteSessionMeta(row.session.id, { title: row.task_title, provider: row.provider, cwd: row.cwd });
        }
        setSessions(page.items);
      })
      .catch(() => {});
  }, []);
  useBoardRefresh(load);
  useEffect(load, [load]);
  // 置顶的在前（人自己动的），其余保持后端给的顺序。
  const rows = useMemo(() => {
    if (!sessions) return [];
    const pinned = sessions.filter((s) => starred.has(s.session.cc_session_id));
    return pinned.length ? [...pinned, ...sessions.filter((s) => !starred.has(s.session.cc_session_id))] : sessions;
  }, [sessions, starred]);
  return { sessions, rows };
}

/**
 * 名单租约（后端 pty.rs ROSTER_LEASE_SESSION）：名单开着时，任何会话来了审批都留在表里等人答，
 * 不把窗口切到那个会话；从某一行的现场退回名单，它挂着的审批也不会被交还终端。
 * 桌面：整个生命周期注册一次，窗口销毁时后端自己清。手机：租约 60 秒过期（锁屏 / 被杀后不能
 * 一直占着），每 20 秒续一次。active=false 时不持有（手机进了某个会话，名单页不在眼前）。
 */
export function useRosterLease(active = true): void {
  useApprovalLease(0, active);
}

/** 持有一份审批租约。sessionId = 0 是名单租约（见上）；> 0 是「正在看这条会话」的专属租约。 */
export function useApprovalLease(sessionId: number, active: boolean): void {
  useEffect(() => {
    if (!active) return;
    const consumerId = `roster-${sessionId}-${Math.random().toString(36).slice(2)}`;
    let disposed = false;
    let timer = 0;
    const register = (attempt: number) => {
      registerApprovalConsumer(sessionId, consumerId)
        .then(() => { if (disposed) void unregisterApprovalConsumer(consumerId).catch(() => {}); })
        .catch(() => {
          // 注册失败不重试到底：没有名单租约只是退回旧行为（来审批时切会话），不会丢审批。
          if (!disposed && attempt < 3) timer = window.setTimeout(() => register(attempt + 1), 500 * 2 ** attempt);
        });
    };
    register(0);
    const heartbeat = remoteUi()
      ? window.setInterval(() => { void registerApprovalConsumer(sessionId, consumerId).catch(() => {}); }, REMOTE_CONSUMER_HEARTBEAT_MS)
      : 0;
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      window.clearInterval(heartbeat);
      void unregisterApprovalConsumer(consumerId).catch(() => {});
    };
  }, [active, sessionId]);
}

/** 半分钟一格的钟。行按内容记忆，内容不变的行不重渲染，而「多久之前」是渲染时现算的：恰恰是停着
 *  等你的行最需要它在走（做空审查第二轮，问题 7）。把这一格当作行的一个属性传下去，格子变了才重画。
 *  名单每次刷新也会重新取一次，页面隐藏时不走。 */
export function useRosterClock(): number {
  const [, tick] = useState(0);
  useVisibleInterval(() => tick((n) => n + 1), 30_000);
  return Math.floor(Date.now() / 30_000);
}


