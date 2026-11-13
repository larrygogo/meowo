import { useState, type ReactElement } from "react";
import { useMenuPopup } from "../menu";
import { Icon } from "../../icons/Icon";
import { icCheck, icChevronDown } from "../../icons/glyphs";

/// 输入框底部的胶囊选择器 + 上弹菜单，长相与对话页 composer 的权限 / 模型胶囊同一套
/// （.chat-model-button / .chat-model-menu）。新建会话页用它摆 Agent / 模型 / 权限：此前那里是
/// 通用 Dropdown，说明行恒为红字、按钮也不是胶囊，和对话页的权限选择两副面孔（用户实拍）。

export type PillOption<T extends string> = {
  value: T;
  /** 菜单里的名字。 */
  label: string;
  /** 胶囊上的短名（对话页的 modeShortNames）；缺省用 label。 */
  short?: string;
  /** 菜单项下的一行后果说明。 */
  desc?: string;
  /** 行尾小字（模式的 CLI 原名、模型清单「以账号为准」一类）。 */
  detail?: string;
  /** 分组头（模型按 provider 分段）：与上一项不同时在它前面插一行组名。 */
  group?: string;
  /** 高风险档：胶囊与菜单项用危险色（同对话页的 is-risk）。 */
  risk?: boolean;
  icon?: ReactElement;
};

const Chevron = () => (
  <Icon glyph={icChevronDown} size={12} className="chat-model-chevron" />
);

export function PillSelect<T extends string>({ value, options, onChange, label, icon, variant = "mode", disabled, tip, testId }: {
  value: T;
  options: readonly PillOption<T>[];
  onChange: (value: T) => void;
  /** 读屏名（维度名，如「权限」）。 */
  label: string;
  /** 胶囊前的维度图标（权限 = 盾牌）。 */
  icon?: ReactElement;
  /** mode = 权限一类（chat-mode-button）；model = 模型 / Agent 一类（chat-model-select）。 */
  variant?: "mode" | "model";
  disabled?: boolean;
  tip?: string;
  testId?: string;
}) {
  const [open, setOpen] = useState(false);
  const ui = useMenuPopup({ cssPositioned: true, open, setOpen });
  const current = options.find((option) => option.value === value);
  return (
    <div className={"chat-model" + (variant === "model" ? " chat-model-select" : "")} ref={ui.ref} onKeyDown={ui.onKeyDown} data-testid={testId}>
      <button
        ref={ui.btnRef}
        type="button"
        className={"chat-model-button" + (variant === "mode" ? " chat-mode-button" : "") + (current?.risk ? " is-risk" : "")}
        disabled={disabled}
        aria-label={label}
        aria-expanded={open}
        data-tip={tip ?? label}
        onClick={() => setOpen(!open)}
      >
        {icon && <span className="chat-mode-icon" aria-hidden="true">{icon}</span>}
        {current?.icon && <span className="chat-model-icon" aria-hidden="true">{current.icon}</span>}
        <span className="chat-model-label">{current?.short ?? current?.label ?? ""}</span>
        <Chevron />
      </button>
      {/* 分组的长清单（模型按 provider 分段）用模型菜单的限高外框、整块滚动，与对话页的模型菜单一致。 */}
      {open && (() => {
        // 两组以上才分段写组名：只有一组时组名就是当前引擎，不必再写。
        const grouped = new Set(options.map((option) => option.group).filter(Boolean)).size > 1;
        const rows = options.map((option, index) => {
          const active = option.value === value;
          const head = grouped && option.group && option.group !== options[index - 1]?.group
            ? <div className="chat-model-quick-head" key={"head:" + option.group}>{option.group}</div>
            : null;
          return [head, (
            <button
              type="button"
              role="menuitem"
              key={option.value}
              // 说明行不进名字（同对话页），读屏名字只报档名。
              aria-label={option.desc ? option.label : undefined}
              className={"chat-model-item" + (active ? " is-active" : "") + (option.risk ? " is-risk" : "")}
              onClick={() => {
                setOpen(false);
                ui.btnRef.current?.focus();
                if (!active) onChange(option.value);
              }}
            >
              {option.icon && <span className="chat-model-icon" aria-hidden="true">{option.icon}</span>}
              <span className="chat-model-item-text">
                <span className="chat-model-item-name">{option.label}</span>
                {option.desc && <span className="chat-model-item-desc">{option.desc}</span>}
              </span>
              {option.detail && <span className="chat-model-item-detail">{option.detail}</span>}
              {active && <Icon glyph={icCheck} size={16} className="chat-model-check" />}
            </button>
          )];
        });
        return (
          <div className={"dd-menu chat-model-menu" + (grouped ? " chat-model-quick" : "")} role="menu">
            {grouped ? <div className="chat-model-quick-section is-list">{rows}</div> : rows}
          </div>
        );
      })()}
    </div>
  );
}
