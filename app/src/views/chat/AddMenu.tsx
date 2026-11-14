import { useState } from "react";
import { useMenuPopup } from "../menu";
import { SheetOnPhone } from "./BottomSheet";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icAt, icPaperclip, icSlash } from "../../icons/glyphs.chat";
import { icPlus } from "../../icons/glyphs";

/// 输入框左下角的「+」（对照 Kimi 的 addMenu：文件 / 命令 / 提及）。斜杠命令和 @ 提及此前只能靠
/// 打字触发，不知道的人永远发现不了；收进「+」里给个入口，选了就把触发符放进输入框，后面照常
/// 走补全菜单。只剩「文件」一项时（agent 没有命令、不支持提及）退回原来的一点即选文件。
export function AddMenu({ phone, onFiles, onCommand, commandBlocked, onMention, disabled = false }: {
  phone: boolean;
  /** 输入框被锁（终端在等选择 / 外部占用 / 已结束 / 已被接替 / 托管已停用）时一并锁住：
   *  附件加了也发不出去，别让人以为还能附文件。 */
  disabled?: boolean;
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
  const plus = <Icon glyph={icPlus} size={20} />;
  if (!onCommand && !onMention) {
    return <button type="button" className="chat-attach-button" aria-label={t.chat.attach} data-tip={t.chat.attach} disabled={disabled} onClick={onFiles}>{plus}</button>;
  }
  // 选完先把焦点还给「+」：菜单项随菜单卸载，不接住就掉到 body（走查 C-1 的兄弟场景）。
  // 插入「/」「@」的动作随后会把焦点交给输入框，选文件则停在「+」上。手机是底部面板，不做。
  const pick = (action: () => void) => {
    if (!phone) ui.btnRef.current?.focus();
    setOpen(false);
    action();
  };
  const items: { key: string; name: string; desc: string; icon: JSX.Element; action: () => void; disabled?: boolean; tip?: string }[] = [
    {
      key: "files", name: t.chat.addFiles, desc: t.chat.addFilesDesc, action: onFiles,
      icon: <Icon glyph={icPaperclip} size={16} />,
    },
  ];
  if (onCommand) items.push({
    key: "command", name: t.chat.addCommand, desc: t.chat.addCommandDesc, action: onCommand,
    disabled: commandBlocked, tip: commandBlocked ? t.chat.addCommandBlocked : undefined,
    icon: <Icon glyph={icSlash} size={16} />,
  });
  if (onMention) items.push({
    key: "mention", name: t.chat.addMention, desc: t.chat.addMentionDesc, action: onMention,
    icon: <Icon glyph={icAt} size={16} />,
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
        disabled={disabled}
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
