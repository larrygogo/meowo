import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { useDismissable } from "../../hooks/useDismissable";
import { useT } from "../../i18n";

/// 一次命中的全部矩形（视口坐标，画叠层时换算成宿主坐标）。
type Match = { boxes: { top: number; left: number; width: number; height: number }[] };

/// 叠层只画前 N 处：超长会话里搜一个 `a` 会命中上千处，逐块画矩形比搜索本身还贵。
const MAX_MARKS = 300;

/**
 * 对话区内搜索（对照 Kimi Code 桌面的 TranscriptSearch）。
 *
 * 高亮不改 DOM：用 Range.getClientRects() 量出每处命中的矩形，在滚动区之上画一层
 * 半透明叠层。理由有二——流式重渲染会把塞进文本节点里的 <mark> 冲掉；改 DOM 还会
 * 触发 content-visibility 的重排，长对话里一搜就掉帧。
 *
 * 叠层坐标是视口系的，所以宿主滚动、内层容器滚动（长日志的 pre）、内容变化
 * （流式追加）都要重测；三处共用 measure()，一次 rAF 内合并。
 */
export function TranscriptSearch({ scrollRef, onClose }: { scrollRef: RefObject<HTMLElement | null>; onClose: () => void }) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  // 输入框显示值与搜索词分开：受控 input 在组字期间若不更新 value，React 会把 DOM 值改回
  // 旧值，拼音一打就被吞（用户反馈搜索输不了中文）。显示值每次都跟，搜索词等组字结束再落。
  const [draft, setDraft] = useState("");
  const [marks, setMarks] = useState<Match[]>([]);
  const [current, setCurrent] = useState(0);
  // IME 合成期间不重测：拼音逐字母上屏会把叠层闪成一串不同宽度，也白白重跑 TreeWalker。
  const composingRef = useRef(false);

  const measure = useCallback((jump: boolean) => {
    const el = scrollRef.current;
    const host = hostRef.current;
    const needle = query.trim().toLowerCase();
    if (!el || !needle) {
      setMarks([]);
      return;
    }
    const hostRect = host?.getBoundingClientRect();
    const originX = hostRect?.left ?? 0;
    const originY = hostRect?.top ?? 0;
    const out: Match[] = [];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => (node.textContent?.trim() ? 1 : 2),
    });
    let text = walker.nextNode();
    while (text && out.length < MAX_MARKS) {
      const value = text.textContent ?? "";
      const hay = value.toLowerCase();
      let at = hay.indexOf(needle);
      while (at >= 0 && out.length < MAX_MARKS) {
        const range = document.createRange();
        range.setStart(text, at);
        range.setEnd(text, Math.min(at + needle.length, value.length));
        // 逐行矩形才能框住折行的句子；jsdom 与旧内核上 Range 的量高方法可能缺席，
        // 退回宿主元素的外接矩形（至少这一处命中看得见）。
        const own = typeof range.getClientRects === "function" ? [...range.getClientRects()] : [];
        const owner = text.parentElement;
        const fallback = own.length > 0
          ? own
          : typeof range.getBoundingClientRect === "function"
            ? [range.getBoundingClientRect()]
            : owner && typeof owner.getBoundingClientRect === "function"
              ? [owner.getBoundingClientRect()]
              : [];
        const boxes = fallback
          .filter((rect) => rect.width > 0 && rect.height > 0)
          .map((rect) => ({
            top: Math.round(rect.top - originY),
            left: Math.round(rect.left - originX),
            width: Math.round(rect.width),
            height: Math.round(rect.height),
          }));
        if (boxes.length > 0) out.push({ boxes });
        at = hay.indexOf(needle, at + needle.length);
      }
      text = walker.nextNode();
    }
    setMarks(out);
    // 只有换了搜索词才回到第一处。滚动/流式追加触发的重测若也归零，「下一处」一滚动就
    // 被拉回第一处，永远翻不过去（手机实测计数恒为 1/N）。
    if (jump) setCurrent(0);
    else setCurrent((index) => (index < out.length ? index : 0));
    if (jump && out.length > 0) scrollTo(out, el, 0, originY);
  }, [query, scrollRef]);

  // 打开时聚焦输入框；查询变化后重测。
  useEffect(() => {
    inputRef.current?.focus();
  }, []);
  useEffect(() => {
    if (composingRef.current) return;
    measure(true);
  }, [measure]);

  // 宿主/内层容器滚动、窗口缩放、流式追加都会让矩形移位——只在真正变化时重测。
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    let frame = 0;
    const remeasure = () => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        measure(false);
      });
    };
    // capture：代码块、diff 表这些内层滚动容器的 scroll 不冒泡，要在捕获段看到。
    el.addEventListener("scroll", remeasure, { passive: true, capture: true });
    window.addEventListener("resize", remeasure);
    const observer = new MutationObserver(remeasure);
    observer.observe(el, { childList: true, characterData: true, subtree: true });
    return () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      el.removeEventListener("scroll", remeasure, true);
      window.removeEventListener("resize", remeasure);
      observer.disconnect();
    };
  }, [measure, scrollRef]);

  // Esc 关闭走统一弹层实现（栈注册 + preventDefault，避免同一次按键顺手拒了审批）。
  useDismissable(hostRef, { onClose });

  const step = (delta: number) => {
    const el = scrollRef.current;
    if (!el || marks.length === 0) return;
    const next = (current + delta + marks.length) % marks.length;
    setCurrent(next);
    scrollTo(marks, el, next, hostRef.current?.getBoundingClientRect().top ?? 0);
  };

  const total = marks.length;
  const capped = total >= MAX_MARKS;

  return (
    <div className="chat-search-host" ref={hostRef}>
      {/* 叠层不吃指针，点击照常落在正文上。 */}
      <div className="chat-search-layer" aria-hidden="true">
        {marks.map((match, index) => match.boxes.map((box, boxIndex) => (
          <span
            key={`${index}:${boxIndex}`}
            className={"chat-search-mark" + (index === current ? " is-current" : "")}
            style={{ top: box.top, left: box.left, width: box.width, height: box.height }}
          />
        )))}
      </div>
      <div className="chat-search" role="search" onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.stopPropagation();
        onClose();
      }}>
        <input
          ref={inputRef}
          className="chat-search-input"
          value={draft}
          placeholder={t.chat.searchPlaceholder}
          aria-label={t.chat.searchTranscript}
          onChange={(event) => {
            setDraft(event.target.value);
            if (composingRef.current || (event.nativeEvent as InputEvent).isComposing) return;
            setQuery(event.target.value);
          }}
          onCompositionEnd={(event) => {
            composingRef.current = false;
            setDraft(event.currentTarget.value);
            setQuery(event.currentTarget.value);
          }}
          onCompositionStart={() => { composingRef.current = true; }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault();
              step(event.shiftKey ? -1 : 1);
            }
          }}
        />
        <span className="chat-search-count" role="status" aria-live="polite">
          {query.trim()
            ? total > 0 ? t.chat.searchCount(current + 1, total) : t.chat.searchNoMatch
            : ""}
        </span>
        {capped && <span className="chat-search-note">{t.chat.searchMarksCapped(MAX_MARKS)}</span>}
        <button type="button" className="chat-search-btn" aria-label={t.chat.searchPrev} data-tip={t.chat.searchPrev} onClick={() => step(-1)}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m18 15-6-6-6 6" /></svg>
        </button>
        <button type="button" className="chat-search-btn" aria-label={t.chat.searchNext} data-tip={t.chat.searchNext} onClick={() => step(1)}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
        </button>
        <button type="button" className="chat-search-btn is-close" aria-label={t.chat.close} onClick={onClose}>×</button>
      </div>
    </div>
  );
}

/// 把第 index 处命中滚到视口中线：直接用「当前 scrollTop + 目标相对视口的偏移」，
/// 不必换算 content-visibility 的占位高度。平滑与否交给 CSS（减动效偏好下自动瞬移）。
/// 矩形存的是叠层宿主坐标（见 measure），滚动区的位置是视口坐标——要先加回宿主顶边
/// 再比。此前直接相减，跳到的命中总比正中低一个标题栏高。
function scrollTo(marks: Match[], el: HTMLElement, index: number, hostTop: number): void {
  const box = marks[index]?.boxes[0];
  if (!box) return;
  const rect = el.getBoundingClientRect();
  el.scrollTop = el.scrollTop + box.top + hostTop - (rect.top + rect.height / 2);
}
