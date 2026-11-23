import { CheckIcon, ChevronDownIcon } from "../sticker/icons";
import { pathKey, tildePath } from "../../paths";
import { useMenuPopup } from "../menu";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icFolder, icPin } from "../../icons/glyphs";
import { icFolderPlus } from "../../icons/glyphs.chat";

/// 新建会话页的工作区选择（对照 Kimi Code：输入框上方单独一个「📁 名字 ⌃」，菜单分
/// 「最近的文件夹」一节——名字白、路径灰，选中项整行底色加灰勾——再隔一条线放「选择文件夹…」）。
/// 不用通用 Dropdown：它的副标题是风险说明的红字、选中项是蓝字，放在这里路径一排全红（用户实拍）。
/// 开合、点外关、Esc、方向键都走同一个 useMenuPopup。

const baseName = (dir: string) => dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

function FolderIcon({ plus = false }: { plus?: boolean }) {
  return (
    <Icon glyph={plus ? icFolderPlus : icFolder} size={14} />
  );
}

export function WorkspacePicker({ cwd, dirs, pinned = [], onTogglePin, onPick, onPickFolder }: {
  /** 当前工作区（空 = 还没选）。 */
  cwd: string;
  /** 最近的文件夹（已含预填目录）。 */
  dirs: readonly string[];
  /** 固定的常用工作区（设置里的 pinned_workspaces）：单列「已固定」一节排在最前。 */
  pinned?: readonly string[];
  /** 行尾图钉：固定 / 取消固定。不给就不出图钉。 */
  onTogglePin?: (dir: string, pinned: boolean) => void;
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
  // 固定的按设置里的先后单列一节（对照 VS Code「打开最近」、Windows 跳转列表的固定区）；
  // 其余仍按最近用过排。预填目录不在清单里时 dirs 已把它补在最前。
  const pinnedKeys = new Set(pinned.map(pathKey));
  const pinnedDirs = dirs.filter((dir) => pinnedKeys.has(pathKey(dir)));
  const recentDirs = dirs.filter((dir) => !pinnedKeys.has(pathKey(dir)));
  const row = (dir: string, isPinned: boolean) => {
    const selected = pathKey(dir) === current;
    // 图钉是行内第二个按钮，不能嵌进选项按钮里：外面包一层，图钉绝对定位在行尾。
    return (
      <div key={dir} className={"nsp-ws-row" + (isPinned ? " is-pinned" : "")}>
        <button
          type="button"
          role="option"
          aria-selected={selected}
          className={"nsp-ws-item" + (selected ? " is-selected" : "") + (onTogglePin ? " has-pin" : "")}
          onClick={() => choose(() => onPick(dir))}
        >
          <FolderIcon />
          <span className="nsp-ws-text">
            <span className="nsp-ws-item-name">{baseName(dir)}</span>
            <span className="nsp-ws-item-path path-tail" data-tip={dir}><bdi>{tildePath(dir)}</bdi></span>
          </span>
          {selected && <CheckIcon className="nsp-ws-check" />}
        </button>
        {onTogglePin && (
          <button
            type="button"
            className="nsp-ws-pin"
            aria-pressed={isPinned}
            aria-label={`${isPinned ? t.chat.wsUnpin : t.chat.wsPin} ${baseName(dir)}`}
            data-tip={isPinned ? t.chat.wsUnpin : t.chat.wsPinTip}
            // 只切固定，不选中、不关菜单：常是一口气固定好几个。
            onClick={(event) => { event.stopPropagation(); onTogglePin(dir, !isPinned); }}
          >
            <Icon glyph={icPin} size={14} />
          </button>
        )}
      </div>
    );
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
          {pinnedDirs.length > 0 && <div className="nsp-ws-section">{t.chat.wsPinned}</div>}
          {pinnedDirs.map((dir) => row(dir, true))}
          {recentDirs.length > 0 && <div className="nsp-ws-section">{t.newSession.recentFolders}</div>}
          {recentDirs.map((dir) => row(dir, false))}
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
