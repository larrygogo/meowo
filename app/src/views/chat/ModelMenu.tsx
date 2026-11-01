import type { ReactNode } from "react";
import { useT } from "../../i18n";
import { SheetOnPhone } from "./BottomSheet";

/** group：分段标题（kimi 配置里的 provider，如 `managed:kimi-code`）；缺省归入引擎名那段。 */
export type ModelMenuItem = { id: string; label: string; desc?: string; group?: string | null };
/** 收藏的其他引擎条目：detail 写引擎名（同名「默认模型」靠它区分）。 */
export type ModelMenuStarred = { id: string; label: string; detail: string };

/// 模型按钮的就地菜单（对照 Kimi 输入框上的模型下拉，结构逐段对齐：收藏 → 当前引擎的模型 →
/// 换模型的代价 → 更多模型…，段间分隔线；右缘对齐按钮向左展开）。
///
/// 为什么不再一点就开模型市场：最常见的操作是在同一引擎的两三个模型之间来回换，每次都
/// 弹一个带搜索框的对话框是重操作。每行只放模型名：说明文字放进悬停提示——逐行两行高时
/// 七八个模型就把菜单撑出一屏，提醒与「更多」被挤到看不见（实拍）。
/// 清单还没学到时，探测中/读不到的状态也在菜单里表达（按钮本身只负责开关）。
/// 思考程度的显示名：CLI 的档位参数是小写单词，菜单与按钮照 CLI 自己的写法首字母大写。
export function effortLabel(level: string): string {
  return level === "xhigh" ? "XHigh" : level.charAt(0).toUpperCase() + level.slice(1);
}

export function ModelMenu({ title, items, currentId, starred = [], effort, probing, unavailable, cacheHint, onSelect, onLoad, onMore, onGoTerminal, phone, onClose }: {
  /** 当前引擎名（分组头）。 */
  title: string;
  items: ModelMenuItem[];
  /** 当前模型对应的预设 id（见 currentPresetId）；对不上为 null，不打勾。 */
  currentId: string | null;
  /** 模型市场里收藏过的其他引擎条目，置顶成「已收藏」段（对照 Kimi 的 starredModels）。 */
  starred?: ModelMenuStarred[];
  /// 思考程度分段控件（对照 Kimi 菜单里的「思考 Low/High/Max」）。当前模型不支持
  /// （CLI 没报档位）或引擎不支持时不传，整段不显示。
  effort?: { levels: readonly string[]; current: string; busy: boolean; onPick: (level: string) => void };
  probing: boolean;
  /** 读不到清单时的说明；null = 不是这个状态。 */
  unavailable: string | null;
  /** 会话已有内容：换模型会让已缓存的提示词作废，提醒一句。 */
  cacheHint: boolean;
  onSelect: (id: string) => void;
  /** 清单还没学到、也没自动去学时的显式入口：点了才让 CLI 弹一次菜单读清单（会往终端写命令）。 */
  onLoad?: () => void;
  /** 有更多可选（其他引擎、需要搜索的长清单）时给「更多模型…」。 */
  onMore?: () => void;
  /** 读不到清单时去终端页用 CLI 自己的菜单切换（远程没有终端页，不给）。 */
  onGoTerminal?: () => void;
  phone: boolean;
  onClose: () => void;
}) {
  const t = useT();
  const check = <svg className="chat-model-check" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M4.5 12.5l5 5 10-11" /></svg>;
  const sections: ReactNode[] = [];
  if (starred.length > 0) {
    sections.push(
      <div className="chat-model-quick-section" key="starred">
        <div className="chat-model-quick-head">{t.chat.modelStarred}</div>
        {starred.map((item) => (
          <button type="button" key={item.id} role="menuitem" className="chat-model-item" onClick={() => onSelect(item.id)}>
            <span className="chat-model-item-name">{item.label}</span>
            <span className="chat-model-item-detail">{item.detail}</span>
          </button>
        ))}
      </div>,
    );
  }
  // 按分组成段（保持出现顺序）；状态行（探测中/读不到/读取入口）挂在最后一段里。
  const groups = new Map<string, ModelMenuItem[]>();
  for (const item of items) {
    const key = item.group || title;
    groups.set(key, [...(groups.get(key) ?? []), item]);
  }
  if (groups.size === 0) groups.set(title, []);
  const renderItem = (item: ModelMenuItem) => {
    const active = currentId === item.id;
    return (
      <button
        type="button"
        key={item.id}
        role="menuitem"
        className={"chat-model-item" + (active ? " is-active" : "")}
        data-tip={item.desc}
        onClick={() => { if (!active) onSelect(item.id); else onClose(); }}
      >
        <span className="chat-model-item-name">{item.label}</span>
        {active && check}
      </button>
    );
  };
  const status = (
    <>
      {items.length === 0 && probing && (
        <div className="chat-model-quick-status" role="status">
          <span className="chat-model-spinner" aria-hidden="true" />{t.chat.modelProbing}
        </div>
      )}
      {items.length === 0 && !probing && !unavailable && onLoad && (
        <button type="button" role="menuitem" className="chat-model-item" onClick={onLoad}>
          <span className="chat-model-item-name">{t.chat.modelLoad}</span>
        </button>
      )}
      {items.length === 0 && !probing && unavailable && (
        <div className="chat-model-quick-status">
          <span>{unavailable}</span>
          {onGoTerminal && <button type="button" role="menuitem" className="chat-model-item" onClick={onGoTerminal}>{t.chat.modelGoTerminal}</button>}
        </div>
      )}
    </>
  );
  // 多段时整块一起滚（各段各滚会出现好几个滚动条）；限高下提醒与「更多」照旧钉在底部。
  sections.push(
    <div className="chat-model-quick-section is-list" key="models">
      {[...groups.entries()].map(([group, groupItems], index, all) => (
        <div className="chat-model-quick-group" key={group}>
          <div className="chat-model-quick-head">{group}</div>
          {groupItems.map(renderItem)}
          {index === all.length - 1 && status}
        </div>
      ))}
    </div>,
  );
  if (effort) {
    sections.push(
      <div className="chat-model-quick-section chat-model-effort" key="effort">
        <span className="chat-model-effort-label">{t.chat.effortLabel}</span>
        <div className="chat-model-effort-seg" role="radiogroup" aria-label={t.chat.effortLabel} aria-busy={effort.busy || undefined}>
          {effort.levels.map((level) => (
            <button
              type="button"
              key={level}
              role="radio"
              aria-checked={effort.current === level}
              className={effort.current === level ? "is-active" : undefined}
              disabled={effort.busy}
              onClick={() => { if (effort.current !== level) effort.onPick(level); }}
            >
              {effortLabel(level)}
            </button>
          ))}
        </div>
      </div>,
    );
  }
  if (cacheHint && items.length > 0) {
    sections.push(<div className="chat-model-quick-section chat-model-quick-hint" key="hint">{t.chat.modelCacheHint}</div>);
  }
  if (onMore) {
    sections.push(
      <div className="chat-model-quick-section" key="more">
        <button type="button" role="menuitem" className="chat-model-item chat-model-quick-more" onClick={onMore}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 6h16M4 12h16M4 18h16" /></svg>
          <span className="chat-model-item-name">{t.chat.modelMore}</span>
        </button>
      </div>,
    );
  }
  return (
    <SheetOnPhone phone={phone} title={t.chat.switchModel} onClose={onClose} ownsEscape={false}>
      <div className={phone ? "chat-sheet-list chat-model-quick" : "dd-menu chat-model-menu chat-model-quick"} role="menu" aria-label={title}>
        {sections}
      </div>
    </SheetOnPhone>
  );
}
