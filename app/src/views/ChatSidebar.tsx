import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getLiveSessionsPage, type LiveSession } from "../api";
import { agentAssets, tintStyle } from "../providers";
import { useT } from "../i18n";

/** 侧栏一次拉的会话数上限。列表只为切换服务，不做分页——超出的老会话去看板找。 */
const PAGE_LIMIT = 60;

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
  const [sessions, setSessions] = useState<LiveSession[]>([]);

  useEffect(() => {
    let cancelled = false;
    const load = () => getLiveSessionsPage("all", null, null, PAGE_LIMIT)
      .then((list) => { if (!cancelled) setSessions(Array.isArray(list) ? list : []); })
      .catch(() => {});
    void load();
    let un: (() => void) | undefined;
    listen("board-changed", () => void load()).then((fn) => {
      if (cancelled) fn(); else un = fn;
    }).catch(() => {});
    return () => { cancelled = true; un?.(); };
  }, []);

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
      <nav className="chat-sidebar-list" aria-label={t.chat.sidebarTitle}>
        {sessions.length === 0 && <div className="chat-sidebar-empty">{t.chat.sidebarEmpty}</div>}
        {sessions.map((item) => {
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
