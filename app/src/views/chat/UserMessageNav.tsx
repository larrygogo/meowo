import { memo, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { type ChatItem } from "../../api";
import { useT } from "../../i18n";
import { imageOnlyPaths, splitUserText } from "./Message";
import { parseUserText } from "./localCommand";

/** 一条导航项：id 对应气泡上的 data-msg-id；kind 决定前缀图标与空正文时的占位。 */
export type UserNavEntry = { id: string; label: string; kind: "text" | "command" | "image" };

/// 时间线里「用户自己说的话」，口径与 Message/Transcript 的渲染一致：只剩免责声明的
/// 本地命令、后台任务通知、跨会话消息都不是用户发的，不进导航；连续纯图消息在
/// Transcript 里并成一组（锚点只挂在组首），组内后续几条跳过。
export function userNavEntries(items: readonly ChatItem[]): UserNavEntry[] {
  const out: UserNavEntry[] = [];
  for (let index = 0; index < items.length; index += 1) {
    const item = items[index];
    if (item.type !== "user_text") continue;
    if (imageOnlyPaths(item.text)) {
      const prev = items[index - 1];
      if (prev?.type === "user_text" && imageOnlyPaths(prev.text)) continue;
      out.push({ id: item.id, label: "", kind: "image" });
      continue;
    }
    const parts = parseUserText(item.text);
    if (parts.local && parts.commands.length > 0) {
      const { name, args } = parts.commands[0];
      const command = name.startsWith("/") ? name : `/${name}`;
      out.push({ id: item.id, label: args ? `${command} ${args}` : command, kind: "command" });
      continue;
    }
    const { body, images } = splitUserText(parts.text);
    const label = body.replace(/\s+/g, " ").trim();
    if (label) out.push({ id: item.id, label, kind: "text" });
    else if (images.length > 0) out.push({ id: item.id, label: "", kind: "image" });
  }
  return out;
}

const escapeId = (id: string) => (typeof CSS !== "undefined" && CSS.escape ? CSS.escape(id) : id.replace(/"/g, '\\"'));

/// 对话页右缘的用户消息导航：每条用户消息一根短横线，当前读到的那条高亮加长；
/// 悬停展开成消息摘要列表，点一条跳到它（用户要求，对照 Codex/Claude 客户端实拍）。
/// 绝对定位在 .chat-main（position: relative）里、贴着滚动区，不随内容滚走。
export const UserMessageNav = memo(function UserMessageNav({ items, scrollRef }: {
  items: readonly ChatItem[];
  scrollRef: RefObject<HTMLElement>;
}) {
  const t = useT();
  const entries = useMemo(() => userNavEntries(items), [items]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [box, setBox] = useState<{ top: number; height: number } | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const entriesRef = useRef(entries);
  entriesRef.current = entries;

  // 贴住滚动区的纵向范围（标题行/运行条/composer 高度随时在变，写死 top/bottom 会错位）。
  useEffect(() => {
    const scroller = scrollRef.current;
    if (!scroller) return;
    const measure = () => setBox((current) => {
      const next = { top: scroller.offsetTop, height: scroller.clientHeight };
      return current && current.top === next.top && current.height === next.height ? current : next;
    });
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(scroller);
    return () => observer.disconnect();
  }, [scrollRef]);

  // 当前项 = 顶端已滚过视口上 1/3 线的最后一条用户消息；贴底时恒为最后一条
  // （末条往往很短，到底了也越不过那条线，不特判会永远亮倒数第二条）。
  useEffect(() => {
    const scroller = scrollRef.current;
    if (!scroller) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      const list = entriesRef.current;
      if (list.length === 0) { setActiveId(null); return; }
      if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 4) {
        setActiveId(list[list.length - 1].id);
        return;
      }
      const line = scroller.getBoundingClientRect().top + scroller.clientHeight / 3;
      let current: string | null = list[0].id;
      for (const entry of list) {
        const el = scroller.querySelector(`[data-msg-id="${escapeId(entry.id)}"]`);
        if (!el) continue;
        if (el.getBoundingClientRect().top > line) break;
        current = entry.id;
      }
      setActiveId(current);
    };
    const schedule = () => { if (!frame) frame = window.requestAnimationFrame(update); };
    schedule();
    scroller.addEventListener("scroll", schedule, { passive: true });
    return () => {
      scroller.removeEventListener("scroll", schedule);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [scrollRef, entries]);

  // 消息多到列表要滚时，让当前项留在列表可视范围内。只动列表自己的 scrollTop——
  // scrollIntoView 会连带滚动外层，把对话区一起拽走。
  useEffect(() => {
    const list = listRef.current;
    if (!list || !activeId) return;
    const row = list.querySelector<HTMLElement>(`[data-nav-id="${escapeId(activeId)}"]`);
    if (!row) return;
    if (row.offsetTop < list.scrollTop) list.scrollTop = row.offsetTop;
    else if (row.offsetTop + row.offsetHeight > list.scrollTop + list.clientHeight) {
      list.scrollTop = row.offsetTop + row.offsetHeight - list.clientHeight;
    }
  }, [activeId]);

  // 只有一条时没有可跳的地方，不占位。
  if (entries.length < 2) return null;
  // 屏外条目是 content-visibility: auto（高度按估值占位），滚过去的途中它们逐个真渲染、
  // 高度变化，一次 scrollIntoView 落点会偏（实测停在目标上方一屏多）。所以用瞬时滚动，
  // 再连续几帧重新对齐，直到沿途布局稳定。平滑滚动在这里只会把偏差放大。
  const jump = (id: string) => {
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-msg-id="${escapeId(id)}"]`);
    if (!el) return;
    const align = (frames: number) => {
      el.scrollIntoView?.({ block: "start", behavior: "instant" });
      if (frames > 0) window.requestAnimationFrame(() => align(frames - 1));
    };
    align(4);
    setActiveId(id);
  };
  return (
    <nav
      className="chat-msg-nav"
      aria-label={t.chat.messageNav}
      style={box ? { top: box.top, height: box.height } : undefined}
    >
      <div className="chat-msg-nav-list" ref={listRef}>
        {entries.map((entry) => (
          <button
            type="button"
            key={entry.id}
            data-nav-id={entry.id}
            className={"chat-msg-nav-item" + (entry.id === activeId ? " is-active" : "")}
            // 收起态下文字是 display:none、横线 aria-hidden：读屏的浏览模式不聚焦就读，
            // 没有显式名字就是一排无名按钮（PR #87 评审指出）。
            aria-label={entry.kind === "image" ? t.chat.messageNavImage : entry.label}
            aria-current={entry.id === activeId ? "true" : undefined}
            onClick={() => jump(entry.id)}
          >
            <span className="chat-msg-nav-label">
              {entry.kind === "command" && (
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M4 17l6-6-6-6M12 19h8" /></svg>
              )}
              <span className="chat-msg-nav-text">{entry.kind === "image" ? t.chat.messageNavImage : entry.label}</span>
            </span>
            <i className="chat-msg-nav-dash" aria-hidden="true" />
          </button>
        ))}
      </div>
    </nav>
  );
});
