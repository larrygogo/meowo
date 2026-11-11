import { memo, type ReactNode } from "react";
import { useT } from "../../i18n";
import { folderName } from "../../paths";
import { openProjectDir, openNewSessionWindow } from "../../api";
import { remoteUi } from "../../remoteMode";
import { useWorkspaceMenu } from "./WorkspaceDirectory";
import { Icon } from "../../icons/Icon";
import { icFolder, icPlus } from "../../icons/glyphs";

/// 工作区分组头（对照 Kimi 的 WorkspaceHome）：工作区名 + 主目录 +
/// 操作入口（打开目录、新建会话）。
export const WorkspaceHeader = memo(function WorkspaceHeader({ cwd, attention = 0, folded, onToggle, leading, summary, trailing, onFilter }: {
  cwd: string | null;
  /// 组内会话数。对照 Kimi 的分组头不显示，调用方仍传着，留作以后的读屏标签。
  count: number;
  /// 组内待处理（审批/提问/阻塞）的会话数，>0 时组头显示「N 项待处理」。
  attention?: number;
  folded: boolean;
  onToggle: () => void;
  leading?: ReactNode;
  /// 组名后的汇总状态点（组折起来时是这一组唯一的状态出口）。
  summary?: ReactNode;
  /// 行尾附加件（侧栏多选态的整组勾选圈）。
  trailing?: ReactNode;
  /// 右键菜单里的「只看这个工作区的会话」（侧栏的工作区筛选）。传稳定引用：组件是 memo 的。
  onFilter?: (cwd: string) => void;
}) {
  const t = useT();
  const name = folderName(cwd);
  // 右键组头出工作区菜单（对照 Kimi 分组头的 onContextmenu），与工作空间页签同一个菜单。
  const menu = useWorkspaceMenu(cwd, onFilter ?? null);
  return (
    <div className={"chat-workspace-head" + (menu.open ? " is-menu-open" : "")} onContextMenu={menu.onContextMenu}>
      <button type="button" className="chat-sidebar-group-head" aria-expanded={!folded} data-tip={cwd ?? undefined} onClick={onToggle}>
        {leading}
        <span className="chat-workspace-name">{name || t.chat.sidebarNoDir}</span>
        {summary}
        {attention > 0 && <span className="chat-workspace-attention">{t.chat.groupAttention(attention)}</span>}
      </button>
      {cwd && !remoteUi() && (
        <button
          type="button"
          className="chat-workspace-action"
          aria-label={t.sticker.openProjectDir}
          data-tip={t.sticker.openProjectDir}
          onClick={(event) => {
            event.stopPropagation();
            void openProjectDir(cwd).catch(() => {});
          }}
        >
          <Icon glyph={icFolder} size={12} />
        </button>
      )}
      {cwd && (
        <button
          type="button"
          className="chat-workspace-action"
          aria-label={t.sticker.newSession}
          data-tip={t.sticker.newSession}
          onClick={(event) => {
            event.stopPropagation();
            void openNewSessionWindow({ cwd }).catch(() => {});
          }}
        >
          <Icon glyph={icPlus} size={12} />
        </button>
      )}
      {trailing}
      {menu.render(null)}
    </div>
  );
});
