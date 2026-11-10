// 路径菜单（复制路径 / 定位 / 打开方式）与它的小图标。从 GitDiffView 拆出来：浏览器页签、
// 面板、源代码管理都要用这份菜单，原先直接从 GitDiffView 引，等于把整个改动面板（连同
// rehype-raw 的 HTML 解析器）静态拖进对话窗主 chunk，ChatWindow 那里的懒加载形同虚设
// （vite 构建警告「GitDiffView is dynamically imported … but also statically imported」）。
import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { listFileOpeners, openPathWith, revealPathInFileManager, type FileOpenerDto } from "../../api";
import { pushEscLayer } from "../../escLayers";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";

/** cwd + rel 拼绝对路径（复制路径用）：cwd 是本机形态，Windows 下 rel 的正斜杠翻成反斜杠。 */
export function joinAbsPath(cwd: string, rel: string): string {
  const base = cwd.replace(/[\\/]+$/, "");
  if (!rel) return base || cwd;
  const windows = cwd.includes("\\") || /^[A-Za-z]:/.test(cwd);
  return windows ? `${base}\\${rel.replace(/\//g, "\\")}` : `${base}/${rel}`;
}

/** rel 路径的小写扩展名（无扩展名/点文件返回空串），打开方式清单按它缓存。 */
export function extKeyOf(rel: string): string {
  const base = rel.split("/").pop() ?? "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

/** 打开方式清单按扩展名跨组件缓存：同类型文件共用一份，右键/切文件不重复枚举。 */
const openerCache = new Map<string, FileOpenerDto[]>();

/** 能打开 rel 的应用清单（rel 为 null/目录/无扩展名时恒空；失败静默为空——
 *  菜单仍有复制/定位/系统默认三项可用）。 */
export function useFileOpeners(cwd: string, rel: string | null): FileOpenerDto[] {
  const ext = rel ? extKeyOf(rel) : "";
  const [openers, setOpeners] = useState<FileOpenerDto[]>(() => (ext ? openerCache.get(ext) ?? [] : []));
  useEffect(() => {
    if (!rel || !ext) {
      setOpeners([]);
      return;
    }
    const cached = openerCache.get(ext);
    if (cached) {
      setOpeners(cached);
      return;
    }
    let alive = true;
    listFileOpeners(cwd, rel)
      .then((list) => {
        openerCache.set(ext, list);
        if (alive) setOpeners(list);
      })
      .catch(() => {
        if (alive) setOpeners([]);
      });
    return () => {
      alive = false;
    };
  }, [cwd, rel, ext]);
  return openers;
}

export type PathMenuItem = { key: string; label: string; icon: ReactNode; onSelect: () => void; /** 右侧的快捷键提示。 */ hint?: string };

/** 路径菜单里的「添加到对话」：把相对路径插进输入框（ChatWindow 经 SidePanel 提供，照 Kimi Code
 *  文件树的同名项）。没人提供（手机页、老的 GitDiffView 面板）就不出这一项。 */
export const PathMentionContext = createContext<((rel: string) => void) | null>(null);

/** 应用自带图标（data URL）优先，取不到落回通用编辑器线稿图标。 */
export function OpenerIcon({ opener }: { opener: FileOpenerDto }) {
  return opener.icon ? <img className="path-app-ico" src={opener.icon} alt="" /> : <EditorIcon />;
}

/** 「打开」菜单条目：复制路径 / 文件管理器定位 / 分隔 / 编辑器们 / 系统默认应用。
 *  动作全部 fire-and-forget（打开失败无处安放弹窗，静默即可——与 openProjectDir 同约定）。 */
export function buildPathMenuItems(
  cwd: string,
  rel: string,
  openers: FileOpenerDto[],
  t: Dict,
  mention: ((rel: string) => void) | null = null,
): (PathMenuItem | "sep")[] {
  const head: (PathMenuItem | "sep")[] = mention
    ? [{ key: "mention", label: t.chat.filesMention, icon: <MentionIcon />, onSelect: () => mention(rel) }, "sep"]
    : [];
  return [
    ...head,
    {
      key: "copy",
      label: t.chat.diffCopyPath,
      icon: <CopyIcon />,
      onSelect: () => void navigator.clipboard?.writeText(joinAbsPath(cwd, rel)).catch(() => {}),
    },
    {
      key: "reveal",
      label: t.chat.diffReveal,
      icon: <RevealIcon />,
      onSelect: () => void revealPathInFileManager(cwd, rel).catch(() => {}),
    },
    "sep",
    ...openers.map((opener) => ({
      key: opener.id,
      label: opener.name,
      icon: <OpenerIcon opener={opener} />,
      onSelect: () => void openPathWith(cwd, rel, opener.id).catch(() => {}),
    })),
    {
      key: "default",
      label: t.chat.diffOpenWithDefault,
      icon: <AppWindowIcon />,
      onSelect: () => void openPathWith(cwd, rel, "default").catch(() => {}),
    },
  ];
}

/** 路径操作菜单：fixed 定位 + 钳位，点外/Esc/失焦/滚动关闭，roving focus——
 *  行为对齐 CardContextMenu（那份注释里的踩坑理由这里全部适用，不再复述）。 */
export function PathMenu({ x, y, items, onClose }: {
  x: number;
  y: number;
  items: (PathMenuItem | "sep")[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const pad = 4;
    setPos({
      left: Math.max(pad, Math.min(x, window.innerWidth - el.offsetWidth - pad)),
      top: Math.max(pad, Math.min(y, window.innerHeight - el.offsetHeight - pad)),
    });
  }, [x, y]);
  useEffect(() => {
    const clickAway = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) {
        e.stopPropagation();
        onClose();
      }
    };
    const ctxAway = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      onClose();
    };
    const closeOnScroll = (e: Event) => {
      if (ref.current && e.target instanceof Node && ref.current.contains(e.target)) return;
      onClose();
    };
    const popLayer = pushEscLayer();
    document.addEventListener("click", clickAway, true);
    document.addEventListener("contextmenu", ctxAway, true);
    document.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    window.addEventListener("scroll", closeOnScroll, true);
    return () => {
      popLayer();
      document.removeEventListener("click", clickAway, true);
      document.removeEventListener("contextmenu", ctxAway, true);
      document.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("scroll", closeOnScroll, true);
    };
  }, [onClose]);
  useEffect(() => {
    const prev = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    ref.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    return () => {
      // eslint-disable-next-line react-hooks/exhaustive-deps -- 关闭时读当下的菜单节点判断焦点在不在菜单里；节点挂载期内不变，读当下值与挂载时快照等价
      if (document.activeElement === document.body || ref.current?.contains(document.activeElement)) {
        prev?.focus();
      }
    };
  }, []);
  const onMenuKeyDown = (e: ReactKeyboardEvent) => {
    const nodes = Array.from(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []);
    if (nodes.length === 0) return;
    const cur = nodes.indexOf(document.activeElement as HTMLElement);
    let next: number;
    if (e.key === "ArrowDown") next = cur >= 0 ? (cur + 1) % nodes.length : 0;
    else if (e.key === "ArrowUp") next = cur >= 0 ? (cur - 1 + nodes.length) % nodes.length : nodes.length - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = nodes.length - 1;
    else if ((e.key === "Enter" || e.key === " ") && cur >= 0) {
      e.preventDefault();
      (document.activeElement as HTMLElement).click();
      return;
    } else return;
    e.preventDefault();
    nodes[next]?.focus();
  };
  return (
    <div ref={ref} className="ctx-menu" role="menu" style={pos} onClick={(e) => e.stopPropagation()} onKeyDown={onMenuKeyDown}>
      {items.map((item, index) =>
        item === "sep" ? (
          <div key={`sep-${index}`} className="ctx-sep" role="separator" />
        ) : (
          <button
            key={item.key}
            type="button"
            role="menuitem"
            className="ctx-item"
            onClick={() => {
              item.onSelect();
              onClose();
            }}
          >
            {item.icon}
            {item.label}
            {item.hint && <kbd className="ctx-hint">{item.hint}</kbd>}
          </button>
        ),
      )}
    </div>
  );
}

/** 树行/结果行右键菜单：按需拉该文件类型的打开方式清单（目录不拉——只有
 *  复制/定位/系统默认三项）；清单异步到位后条目就地补齐（有扩展名缓存，通常瞬时）。 */
export function PathContextMenu({ cwd, rel, isDir, x, y, onClose }: {
  cwd: string;
  rel: string;
  isDir: boolean;
  x: number;
  y: number;
  onClose: () => void;
}) {
  const t = useT();
  const openers = useFileOpeners(cwd, isDir ? null : rel);
  const mention = useContext(PathMentionContext);
  return <PathMenu x={x} y={y} items={buildPathMenuItems(cwd, rel, openers, t, mention)} onClose={onClose} />;
}

/** 树行悬停时右端的「…」：和右键同一份路径菜单，照 Kimi Code 的文件树（不知道能右键的人也找得到）。 */
export function RowMore({ label, open, onOpen }: { label: string; open: boolean; onOpen: (x: number, y: number) => void }) {
  return (
    <button
      type="button"
      className="chat-diff-tree-more"
      aria-label={label}
      data-tip={label}
      aria-haspopup="menu"
      aria-expanded={open}
      onClick={(event) => {
        event.stopPropagation();
        const rect = event.currentTarget.getBoundingClientRect();
        onOpen(rect.left, rect.bottom + 2);
      }}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="6" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18" cy="12" r="1.6" /></svg>
    </button>
  );
}

function MentionIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="4" /><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-3.9 7.9" />
    </svg>
  );
}

/* ——「打开」按钮与路径菜单的小图标（与全库同款 lucide 线稿风）—— */

export function OpenIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
      <polyline points="15 3 21 3 21 9" />
      <line x1="10" y1="14" x2="21" y2="3" />
    </svg>
  );
}

export function CopyIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

function RevealIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m6 14 1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.55 6a2 2 0 0 1-1.94 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

function EditorIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="16 18 22 12 16 6" />
      <polyline points="8 6 2 12 8 18" />
    </svg>
  );
}

function AppWindowIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="2" y="4" width="20" height="16" rx="2" />
      <path d="M10 4v4" />
      <path d="M2 8h20" />
      <path d="M6 4v4" />
    </svg>
  );
}
