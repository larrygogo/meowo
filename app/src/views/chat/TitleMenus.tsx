/**
 * 标题栏菜单（从 ChatWindow.tsx 原样搬出，行为不变）：
 * - ChatTitleMenu：标题胶囊 + 会话动作菜单（置顶/打开目录/重命名/分叉/归档）。
 * 原标题栏任务进度入口（ChatTodoMenu）已按用户要求取消：待办走底部停靠栏，子任务在对话流里。
 */
import { openProjectDir } from "../../api";
import { remoteUi } from "../../remoteMode";
import { useT } from "../../i18n";
import { useMenuPopup } from "../menu";
import { ArchiveIcon, TopIcon } from "../sticker/icons";

function MoreIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <circle cx="5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="19" cy="12" r="1.6" />
    </svg>
  );
}
import { agentAssets, tintStyle } from "../../providers";
import { folderName } from "../../paths";

/** 标题胶囊 + 会话动作菜单（Kimi 式）：标题本身是触发钮（▾），置顶/打开目录/重命名/
 *  归档都收进菜单——标题栏只留「正在看哪条 + 状态 + 视图切换」。重命名走独立 modal
 *  （onRename 只负责打开它）。 */
export function ChatTitleMenu({ title, provider, cwd, archived, archiving, starred, onToggleStar, onRename, onFork, forkDisabled, onCopyResume, onCopyConversation, onCopyFinalAnswer, onExportConversation, onToggleArchived, onSearch, t }: {
  title: string;
  provider: string;
  cwd: string | null;
  archived: boolean;
  archiving: boolean;
  starred: boolean;
  onToggleStar: () => void;
  onRename: () => void;
  /** 分叉会话（对照 Kimi 会话菜单）：该 agent 声明了原生分叉才给。 */
  onFork?: () => void;
  /** 运行中置灰：回合进行到一半，分出去的上下文是残缺的。 */
  forkDisabled?: boolean;
  /** 复制在终端里恢复本会话的命令（对照 Kimi「复制 Session ID」）。该 agent 不支持恢复时不给。 */
  onCopyResume?: () => void;
  /** 复制整段对话为 Markdown / 只复制最后一条回答（对照 Kimi 会话菜单）。没有可复制的内容时不传。 */
  onCopyConversation?: () => void;
  onCopyFinalAnswer?: () => void;
  /** 导出会话为 .md 文件（对照 Kimi「导出会话」）；宿主执行类，远程不传。 */
  onExportConversation?: () => void;
  onToggleArchived: () => void;
  /** 在对话中搜索。桌面另有 Ctrl+Shift+F，手机没有快捷键也没有命令面板，这里是唯一入口。 */
  onSearch?: () => void;
  t: ReturnType<typeof useT>;
}) {
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({ align: "left" });
  const { Icon } = agentAssets(provider);
  // 先收菜单再执行动作：动作可能弹确认/切会话，菜单不该压在那些之上。
  const pick = (action: () => void) => { setOpen(false); btnRef.current?.focus(); action(); };
  return (
    <div className="dd chat-title-menu" ref={ref} onKeyDown={onKeyDown}>
      {/* 「项目 / 标题 ⋯」面包屑（对照 Kimi 标题栏）：标题与项目都是拖窗区，动作收在 ⋯ 里。
          前面留一枚 agent 小图标：Meowo 托管多家 CLI，正在跟谁说话要一眼可见（Kimi 只有一家）。 */}
      <span className="chat-title-agent" style={tintStyle(provider, true)} aria-hidden="true" data-tauri-drag-region><Icon /></span>
      {cwd && <>
        <span className="chat-title-project" data-tip={cwd} data-tauri-drag-region>{folderName(cwd)}</span>
        <span className="chat-title-sep" aria-hidden="true" data-tauri-drag-region>/</span>
      </>}
      <span className="chat-title" data-tauri-drag-region>{title}</span>
      <button ref={btnRef} type="button" className="chat-title-btn" aria-label={t.chat.sessionActions} data-tip={t.chat.sessionActions} aria-haspopup="menu" aria-expanded={open} onClick={toggle}>
        <MoreIcon />
      </button>
      {open && (
        /* 不铺 pos.width：菜单宽随内容（.dd .dd-menu 的 min-width: max-content），
           只沿用左对齐与视口钳制。 */
        <div className="dd-menu chat-title-actions" role="menu" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, left: pos.left }}>
          <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onToggleStar)}>
            <span className="chat-title-action-ico"><TopIcon /></span>
            <span className="dd-label">{starred ? t.sticker.unstar : t.sticker.star}</span>
          </button>
          {onSearch && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onSearch)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>
              </span>
              <span className="dd-label">{t.chat.searchTranscript}</span>
            </button>
          )}
          {/* open_project_dir 开的是宿主文件管理器,/rpc 拒绝项——远程隐藏(侧栏同款动作同款门控)。 */}
          {cwd && !remoteUi() && (
            // 菜单内刻意用原生 title：TooltipLayer 对 .dd-menu 区域静默（自绘提示会糊在菜单上）。
            <button type="button" role="menuitem" className="dd-item" title={cwd} onClick={() => pick(() => void openProjectDir(cwd).catch(() => {}))}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" /></svg>
              </span>
              <span className="dd-label">{t.sticker.openProjectDir}</span>
            </button>
          )}
          <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onRename)}>
            <span className="chat-title-action-ico">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" /></svg>
            </span>
            <span className="dd-label">{t.sticker.renameTitle}</span>
          </button>
          {onFork && (
            <button type="button" role="menuitem" className="dd-item" disabled={forkDisabled} title={forkDisabled ? t.chat.forkBusy : t.chat.forkTip} onClick={() => pick(onFork)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><circle cx="6" cy="5" r="2" /><circle cx="18" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><path d="M6 7v10" /><path d="M18 7c0 5-6 5-11.5 10" /></svg>
              </span>
              <span className="dd-label">{t.chat.forkSession}</span>
            </button>
          )}
          {onCopyResume && (
            <button type="button" role="menuitem" className="dd-item" title={t.chat.resumeCommandTip} onClick={() => pick(onCopyResume)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m4 17 6-6-6-6" /><path d="M12 19h8" /></svg>
              </span>
              <span className="dd-label">{t.chat.copyResumeCommand}</span>
            </button>
          )}
          {onCopyConversation && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onCopyConversation)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></svg>
              </span>
              <span className="dd-label">{t.chat.copyConversation}</span>
            </button>
          )}
          {onCopyFinalAnswer && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onCopyFinalAnswer)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M4 6h16M4 12h10M4 18h7" /></svg>
              </span>
              <span className="dd-label">{t.chat.copyFinalAnswer}</span>
            </button>
          )}
          {onExportConversation && (
            <button type="button" role="menuitem" className="dd-item" onClick={() => pick(onExportConversation)}>
              <span className="chat-title-action-ico">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19h14" /></svg>
              </span>
              <span className="dd-label">{t.chat.exportConversation}</span>
            </button>
          )}
          <button type="button" role="menuitem" className="dd-item" disabled={archiving} onClick={() => pick(onToggleArchived)}>
            <span className="chat-title-action-ico"><ArchiveIcon archived={archived} /></span>
            <span className="dd-label">{archived ? t.sticker.unarchive : t.sticker.archive}</span>
          </button>
        </div>
      )}
    </div>
  );
}

/** 首读回来之前的标题占位：与 ChatTitleMenu 同一套版式（agent 图标 + 标题 + 项目名），
 *  数据取自会话列表已知的信息。不可点——菜单里的动作要等会话数据到了才有意义。 */
export function ChatTitlePlaceholder({ title, provider, cwd }: { title: string; provider: string; cwd: string | null }) {
  const { Icon } = agentAssets(provider);
  return (
    <div className="dd chat-title-menu" aria-busy="true" data-tauri-drag-region>
      <span className="chat-title-agent" style={tintStyle(provider, true)} aria-hidden="true" data-tauri-drag-region><Icon /></span>
      {cwd && <>
        <span className="chat-title-project" data-tip={cwd} data-tauri-drag-region>{folderName(cwd)}</span>
        <span className="chat-title-sep" aria-hidden="true" data-tauri-drag-region>/</span>
      </>}
      <span className="chat-title" data-tauri-drag-region>{title}</span>
      <span className="chat-title-btn is-placeholder" aria-hidden="true"><MoreIcon /></span>
    </div>
  );
}
