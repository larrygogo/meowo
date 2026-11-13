import { useState } from "react";
import { Icon } from "../../icons/Icon";
import { icCheck, icChevronDown } from "../../icons/glyphs";
import { useMenuPopup } from "../menu";
import { SheetOnPhone } from "./BottomSheet";

export type ManagedPickerOption = { id: string; label: string };

/// 托管会话输入框里的模型/模式选择。与原生会话的模型、模式菜单同一套部件：
/// .chat-model-button 胶囊 + .dd-menu.chat-model-menu 浮层 + .chat-model-item（当前项打勾），
/// 手机窄屏换底部面板；点外面关闭、Esc 关闭并把焦点还给按钮、方向键移动都由 useMenuPopup 负责。
/// 此前用系统 <select>，弹出的是 Windows 原生下拉（白底高亮、系统字体），还会把占位「请选择」露出来
/// （leader 截图：「还是好丑」，2026-11-13）。
export function ManagedPicker({ label, value, options, disabled, primary, phone, fallback, onPick }: {
  /** 读屏与提示用的名字（「模型」「工作模式」），按钮上不显示。 */
  label: string;
  value: string | null | undefined;
  options: ManagedPickerOption[];
  disabled?: boolean;
  /** 模型用主样式（正文色加粗，同 .chat-model-select），模式弱一档。 */
  primary?: boolean;
  phone?: boolean;
  /** 当前值不在清单里时按钮上显示的字（原生给的模型名，或「模型」）；不显示占位「请选择」。 */
  fallback: string;
  onPick: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const ui = useMenuPopup({ cssPositioned: true, open, setOpen });
  const current = options.find((option) => option.id === value);
  const shown = current?.label ?? fallback;
  return <div className={"chat-model" + (primary ? " chat-model-select" : "")} ref={ui.ref} onKeyDown={ui.onKeyDown}>
    <button
      ref={ui.btnRef}
      type="button"
      className="chat-model-button"
      disabled={disabled}
      aria-label={`${label}: ${shown}`}
      aria-haspopup="menu"
      aria-expanded={open}
      data-tip={label}
      onClick={() => setOpen((value) => !value)}
    >
      <span className="chat-model-label">{shown}</span>
      <Icon glyph={icChevronDown} size={12} className="chat-model-chevron" />
    </button>
    {open && <SheetOnPhone phone={!!phone} title={label} onClose={() => setOpen(false)} ownsEscape={false}>
      <div className={phone ? "chat-sheet-list" : "dd-menu chat-model-menu"} role="menu" aria-label={label}>
        {options.map((option) => {
          const active = option.id === value;
          return <button
            type="button"
            key={option.id}
            role="menuitem"
            className={"chat-model-item" + (active ? " is-active" : "")}
            onClick={() => { setOpen(false); if (!active) onPick(option.id); }}
          >
            <span className="chat-model-item-text"><span className="chat-model-item-name">{option.label}</span></span>
            {active && <Icon glyph={icCheck} size={16} className="chat-model-check" />}
          </button>;
        })}
      </div>
    </SheetOnPhone>}
  </div>;
}
