/// 侧栏「工作区」页签（对照 Kimi Code 的 WorkspaceDirectory）：一行一个工作区，合上的文件夹
/// 图标 + 名称，下面一行是路径；点一下就在这个工作区新建会话，悬停出现「⋯」，右键出同一个菜单。
///
/// Meowo 没有单独的工作区登记表：工作区就是会话用过的目录（recent_cwds，最近用过的在前）。
/// 所以 Kimi 的「重命名」这里不做——没有可改的记录。菜单里换成 Meowo 用得上的：
/// 只看这个工作区的会话（切回进行中并按它筛选）、打开文件夹（桌面）、从列表中移除（用户要求，
/// 2026-11-09：只是不显示，磁盘不动；之后又在这里开会话会自动回来，见 hide_workspace）。

import { useEffect, useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useT } from "../../i18n";
import { hideWorkspace, openNewSessionWindow, openProjectDir, restoreWorkspace } from "../../api";
import { remoteUi } from "../../remoteMode";
import { useMenuPopup } from "../menu";

/// 工作区菜单：工作区页签的目录行与会话列表的分组头共用。「⋯」按钮在按钮下方打开，右键
/// 在鼠标处打开（对照 Kimi 的 onContextmenu）。关闭（点外 / Esc / 滚动）走 useMenuPopup 的
/// useDismissable，与其它弹层同一套。
export function useWorkspaceMenu(cwd: string | null, onFilter: ((cwd: string) => void) | null, onHide: ((cwd: string) => void) | null = null) {
  const t = useT();
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({});
  // 右键打开时的鼠标位置；null = 从「⋯」按钮打开，用 hook 按按钮算的坐标。
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const create = () => { if (cwd) void openNewSessionWindow({ cwd }).catch(() => {}); };
  const pick = (action: () => void) => { setOpen(false); action(); };
  const onContextMenu = (event: ReactMouseEvent) => {
    if (!cwd) return;
    event.preventDefault();
    event.stopPropagation();
    // 菜单约 220×130，贴着窗口右缘 / 底边时往回收，不被裁掉。
    setAt({ x: Math.min(event.clientX, window.innerWidth - 228), y: Math.min(event.clientY, window.innerHeight - 140) });
    setOpen(true);
  };
  const render = (button: ReactNode | null) => (
    <div className="dd chat-ws-menu" ref={ref} onKeyDown={onKeyDown} onClick={(event) => event.stopPropagation()}>
      {button && (
        <button
          ref={btnRef}
          type="button"
          className="chat-sidebar-item-menu"
          aria-label={t.chat.wsOptions}
          data-tip={t.chat.wsOptions}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => { setAt(null); toggle(); }}
        >{button}</button>
      )}
      {open && cwd && (
        <div
          className="dd-menu"
          role="menu"
          ref={menuRef}
          style={at ? { position: "fixed", left: Math.max(6, at.x), top: Math.max(6, at.y) } : { position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}
        >
          <button type="button" role="menuitem" className="dd-item" onClick={() => pick(create)}>{t.chat.wsNewHere}</button>
          {onFilter && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(() => onFilter(cwd))}>{t.chat.wsShowSessions}</button>
          )}
          {!remoteUi() && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(() => void openProjectDir(cwd).catch(() => {}))}>{t.sticker.openProjectDir}</button>
          )}
          {onHide && (
            <button type="button" role="menuitem" className="dd-item" data-tip={t.chat.wsHideTip} onClick={() => pick(() => onHide(cwd))}>{t.chat.wsHide}</button>
          )}
        </div>
      )}
    </div>
  );
  return { open, onContextMenu, render };
}

const MORE = <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="6" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18" cy="12" r="1.6" /></svg>;

function WorkspaceRow({ cwd, label, onFilter, onHide }: { cwd: string; label: string; onFilter: (cwd: string) => void; onHide: (cwd: string) => void }) {
  const menu = useWorkspaceMenu(cwd, onFilter, onHide);
  const create = () => void openNewSessionWindow({ cwd }).catch(() => {});
  return (
    <div
      className={"chat-ws-dir" + (menu.open ? " is-menu-open" : "")}
      role="button"
      tabIndex={0}
      onClick={create}
      onContextMenu={menu.onContextMenu}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
        event.preventDefault();
        create();
      }}
    >
      <div className="chat-ws-dir-row">
        <svg className="chat-ws-dir-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M3.5 7v10a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2V9.5a2 2 0 0 0-2-2h-6l-2-2.5h-5a2 2 0 0 0-2 2z" />
        </svg>
        <span className="chat-ws-dir-name">{label}</span>
      </div>
      <div className="chat-ws-dir-sub">{cwd}</div>
      {/* 「⋯」：点它不触发整行的新建会话。 */}
      <div className="chat-ws-dir-act">{menu.render(MORE)}</div>
    </div>
  );
}

/// 「添加工作区…」（对照 Kimi 的 addWorkspace）：Meowo 的工作区就是会话用过的目录，没开过会话的
/// 新项目在这个页签里没有入口。桌面弹系统目录框、选好直接进新建会话页（预填它）；手机弹不出
/// 系统目录框，打开新建会话面板，用它的逐级目录浏览选。
async function addWorkspace() {
  if (remoteUi()) {
    void openNewSessionWindow().catch(() => {});
    return;
  }
  const picked = await open({ directory: true }).catch(() => null);
  if (typeof picked === "string") void openNewSessionWindow({ cwd: picked }).catch(() => {});
}

export function WorkspaceDirectory({ options, onFilter, onHidden }: {
  /// 工作区（侧栏筛选同一份清单：已按路径归一去重，同名目录带上一级消歧）。
  options: readonly { value: string; label: string }[];
  /// 「只看这个工作区的会话」：切回进行中并按它筛选。
  onFilter: (cwd: string) => void;
  /// 移除成功后让上层重拉清单。
  onHidden?: () => void;
}) {
  const t = useT();
  // 先在本地摘掉（点了立刻消失），后端失败再放回来。
  const [gone, setGone] = useState<ReadonlySet<string>>(new Set());
  const unGone = (cwd: string) => setGone((prev) => { const next = new Set(prev); next.delete(cwd); return next; });
  // 刚移除的那一个：底部撤销条 5 秒（UI 调研 shell #15：误点之后没有立即找回的办法；标记完成有 8 秒撤销，
  // 这里只是「不显示」、代价更小，5 秒够）。后端照常立刻写入，撤销时再写一次 restore，不靠延迟提交——
  // 窗口在 5 秒内关掉也不会丢掉这次移除。
  const [undo, setUndo] = useState<string | null>(null);
  useEffect(() => {
    if (!undo) return;
    const timer = window.setTimeout(() => setUndo(null), 5_000);
    return () => window.clearTimeout(timer);
  }, [undo]);
  const hide = (cwd: string) => {
    setGone((prev) => new Set(prev).add(cwd));
    setUndo(cwd);
    hideWorkspace(cwd)
      .then(() => onHidden?.())
      .catch(() => { unGone(cwd); setUndo(null); });
  };
  const restore = (cwd: string) => {
    setUndo(null);
    unGone(cwd);
    restoreWorkspace(cwd).then(() => onHidden?.()).catch(() => {});
  };
  const shown = options.filter((option) => !gone.has(option.value));
  return (
    <>
      <button type="button" className="chat-ws-add" onClick={() => void addWorkspace()}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M3.5 7v10a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2V9.5a2 2 0 0 0-2-2h-6l-2-2.5h-5a2 2 0 0 0-2 2z" /><path d="M12 11v5M9.5 13.5h5" />
        </svg>
        <span>{t.chat.wsAdd}</span>
      </button>
      {shown.length === 0
        ? <div className="chat-sidebar-empty">{t.chat.wsEmpty}</div>
        : shown.map((option) => <WorkspaceRow key={option.value} cwd={option.value} label={option.label} onFilter={onFilter} onHide={hide} />)}
      {undo && (
        <div className="chat-sidebar-error is-undo chat-ws-undo" role="status">
          <span>{t.chat.wsHidden(options.find((o) => o.value === undo)?.label ?? undo)}</span>
          <button type="button" onClick={() => restore(undo)}>{t.sticker.archiveUndo}</button>
        </div>
      )}
    </>
  );
}
