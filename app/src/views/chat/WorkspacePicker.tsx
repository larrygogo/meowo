import { CheckIcon, ChevronDownIcon } from "../sticker/icons";
import { pathKey } from "../../paths";
import { useMenuPopup } from "../menu";
import { useT } from "../../i18n";

/// 新建会话页的工作区选择（对照 Kimi Code：输入框上方单独一个「📁 名字 ⌃」，菜单分
/// 「最近的文件夹」一节——名字白、路径灰，选中项整行底色加灰勾——再隔一条线放「选择文件夹…」）。
/// 不用通用 Dropdown：它的副标题是风险说明的红字、选中项是蓝字，放在这里路径一排全红（用户实拍）。
/// 开合、点外关、Esc、方向键都走同一个 useMenuPopup。

const baseName = (dir: string) => dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

function FolderIcon({ plus = false }: { plus?: boolean }) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
      {plus && <path d="M12 10v6M9 13h6" />}
    </svg>
  );
}

export function WorkspacePicker({ cwd, dirs, onPick, onPickFolder }: {
  /** 当前工作区（空 = 还没选）。 */
  cwd: string;
  /** 最近的文件夹（已含预填目录）。 */
  dirs: readonly string[];
  onPick: (dir: string) => void;
  /** 「选择文件夹…」：走系统目录对话框。 */
  onPickFolder: () => void;
}) {
  const t = useT();
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({ align: "left" });
  const current = cwd ? pathKey(cwd) : null;
  const choose = (action: () => void) => {
    setOpen(false);
    btnRef.current?.focus();
    action();
  };
  return (
    <div className="nsp-ws" ref={ref} onKeyDown={onKeyDown} data-testid="nsp-workspace">
      <button
        ref={btnRef}
        type="button"
        className={"nsp-ws-btn" + (open ? " open" : "") + (cwd ? "" : " is-empty")}
        aria-haspopup="listbox"
        aria-expanded={open}
        data-tip={cwd || undefined}
        onClick={toggle}
      >
        <FolderIcon />
        <span className="nsp-ws-name">{cwd ? baseName(cwd) : t.newSession.chooseWorkspace}</span>
        <ChevronDownIcon className="nsp-ws-chev" />
      </button>
      {open && (
        // 宽度随内容（路径要看全），不钉成按钮宽：pos.width 不用。
        <div className="nsp-ws-menu" role="listbox" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, left: pos.left }}>
          {dirs.length > 0 && <div className="nsp-ws-section">{t.newSession.recentFolders}</div>}
          {dirs.map((dir) => {
            const selected = pathKey(dir) === current;
            return (
              <button
                key={dir}
                type="button"
                role="option"
                aria-selected={selected}
                className={"nsp-ws-item" + (selected ? " is-selected" : "")}
                onClick={() => choose(() => onPick(dir))}
              >
                <FolderIcon />
                <span className="nsp-ws-text">
                  <span className="nsp-ws-item-name">{baseName(dir)}</span>
                  <span className="nsp-ws-item-path">{dir}</span>
                </span>
                {selected && <CheckIcon className="nsp-ws-check" />}
              </button>
            );
          })}
          {dirs.length > 0 && <div className="nsp-ws-sep" role="separator" />}
          <button type="button" role="option" aria-selected={false} className="nsp-ws-item is-action" onClick={() => choose(onPickFolder)}>
            <FolderIcon plus />
            <span className="nsp-ws-text"><span className="nsp-ws-item-name">{t.newSession.pickFolder}</span></span>
          </button>
        </div>
      )}
    </div>
  );
}
