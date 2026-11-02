/// 侧栏「工作空间」页签（对照 Kimi Code 的 WorkspaceDirectory）：一行一个工作区，合上的文件夹
/// 图标 + 名称，下面一行是路径；点一下就在这个工作区新建会话，悬停出现「⋯」，右键出同一个菜单。
///
/// Meowo 没有单独的工作区登记表：工作区就是会话用过的目录（recent_cwds，最近用过的在前）。
/// 所以 Kimi 的「重命名 / 移除工作区」这里不做——没有可改的记录。菜单里换成 Meowo 用得上的：
/// 只看这个工作区的会话（切回进行中并按它筛选）、打开文件夹（桌面）。

import { useState, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { useT } from "../../i18n";
import { openNewSessionWindow, openProjectDir } from "../../api";
import { remoteUi } from "../../remoteMode";
import { useMenuPopup } from "../menu";

/// 工作区菜单：工作空间页签的目录行与会话列表的分组头共用。「⋯」按钮在按钮下方打开，右键
/// 在鼠标处打开（对照 Kimi 的 onContextmenu）。关闭（点外 / Esc / 滚动）走 useMenuPopup 的
/// useDismissable，与其它弹层同一套。
export function useWorkspaceMenu(cwd: string | null, onFilter: ((cwd: string) => void) | null) {
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
        </div>
      )}
    </div>
  );
  return { open, onContextMenu, render };
}

const MORE = <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="6" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18" cy="12" r="1.6" /></svg>;

function WorkspaceRow({ cwd, label, onFilter }: { cwd: string; label: string; onFilter: (cwd: string) => void }) {
  const menu = useWorkspaceMenu(cwd, onFilter);
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

export function WorkspaceDirectory({ options, onFilter }: {
  /// 工作区（侧栏筛选同一份清单：已按路径归一去重，同名目录带上一级消歧）。
  options: readonly { value: string; label: string }[];
  /// 「只看这个工作区的会话」：切回进行中并按它筛选。
  onFilter: (cwd: string) => void;
}) {
  const t = useT();
  if (options.length === 0) return <div className="chat-sidebar-empty">{t.chat.wsEmpty}</div>;
  return (
    <>
      {options.map((option) => <WorkspaceRow key={option.value} cwd={option.value} label={option.label} onFilter={onFilter} />)}
    </>
  );
}
