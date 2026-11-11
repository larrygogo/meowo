/**
 * 对话窗标题栏（从 ChatWindow.tsx 拆出，行为不变）：外壳 + 右侧一排工具。
 * 左侧的标题菜单绑着十几个会话动作，留在 ChatWindow 拼好后以 `title` 插槽传进来。
 *
 * 右侧照 Kimi：[标记完成] [结束会话] [用编辑器打开 ▾] [分支 ↑领先 +增-删] [PR] [终端] [侧边面板]。
 * 「标记完成」「结束会话」是 Meowo 自己的收尾动作，放在 Kimi 那组前面。
 */
import { useEffect, useState, type ReactNode } from "react";
import { getPullRequest, type GitDiffSummaryDto, type PullRequestDto } from "../../api";
import { useT } from "../../i18n";
import { remoteUi } from "../../remoteMode";
import { ArchiveIcon } from "../sticker/icons";
import { GitStatusChip, OpenInEditor, PullRequestChip } from "./TitleBarTools";
import { tipWithKeys } from "./WindowModals";

/// 当前分支的 PR（对照 Kimi 标题栏）：换目录 / 换分支时查一次，之后 3 分钟一次——gh 要联网，
/// 不跟改动摘要的轮询走（后端另按目录缓存 60s）。手机不查：桌面专用命令，桥上不放行。
function usePullRequest(cwd: string | null, branch: string | null): PullRequestDto | null {
  const [pullRequest, setPullRequest] = useState<PullRequestDto | null>(null);
  useEffect(() => {
    setPullRequest(null);
    if (!cwd || !branch || remoteUi()) return;
    let alive = true;
    const fetchPr = () => void getPullRequest(cwd).then((pr) => { if (alive) setPullRequest(pr ?? null); }).catch(() => {});
    fetchPr();
    // 窗口隐藏时不查（规格：PR/CI 轮询隐藏时 0 次），首次查询照常。
    const timer = window.setInterval(() => { if (document.visibilityState !== "hidden") fetchPr(); }, 180_000);
    return () => { alive = false; window.clearInterval(timer); };
  }, [cwd, branch]);
  return pullRequest;
}

export function ChatTitleBar({
  leading, title, archive, end, cwd, prCwd, gitSummary, changesOpen, panelOpen, terminalOn,
  onToggleChanges, onTogglePanel, onToggleTerminal, onError, panelAgentBusy = false, panelWaiting = false, info = null,
}: {
  /** 标题前的侧栏开关（macOS 侧栏收起时 / 手机紧凑形态才有）。 */
  leading: ReactNode;
  /** 标题菜单（或首读回来前的占位）。 */
  title: ReactNode;
  /** 「标记完成」直达钮；已完成 / 临时会话不给。 */
  archive: { busy: boolean; onClick: () => void } | null;
  /** 「结束会话」；不可结束时不给。 */
  end: { busy: boolean; onClick: () => void } | null;
  cwd: string | null;
  /** 查 PR 的目录：工作目录不是仓库时是第一个仓库（与标题栏分支同一个），见 ChatWindow。 */
  prCwd?: string | null;
  gitSummary: GitDiffSummaryDto | null;
  /** 停靠面板开着且在改动页签 / 开着（任意页签）。 */
  changesOpen: boolean;
  panelOpen: boolean;
  terminalOn: boolean;
  onToggleChanges: () => void;
  onTogglePanel: () => void;
  onToggleTerminal: () => void;
  onError: (message: string) => void;
  /** 面板收起着，但 agent 正在操作这个会话的面板浏览器：开关上亮一个点（同 Kimi Code）。 */
  panelAgentBusy?: boolean;
  /** 面板收起时，里面的网页在等用户答复权限请求。 */
  panelWaiting?: boolean;
  /** 会话信息栏开关（面板收起时才有，同 Kimi Code 的「显示 / 隐藏会话信息」）。 */
  info?: { open: boolean; onToggle: () => void } | null;
}) {
  const t = useT();
  const pullRequest = usePullRequest(prCwd ?? cwd, gitSummary?.isRepo ? gitSummary.branch : null);
  return (
    <header className="chat-bar" data-tauri-drag-region>
      {leading}
      {title}
      {/* 标题栏刻意不放运行状态徽标（实拍反馈嫌吵）：运行态已有多处冗余信号——
          窗口标题的 ▶ 标记、对话区底部的脉冲指示条、侧栏状态点。
          也不放任务进度入口（用户要求取消）：待办走底部停靠栏的「当前进度」，子任务在对话流里。 */}
      {/* 「标记完成」直达钮（对照 Kimi 标题栏的 markSessionDone）：做完一件事就勾掉，
          是对话页最高频的收尾动作，不该藏在标题菜单的第二层。「恢复进行中」仍在标题菜单里（低频）。 */}
      {archive && (
        <button
          type="button"
          className="chat-todo-btn"
          data-tip={t.sticker.archive}
          aria-label={t.sticker.archive}
          disabled={archive.busy}
          onClick={archive.onClick}
        >
          {/* 16px 与同排的文件/改动入口图标对齐（菜单里的同款是 13px）。 */}
          <ArchiveIcon archived={false} size={16} />
        </button>
      )}
      {/* endable 口径:托管 PTY 或进程仍活着的孤儿会话(后端按 pid 杀树兜底),见 ChatHistoryDto。
          远程不再隐藏:stop_managed_terminal 已进 /rpc 白名单(remote.rs 头注释记录了放行理由),
          确认框在手机上走 window.confirm(transport 的 confirm_dialog 替身)。 */}
      {end && (
        <button type="button" className="chat-end" disabled={end.busy} onClick={end.onClick}>
          {end.busy ? t.chat.terminalStopping : t.chat.endSession}
        </button>
      )}
      {/* 用编辑器打开会话目录（宿主执行类，远程隐藏）。 */}
      {!remoteUi() && cwd && <OpenInEditor cwd={cwd} onError={onError} />}
      {/* 分支 + 改动：点开 / 收起改动页签（远程放行改动列表，remote.rs 模块头「例外」）。 */}
      {cwd && gitSummary?.isRepo && <GitStatusChip summary={gitSummary} active={changesOpen} onToggle={onToggleChanges} />}
      {pullRequest && <PullRequestChip pr={pullRequest} />}
      {/* 终端：对话与终端两个视图之间切换（原「对话 | 终端」页签收成一颗开关，对照 Kimi）。
          远程没有终端视图（见 ChatWindow 的 setView 说明），不给死按钮。 */}
      {!remoteUi() && (
        <button
          type="button"
          className={"chat-todo-btn" + (terminalOn ? " is-active" : "")}
          data-tip={tipWithKeys(t.chat.terminal, terminalOn ? "Ctrl+1" : "Ctrl+2")}
          aria-label={t.chat.terminal}
          aria-pressed={terminalOn}
          onClick={onToggleTerminal}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <rect x="3" y="4.5" width="18" height="15" rx="3" /><path d="m7.5 9.5 3 2.5-3 2.5M13 15h4" />
          </svg>
        </button>
      )}
      {/* 侧边面板（文件 / 改动停靠面板）开合，回到上次看的页签；不是仓库就只有文件页。
          远程不放行文件页，面板只能经改动入口打开。 */}
      {info && (
        <button
          type="button"
          className={"chat-todo-btn" + (info.open ? " is-active" : "")}
          data-tip={info.open ? t.chat.sessionInfoHide : t.chat.sessionInfoShow}
          aria-label={info.open ? t.chat.sessionInfoHide : t.chat.sessionInfoShow}
          aria-pressed={info.open}
          onClick={info.onToggle}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
            <path d="M9 6h11" /><path d="M9 12h11" /><path d="M9 18h11" /><circle cx="4.5" cy="6" r="1" /><circle cx="4.5" cy="12" r="1" /><circle cx="4.5" cy="18" r="1" />
          </svg>
        </button>
      )}
      {!remoteUi() && cwd && (
        <button
          type="button"
          className={"chat-todo-btn chat-panel-toggle" + (panelOpen ? " is-active" : "")}
          data-tip={tipWithKeys(panelWaiting ? `${t.chat.browserPermissionWaiting} · ${t.chat.togglePanel}` : panelAgentBusy ? `${t.chat.browserAgentBusyTip} · ${t.chat.togglePanel}` : t.chat.togglePanel, "Ctrl+Shift+B")}
          aria-label={panelWaiting ? `${t.chat.browserPermissionWaiting} · ${t.chat.togglePanel}` : panelAgentBusy ? `${t.chat.browserAgentBusyTip} · ${t.chat.togglePanel}` : t.chat.togglePanel}
          aria-pressed={panelOpen}
          onClick={onTogglePanel}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden="true">
            <rect x="3.5" y="4.5" width="17" height="15" rx="3" /><path d="M14.5 4.5v15" />
          </svg>
          {panelWaiting
            ? <span className="chat-panel-waiting-dot" aria-hidden="true" />
            : panelAgentBusy && <span className="chat-panel-agent-dot" aria-hidden="true" />}
        </button>
      )}
    </header>
  );
}
