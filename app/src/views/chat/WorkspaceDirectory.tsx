/// 侧栏「工作空间」页签（对照 Kimi Code 的 WorkspaceDirectory）：一行一个工作区，合上的文件夹
/// 图标 + 名称，下面一行是路径；点一下就在这个工作区新建会话，悬停出现「⋯」。
///
/// Meowo 没有单独的工作区登记表：工作区就是会话用过的目录（recent_cwds，最近用过的在前）。
/// 所以 Kimi 的「重命名 / 移除工作区」这里不做——没有可改的记录。「⋯」里换成 Meowo 用得上的：
/// 只看这个工作区的会话（切回进行中并按它筛选）、打开文件夹（桌面）。

import { useT } from "../../i18n";
import { openNewSessionWindow, openProjectDir } from "../../api";
import { remoteUi } from "../../remoteMode";
import { useMenuPopup } from "../menu";

function WorkspaceRow({ cwd, label, onFilter }: { cwd: string; label: string; onFilter: (cwd: string) => void }) {
  const t = useT();
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({});
  const create = () => void openNewSessionWindow({ cwd }).catch(() => {});
  const pick = (action: () => void) => { setOpen(false); action(); };
  return (
    <div
      className={"chat-ws-dir" + (open ? " is-menu-open" : "")}
      role="button"
      tabIndex={0}
      onClick={create}
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
      <div className="dd chat-ws-dir-act" ref={ref} onKeyDown={onKeyDown} onClick={(event) => event.stopPropagation()}>
        <button
          ref={btnRef}
          type="button"
          className="chat-sidebar-item-menu"
          aria-label={t.chat.wsOptions}
          data-tip={t.chat.wsOptions}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={toggle}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="6" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18" cy="12" r="1.6" /></svg>
        </button>
        {open && (
          <div className="dd-menu" role="menu" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(create)}>{t.chat.wsNewHere}</button>
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(() => onFilter(cwd))}>{t.chat.wsShowSessions}</button>
            {!remoteUi() && (
              <button type="button" role="menuitem" className="dd-item" onClick={() => pick(() => void openProjectDir(cwd).catch(() => {}))}>{t.sticker.openProjectDir}</button>
            )}
          </div>
        )}
      </div>
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
