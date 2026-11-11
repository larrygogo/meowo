// 名单的数据与眼睛站位：桌面主窗口（RosterShell）和手机页（PhoneRoster）共用，两端是同一张名单。
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { getLiveSessionsPage, registerApprovalConsumer, unregisterApprovalConsumer } from "../../api";
import { useBoardRefresh } from "../../hooks/useBoardRefresh";
import { remoteUi } from "../../remoteMode";
import { noteSessionMeta } from "../chat/historyCache";
import { REMOTE_CONSUMER_HEARTBEAT_MS } from "../chat/useApprovalChannel";
import { useStarred } from "../sticker/helpers";
import type { Item } from "../sticker/types";
import { attentionFocus, type Attention } from "./attention";

/** 名单一次取多少行。名单不翻页：超过这个数的老会话去「已完成 / 工作区」里找。 */
const ROSTER_LIMIT = 200;
/** 眼睛回轨顶时离轨顶边的距离（眼睛中心，px）。 */
export const EYES_HOME_Y = 20;

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
  useEffect(() => {
    if (!active) return;
    const consumerId = `roster-${Math.random().toString(36).slice(2)}`;
    let disposed = false;
    let timer = 0;
    const register = (attempt: number) => {
      registerApprovalConsumer(0, consumerId)
        .then(() => { if (disposed) void unregisterApprovalConsumer(consumerId).catch(() => {}); })
        .catch(() => {
          // 注册失败不重试到底：没有名单租约只是退回旧行为（来审批时切会话），不会丢审批。
          if (!disposed && attempt < 3) timer = window.setTimeout(() => register(attempt + 1), 500 * 2 ** attempt);
        });
    };
    register(0);
    const heartbeat = remoteUi()
      ? window.setInterval(() => { void registerApprovalConsumer(0, consumerId).catch(() => {}); }, REMOTE_CONSUMER_HEARTBEAT_MS)
      : 0;
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      window.clearInterval(heartbeat);
      void unregisterApprovalConsumer(consumerId).catch(() => {});
    };
  }, [active]);
}

/** 眼睛站谁那里。记着它此刻站的会话：同样要紧的新来的不抢（见 attentionFocus）。 */
export function useAttention(rows: readonly Item[]): Attention {
  const stickyRef = useRef<number | null>(null);
  const attention = useMemo(() => attentionFocus(rows, stickyRef.current), [rows]);
  stickyRef.current = attention.id;
  return attention;
}

/**
 * 眼睛中心的高度（相对 root 顶边）。selector 找到轮到你的那一行就站到它的中线；找不到（行被折叠、
 * 滚出去了）退到 fallback 旁边，神态不变；没人需要你回轨顶。root 尺寸变化、里面任何地方滚动都重新量。
 */
export function useEyesY(root: RefObject<HTMLElement | null>, selector: string | null, fallback?: RefObject<HTMLElement | null>): number {
  const [eyesY, setEyesY] = useState(EYES_HOME_Y);
  const place = useCallback(() => {
    const el = root.current;
    if (!el) return;
    const top = el.getBoundingClientRect().top;
    const centerOf = (node: Element | null | undefined) => {
      if (!node) return null;
      const r = node.getBoundingClientRect();
      return r.height > 0 ? r.top - top + r.height / 2 : null;
    };
    const y = selector ? centerOf(el.querySelector(selector)) ?? centerOf(fallback?.current) : null;
    const max = el.clientHeight - 96;
    const next = Math.round(Math.max(EYES_HOME_Y, Math.min(y ?? EYES_HOME_Y, max)));
    setEyesY((prev) => (prev === next ? prev : next));
  }, [root, selector, fallback]);
  useLayoutEffect(place);
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    // 老内核 / 测试环境没有 ResizeObserver：退化成只在渲染和滚动时重新量。
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(el);
    // 名单或侧栏滚动时眼睛跟着那一行走（scroll 不冒泡，用捕获）。
    el.addEventListener("scroll", place, true);
    return () => {
      observer?.disconnect();
      el.removeEventListener("scroll", place, true);
    };
  }, [root, place]);
  return eyesY;
}
