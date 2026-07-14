// 设置窗口的通用 UI 组件：开关、下拉、分段选择、色板、离散滑块。
// 纯展示、无业务耦合，供各 section 复用。
import { useEffect, useRef, useState, type ReactElement } from "react";
import { STICKER_COLORS, STICKER_COLOR_KEYS } from "../../appearance";

export function Switch({ checked, onChange, disabled }: { checked: boolean; onChange: () => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className={"pswitch" + (checked ? " on" : "")}
      onClick={onChange}
    >
      <span className="pswitch-knob" />
    </button>
  );
}

/**
 * 弹出层的定位与关闭。`Dropdown`（选值）与 `ActionMenu`（执行动作）共用。
 *
 * 菜单用 **fixed 定位**：`.row-card` / `.main-body` 有 overflow 裁剪，absolute 的菜单会被切掉。
 * 坐标在打开时一次性测量，故滚动后会与按钮错位 → 滚动即关（capture 捕获内层滚动）。
 * WebView 的内容无法溢出原生窗口，所以按钮靠近窗口底部时要向上翻转。
 *
 * 抽出来是因为这套东西**两份必然漂移**：改了一处翻转阈值、忘了另一处，就会出现「有的菜单被
 * 窗口底边切掉」这种只在特定滚动位置复现的怪 bug。
 */
function usePopup(itemCount: number) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top?: number; bottom?: number; right: number }>({ top: 0, right: 0 });
  const ref = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const close = () => setOpen(false);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    document.addEventListener("keydown", onKey); // Esc 关闭
    return () => {
      document.removeEventListener("mousedown", onDoc);
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  const toggle = () => {
    if (!open) {
      const r = btnRef.current?.getBoundingClientRect();
      if (r) {
        const right = Math.max(0, window.innerWidth - r.right);
        // 估算菜单高（项高约 30px + 容器内边距），下方放不下且上方空间更充裕时向上弹。
        const estHeight = itemCount * 30 + 10;
        const fitsBelow = r.bottom + 6 + estHeight <= window.innerHeight;
        if (!fitsBelow && r.top > window.innerHeight - r.bottom) {
          setPos({ bottom: window.innerHeight - r.top + 6, right });
        } else {
          setPos({ top: r.bottom + 6, right });
        }
      }
    }
    setOpen((v) => !v);
  };
  return { open, setOpen, pos, ref, btnRef, toggle };
}

/**
 * 动作菜单（`⋯`）：点一项就执行它，**没有「当前选中值」**——这是它与 `Dropdown` 的根本区别。
 *
 * 用于把一行里挤成一排的按钮收进去（账号行的 退出登录 / 重命名 / 删除）。
 */
export function ActionMenu({
  items,
  label,
  testId,
}: {
  items: { key: string; label: string; danger?: boolean; onSelect: () => void }[];
  /** 触发按钮的无障碍名（也用作 tooltip）。 */
  label: string;
  testId?: string;
}) {
  const { open, setOpen, pos, ref, btnRef, toggle } = usePopup(items.length);
  if (items.length === 0) return null;
  return (
    <div className="dd" ref={ref}>
      <button
        ref={btnRef}
        type="button"
        className="icon-btn"
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        data-tip={label}
        data-testid={testId}
        onClick={toggle}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <circle cx="5" cy="12" r="1.6" />
          <circle cx="12" cy="12" r="1.6" />
          <circle cx="19" cy="12" r="1.6" />
        </svg>
      </button>
      {open && (
        <div className="dd-menu" role="menu" style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
          {items.map((it) => (
            <button
              key={it.key}
              type="button"
              role="menuitem"
              className={"dd-item" + (it.danger ? " dd-item-danger" : "")}
              data-testid={testId ? `${testId}-${it.key}` : undefined}
              onClick={() => {
                setOpen(false);
                it.onSelect();
              }}
            >
              <span>{it.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Dropdown<T extends string | number>({
  value,
  options,
  onChange,
}: {
  value: T;
  // icon 可选：给「选择器」型下拉（如账号页的模型切换）在按钮与每个选项前挂一个徽标；
  // 不传则退化成纯文字下拉，既有调用方无需改动。
  options: { value: T; label: string; icon?: ReactElement }[];
  onChange: (v: T) => void;
}) {
  const { open, pos, ref, btnRef, toggle, setOpen } = usePopup(options.length);
  const cur = options.find((o) => o.value === value);
  return (
    <div className="dd" ref={ref}>
      <button ref={btnRef} type="button" className={"dd-btn" + (open ? " open" : "")} onClick={toggle}>
        <span className="dd-val">
          {cur?.icon && <span className="dd-ico">{cur.icon}</span>}
          <span className="dd-label">{cur?.label ?? ""}</span>
        </span>
        <svg className="dd-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {open && (
        <div className="dd-menu" role="listbox" style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
          {options.map((o) => (
            <button
              type="button"
              role="option"
              aria-selected={o.value === value}
              key={o.value}
              className={"dd-item" + (o.value === value ? " sel" : "")}
              onClick={() => { onChange(o.value); setOpen(false); }}
            >
              <span className="dd-val">
                {o.icon && <span className="dd-ico">{o.icon}</span>}
                <span className="dd-label">{o.label}</span>
              </span>
              {o.value === value && (
                <svg className="dd-check" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              )}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// 一排互斥的分段按钮（外观模式 / 界面密度）：语义上是单选，用 radiogroup/radio。
export function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          type="button"
          role="radio"
          aria-checked={o.value === value}
          key={String(o.value)}
          className={"seg-btn" + (o.value === value ? " on" : "")}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

// 贴纸颜色色板：一排圆色块（鲜亮代表色），选中加高亮描边圈；点选即换。语义上单选，用 radiogroup/radio。
export function SwatchPicker({
  value,
  onChange,
  label,
  names,
}: {
  value: string;
  onChange: (v: string) => void;
  label: string;
  names: Record<string, string>;
}) {
  return (
    <div className="swatches" role="radiogroup" aria-label={label}>
      {STICKER_COLOR_KEYS.map((k) => (
        <button
          type="button"
          role="radio"
          aria-checked={k === value}
          tabIndex={k === value ? 0 : -1}
          key={k}
          className={"swatch" + (k === value ? " sel" : "") + (k === "neutral" ? " swatch-none" : "")}
          style={{ background: STICKER_COLORS[k].swatch }}
          data-tip={names[k] ?? k}
          aria-label={names[k] ?? k}
          onClick={() => onChange(k)}
          onKeyDown={(e) => {
            const handledKeys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", " ", "Enter"];
            if (!handledKeys.includes(e.key)) return;
            e.preventDefault();

            const cur = STICKER_COLOR_KEYS.indexOf(k);
            const next =
              e.key === "Home"
                ? 0
                : e.key === "End"
                  ? STICKER_COLOR_KEYS.length - 1
                  : e.key === "ArrowLeft" || e.key === "ArrowUp"
                    ? (cur - 1 + STICKER_COLOR_KEYS.length) % STICKER_COLOR_KEYS.length
                    : (cur + 1) % STICKER_COLOR_KEYS.length;

            const nextKey = STICKER_COLOR_KEYS[next];
            if (nextKey) onChange(nextKey);

            const radios = Array.from(e.currentTarget.parentElement?.querySelectorAll<HTMLElement>("[role=radio]") ?? []);
            radios[next]?.focus();
          }}
        />
      ))}
    </div>
  );
}

// 三等分离散滑块（字体大小 小/中/大）：轨道 + 滑钮 + 底部标签。
export function FontSizeSlider({
  value,
  options,
  onChange,
  label,
}: {
  value: number;
  options: { value: number; label: string }[];
  onChange: (v: number) => void;
  label: string;
}) {
  const index = Math.max(0, options.findIndex((o) => o.value === value));
  return (
    <div className="dslider" role="radiogroup" aria-label={label}>
      <div className="dslider-track">
        <div className="dslider-knob-wrap">
          <div className="dslider-knob" style={{ left: `${(index / (options.length - 1)) * 100}%` }} />
        </div>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={o.value === value}
            className="dslider-point"
            onClick={() => onChange(o.value)}
          />
        ))}
      </div>
      <div className="dslider-labels">
        {options.map((o) => (
          <span key={o.value} className="dslider-label">{o.label}</span>
        ))}
      </div>
    </div>
  );
}
