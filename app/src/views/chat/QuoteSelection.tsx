// 选中 AI 回复里的一段 → 浮出「引用」→ 这段以 Markdown 引用块（`> `）进输入框，用户接着写。
// 对照 ChatGPT 的「Ask ChatGPT」。它被抱怨最多的两点这里避开（调研 landscape.md 第 2 节）：
// 一次只能引用一段（这里每次引用都接在输入框已有内容后面，可以引几段、中间夹着自己的话）；
// 发出后找不回原来读到的位置（引用不滚动对话区，聚焦输入框也不带滚动；发送时对话区本来就
// 不会把已上翻的人拽到底，见 useStickToBottom）。
//
// 只认 AI 正文（.chat-message.is-assistant .chat-text）：工具行、卡片、用户自己的消息里选字不出按钮。
import { useEffect, useState, type RefObject } from "react";
import { useT } from "../../i18n";

const ASSISTANT_TEXT = ".chat-message.is-assistant .chat-text";
/** 一次引用的上限：再长就不是「对这一句有疑问」了，截断免得把整篇回复塞回去。 */
const QUOTE_MAX = 2000;

/** 选中的文字 → 引用块：每行前加 `> `，空行保留成 `>`，多余的空行收成一个。 */
export function quoteBlock(text: string): string {
  const clipped = text.length > QUOTE_MAX ? `${text.slice(0, QUOTE_MAX).trimEnd()}…` : text;
  return clipped
    .replace(/\r\n?/g, "\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim()
    .split("\n")
    .map((line) => (line.trim() ? `> ${line}` : ">"))
    .join("\n");
}

/** 把一段引用接到输入框已有内容后面：引用块前后各空一行，光标停在引用块之后接着写。 */
export function appendQuote(current: string, text: string): string {
  const body = current.replace(/\s+$/, "");
  return `${body ? `${body}\n\n` : ""}${quoteBlock(text)}\n\n`;
}

function elementOf(node: Node | null): Element | null {
  return node instanceof Element ? node : node?.parentElement ?? null;
}

type Picked = { text: string; top: number; bottom: number; left: number; right: number };

/** 当前选区若整段落在 root 里的 AI 正文中，返回选中的文字与位置；否则 null。 */
export function pickQuote(root: HTMLElement | null): Picked | null {
  const selection = typeof window !== "undefined" ? window.getSelection() : null;
  if (!root || !selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const range = selection.getRangeAt(0);
  const start = elementOf(range.startContainer)?.closest(ASSISTANT_TEXT);
  const end = elementOf(range.endContainer)?.closest(ASSISTANT_TEXT);
  if (!start || !end || !root.contains(start) || !root.contains(end)) return null;
  const text = selection.toString().trim();
  if (!text) return null;
  const rect = typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : null;
  return { text, top: rect?.top ?? 0, bottom: rect?.bottom ?? 0, left: rect?.left ?? 0, right: rect?.right ?? 0 };
}

export function QuoteSelection({ root, onQuote }: { root: RefObject<HTMLElement | null>; onQuote: (text: string) => void }) {
  const t = useT();
  const [picked, setPicked] = useState<Picked | null>(null);
  useEffect(() => {
    let frame = 0;
    // 选区在拖动中会连发很多次 selectionchange，合到下一帧算一次。
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setPicked(pickQuote(root.current)));
    };
    const scroller = root.current;
    document.addEventListener("selectionchange", update);
    scroller?.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("selectionchange", update);
      scroller?.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  }, [root]);
  if (!picked) return null;
  // 触屏上系统的复制菜单出在选区上方：按钮放到选区下方，两者不叠。桌面放在选区上方，离鼠标近。
  const coarse = typeof window.matchMedia === "function" && window.matchMedia("(pointer: coarse)").matches;
  const top = coarse ? picked.bottom + 8 : Math.max(8, picked.top - 38);
  const left = Math.min(Math.max(8, picked.right - 40), window.innerWidth - 96);
  return (
    <button
      type="button"
      className="chat-quote-button"
      style={{ top, left }}
      // 按下时不让浏览器先把选区清掉（清掉了 click 时就拿不到选中的字）。
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        onQuote(picked.text);
        window.getSelection()?.removeAllRanges();
        setPicked(null);
      }}
      data-tip={t.chat.quoteTip}
    >
      {t.chat.quote}
    </button>
  );
}
