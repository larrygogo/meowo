import { useEffect, useState, type UIEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getLiveSessionsPage, type LiveSession } from "../api";
import { agentAssets, tintStyle } from "../providers";
import { useT } from "../i18n";

/** 每翻一页新增的会话数。滚到底自动加载下一页，直到后端给不满为止。 */
const PAGE_LIMIT = 60;

/** board-changed 刷新的冷却窗口（ms）：该事件会三连发（命令写库通知 + db-watcher 回声 +
 *  liveness 轮询），与 App.tsx 看板刷新的 leading+trailing 节流同参数、同行为。 */
const REFRESH_THROTTLE_MS = 400;

/** cwd 末段目录名作展示，完整路径进 title。与贴纸 stk-repo 同款。 */
function folderName(cwd: string | null): string {
  if (!cwd) return "";
  return cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd;
}

/**
 * 对话窗口左侧的会话切换列表，与右侧内容列并排、占满整窗高度。数据与看板同源
 * （get_live_sessions_page），靠 board-changed 广播刷新——它已经过后端合流去抖，
 * 这里不再自设轮询。折叠状态由 ChatWindow 持有（收起后展开入口在标题栏），
 * 本组件收起时整个卸载，数据加载随之停止。
 */
export function ChatSidebar({ activeId, onSelect, onCollapse }: {
  activeId: number;
  onSelect: (id: number) => void;
  onCollapse: () => void;
}) {
  const t = useT();
  // null = 首次加载尚未完成：与「真空」区分，避免首帧误闪「暂无会话」。
  const [sessions, setSessions] = useState<LiveSession[] | null>(null);
  // 翻页用「从头取 limit 条」而不是游标：后端会把已连接的会话顶到结果最前面，
  // 按最后一项的 last_event_at 续查会漏掉/重复中间那段。整段重取也让 board-changed
  // 刷新走同一条路径，不必维护「已加载的页」这份额外状态。
  const [limit, setLimit] = useState(PAGE_LIMIT);
  const [reachedEnd, setReachedEnd] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = () => getLiveSessionsPage("all", null, null, limit)
      .then((list) => {
        if (cancelled) return;
        const rows = Array.isArray(list) ? list : [];
        setSessions(rows);
        // 后端给不满就说明没有下一页了（它内部已经补页跳过空会话）。
        setReachedEnd(rows.length < limit);
      })
      // 首载失败降级为空列表（显示「暂无会话」），而不是永远停在加载占位。
      .catch(() => { if (!cancelled) setSessions((s) => s ?? []); });
    void load();
    // board-changed 会三连发（见 REFRESH_THROTTLE_MS）：leading + trailing 节流，
    // 首个事件立即刷新，冷却窗口内的后续事件合并成窗口末尾的一次刷新。
    let timer: number | undefined;
    let lastRun = 0;
    const throttledLoad = () => {
      if (timer !== undefined) return; // trailing 已排队，本次并入
      const fire = () => {
        timer = undefined;
        lastRun = Date.now();
        void load();
      };
      const since = Date.now() - lastRun;
      if (since >= REFRESH_THROTTLE_MS) fire();
      else timer = window.setTimeout(fire, REFRESH_THROTTLE_MS - since);
    };
    let un: (() => void) | undefined;
    listen("board-changed", throttledLoad).then((fn) => {
      if (cancelled) fn(); else un = fn;
    }).catch(() => {});
    return () => { cancelled = true; un?.(); if (timer !== undefined) window.clearTimeout(timer); };
  }, [limit]);

  // 滚到底前 120px 就预取下一页。`sessions.length >= limit` 兼作防重入：上一页还在
  // 路上时 sessions 仍是旧的短列表，条件不成立，不会连着叠加好几页。
  const onScroll = (event: UIEvent<HTMLElement>) => {
    if (reachedEnd || sessions === null) return;
    const el = event.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight > 120) return;
    if (sessions.length >= limit) setLimit((n) => n + PAGE_LIMIT);
  };

  return (
    <aside className="chat-sidebar">
      {/* 窗口无系统装饰，侧栏顶部也要能拖动窗口——与右列标题栏同为 drag region。 */}
      <div className="chat-sidebar-head" data-tauri-drag-region>
        <span className="chat-sidebar-title" data-tauri-drag-region>{t.chat.sidebarTitle}</span>
        <div className="chat-sidebar-head-actions">
          <button
            type="button"
            className="chat-sidebar-toggle"
            aria-label={t.sticker.newSession}
            data-tip={t.sticker.newSession}
            onClick={() => void invoke("open_new_session_window").catch(() => {})}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 5v14M5 12h14" />
            </svg>
          </button>
          <button
            type="button"
            className="chat-sidebar-toggle"
            aria-label={t.chat.sidebarCollapse}
            data-tip={t.chat.sidebarCollapse}
            onClick={onCollapse}
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M15 6l-6 6 6 6" />
            </svg>
          </button>
        </div>
      </div>
      <nav className="chat-sidebar-list" aria-label={t.chat.sidebarTitle} onScroll={onScroll}>
        {sessions === null && <div className="chat-sidebar-empty">{t.chat.sidebarLoading}</div>}
        {sessions !== null && sessions.length === 0 && <div className="chat-sidebar-empty">{t.chat.sidebarEmpty}</div>}
        {(sessions ?? []).map((item) => {
          const Icon = agentAssets(item.provider).Icon;
          const dir = folderName(item.cwd);
          return (
            <button
              type="button"
              key={item.session.id}
              className={"chat-sidebar-item" + (item.session.id === activeId ? " is-active" : "")}
              aria-current={item.session.id === activeId ? "true" : undefined}
              title={item.task_title}
              onClick={() => onSelect(item.session.id)}
            >
              {/* 状态指示兼 agent 标识：连接=品牌色徽标，未连接=灰（与贴纸同一套）。 */}
              <span
                className={"chat-sidebar-agent-icon" + (item.connected ? "" : " is-off")}
                style={tintStyle(item.provider, item.connected)}
                role="img"
                aria-label={item.provider}
              >
                <Icon />
              </span>
              <span className="chat-sidebar-text">
                <span className="chat-sidebar-name">{item.task_title || t.sticker.waitingFirstInput}</span>
                {dir && <span className="chat-sidebar-meta" title={item.cwd ?? undefined}>{dir}</span>}
              </span>
            </button>
          );
        })}
        {/* 下一页在路上：此时 sessions 仍是上一页，长度短于 limit。 */}
        {sessions !== null && sessions.length > 0 && !reachedEnd && sessions.length < limit && (
          <div className="chat-sidebar-empty">{t.chat.sidebarLoading}</div>
        )}
      </nav>
      {/* 常驻底部：会话列表可能很长并滚动，设置入口不能跟着滚走。 */}
      <div className="chat-sidebar-footer">
        <button
          type="button"
          className="chat-sidebar-settings"
          onClick={() => void invoke("open_settings").catch(() => {})}
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7">
            <circle cx="12" cy="12" r="3.2" />
            <path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 9 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 4.6 9a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z" />
          </svg>
          <span>{t.sticker.openSettings}</span>
        </button>
      </div>
    </aside>
  );
}
