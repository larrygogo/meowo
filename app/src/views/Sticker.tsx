import {
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  availableAgents,
  LiveSession,
  LiveSessionCounts,
  Settings,
  StickerFilter,
  TerminalOpenMode,
  getSettings,
  getAccounts,
  refreshUsage,
  type CardMenuMode,
  type ProviderUsage,
  type UsageLane,
} from "../api";
import { isMacPanel } from "../platform";
import { providerConfig } from "../providers";
import { useT } from "../i18n";
import type { Dict } from "../i18n/zh";

const DAY_MS = 86_400_000;

// 行内编辑器（重命名/便签）共用的键盘处理:IME 组字中的 Enter 不提交/Escape 不取消
// （WKWebView 上上屏 Enter 派发 keyCode 229,compositionend 后 isComposing 可能已 false,
// 故两条件都判）,Enter 提交、Escape 取消。
const editorKeyDown =
  (submit: () => void, cancel: () => void) =>
  (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (e.key === "Enter") submit();
    else if (e.key === "Escape") cancel();
  };

function fmtAgo(ms: number, t: Dict): string {
  const m = Math.floor((Date.now() - ms) / 60000);
  if (m < 1) return t.time.now;
  if (m < 60) return t.time.minAgo(m);
  const h = Math.floor(m / 60);
  if (h < 24) return t.time.hourAgo(h);
  return t.time.dayAgo(Math.floor(h / 24));
}

function ArchiveIcon({ archived }: { archived: boolean }) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect width="20" height="5" x="2" y="3" rx="1" />
      <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
      {archived ? <path d="m9 15 3-3 3 3" /> /* 还原：向上箭头 */ : <path d="M10 12h4" /> /* 归档：把手 */}
    </svg>
  );
}

function PencilIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

function OpenIcon() {
  // lucide terminal：命令行 >_ 符号，明确表达「打开/跳转终端」（原 share 样图标易误读为分享）
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <polyline points="4 17 10 11 4 5" />
      <line x1="12" y1="19" x2="20" y2="19" />
    </svg>
  );
}

function MoreIcon() {
  // lucide ellipsis：卡片菜单按钮（card_menu_mode=button 时替代右键触发）
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="1" />
      <circle cx="5" cy="12" r="1" />
      <circle cx="19" cy="12" r="1" />
    </svg>
  );
}

function CheckIcon() {
  // lucide check：行内编辑器确认钮
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}

function XIcon() {
  // lucide x：行内编辑器取消钮
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  );
}

function FolderIcon() {
  // lucide folder-open：右键菜单「打开项目目录」用
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

function NoteIcon() {
  // lucide sticky-note：折角便签纸，区别于 rename 的铅笔
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" />
      <path d="M15 21v-5a1 1 0 0 1 1-1h5" />
    </svg>
  );
}

function StarIcon({ starred }: { starred: boolean }) {
  // lucide star：未星标描边、星标时填充金色以示激活
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill={starred ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.79 21.55a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.554 10.34a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
    </svg>
  );
}

type Item = LiveSession & { connected: boolean };
type Tab = "all" | "waiting" | "running" | "archived";

function TabIcon({ tab }: { tab: Tab }) {
  const common = { width: 11, height: 11, viewBox: "0 0 16 16", "aria-hidden": true } as const;
  switch (tab) {
    case "all": // 列表
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
          <line x1="3" y1="4.5" x2="13" y2="4.5" />
          <line x1="3" y1="8" x2="13" y2="8" />
          <line x1="3" y1="11.5" x2="13" y2="11.5" />
        </svg>
      );
    case "waiting": // 举手（待交互）——与 macOS 菜单栏 hand.raised 同隐喻；lucide hand
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头（运行中）
      return (
        <svg {...common} viewBox="0 0 24 24" fill="none" stroke="currentColor"
          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common} fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
          <rect x="2.5" y="3" width="11" height="3" rx="0.5" />
          <path d="M3.5 6v6.5h9V6" />
          <line x1="6.5" y1="9" x2="9.5" y2="9" strokeLinecap="round" />
        </svg>
      );
  }
}

function PinIcon({ pinned }: { pinned: boolean }) {
  // lucide pin：未置顶描边、置顶时填充以示激活（尺寸与搜索/设置图标统一为 13）
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill={pinned ? "currentColor" : "none"}
      stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 17v5" />
      <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
    </svg>
  );
}

function GearIcon() {
  // lucide settings：齿轮，打开设置
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function SearchIcon() {
  // lucide search：放大镜
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

function CloseIcon() {
  // lucide x：关闭搜索
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

/** 状态徽标：圆角矩形边框上流动的亮线（conic 渐变 + transform 旋转，纯 GPU 合成，
 *  拖动窗口不占主线程）+ 中心实心圆，圆内显示 Content 已用百分比。
 *  tone：running=绿（运行中），waiting=黄（待交互），pending=琥珀（待审批）。 */
function RunBadge({
  pct,
  tone = "running",
}: {
  pct: number | null;
  tone?: "running" | "waiting" | "pending";
}) {
  const t = useT();
  const what = tone === "waiting" ? t.badge.waiting : t.badge.running;
  const label = pct != null ? t.badge.full(what, pct) : what;
  return (
    <span
      className={"run-badge" + (tone === "waiting" ? " run-badge--waiting" : tone === "pending" ? " run-badge--pending" : "")}
      role="img"
      aria-label={label}
      data-tip={label}
    >
      {/* 旋转的亮段（被 .run-badge 的圆角裁剪 → 光点沿边框跑） */}
      <span className="run-sweep" />
      {/* 遮住中心黑底，只露出外圈一圈边框 */}
      <span className="run-mask" />
      {/* 中心实心圆 + 百分比 */}
      <span className="run-core">{pct != null ? `${pct}%` : ""}</span>
    </span>
  );
}

// 卡片右键菜单：星标/便签/重命名/归档收拢于此（替代原 hover 图标行，卡片标题行更干净）。
// fixed 定位 + useLayoutEffect 钳位：贴纸窗口小，菜单贴边时向内收、不被窗口边缘裁掉。
// 关闭时机：点菜单外任意处 / Escape / 窗口失焦 / 任一菜单项执行后。
function CardContextMenu({
  x,
  y,
  starred,
  hasNote,
  archived,
  onStar,
  onNote,
  onRename,
  onArchive,
  onNewSession,
  onOpenDir,
  onClose,
}: {
  x: number;
  y: number;
  starred: boolean;
  hasNote: boolean;
  archived: boolean;
  onStar: () => void;
  onNote: () => void;
  onRename: () => void;
  onArchive: () => void;
  /** 用当前会话的路径和模型新建会话。 */
  onNewSession: () => void;
  /** 打开项目目录；会话无 cwd（旧数据）时传 null 隐藏该项。 */
  onOpenDir: (() => void) | null;
  onClose: () => void;
}) {
  const t = useT();
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
    // 点菜单外关闭：用 click **捕获相**而非 mousedown——捕获相里 stopPropagation 把这次点击
    // 整个拦下，不再传到卡片的 onClick（否则点外部关个菜单会顺手触发卡片点击、把终端打开）。
    // 菜单项在 ref 内不受拦截；本监听在菜单挂载后才注册，打开菜单的那次点击不会误触发。
    const clickAway = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) {
        e.stopPropagation();
        onClose();
      }
    };
    // 右键他处：只关闭本菜单、不拦事件——落在卡片上时让其 onContextMenu 原地弹出新菜单。
    const ctxAway = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("click", clickAway, true);
    document.addEventListener("contextmenu", ctxAway, true);
    document.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("click", clickAway, true);
      document.removeEventListener("contextmenu", ctxAway, true);
      document.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  const act = (fn: () => void) => () => {
    fn();
    onClose();
  };
  return (
    <div ref={ref} className="ctx-menu" role="menu" style={pos} onClick={(e) => e.stopPropagation()}>
      <button type="button" role="menuitem" className="ctx-item" onClick={act(onStar)}>
        <StarIcon starred={starred} />
        {starred ? t.sticker.unstar : t.sticker.star}
      </button>
      <button type="button" role="menuitem" className="ctx-item" onClick={act(onNote)}>
        <NoteIcon />
        {hasNote ? t.sticker.noteEdit : t.sticker.noteAdd}
      </button>
      <button type="button" role="menuitem" className="ctx-item" onClick={act(onRename)}>
        <PencilIcon />
        {t.sticker.renameTitle}
      </button>
      <button type="button" role="menuitem" className="ctx-item" onClick={act(onArchive)}>
        <ArchiveIcon archived={archived} />
        {archived ? t.sticker.unarchive : t.sticker.archive}
      </button>
      <div className="ctx-sep" role="separator" />
      <button type="button" role="menuitem" className="ctx-item" onClick={act(onNewSession)}>
        <PlusIcon />
        {t.sticker.newSession}
      </button>
      {onOpenDir && (
        <>
          <div className="ctx-sep" role="separator" />
          <button type="button" role="menuitem" className="ctx-item" onClick={act(onOpenDir)}>
            <FolderIcon />
            {t.sticker.openProjectDir}
          </button>
        </>
      )}
    </div>
  );
}

const PIN_KEY = "meowo-pinned";
const STAR_KEY = "meowo-starred";
// 用量屏选中的 provider 偏好：折叠/展开会卸载重挂 UsageScreen，持久化以记住上次选择
// （该 provider 仍在活跃列表就沿用，被关/找不到才退回第一个——见 UsageScreen selected 计算）。
const USAGE_KEY = "meowo-usage-provider";
const TAB_KEYS: Tab[] = ["all", "waiting", "running", "archived"];

/** 读取已星标会话集合（按 cc_session_id 持久化，跨重启/换库稳定）。 */
function loadStarred(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(STAR_KEY) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

function match(tab: Tab, l: Item, hideDays = 0): boolean {
  if (tab === "archived") {
    if (!l.archived) return false;
    // 归档超过 hideDays 天自动隐藏；archived_at 缺失的旧条目不隐藏。
    if (hideDays > 0 && l.archived_at && Date.now() - l.archived_at > hideDays * DAY_MS) {
      return false;
    }
    return true;
  }
  if (l.archived) return false; // 已归档的不在其它分类显示
  if (tab === "all") return true;
  // running = AI 自主运行且无需用户介入；waiting = 等用户交互（status=waiting 或 pending_review）。
  // 与后端 live_sessions 语义保持一致。
  if (tab === "waiting") return l.session.status === "waiting" || l.pending_review != null;
  if (tab === "running") return l.session.status === "running" && l.pending_review == null;
  return true;
}

function emptyCopy(tab: Tab, t: Dict): { title: string; hint: string | null } {
  switch (tab) {
    case "all": return { title: t.empty.allTitle, hint: t.empty.allHint };
    case "waiting": return { title: t.empty.waitingTitle, hint: t.empty.waitingHint };
    case "running": return { title: t.empty.runningTitle, hint: null };
    case "archived": return { title: t.empty.archivedTitle, hint: t.empty.archivedHint };
  }
}

function EmptyIcon({ tab }: { tab: Tab }) {
  const common = {
    width: 28, height: 28, viewBox: "0 0 24 24", fill: "none",
    stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round",
    strokeLinejoin: "round", "aria-hidden": true,
  } as const;
  switch (tab) {
    case "all": // 显示器
      return (
        <svg {...common}>
          <rect width="20" height="14" x="2" y="3" rx="2" />
          <line x1="8" y1="21" x2="16" y2="21" />
          <line x1="12" y1="17" x2="12" y2="21" />
        </svg>
      );
    case "waiting": // 举手（待交互）
      return (
        <svg {...common}>
          <path d="M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2" />
          <path d="M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2" />
          <path d="M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8" />
          <path d="M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15" />
        </svg>
      );
    case "running": // 循环箭头
      return (
        <svg {...common}>
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M3 21v-5h5" />
        </svg>
      );
    case "archived": // 归档盒
      return (
        <svg {...common}>
          <rect width="20" height="5" x="2" y="3" rx="1" />
          <path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" />
          <path d="M10 12h4" />
        </svg>
      );
  }
}

export function EmptyState({ tab, onNew }: { tab: Tab; onNew?: () => void }) {
  const t = useT();
  const { title, hint } = emptyCopy(tab, t);
  return (
    <div className="stk-empty">
      <span className="stk-empty-icon"><EmptyIcon tab={tab} /></span>
      <div className="stk-empty-title">{title}</div>
      {hint && <div className="stk-empty-hint">{hint}</div>}
      {onNew && (
        <button type="button" className="stk-empty-cta" data-testid="empty-new-cta" onClick={onNew}>
          {t.newSession.emptyCta}
        </button>
      )}
    </div>
  );
}

/** 底部用量：嵌在底栏左侧的「凹陷小屏读数」——标签式：一行品牌图标标签，点选后显示该
   provider 的 5h + 7d/weekly 用量条；与右侧凸起按钮组成「凹陷显示屏 + 凸起按钮」的物理设备面板。 */
// 利用率档位 → 复用应用既有状态色(绿/黄/红)，与卡片状态点同语义；越满越红即预警。
function usageSev(pct: number): string {
  return pct >= 80 ? "is-high" : pct >= 50 ? "is-warn" : "is-ok";
}

// 单条用量泳道（进度条型或余额数值型）
function LaneRow({ lane, label }: { lane: UsageLane; label: string }) {
  if (lane.used_pct != null) {
    const pct = Math.max(0, Math.min(100, lane.used_pct));
    return (
      <div className="stk-urow">
        <span className="stk-ulabel">{label}</span>
        <span className="stk-utrack">
          <i className={"stk-ufill " + usageSev(pct)} style={{ width: `${pct}%` }} />
        </span>
        <span className="stk-uval">{Math.round(pct)}%</span>
      </div>
    );
  }
  // 余额型：显数值，不画进度条
  const valText = lane.used != null ? `${lane.used}${lane.unit ? ` ${lane.unit}` : ""}` : "—";
  return (
    <div className="stk-urow">
      <span className="stk-ulabel">{label}</span>
      <span className="stk-uval">{valText}</span>
    </div>
  );
}

/** 标签式用量屏：每个开启配额的 provider 一个图标标签，点选后显示其 5h + 7d/weekly 条。
 *  符合条件 provider 为空 → 不渲染。 */
export function UsageScreen({
  quotaProviders,
  usageMap,
}: {
  quotaProviders: string[];
  usageMap: Record<string, ProviderUsage>;
}) {
  const t = useT();
  // 用户偏好选中的 provider（持久化：折叠/展开重挂后记住；若不在当前活跃列表中则退回第一个）
  const [selectedPref, setSelectedPref] = useState<string>(() => localStorage.getItem(USAGE_KEY) ?? "");
  const pick = (p: string) => {
    setSelectedPref(p);
    localStorage.setItem(USAGE_KEY, p);
  };

  // 仅显示「在配额列表中且有用量数据」的 provider
  const activeProviders = quotaProviders.filter((p) => !!usageMap[p]);
  if (!activeProviders.length) return null;

  // 选中态：优先用户选择，其次第一个
  const selected = activeProviders.includes(selectedPref) ? selectedPref : activeProviders[0];

  const usage = usageMap[selected];
  const fiveHourLane = usage?.lanes.find((l) => l.kind === "five_hour") ?? null;
  const sevenDayLane = usage?.lanes.find((l) => l.kind === "seven_day" || l.kind === "weekly") ?? null;

  return (
    <div className="stk-uscreen" role="group" aria-label={t.account.quota}>
      {/* 品牌图标标签行（每个 provider 一个，点选切换） */}
      <div className="stk-utabs">
        {activeProviders.map((p) => {
          const { Icon } = providerConfig(p);
          return (
            <button
              key={p}
              type="button"
              className={"stk-utab" + (p === selected ? " on" : "")}
              aria-pressed={p === selected}
              onClick={() => pick(p)}
            >
              <Icon />
            </button>
          );
        })}
      </div>
      {/* 选中 provider 的 5h 和 7d/weekly 用量条 */}
      {fiveHourLane && <LaneRow lane={fiveHourLane} label={t.account.laneFiveHour} />}
      {sevenDayLane && <LaneRow lane={sevenDayLane} label={sevenDayLane.kind === "weekly" ? t.account.laneWeekly : t.account.laneSevenDay} />}
    </div>
  );
}

export function Sticker({
  filter,
  onFilterChange,
  data,
  counts: countsProp,
  total,
  hasMore: hasMoreProp,
  loadMore,
  loadingMore,
  hasUpdate,
  search,
  onSearchChange,
}: {
  filter: StickerFilter;
  onFilterChange?: (f: StickerFilter) => void;
  data: Item[];
  counts?: LiveSessionCounts;
  total?: number;
  hasMore?: boolean;
  loadMore?: () => void;
  loadingMore?: boolean;
  hasUpdate?: boolean;
  search?: string;
  onSearchChange?: (q: string) => void;
}) {
  // hasMore 由父组件传入；未传入时退化为 data.length < total。
  const totalCount = total ?? data.length;
  const onLoadMore = loadMore ?? (() => {});
  const isLoadingMore = loadingMore ?? false;
  const hasMore = hasMoreProp ?? data.length < totalCount;
  const t = useT();
  const tab = filter;

  const pick = (t: Tab) => {
    onFilterChange?.(t);
    closeSearch(); // 切 tab 即退出搜索，避免 tab 高亮与过滤结果不一致
  };

  // 会话搜索：激活时底栏整条变成输入框；搜索词经 onSearchChange 交给父组件下沉到后端
  // （当前 tab 内全库搜，覆盖未加载数据），本组件不再持有搜索词、不做客户端过滤。
  const [searchOpen, setSearchOpen] = useState(false);
  const q = search ?? "";
  const closeSearch = () => {
    setSearchOpen(false);
    onSearchChange?.("");
  };

  // 归档自动隐藏天数 + 打开终端方式 + 卡片菜单方式 + 贴纸配额 provider 列表：启动时读设置，并监听实时变更。
  const [hideDays, setHideDays] = useState(0);
  const [openMode, setOpenMode] = useState<TerminalOpenMode>("card");
  const [menuMode, setMenuMode] = useState<CardMenuMode>("context");
  const [previewEnabled, setPreviewEnabled] = useState(true);
  const [quotaProviders, setQuotaProviders] = useState<string[]>(["claude"]);
  const [availAgents, setAvailAgents] = useState<string[]>([]);
  useEffect(() => {
    const apply = (s: Settings) => {
      setHideDays(s.archive_hide_days);
      setOpenMode(s.terminal_open_mode);
      setMenuMode(s.card_menu_mode ?? "context");
      setPreviewEnabled(s.preview_enabled);
      setQuotaProviders(s.sticker_quota_providers ?? ["claude"]);
    };
    getSettings().then(apply).catch(() => {});
    // cleanup 可能先于 listen resolve 执行：用 cancelled 标记，resolve 后立即注销，防监听器泄漏。
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen<Settings>("settings-changed", (e) => apply(e.payload))
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      cancelled = true;
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 已装 provider 列表：用于过滤配额显示（只显示既在配额设置里、又已装的 provider）
  useEffect(() => {
    availableAgents().then(setAvailAgents).catch(() => {});
  }, []);

  // 相对时间（fmtAgo）每分钟重算：递增计数触发轻量重渲染。
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(id);
  }, []);

  // 置顶开关：默认不置顶，激活后才把窗口设为 alwaysOnTop，状态持久化。
  const [pinned, setPinned] = useState<boolean>(() => localStorage.getItem(PIN_KEY) === "1");
  useEffect(() => {
    // 非 Tauri 环境（测试/浏览器）下 getCurrentWindow 会抛错，吞掉即可。
    try {
      getCurrentWindow().setAlwaysOnTop(pinned).catch(() => {});
    } catch {
      /* noop */
    }
  }, [pinned]);
  const togglePin = () => {
    setPinned((p) => {
      const next = !p;
      localStorage.setItem(PIN_KEY, next ? "1" : "0");
      return next;
    });
  };

  // 托盘「找回贴纸」：窗口居中/置顶由 App + 后端负责，这里把置顶按钮 UI 同步为已置顶。
  useEffect(() => {
    let cancelled = false;
    let un: (() => void) | undefined;
    try {
      listen("recall-sticker", () => setPinned(true))
        .then((f) => {
          if (cancelled) f();
          else un = f;
        })
        .catch(() => {});
    } catch {
      /* 非 Tauri 环境（测试/浏览器） */
    }
    return () => {
      cancelled = true;
      try { un?.(); } catch { /* noop */ }
    };
  }, []);

  // 会话星标：星标的会话永远排到列表最前（跨重启保留）。与「置顶窗口(pin)」是两回事。
  const [starred, setStarred] = useState<Set<string>>(loadStarred);
  const toggleStar = (sid: string) => {
    setStarred((prev) => {
      const next = new Set(prev);
      if (next.has(sid)) next.delete(sid);
      else next.add(sid);
      localStorage.setItem(STAR_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  // 重命名：editingId 为正在编辑的会话 id，draft 为输入内容。
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const startRename = (l: Item) => {
    setNotingId(null); // 与便签编辑互斥，同卡只开一个编辑器
    const cur = l.task_title && l.task_title !== "(未命名会话)" ? l.task_title : "";
    setDraft(cur);
    setEditingId(l.session.id);
  };
  const submitRename = (l: Item) => {
    const t = draft.trim();
    if (t && t !== l.task_title) {
      invoke("rename_session", { cwd: l.cwd, sessionId: l.session.cc_session_id, title: t, provider: l.provider }).catch(() => {});
    }
    setEditingId(null);
  };

  // 卡片右键菜单：记录目标会话 id 与打开坐标。item 每次渲染按 id 从 data 现查——
  // 数据刷新后菜单内容(星标/便签/归档态)保持最新,会话消失则菜单自然不渲染。
  const [ctxMenu, setCtxMenu] = useState<{ sid: number; x: number; y: number } | null>(null);
  const ctxItem = ctxMenu ? data.find((l) => l.session.id === ctxMenu.sid) ?? null : null;

  // 便签编辑：notingId 为正在编辑便签的会话 id，noteDraft 为输入内容。与重命名互斥（同卡只开一个）。
  const [notingId, setNotingId] = useState<number | null>(null);
  const [noteDraft, setNoteDraft] = useState("");
  const startNote = (l: Item) => {
    setEditingId(null);
    setNoteDraft(l.note ?? "");
    setNotingId(l.session.id);
  };
  const submitNote = (l: Item) => {
    if (noteDraft !== (l.note ?? "")) {
      invoke("set_session_note", { sessionId: l.session.cc_session_id, note: noteDraft }).catch(() => {});
    }
    setNotingId(null);
  };

  // 打开终端：连接中→跳转 WT 标签页；已断开未归档→新建终端 resume；归档不开。
  const buttonMode = openMode === "button";
  const canOpen = (l: Item) => l.connected || !l.archived;
  const openTerminal = (l: Item) => {
    if (l.connected) {
      if (l.pid)
        invoke("focus_session", {
          pid: l.pid,
          title: l.task_title,
          cwd: l.cwd,
          sessionId: l.session.cc_session_id,
          provider: l.provider,
        }).catch(() => {});
    } else if (!l.archived) {
      invoke("resume_session", { cwd: l.cwd, sessionId: l.session.cc_session_id, provider: l.provider }).catch(() => {});
    }
  };

  // 先按当前 tab 过滤，再排序：星标恒在最前。match(tab) 是安全网（后端已按 tab/search 过滤，
  // 这里兜底防御性重过滤一遍）；搜索过滤已下沉后端（父组件按 search 请求当前 tab 内全库），
  // 本组件不再做客户端搜索过滤。waiting「等最久优先」由后端 ASC 排序保证，客户端只做 starred 浮顶。
  // useMemo 缓存：编辑便签/重命名时每次按键都会重渲染，不必每次重跑 filter+sort。
  const isStarred = (l: Item) => starred.has(l.session.cc_session_id);
  const shown = useMemo(() => {
    return data
      .filter((l) => match(tab, l, hideDays))
      .sort(
        (a, b) =>
          Number(starred.has(b.session.cc_session_id)) -
          Number(starred.has(a.session.cc_session_id))
      );
  }, [data, tab, hideDays, starred]);

  // 贴纸会话虚拟列表：只挂载可视区 + overscan 内的卡片，避免大量 DOM。
  // estimateSize 取常见卡片高度（无便签/preview 时约 76–84px），实际高度由 measureElement 动态测量。
  const virtualizer = useVirtualizer({
    count: shown.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 82,
    overscan: 6,
    // 贴纸窗口初始约 460×420，减去 tabs/底栏后可视区约 400×320；给 initialRect 避免首帧
    // 没拿到 ResizeObserver 前渲染 0 条。后续真实尺寸进来会立即修正。
    initialRect: { width: 400, height: 320 },
    measureElement:
      typeof window !== "undefined" && "ResizeObserver" in window
        ? (el) => el.getBoundingClientRect().height
        : undefined,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const totalSize = virtualizer.getTotalSize();

  // 无限滚动触发：当可视区底部进入「已加载列表末尾前 10 条」且仍有数据时，通知父组件加载下一页。
  useEffect(() => {
    if (isLoadingMore || !hasMore || shown.length === 0) return;
    const last = virtualItems[virtualItems.length - 1];
    if (last && last.index >= shown.length - 10) {
      onLoadMore();
    }
  }, [virtualItems, shown.length, isLoadingMore, hasMore, onLoadMore]);

  // 各标签角标计数：优先用后端返回的总数（稳定、不随懒加载闪烁）；
  // 未传入时（测试/旧调用）退化为按已加载 data 估算。
  const counts = useMemo<Record<Tab, number>>(() => {
    if (countsProp) {
      return {
        all: countsProp.total - countsProp.archived,
        running: countsProp.running,
        waiting: countsProp.waiting,
        archived: countsProp.archived,
      };
    }
    const c = {} as Record<Tab, number>;
    for (const k of TAB_KEYS) c[k] = data.filter((l) => match(k, l, hideDays)).length;
    return c;
  }, [countsProp, data, hideDays]);

  // 自绘 overlay 滚动条：原生滚动条全程隐藏(不占布局→无抖动)，这里按滚动位置算出
  // thumb 的高度/位置，浮在内容右侧。null = 内容未超出、不需要滚动条。
  const scrollRef = useRef<HTMLDivElement>(null);
  const scrollInnerRef = useRef<HTMLDivElement>(null);
  const [sb, setSb] = useState<{ top: number; height: number } | null>(null);
  const [sbDrag, setSbDrag] = useState(false);
  // 滚动边缘淡出：仅当该方向确有被遮内容时才淡(滚到顶/底则对应边不淡，首/末卡保持清晰)。
  const [edge, setEdge] = useState({ top: false, bottom: false });

  // 底部用量：多 provider。先从 getAccounts() 拿缓存快速预填(仅在还没有联网值时填充，
  // 避免缓存晚到覆盖更新的联网值)，再对有账号且 usage_supported 的 provider 定时刷新（5 min）。
  // usageMap 只存原始 ProviderUsage，label 在渲染时取当前语言，切换即时生效。
  const [usageMap, setUsageMap] = useState<Record<string, ProviderUsage>>({});
  useEffect(() => {
    let cancelled = false;
    const timers: number[] = [];
    getAccounts()
      .then((ps) => {
        if (cancelled) return;
        // 用缓存 usage 快速预填（保留已有的联网值不被缓存覆盖）
        const cached: Record<string, ProviderUsage> = {};
        ps.forEach((p) => { if (p.usage) cached[p.provider] = p.usage; });
        setUsageMap((cur) => {
          const next: Record<string, ProviderUsage> = { ...cached };
          Object.keys(cur).forEach((k) => { if (cur[k]) next[k] = cur[k]; });
          return next;
        });
        // 对有账号且支持用量的 provider：立即刷新 + 定时刷新
        ps.filter((p) => p.account != null && p.usage_supported).forEach(({ provider }) => {
          const doRefresh = () => {
            if (cancelled) return;
            refreshUsage(provider)
              .then((u) => { if (!cancelled) setUsageMap((m) => ({ ...m, [provider]: u })); })
              .catch(() => {}); // USAGE_UNSUPPORTED / 网络错误：保持无用量，不显示该行
          };
          doRefresh();
          timers.push(window.setInterval(doRefresh, 5 * 60_000));
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      timers.forEach((id) => window.clearInterval(id));
    };
  }, []);
  // usageMap 与 quotaProviders 直接传入 UsageScreen，不再在父层预处理为行数组。
  // 交叉过滤：只显示既在配额设置里、又已装的 provider（availAgents 为空=未加载时不过滤，避免闪空）
  const shownQuota = quotaProviders.filter((p) => availAgents.length === 0 || availAgents.includes(p));

  const syncSb = () => {
    const el = scrollRef.current;
    if (!el) return;
    const { scrollTop, scrollHeight, clientHeight, offsetTop } = el;
    if (scrollHeight <= clientHeight + 1) {
      setSb(null);
      setEdge((p) => (p.top || p.bottom ? { top: false, bottom: false } : p));
      return;
    }
    const canUp = scrollTop > 1;
    const canDown = scrollTop + clientHeight < scrollHeight - 1;
    setEdge((p) => (p.top === canUp && p.bottom === canDown ? p : { top: canUp, bottom: canDown }));
    const thumbH = Math.max(28, (clientHeight * clientHeight) / scrollHeight);
    const top = offsetTop + (clientHeight - thumbH) * (scrollTop / (scrollHeight - clientHeight));
    // 相等守卫(与 setEdge 同款):逐卡片 observe 后 RO 触发频度到每秒级,thumb 几何没变时
    // 复用旧引用,避免每次数据刷新都强制整树重渲染。
    setSb((p) => (p && p.top === top && p.height === thumbH ? p : { top, height: thumbH }));
  };
  // 列表内容(shown)或可视尺寸变化时重算 thumb。
  useEffect(() => {
    syncSb();
    const el = scrollRef.current;
    const inner = scrollInnerRef.current;
    // 测试/非浏览器环境无 ResizeObserver：仅同步一次即可。
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(syncSb);
    ro.observe(el);
    // 虚拟列表下，滚动高度来自 inner 容器的总高；观察 inner 即可在 totalSize 或卡片高度变化时
    // 触发重算。同时观察可见卡片 wrapper，确保单张卡展开便签/重命名编辑器时也能及时更新 thumb。
    if (inner) ro.observe(inner);
    for (const child of Array.from(inner?.children ?? el.children)) ro.observe(child);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown]);

  // 拖拽 thumb 滚动列表。
  const onSbDown = (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const el = scrollRef.current;
    if (!el) return;
    setSbDrag(true);
    const startY = e.clientY;
    const startScroll = el.scrollTop;
    const { scrollHeight, clientHeight } = el;
    const thumbH = Math.max(28, (clientHeight * clientHeight) / scrollHeight);
    const move = (ev: MouseEvent) => {
      const ratio = (ev.clientY - startY) / (clientHeight - thumbH);
      el.scrollTop = startScroll + ratio * (scrollHeight - clientHeight);
    };
    const up = () => {
      setSbDrag(false);
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className="sticker">
      {!isMacPanel() && <div className="drag" data-tauri-drag-region />}
      <div className="tabs">
        <div className="tabseg">
          {/* 选中态立体滑块：切换时平滑滑到目标 tab(translateX 动画) */}
          <span
            className="tabseg-slider"
            style={{ transform: `translateX(${TAB_KEYS.indexOf(tab) * 100}%)` }}
          />
          {TAB_KEYS.map((k) => {
            const n = counts[k];
            return (
              <span
                key={k}
                className={"stab " + (tab === k ? "stab-on" : "")}
                onClick={() => pick(k)}
              >
                <TabIcon tab={k} />
                {t.tabs[k]}
                {k !== "all" && k !== "archived" && <span className="stab-n">{n > 99 ? "99+" : n}</span>}
              </span>
            );
          })}
        </div>
      </div>
      <div
        className={"stk-scroll" + (edge.top ? " fade-top" : "") + (edge.bottom ? " fade-bottom" : "")}
        ref={scrollRef}
        onScroll={syncSb}
      >
        {shown.length === 0 ? (
          isLoadingMore ? (
            <div className="stk-loading">{t.sticker.loading}</div>
          ) : (
            <EmptyState tab={tab} onNew={() => invoke("open_new_session_window").catch(() => {})} />
          )
        ) : (
          <div
            ref={scrollInnerRef}
            style={{ height: totalSize, width: "100%", position: "relative" }}
          >
            {virtualItems.map((virtualItem) => {
              const l = shown[virtualItem.index];
              const unnamed = !l.task_title || l.task_title === "(未命名会话)";
              const title = unnamed ? t.sticker.waitingFirstInput : l.task_title;
              const agentCfg = providerConfig(l.provider); // provider 品牌图标 + 标签，按表查
              const agentLabel = agentCfg.label(t);
              // AI 活动行显示「最近一条 AI 正文」(last_ai_text，回退 transcript preview)；出错优先显示错误标签；
              // previewEnabled（对话预览开关）关闭则不显示 AI 正文（仅保留错误）；用户行同受该开关门控。
              const sub = l.errored && l.error_label
                ? t.errorLabels[l.error_label] ?? l.error_label
                : previewEnabled && (l.last_ai_text ?? l.preview)
                ? (l.last_ai_text ?? l.preview)
                : null;
              const subTitle = l.errored ? l.error_raw ?? undefined : sub ?? undefined;
              const indicator = !l.connected ? (
                <span className="ring-stop" data-tip={t.sticker.stopped} />
              ) : l.errored ? (
                <span className="needs-error" data-tip={l.error_raw ?? t.sticker.sessionError} />
              ) : l.pending_review ? (
                <RunBadge pct={l.context_pct} tone="pending" />
              ) : l.session.status === "running" ? (
                <RunBadge pct={l.context_pct} />
              ) : l.session.status === "waiting" ? (
                <RunBadge pct={l.context_pct} tone="waiting" />
              ) : (
                <span className="sdot sdot-on" data-tip={t.sticker.online} />
              );
              return (
                <div
                  key={virtualItem.key}
                  data-index={virtualItem.index}
                  ref={virtualizer.measureElement}
                  className="stk-vitem"
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${virtualItem.start}px)`,
                  }}
                >
                  <div
                    className={"stk-card" + (isStarred(l) ? " is-star" : "")}
                    onClick={() => {
                      // 编辑态(重命名/便签)下，点击卡片仅用于关闭编辑器（失焦），绝不导航开终端。
                      // 注：编辑输入框已 stopPropagation，这里只会被「点击卡片空白处」触发。
                      if (editingId !== null || notingId !== null) {
                        setEditingId(null);
                        setNotingId(null);
                        return;
                      }
                      // 按钮模式：点击卡片不开终端，改由卡片上的打开按钮触发。
                      if (buttonMode) return;
                      openTerminal(l);
                    }}
                    onContextMenu={(e) => {
                      // 与卡片菜单按钮二选一：button 模式下右键只吞掉默认 webview 菜单、不弹卡片菜单。
                      e.preventDefault();
                      if (menuMode === "context") {
                        setCtxMenu({ sid: l.session.id, x: e.clientX, y: e.clientY });
                      }
                    }}
                    style={{ cursor: !buttonMode && canOpen(l) ? "pointer" : "default" }}
                    data-tip={buttonMode ? "" : l.connected ? t.sticker.jumpToTerminal : l.archived ? "" : t.sticker.resumeInTerminal}
                  >
                    <div className="stk-top">
                      <span className="stk-ind">{indicator}</span>
                      <div className="stk-top-body">
                        <div className="stk-line1">
                          {editingId === l.session.id ? (
                            <div className="stk-editbox" onClick={(e) => e.stopPropagation()}>
                              <input
                                className="stk-edit"
                                autoFocus
                                value={draft}
                                placeholder={t.sticker.renamePlaceholder}
                                onChange={(e) => setDraft(e.target.value)}
                                onKeyDown={editorKeyDown(() => submitRename(l), () => setEditingId(null))}
                              />
                              {/* mousedown preventDefault：点按钮不抢走输入框焦点，避免触发其它失焦逻辑 */}
                              <button
                                type="button"
                                className="stk-ebtn stk-ebtn-ok"
                                aria-label={t.sticker.noteSave}
                                data-tip={t.sticker.noteSave}
                                onMouseDown={(e) => e.preventDefault()}
                                onClick={() => submitRename(l)}
                              ><CheckIcon /></button>
                              <button
                                type="button"
                                className="stk-ebtn"
                                aria-label={t.sticker.noteCancel}
                                data-tip={t.sticker.noteCancel}
                                onMouseDown={(e) => e.preventDefault()}
                                onClick={() => setEditingId(null)}
                              ><XIcon /></button>
                            </div>
                          ) : (
                            <>
                              <span className="stk-title">{title}</span>
                              {l.pending_review && (
                                <span className={"pending-pill pending-" + l.pending_review}>
                                  {t.pending[l.pending_review]}
                                </span>
                              )}
                              <span className="stk-time">{fmtAgo(l.session.last_event_at, t)}</span>
                              {/* 星标/便签/重命名/归档操作收进卡片菜单（CardContextMenu），标题行不再挤 hover 图标。
                                  默认右键触发；card_menu_mode=button（触屏等不便右键）时改为此处的常显菜单按钮，
                                  两种触发方式二选一。星标态由卡片金角、便签由便签块表达，收起入口不丢信息。 */}
                              {menuMode === "button" && (
                                <button
                                  type="button"
                                  className="stk-menu-btn"
                                  aria-label={t.sticker.cardMenu}
                                  data-tip={t.sticker.cardMenu}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    const r = e.currentTarget.getBoundingClientRect();
                                    setCtxMenu({ sid: l.session.id, x: r.right, y: r.bottom + 4 });
                                  }}
                                ><MoreIcon /></button>
                              )}
                            </>
                          )}
                        </div>
                        {editingId === l.session.id && l.connected && (
                          <div className="stk-edit-hint">{t.sticker.renameHint}</div>
                        )}
                        <div className="stk-line2">
                          <span
                            className={"stk-agent" + (l.connected ? "" : " stk-agent-off")}
                            data-tip={agentLabel}
                            aria-label={agentLabel}
                          >
                            <agentCfg.Icon />
                          </span>
                          {l.cwd && (
                            <span className="stk-repo" data-tip={l.cwd}>
                              {l.cwd.split(/[\\/]/).filter(Boolean).pop() ?? l.cwd}
                            </span>
                          )}
                          {l.model && <span className="stk-model">{l.model}</span>}
                        </div>
                      </div>
                    </div>
                    {notingId === l.session.id ? (
                      <div className="stk-editbox stk-editbox-note" onClick={(e) => e.stopPropagation()}>
                        <span className="stk-editbox-ico"><NoteIcon /></span>
                        <input
                          className="stk-note-edit"
                          autoFocus
                          value={noteDraft}
                          placeholder={t.sticker.notePlaceholder}
                          onChange={(e) => setNoteDraft(e.target.value)}
                          onKeyDown={editorKeyDown(() => submitNote(l), () => setNotingId(null))}
                        />
                        {/* mousedown preventDefault：点按钮不抢走输入框焦点，避免触发其它失焦逻辑 */}
                        <button
                          type="button"
                          className="stk-ebtn stk-ebtn-ok"
                          aria-label={t.sticker.noteSave}
                          data-tip={t.sticker.noteSave}
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => submitNote(l)}
                        ><CheckIcon /></button>
                        <button
                          type="button"
                          className="stk-ebtn"
                          aria-label={t.sticker.noteCancel}
                          data-tip={t.sticker.noteCancel}
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => setNotingId(null)}
                        ><XIcon /></button>
                      </div>
                    ) : l.note ? (
                      <div
                        className="stk-note"
                        data-tip={t.sticker.noteEdit}
                        onClick={(e) => { e.stopPropagation(); startNote(l); }}
                      >
                        <span className="stk-note-icon"><NoteIcon /></span>
                        <span className="stk-note-txt">{l.note}</span>
                      </div>
                    ) : null}
                    {previewEnabled && l.last_user_text && (
                      <div className="stk-subrow stk-userrow">
                        <span className="stk-msg-tag">{t.sticker.youPrefix}</span>
                        <span className="stk-sub" data-tip={l.last_user_text}>{l.last_user_text}</span>
                      </div>
                    )}
                    {(sub || (buttonMode && canOpen(l))) && (
                      <div className="stk-subrow">
                        {/* 活动行：最近一条 AI 正文(或错误标签)，单行截断；title 给完整文本，hover 原生提示可读全文 */}
                        {sub && !l.errored && <span className="stk-msg-tag is-ai">{t.sticker.aiPrefix}</span>}
                        {sub && <span className={"stk-sub" + (l.errored ? " stk-sub-err" : "")} data-tip={subTitle}>{sub}</span>}
                        {/* 按钮模式：打开终端按钮内联在该行末尾，不突兀 */}
                        {buttonMode && canOpen(l) && (
                          <button
                            type="button"
                            className="stk-open"
                            data-tip={l.connected ? t.sticker.jumpToTerminal : t.sticker.resumeInTerminal}
                            aria-label={l.connected ? t.sticker.jumpToTerminal : t.sticker.resumeInTerminal}
                            onClick={(e) => { e.stopPropagation(); openTerminal(l); }}
                          ><OpenIcon /></button>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
            {isLoadingMore && (
              <div className="stk-loadmore" style={{ position: "absolute", top: totalSize, left: 0, width: "100%" }}>
                <span className="stk-loadmore-dot" />
                <span className="stk-loadmore-dot" />
                <span className="stk-loadmore-dot" />
              </div>
            )}
          </div>
        )}
      </div>
      {sb && (
        <div
          className={"stk-sb" + (sbDrag ? " is-drag" : "")}
          style={{ top: sb.top, height: sb.height }}
          onMouseDown={onSbDown}
        />
      )}
      {ctxMenu && ctxItem && (
        <CardContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          starred={isStarred(ctxItem)}
          hasNote={!!ctxItem.note}
          archived={ctxItem.archived}
          onStar={() => toggleStar(ctxItem.session.cc_session_id)}
          onNote={() => startNote(ctxItem)}
          onRename={() => startRename(ctxItem)}
          onArchive={() =>
            invoke("set_archived", { sessionId: ctxItem.session.id, archived: !ctxItem.archived }).catch(() => {})
          }
          onNewSession={() =>
            invoke("open_new_session_window", {
              cwd: ctxItem.cwd,
              provider: ctxItem.provider,
            }).catch(() => {})
          }
          onOpenDir={
            ctxItem.cwd ? () => invoke("open_project_dir", { cwd: ctxItem.cwd }).catch(() => {}) : null
          }
          onClose={() => setCtxMenu(null)}
        />
      )}
      {/* 底栏:用量(左) + 搜索/设置/固定(右)聚为一处;搜索激活时整条变输入框。 */}
      <div className="stk-bar">
        {searchOpen ? (
          <div className="stk-search">
            <span className="stk-search-ic">
              <SearchIcon />
            </span>
            <input
              className="stk-search-in"
              autoFocus
              value={q}
              placeholder={t.sticker.searchPlaceholder}
              onChange={(e) => onSearchChange?.(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") closeSearch();
              }}
            />
            <span className="stk-act stk-search-x" data-tip={t.sticker.searchClose} aria-label={t.sticker.searchClose} onClick={closeSearch}>
              <CloseIcon />
            </span>
          </div>
        ) : (
          <>
            <UsageScreen quotaProviders={shownQuota} usageMap={usageMap} />
            <div className="stk-bar-actions">
              <span
                className="stk-act"
                data-tip={t.newSession.newButton}
                aria-label={t.newSession.newButton}
                data-testid="bar-new"
                onClick={() => invoke("open_new_session_window").catch(() => {})}
              >
                <PlusIcon />
              </span>
              <span className="stk-act" data-tip={t.sticker.search} aria-label={t.sticker.search} onClick={() => setSearchOpen(true)}>
                <SearchIcon />
              </span>
              <span
                className="stk-act"
                data-tip={hasUpdate ? t.sticker.updateAvailable : t.sticker.openSettings}
                aria-label={hasUpdate ? t.sticker.updateAvailable : t.sticker.openSettings}
                // 有更新时红点按钮直达更新窗口，否则照常打开设置。
                onClick={() => invoke(hasUpdate ? "open_update_window" : "open_settings").catch(() => {})}
              >
                <GearIcon />
                {hasUpdate && <span className="stk-dot" aria-hidden="true" />}
              </span>
              {!isMacPanel() && (
                <span
                  className={"stk-act " + (pinned ? "stk-pin-on" : "")}
                  data-tip={pinned ? t.sticker.pinOn : t.sticker.pinOff}
                  aria-label={pinned ? t.sticker.pinOn : t.sticker.pinOff}
                  onClick={togglePin}
                >
                  <PinIcon pinned={pinned} />
                </span>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
