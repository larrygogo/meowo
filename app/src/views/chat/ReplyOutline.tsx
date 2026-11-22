// 长回复的目录：AI 一次写出好几屏的分析（「问题一…问题五」）时，回复右上角给一个「目录」，点开列出小标题，
// 点哪条跳到哪条。对照 GitHub README 右上角的大纲按钮（调研 landscape.md 第 5 节）。
//
// 只在回复写完以后出现（assistant_text；流式中的 assistant_delta 不给）：边写边变的目录会一直闪。
// 按钮 sticky 贴在这条回复的顶边，往下读的过程中一直够得着；不占正文的位置，只放图标和节数，尽量少压字。
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useT } from "../../i18n";
import { Icon } from "../../icons/Icon";
import { icList } from "../../icons/glyphs.chat";

/** 正文至少这么多字、至少两个小标题才给目录：短回复一屏看得完，给了反而是噪声。 */
export const OUTLINE_MIN_CHARS = 1200;
export const OUTLINE_MIN_HEADINGS = 2;

type Heading = { el: HTMLElement; text: string; level: number };

/** 量出正文里的小标题（h1~h3）；不够条件返回空。 */
export function useOutline(body: RefObject<HTMLElement | null>, text: string): Heading[] {
  const [headings, setHeadings] = useState<Heading[]>([]);
  useLayoutEffect(() => {
    const el = body.current;
    if (!el || text.length < OUTLINE_MIN_CHARS) {
      setHeadings([]);
      return;
    }
    const found = [...el.querySelectorAll<HTMLElement>("h1, h2, h3")]
      .map((node) => ({ el: node, text: (node.textContent ?? "").trim(), level: Number(node.tagName.slice(1)) }))
      .filter((heading) => heading.text);
    setHeadings(found.length >= OUTLINE_MIN_HEADINGS ? found : []);
  }, [body, text]);
  return headings;
}

export function ReplyOutline({ headings }: { headings: Heading[] }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      if (event instanceof KeyboardEvent ? event.key === "Escape" : !wrapRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);
  if (headings.length === 0) return null;
  const top = Math.min(...headings.map((heading) => heading.level));
  return (
    <div className="chat-outline" ref={wrapRef}>
      {/* 只放图标和节数：sticky 跟着往下读时会压在行尾，按钮越小压住的字越少。 */}
      <button type="button" className="chat-outline-toggle" aria-expanded={open} aria-haspopup="true"
        aria-label={t.chat.outline(headings.length)} data-tip={t.chat.outlineLabel} onClick={() => setOpen((v) => !v)}>
        <Icon glyph={icList} size={14} />
        <span aria-hidden="true">{headings.length}</span>
      </button>
      {open && (
        <nav className="chat-outline-menu" aria-label={t.chat.outlineLabel}>
          {headings.map((heading, index) => (
            <button key={index} type="button" className="chat-outline-item" style={{ paddingLeft: `calc(var(--sp-10) + ${(heading.level - top) * 12}px)` }}
              onClick={() => {
                setOpen(false);
                heading.el.scrollIntoView({ block: "start", behavior: "smooth" });
              }}>
              {heading.text}
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}
