import { memo, type ReactNode } from "react";
import { useT } from "../../i18n";
import { folderName } from "../../paths";
import { openProjectDir, openNewSessionWindow } from "../../api";
import { remoteUi } from "../../remoteMode";

/// 工作区分组头（对照 Kimi 的 WorkspaceHome）：工作区名 + 主目录 +
/// 操作入口（打开目录、新建会话）。
export const WorkspaceHeader = memo(function WorkspaceHeader({ cwd, count, attention = 0, folded, onToggle, leading, trailing }: {
  cwd: string | null;
  count: number;
  /// 组内待处理（审批/提问/阻塞）的会话数，>0 时组头显示「N 项待处理」。
  attention?: number;
  folded: boolean;
  onToggle: () => void;
  leading?: ReactNode;
  /// 行尾附加件（侧栏多选态的整组勾选圈）。
  trailing?: ReactNode;
}) {
  const t = useT();
  const name = folderName(cwd);
  return (
    <div className="chat-workspace-head">
      <button type="button" className="chat-sidebar-group-head" aria-expanded={!folded} data-tip={cwd ?? undefined} onClick={onToggle}>
        {leading}
        <span className="chat-workspace-name">{name || t.chat.sidebarNoDir}</span>
        {attention > 0 && <span className="chat-workspace-attention">{t.chat.groupAttention(attention)}</span>}
        <span className="chat-sidebar-group-count">{count}</span>
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
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" /></svg>
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
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M12 5v14M5 12h14" /></svg>
        </button>
      )}
      {trailing}
    </div>
  );
});
