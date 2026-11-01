import { useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useDismissable } from "../../hooks/useDismissable";

type Anchor = { text: string; top: number; left: number };

/// 选中对话里的一段文字，浮出「引用」（对照 Kimi 选区浮条的「添加到对话」）：针对回复里某一句
/// 追问时，不必手动复制粘贴、也不必再解释「你上面说的那句」。引用以 Markdown 引用块交给输入框
///——我们把文本递给终端里的 CLI，引用块是任何模型都认得的形态（Kimi 用的是自家协议的胶囊）。
///
/// 只认对话流（container）里的选区：输入框、终端、弹层里的选择不弹。松手（pointerup / 键盘扩选
/// 的 keyup；触屏是选区停稳 400ms）后才出现，拖选过程中不跟着闪；选区收起即消失。浮条按下时 preventDefault，免得点它
/// 那一下先把选区清掉。
export function SelectionQuote({ container, label, onQuote }: {
  container: RefObject<HTMLElement | null>;
  label: string;
  onQuote: (text: string) => void;
}) {
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const popRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const read = (): Anchor | null => {
      const root = container.current;
      const selection = window.getSelection();
      if (!root || !selection || selection.isCollapsed || selection.rangeCount === 0) return null;
      const range = selection.getRangeAt(0);
      if (!root.contains(range.commonAncestorContainer)) return null;
      const node = range.commonAncestorContainer;
      const element = node instanceof Element ? node : node.parentElement;
      if (element?.closest("textarea, input, [contenteditable='true']")) return null;
      const text = selection.toString().replace(/ /g, " ").trim();
      if (!text) return null;
      // 跨行选区取 Range 的外接框；拿不到（jsdom 等）退回所在元素的。
      const rect = typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : (element ?? root).getBoundingClientRect();
      const bounds = root.getBoundingClientRect();
      // 默认浮在选区上方；顶到对话区上沿就放到下方。水平居中并夹在窗口内。
      const above = rect.top - 40;
      const top = above < bounds.top ? Math.min(rect.bottom + 8, window.innerHeight - 40) : above;
      const left = Math.min(Math.max(rect.left + rect.width / 2, 48), window.innerWidth - 48);
      return { text, top, left };
    };
    const show = () => setAnchor(read());
    // 触屏上长按选字后拖的是系统选区手柄，不一定有 pointerup：选区停稳一会儿再浮出。
    const coarse = typeof window.matchMedia === "function" && window.matchMedia("(pointer: coarse)").matches;
    let settle: number | undefined;
    const onSelectionChange = () => {
      const selection = window.getSelection();
      window.clearTimeout(settle);
      if (!selection || selection.isCollapsed) setAnchor(null);
      else if (coarse) settle = window.setTimeout(show, 400);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.shiftKey || event.key === "Shift") show();
    };
    document.addEventListener("pointerup", show);
    document.addEventListener("keyup", onKeyUp);
    document.addEventListener("selectionchange", onSelectionChange);
    return () => {
      document.removeEventListener("pointerup", show);
      document.removeEventListener("keyup", onKeyUp);
      document.removeEventListener("selectionchange", onSelectionChange);
      window.clearTimeout(settle);
    };
  }, [container]);

  useDismissable(popRef, { open: anchor !== null, onClose: () => setAnchor(null), closeOnScroll: true, closeOnResize: true });

  if (!anchor) return null;
  return createPortal(
    <div ref={popRef} className="chat-quote-pop" style={{ top: anchor.top, left: anchor.left }}>
      <button
        type="button"
        className="chat-quote-button"
        onPointerDown={(event) => event.preventDefault()}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          onQuote(anchor.text);
          window.getSelection()?.removeAllRanges();
          setAnchor(null);
        }}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M7 7h4v4c0 2.5-1.5 4.5-4 5M14 7h4v4c0 2.5-1.5 4.5-4 5" /></svg>
        <span>{label}</span>
      </button>
    </div>,
    document.body,
  );
}

/// 把选中的文字变成 Markdown 引用块，接在输入框现有内容之后（空一行隔开），末尾留空行给追问。
export function appendQuote(current: string, text: string): string {
  const block = text.split(/\r?\n/).map((line) => (line.trim() ? `> ${line.trimEnd()}` : ">")).join("\n");
  const head = current.replace(/\s+$/, "");
  return head ? `${head}\n\n${block}\n\n` : `${block}\n\n`;
}
