import type { ReactNode } from "react";
import { useT } from "../../i18n";
import { SheetOnPhone } from "./BottomSheet";
import { Icon } from "../../icons/Icon";
import { icMenu } from "../../icons/glyphs.chat";
import { icCheck } from "../../icons/glyphs";

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

export function ModelMenu({ title, items, currentId, starred = [], effort, probing, unavailable, cacheHint, savesDefault, queuedNote, blocked, loadBlocked, onSelect, onLoad, onMore, onGoTerminal, phone, onClose }: {
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
  /** 在这里切换会顺带改写用户 CLI 默认配置时的提醒（chat/availability 的 savesDefaultHint）；不改的不传。 */
  savesDefault?: string | null;
  /** 此刻选了不会立刻切、会排到稍后自动执行时的说明（chat/availability 的 queued）；先说清楚再让人选。 */
  queuedNote?: string | null;
  /** 此刻选模型做不成的原因（availability 的 disabled）：模型项置灰，原因写在清单下面。 */
  blocked?: string | null;
  /** 此刻读不了模型清单的原因：读取入口置灰并写明原因，不再点了才报错。 */
  loadBlocked?: string | null;
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
  const check = <Icon glyph={icCheck} size={16} className="chat-model-check" />;
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
        // aria-disabled 而不是 disabled：项仍可用方向键走到、读屏念得到，只是点了不动。
        aria-disabled={(!active && !!blocked) || undefined}
        onClick={() => { if (active) onClose(); else if (!blocked) onSelect(item.id); }}
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
        <button type="button" role="menuitem" className="chat-model-item" aria-disabled={!!loadBlocked || undefined} onClick={() => { if (!loadBlocked) onLoad(); }}>
          <span className="chat-model-item-text">
            <span className="chat-model-item-name">{t.chat.modelLoad}</span>
            {loadBlocked && <span className="chat-model-item-desc">{loadBlocked}</span>}
          </span>
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
          {/* 只有一组时不写组名：一行组名说的就是当前引擎，纯占地方（2026-11-14 实拍「managed:kimi-code」）。 */}
          {all.length > 1 && <div className="chat-model-quick-head">{group}</div>}
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
              // 切换在途用 aria-disabled：原先 disabled 会把刚按下、正聚焦的这颗钮的焦点踢到 body
              //（走查 C-1 同类：Chromium/WebView2 对被禁用的焦点元素一律失焦）。
              aria-disabled={effort.busy || undefined}
              onClick={() => { if (effort.current !== level && !effort.busy) effort.onPick(level); }}
            >
              {effortLabel(level)}
            </button>
          ))}
        </div>
      </div>,
    );
  }
  // 排队与置灰的说明放在清单与思考档之后、缓存提醒之前：它说的是「现在点会怎样」。
  const stateNote = blocked ?? queuedNote;
  if (stateNote && items.length > 0) {
    sections.push(<div className="chat-model-quick-section chat-model-quick-hint" key="state" role="status">{stateNote}</div>);
  }
  if (savesDefault && (items.length > 0 || effort)) {
    sections.push(<div className="chat-model-quick-section chat-model-quick-hint" key="saves">{savesDefault}</div>);
  }
  if (cacheHint && items.length > 0) {
    sections.push(<div className="chat-model-quick-section chat-model-quick-hint" key="hint">{t.chat.modelCacheHint}</div>);
  }
  if (onMore) {
    sections.push(
      <div className="chat-model-quick-section" key="more">
        <button type="button" role="menuitem" className="chat-model-item chat-model-quick-more" onClick={onMore}>
          <Icon glyph={icMenu} size={14} />
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
