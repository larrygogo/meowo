import { useState } from "react";
import { useMenuPopup } from "../menu";
import { SheetOnPhone } from "./BottomSheet";
import { useT } from "../../i18n";

/// 输入框左下角的「+」（对照 Kimi 的 addMenu：文件 / 命令 / 提及）。斜杠命令和 @ 提及此前只能靠
/// 打字触发，不知道的人永远发现不了；收进「+」里给个入口，选了就把触发符放进输入框，后面照常
/// 走补全菜单。只剩「文件」一项时（agent 没有命令、不支持提及）退回原来的一点即选文件。
export function AddMenu({ phone, onFiles, onCommand, commandBlocked, onMention }: {
  phone: boolean;
  onFiles: () => void;
  /** 插入「/」打开命令补全；该 agent 没有可补全的命令时不传。 */
  onCommand?: () => void;
  /** 斜杠命令只在输入框开头生效：已有内容时这一项置灰。 */
  commandBlocked?: boolean;
  /** 插入「@」打开文件补全；不支持提及时不传。 */
  onMention?: () => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const ui = useMenuPopup({ cssPositioned: true, open, setOpen });
  const plus = <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>;
  if (!onCommand && !onMention) {
    return <button type="button" className="chat-attach-button" aria-label={t.chat.attach} data-tip={t.chat.attach} onClick={onFiles}>{plus}</button>;
  }
  const pick = (action: () => void) => { setOpen(false); action(); };
  const items: { key: string; name: string; desc: string; icon: JSX.Element; action: () => void; disabled?: boolean; tip?: string }[] = [
    {
      key: "files", name: t.chat.addFiles, desc: t.chat.addFilesDesc, action: onFiles,
      icon: <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="m21.4 11.6-9.2 9.2a6 6 0 0 1-8.5-8.5l9.2-9.2a4 4 0 0 1 5.7 5.7l-9.2 9.2a2 2 0 0 1-2.8-2.8l8.5-8.5" /></svg>,
    },
  ];
  if (onCommand) items.push({
    key: "command", name: t.chat.addCommand, desc: t.chat.addCommandDesc, action: onCommand,
    disabled: commandBlocked, tip: commandBlocked ? t.chat.addCommandBlocked : undefined,
    icon: <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round"><path d="M15 4 9 20" /></svg>,
  });
  if (onMention) items.push({
    key: "mention", name: t.chat.addMention, desc: t.chat.addMentionDesc, action: onMention,
    icon: <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="4" /><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8" /></svg>,
  });
  return (
    <div className="chat-model chat-add" ref={ui.ref} onKeyDown={ui.onKeyDown}>
      <button
        ref={ui.btnRef}
        type="button"
        className="chat-attach-button"
        aria-label={t.chat.addMenu}
        data-tip={t.chat.addMenu}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {plus}
      </button>
      {open && (
        <SheetOnPhone phone={phone} title={t.chat.addMenu} onClose={() => setOpen(false)} ownsEscape={false}>
          <div className={phone ? "chat-sheet-list" : "dd-menu chat-model-menu chat-add-menu"} role="menu">
            {items.map((item) => (
              <button
                type="button"
                role="menuitem"
                key={item.key}
                className="chat-model-item"
                aria-label={item.name}
                disabled={item.disabled}
                // 菜单内刻意用原生 title：TooltipLayer 对 .dd-menu 区域静默。
                title={item.tip}
                onClick={() => pick(item.action)}
              >
                <span className="chat-add-ico" aria-hidden="true">{item.icon}</span>
                <span className="chat-model-item-text">
                  <span className="chat-model-item-name">{item.name}</span>
                  <span className="chat-model-item-desc">{item.tip ?? item.desc}</span>
                </span>
              </button>
            ))}
          </div>
        </SheetOnPhone>
      )}
    </div>
  );
}
