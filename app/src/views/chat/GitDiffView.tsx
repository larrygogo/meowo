import {
  Fragment,
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MutableRefObject,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  getGitFileDiff,
  listDirEntries,
  listFileOpeners,
  openPathWith,
  readFileText,
  readImagePreview,
  revealPathInFileManager,
  searchProjectFiles,
  type DirEntryDto,
  type FileOpenerDto,
  type FileTextDto,
  type GitDiffSummaryDto,
  type RepoChangesDto,
  type GitFileDiffDto,
  type ImagePreviewDto,
  type SearchResultDto,
} from "../../api";
import { FileMarkdown } from "./FileMarkdown";
import { useMenuPopup } from "../menu";
import { folderName } from "../../paths";
import { pushEscLayer } from "../../escLayers";
import { remoteUi } from "../../remoteMode";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import type { Dict } from "../../i18n/zh";
import { buildFileTree, diffLineClass, normPath, touchedByPath, type FileTreeNode } from "./gitDiff";
import { SourceControl } from "./SourceControl";
import { diffLineAnchors, diffLineNumbers, type DiffLineAnchor, type ReviewComment } from "./diffComments";
import { highlightLines, languageForPath, useHighlighter } from "./highlight";
import {
  buildPathMenuItems, CopyIcon, extKeyOf, OpenerIcon, OpenIcon, PathContextMenu, PathMentionContext, PathMenu, RowMore, useFileOpeners,
} from "./pathMenu";
import { FileIcon, FolderIcon, LineCounts, readDiffLayout, saveDiffLayout, singleRepo, touchedInRoot } from "./diffShared";
import { registerDiffModule } from "./lazyDiff";

// 拆到 pathMenu / diffShared 之前这些都从这里导出，老调用方（含测试）照旧能从这里拿。
export { OpenIcon, PathContextMenu, PathMentionContext, PathMenu, RowMore, type PathMenuItem } from "./pathMenu";
export { FileIcon, FolderIcon, LineCounts, readDiffLayout, saveDiffLayout, singleRepo, touchedInRoot } from "./diffShared";

/** 后端对二进制 diff 给的占位行（见 git.rs untracked_file_diff / GitFileDiffDto 注释）。 */
const BINARY_PLACEHOLDER = "(binary file)";

/** 估算等宽网格下最宽一行的列数（ASCII=1、宽字符按 2、tab 按 8）。行容器开了
 *  content-visibility 后，跳过渲染的行不参与 pre 的 max-content 测量——不用这个
 *  估算钉住 min-width 的话，滚动时横向宽度/滚动条会随视口内最宽行跳变。 */
function estimateMaxColumns(lines: string[]): number {
  let max = 0;
  for (const line of lines) {
    let cols = 0;
    for (let i = 0; i < line.length; i += 1) {
      const code = line.charCodeAt(i);
      cols += code === 9 ? 8 : code < 0x1100 ? 1 : 2;
    }
    if (cols > max) max = cols;
  }
  return max;
}

/** 「文件」页签可直接预览的图片扩展名（与后端 image_mime 同表）。 */
const IMAGE_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg", "avif"]);
const MD_EXTS = new Set(["md", "markdown"]);

/** 文件的预览形态：图片直显 / markdown 排版渲染 / null = 纯文本（走高亮源码视图）。 */
function previewKindOf(rel: string): "image" | "markdown" | null {
  const ext = extKeyOf(rel);
  if (IMAGE_EXTS.has(ext)) return "image";
  if (MD_EXTS.has(ext)) return "markdown";
  return null;
}

/** 以 md 文件所在目录为基准解析文内相对引用（`/` 开头按仓库根）；越出仓库或
 *  解析失败返回 null（后端 resolve_inside 仍是最终防线）。 */
function resolveMdRel(baseDir: string, src: string): string | null {
  let clean = src.split(/[?#]/)[0];
  try {
    clean = decodeURIComponent(clean);
  } catch {
    return null;
  }
  if (!clean) return null;
  // 分段必须同时按 `/` 与 `\` 切:Windows 上文内引用可能写成 `..\..\x`,只按 `/` 切会让
  // 整串成为单个「段」,`..` 上跳守卫（out.pop）永不触发,穿越判定被绕过（后端 resolve_inside
  // 仍兜底,但前端不该把越界路径当合法解析出来）。根前缀也一并认反斜杠。
  const rooted = clean.startsWith("/") || clean.startsWith("\\");
  const body = rooted ? clean.replace(/^[\\/]+/, "") : baseDir ? `${baseDir}/${clean}` : clean;
  const out: string[] = [];
  for (const part of body.split(/[\\/]+/)) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (out.length === 0) return null;
      out.pop();
      continue;
    }
    out.push(part);
  }
  return out.length ? out.join("/") : null;
}

/** 预览里高亮首个命中片段（大小写不敏感；找不到原样返回）。 */
function markMatch(text: string, query: string): ReactNode {
  const needle = query.trim().toLowerCase();
  if (!needle) return text;
  const index = text.toLowerCase().indexOf(needle);
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <mark className="chat-diff-hit-mark">{text.slice(index, index + needle.length)}</mark>
      {text.slice(index + needle.length)}
    </>
  );
}

/** 文件头「打开」分体按钮：主键一键用系统首选应用（清单为空则系统默认关联）打开，
 *  箭头展开完整菜单（复制路径/文件管理器定位/各打开方式）。 */
function OpenSplitButton({ cwd, rel }: { cwd: string; rel: string }) {
  const t = useT();
  const openers = useFileOpeners(cwd, rel);
  const mention = useContext(PathMentionContext);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const primary = openers[0] ?? null;
  return (
    <div className="chat-diff-open" ref={wrapRef}>
      <button
        type="button"
        className="chat-diff-open-main"
        data-tip={primary ? primary.name : t.chat.diffOpenWithDefault}
        onClick={() => void openPathWith(cwd, rel, primary?.id ?? "default").catch(() => {})}
      >
        {primary ? <OpenerIcon opener={primary} /> : <OpenIcon />}
        {t.chat.diffOpen}
      </button>
      <button
        type="button"
        className="chat-diff-open-caret"
        aria-label={t.chat.diffOpenWith}
        aria-haspopup="menu"
        aria-expanded={!!menu}
        onClick={() => {
          if (menu) {
            setMenu(null);
            return;
          }
          const rect = wrapRef.current?.getBoundingClientRect();
          if (rect) setMenu({ x: rect.left, y: rect.bottom + 4 });
        }}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
      {menu && (
        <PathMenu x={menu.x} y={menu.y} items={buildPathMenuItems(cwd, rel, openers, t, mention)} onClose={() => setMenu(null)} />
      )}
    </div>
  );
}

type FetchEntry<T> = { loading: boolean; error: string | null; data: T | null };

/** 面板里几处小开关（换行、文件树位置/宽度/显隐）记在 localStorage；隐私模式下读写会抛，抛了就是不记。 */
function readPref(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function savePref(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* 只是不记 */ }
}

/** 代码和 diff 的自动换行，「文件」「更改」共用一个开关：Kimi Code 两处各有一个，但想要换行的人两处都想要。 */
const WRAP_KEY = "meowo-code-wrap";
function useWrapPref(): [boolean, () => void] {
  const [wrap, setWrap] = useState(() => readPref(WRAP_KEY) === "1");
  const toggle = () => setWrap((prev) => {
    savePref(WRAP_KEY, prev ? "0" : "1");
    return !prev;
  });
  return [wrap, toggle];
}

/// 正在看的文件被 agent 改了没有：面板可见时每 5 秒在后台重拉一次，和眼前显示的比。
/// 不一样就把新结果先攒着，表头的「刷新」亮成橙色，点了才换（照 Kimi Code：正看着的内容
/// 不自己跳走）。`bump` 变了（改动清单刷新过）立刻查一次。返回攒着的新结果，没有为 null。
const STALE_POLL_MS = 5000;
function useStaleWatch<T>(
  key: string | null,
  current: T | null,
  fetcher: () => Promise<T>,
  same: (a: T, b: T) => boolean,
  live: boolean,
  bump: unknown,
): T | null {
  const [fresh, setFresh] = useState<{ key: string; base: T; data: T } | null>(null);
  const latest = useRef({ key, current, fetcher, same, live });
  latest.current = { key, current, fetcher, same, live };
  const check = useCallback(() => {
    const { key: k, current: base, fetcher: fetch, same: eq, live: on } = latest.current;
    if (!on || !k || base == null || document.hidden) return;
    fetch()
      .then((data) => {
        const now = latest.current;
        if (now.key === k && now.current === base && !eq(base, data)) setFresh({ key: k, base, data });
      })
      .catch(() => {});
  }, []);
  useEffect(() => {
    if (!live || !key || current == null) return;
    const timer = window.setInterval(check, STALE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [live, key, current, check]);
  const mounted = useRef(false);
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    check();
  }, [bump, check]);
  return fresh && fresh.key === key && fresh.base === current ? fresh.data : null;
}

/** 表头的「刷新」：平时是个灰图标，内容过期时亮成带字的橙色按钮。 */
function RefreshButton({ stale, onClick }: { stale: boolean; onClick: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      className={"chat-diff-iconbtn chat-diff-refresh" + (stale ? " is-stale" : "")}
      aria-label={stale ? t.chat.previewStale : t.chat.previewRefresh}
      data-tip={stale ? t.chat.previewStale : t.chat.previewRefresh}
      onClick={onClick}
    >
      <RefreshIcon />
      {stale && <span>{t.chat.previewRefresh}</span>}
    </button>
  );
}

function WrapButton({ wrap, onToggle }: { wrap: boolean; onToggle: () => void }) {
  const t = useT();
  return (
    <button type="button" className="chat-diff-iconbtn" aria-pressed={wrap} aria-label={t.chat.codeWrap} data-tip={t.chat.codeWrap} onClick={onToggle}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M4 6h16M4 12h13a3 3 0 0 1 0 6h-4" /><path d="M15 16l-2 2 2 2" /><path d="M4 18h5" />
      </svg>
    </button>
  );
}

/** 压成一行的 JSON（接口回包、构建产物常见）预览时展开成缩进格式，照 Kimi Code 的文件预览。
 *  本来就分行的不动（行号要和编辑器里一致），截断了的解析不了也不动。 */
function prettyJson(rel: string, text: string, truncated: boolean): string {
  if (truncated || extKeyOf(rel) !== "json" || text.trim().includes("\n")) return text;
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

/** 文件树侧栏的宽度（照 Kimi Code：默认 224，160–400）、位置和显隐。 */
const TREE_OPEN_KEY = "meowo-file-tree-open";
const TREE_POS_KEY = "meowo-file-tree-position";
const TREE_WIDTH_KEY = "meowo-file-tree-width";
const TREE_WIDTH_MIN = 160;
const TREE_WIDTH_MAX = 400;
const TREE_WIDTH_DEFAULT = 224;
function clampTreeWidth(width: number): number {
  return Math.round(Math.max(TREE_WIDTH_MIN, Math.min(TREE_WIDTH_MAX, width)));
}

/**
 * 「文件 / 更改」停靠面板：挂在 .chat-body 行内的右列（对话区收缩让位，非覆盖层）。
 * 「更改」页签仅仓库会话可见（summary?.isRepo）；非仓库会话打开时只有「文件」页签。
 * memo：折叠只是 display:none 不卸载，不隔断的话 ChatWindow 每次击键都要对面板里
 * 可能上千行的文件 DOM 做全量 reconcile；父层回调已换稳定引用配合生效。
 */
export const GitDiffView = memo(GitDiffViewImpl);
export type DiffPanelTab = "changes" | "files";

function GitDiffViewImpl({ cwd, summary, repos = null, width, collapsed, maximized, onCollapse, onToggleMaximize, comments, onAddComment, onRemoveComment, touchedPaths, turnTouchedPaths = [], activeTab, onTabChange, selectedRoot = null, onSelectRoot, onChanged, sessionId = null }: {
  /** 会话目录：文件树与更改都对着它。 */
  cwd: string;
  summary: GitDiffSummaryDto | null;
  /** 多仓库「更改」（见 SourceControl）；null = 还没拉到，按 summary 单仓库显示。 */
  repos?: readonly RepoChangesDto[] | null;
  /** 停靠宽度（px，ChatWindow 侧的分栏柄拖动改写）；最大化时忽略（面板占满整宽）。 */
  width: number;
  /** 折叠 = 整面板 display:none（不卸载，内部状态全保留）。 */
  collapsed: boolean;
  /** 最大化 = 隐藏对话卡（.chat-main），面板占满整宽。 */
  maximized: boolean;
  onCollapse: () => void;
  onToggleMaximize: () => void;
  /** 本会话攒着的改动评论（全部仓）；面板只显示激活仓的那些。 */
  comments: readonly ReviewComment[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">, dir: string) => void;
  onRemoveComment: (id: string) => void;
  /** 本会话编辑工具碰过的文件（见 sessionTouchedPaths）；空数组 = 认不出，不出过滤开关。 */
  touchedPaths: readonly string[];
  /** 最近一轮（最后一条用户消息之后）编辑工具碰过的文件，口径同 touchedPaths。 */
  turnTouchedPaths?: readonly string[];
  /** ChatWindow 顶栏直达入口需要受控切页；独立使用时保留内部状态。 */
  activeTab?: DiffPanelTab;
  onTabChange?: (tab: DiffPanelTab) => void;
  /** 选中的仓库（见 SourceControl）。 */
  selectedRoot?: string | null;
  onSelectRoot?: (root: string) => void;
  onChanged?: () => void;
  /** 会话 id：PR 合并后提示清理它的 worktree（见 ForgeBar）。 */
  sessionId?: number | null;
}) {
  const t = useT();
  const changesAvailable = !!summary?.isRepo || (repos?.length ?? 0) > 0;
  const [internalTab, setInternalTab] = useState<DiffPanelTab>(changesAvailable ? "changes" : "files");
  const tab = activeTab ?? internalTab;
  const selectTab = (next: DiffPanelTab) => {
    setInternalTab(next);
    onTabChange?.(next);
  };
  // 评论框开着时 Esc 只关评论框：面板的 Esc 监听挂在捕获段、先于评论框收到按键，
  // 不在这里分流的话一按 Esc 整个面板就收起了，写了一半的评论也看不见了。
  const escOverrideRef = useRef<(() => void) | null>(null);
  const dirComments = useMemo(() => comments.filter((comment) => comment.dir === cwd), [comments, cwd]);

  // Esc 折叠面板。挂捕获段并拦住传播（ChatWindow 有全局 Esc=拒绝审批的监听，
  // 且焦点可能在 composer 输入框里——那里的 Esc 处理器不查层栈，只有捕获段
  // 截停才拦得住），同时注册 Esc 层作为统一让位判据——与灯箱同一套双保险。
  // 折叠期间不挂：面板不可见时 Esc 归对话页（含 Esc=拒绝审批）原语义。
  useEffect(() => {
    if (collapsed) return;
    const popLayer = pushEscLayer();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // 页内弹层（AppModal：重命名、截图标注…）开着时 Esc 归弹层。这里挂在捕获段，
      // 不让开的话弹层收不到 Esc，按下去收起的是背后的面板。
      if (document.querySelector(".chat-modal-overlay")) return;
      // 面板里的菜单（文件右键菜单）开着时 Esc 只关菜单。这里挂在捕获段，不让开的话
      // 先收起的是整个面板。
      if (document.querySelector('.chat-diff-panel [role="menu"]')) return;
      event.stopPropagation();
      if (escOverrideRef.current) escOverrideRef.current();
      else onCollapse();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey, true);
    };
  }, [collapsed, onCollapse]);

  return (
    <aside
      className={"chat-diff-panel" + (collapsed ? " is-collapsed" : "")}
      style={maximized ? undefined : { width }}
      aria-label={t.chat.diff}
    >
      {/* 最大化时对话卡（含承载窗口拖拽区的 .chat-bar）整体隐藏，
          面板头行因此也要是拖拽区；行内按钮不带该属性，点击不受影响（同 .chat-bar 约定）。 */}
      <header className="chat-diff-head" data-tauri-drag-region>
        {/* 手机上面板铺满全屏、只有「更改」一页：孤零零一个分段页签加右上角收起图标读不出
          「这是哪、怎么回去」（用户实拍）。改成与对话页同款的返回箭头 + 标题。 */}
        {remoteUi() && (
          <>
            <button type="button" className="chat-diff-back" aria-label={t.chat.diffBack} onClick={onCollapse}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <span className="chat-diff-title">{t.chat.diffChangesTab}</span>
          </>
        )}
        {!remoteUi() && <div className="chat-diff-tabs">
          {/* 远程只放行改动列表（remote.rs 模块头「例外」），「文件」页签要读任意文件，不给。 */}
          {!remoteUi() && <button type="button" className={tab === "files" || !changesAvailable ? "is-active" : ""} onClick={() => selectTab("files")}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            </svg>
            {t.chat.filesTab}
          </button>}
          {changesAvailable && (
            <button type="button" className={tab === "changes" ? "is-active" : ""} onClick={() => selectTab("changes")}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="9" r="2.5" />
                <path d="M6 8.5v7" /><path d="M18 11.5c0 4-4 4.5-9.2 4.6" />
              </svg>
              {t.chat.diffChangesTab}
            </button>
          )}
        </div>}
        {/* 分支名刻意不显示：长分支名（feat/xxx-日期-作者）会把头行挤到换行。 */}
        <span className="chat-diff-spacer" />
        {/* 最大化/还原：隐藏对话卡让面板占满整宽。图标走对角箭头——展开时两角向外，
            最大化后两角向内（还原）。远程页面板恒占满全屏，不给这颗钮。 */}
        {!remoteUi() && <button
          type="button"
          className="chat-diff-action"
          data-tip={maximized ? t.chat.diffRestore : t.chat.diffMaximize}
          aria-label={maximized ? t.chat.diffRestore : t.chat.diffMaximize}
          onClick={onToggleMaximize}
        >
          {maximized ? (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M4 14h6v6" /><path d="M20 10h-6V4" /><path d="M14 10l7-7" /><path d="M3 21l7-7" />
            </svg>
          ) : (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M15 3h6v6" /><path d="M9 21H3v-6" /><path d="M21 3l-7 7" /><path d="M3 21l7-7" />
            </svg>
          )}
        </button>}
        {/* 折叠：面板收成隐藏（不卸载，状态保留）。图标与侧栏开关同款「面板+分隔线」，
            镜像到右缘（面板在右侧，线靠右）。 */}
        {!remoteUi() && <button type="button" className="chat-diff-action" data-tip={t.chat.diffCollapse} aria-label={t.chat.diffCollapse} onClick={onCollapse}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
            <rect x="3.5" y="4.5" width="17" height="15" rx="3" />
            <path d="M14.5 4.5v15" />
          </svg>
        </button>}
      </header>
      {/* key=cwd:切仓即重挂,树展开态/选中文件不跨仓串场。 */}
      {tab === "changes" && ((repos && repos.length > 0) || (changesAvailable && summary?.isRepo))
        ? (
          <SourceControl
            key={cwd}
            cwd={cwd}
            repos={repos && repos.length > 0 ? repos : singleRepo(cwd, summary!)}
            selectedRoot={selectedRoot}
            onSelectRoot={onSelectRoot ?? (() => {})}
            onChanged={onChanged ?? (() => {})}
            comments={comments}
            touchedPaths={touchedPaths}
            turnTouchedPaths={turnTouchedPaths}
            onAddComment={onAddComment}
            onRemoveComment={onRemoveComment}
            escOverrideRef={escOverrideRef}
            sessionId={sessionId}
            visible={!collapsed}
          />
        )
        : !remoteUi() && <FilesView key={cwd} cwd={cwd} />}
    </aside>
  );
}

/** 「更改」页签：可搜索的变更目录树 + 单文件 diff（返回行 + 着色行）。 */
export function ChangesView({ cwd, summary, comments, touchedPaths, turnTouchedPaths, onAddComment, onRemoveComment, escOverrideRef, live = false, diffBase = null, sessionCwd = null, initialPath = null, onBack, onSelectPath }: {
  cwd: string;
  summary: GitDiffSummaryDto;
  comments: readonly ReviewComment[];
  touchedPaths: readonly string[];
  turnTouchedPaths: readonly string[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">) => void;
  onRemoveComment: (id: string) => void;
  escOverrideRef: MutableRefObject<(() => void) | null>;
  /** 面板正显示着：这时才在后台查打开的文件过期没有。 */
  live?: boolean;
  /** 「已提交未合并」段：单文件 diff 看 diffBase..HEAD（见 SourceControl）。 */
  diffBase?: string | null;
  /** cwd 不是会话目录本身时的会话目录（手机远程页收口用）。 */
  sessionCwd?: string | null;
  /** 进来就打开的文件（从多仓库分组列表点进来）。 */
  initialPath?: string | null;
  /** 从文件页返回时交给外层（回到多仓库分组列表），不回本组件自己的列表。 */
  onBack?: () => void;
  /** 打开 / 关上的文件告诉外层：轮询让面板在单仓库与分组形态间切换时，外层据此把它重新打开。 */
  onSelectPath?: (path: string | null) => void;
}) {
  const t = useT();
  // 当前查看的文件（null = 文件树形态）；diff 结果按路径缓存，往返切换不重复拉取。
  const [selectedPath, setSelectedPath] = useState<string | null>(initialPath);
  const onSelectPathRef = useRef(onSelectPath);
  onSelectPathRef.current = onSelectPath;
  useEffect(() => { onSelectPathRef.current?.(selectedPath); }, [selectedPath]);
  const [entries, setEntries] = useState<Map<string, FetchEntry<GitFileDiffDto>>>(new Map());
  // 图片改动的预览缓存(与 FilesView 的图片通道同款):git 对二进制只会说 "(binary file)",
  // 但图片完全可以直接把工作区当前版本渲染出来——「二进制文件,无法显示 diff」对一张
  // 刚生成的截图毫无帮助(实拍反馈)。
  const [images, setImages] = useState<Map<string, FetchEntry<ImagePreviewDto>>>(new Map());
  useEffect(() => {
    if (!selectedPath || previewKindOf(selectedPath) !== "image" || images.has(selectedPath)) return;
    const rel = selectedPath;
    setImages((prev) => new Map(prev).set(rel, { loading: true, error: null, data: null }));
    readImagePreview(cwd, rel)
      .then((data) => setImages((prev) => new Map(prev).set(rel, { loading: false, error: null, data })))
      .catch((error) => setImages((prev) => new Map(prev).set(rel, { loading: false, error: formatBackendError(error, t.locale), data: null })));
  }, [selectedPath, images, cwd, t.locale]);
  const [search, setSearch] = useState("");
  // 目录树默认全部展开，这里只记录被手动折叠的目录路径。
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  // 树行右键的路径菜单（复制/定位/打开方式）。已删除文件（status D）盘上无实体，不出菜单。
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; rel: string; isDir: boolean } | null>(null);
  const openCtxMenu = (event: ReactMouseEvent, rel: string, isDir: boolean) => {
    // 远程页不给路径菜单：里面全是在桌面上开程序/定位文件的动作，手机上点了没有意义
    // （远程桥也不放行），长按交还浏览器默认行为。
    if (remoteUi()) return;
    event.preventDefault();
    setCtxMenu({ x: event.clientX, y: event.clientY, rel, isDir });
  };
  // 行尾「…」：和右键同一份菜单。手机页没有路径菜单，已删除的文件盘上没东西，都不出。
  const rowMore = (rel: string, isDir: boolean) =>
    remoteUi() ? null : (
      <RowMore label={t.chat.filesMore} open={ctxMenu?.rel === rel} onOpen={(x, y) => setCtxMenu({ x, y, rel, isDir })} />
    );

  // summary 每次刷新（面板重新可见、「更改」开着时 5 秒轮询到变化）都作废缓存：agent 随时
  // 在提交/改写文件，旧 diff 相对旧清单、旧图片是改写前的字节。images 与 entries 必须一起清，
  // 只清 entries 会让「改过的图重开仍显旧图」。正在看的那个留着：换不换交给表头的「刷新」
  // （useStaleWatch），不然轮询一到眼前的 diff 就闪一下重新加载。
  const selectedRef = useRef(selectedPath);
  selectedRef.current = selectedPath;
  useEffect(() => {
    const keep = selectedRef.current;
    const prune = <T,>(prev: Map<string, T>): Map<string, T> => {
      const kept = keep ? prev.get(keep) : undefined;
      if (prev.size === 0 || (prev.size === 1 && kept !== undefined)) return prev;
      return kept !== undefined && keep ? new Map([[keep, kept]]) : new Map();
    };
    setEntries(prune);
    setImages(prune);
  }, [summary]);

  const fileByPath = useMemo(() => new Map(summary.files.map((file) => [file.path, file])), [summary.files]);

  // 改动范围：全部 / 本会话 / 本轮。工作区里常混着用户手改的、别的并行会话改的文件；
  // 「本轮」再收窄到最后一条用户消息之后（对照 Kimi 的「本轮改动」）。只列文件，单文件
  // diff 仍是相对上次提交的全部改动。只有认得出编辑路径的 agent（touchedPaths 非空）才出开关。
  const [scope, setScope] = useState<"all" | "session" | "turn">("all");
  const sessionFiles = useMemo(
    () => (touchedPaths.length > 0 ? new Set(summary.files.filter((file) => touchedByPath(touchedPaths, cwd, file.path)).map((file) => file.path)) : null),
    [touchedPaths, cwd, summary.files],
  );
  const turnFiles = useMemo(
    () => new Set(summary.files.filter((file) => touchedByPath(turnTouchedPaths, cwd, file.path)).map((file) => file.path)),
    [turnTouchedPaths, cwd, summary.files],
  );
  const scopeFiles = !sessionFiles ? null : scope === "session" ? sessionFiles : scope === "turn" ? turnFiles : null;
  const filterSession = !!scopeFiles;
  const visibleFiles = useMemo(
    () => (scopeFiles ? summary.files.filter((file) => scopeFiles.has(file.path)) : summary.files),
    [scopeFiles, summary.files],
  );
  // 合计跟着当前列表走：过滤到本会话时显示的就是本会话改了多少。
  const totals = useMemo(() => {
    const counted = visibleFiles.filter((file) => file.additions != null && file.deletions != null);
    if (counted.length === 0) return null;
    return {
      additions: counted.reduce((sum, file) => sum + (file.additions ?? 0), 0),
      deletions: counted.reduce((sum, file) => sum + (file.deletions ?? 0), 0),
    };
  }, [visibleFiles]);

  const listedFiles = useMemo(() => {
    const query = search.trim().toLowerCase();
    return query ? visibleFiles.filter((file) => file.path.toLowerCase().includes(query)) : visibleFiles;
  }, [visibleFiles, search]);
  const tree = useMemo(() => buildFileTree(listedFiles), [listedFiles]);
  // 列表 / 树形（对照 Kimi 改动面板）：列表一行一个完整路径，改动散在多个目录时一眼扫完；
  // 树形按目录收拢。默认列表，选了就记住。
  const [layout, setLayoutState] = useState<"list" | "tree">(readDiffLayout);
  const setLayout = (next: "list" | "tree") => { setLayoutState(next); saveDiffLayout(next); };

  // 按需拉单文件 diff：一次会话改动可以很大，不随摘要预取全部。
  // 已缓存（含加载中/失败）的路径跳过，失败结果回树后重新点开即重试。
  useEffect(() => {
    if (!selectedPath || entries.has(selectedPath)) return;
    // 清单里可能已经没有它了（看着的时候被提交了，点了「刷新」）：照样拉，拉回空 diff
    // 就是「已是最新」的提示，不能停在加载中。
    const file = fileByPath.get(selectedPath);
    const path = selectedPath;
    setEntries((prev) => new Map(prev).set(path, { loading: true, error: null, data: null }));
    getGitFileDiff(cwd, path, !diffBase && file?.status === "U", diffBase, sessionCwd)
      .then((data) => setEntries((prev) => new Map(prev).set(path, { loading: false, error: null, data })))
      .catch((error) => setEntries((prev) => new Map(prev).set(path, { loading: false, error: formatBackendError(error, t.locale), data: null })));
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 组件按仓库与分段 key 挂载，diffBase/sessionCwd 随 key 一起换；t.locale 只用来格式化报错文案
  }, [selectedPath, entries, cwd, fileByPath]);

  const toggleDir = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const selected = selectedPath ? fileByPath.get(selectedPath) ?? null : null;
  const selectedEntry = selectedPath ? entries.get(selectedPath) ?? null : null;

  // diff 文本高亮（useMemo 按 diff 对象缓存，切换/重渲染不重算）：
  // meta（diff --git/index/---/+++）与 hunk（@@）行返回 null 占位——保持原有配色不高亮。
  // 正文行不逐行独立高亮，而是拆成新旧两篇整体送 hljs（跨行 token 状态保留）：
  // del 行只属旧文，add 行只属新文，上下文行两篇都进（作双方的语法上下文，
  // 渲染取新文侧结果）。hunk 之间仍是拼接断层——可能错色，但比逐行强得多。
  const activeDiff = selectedEntry?.data ?? null;
  // hljs 懒加载（见 highlight.ts）：就绪与否进依赖，引擎到了重算出带颜色的行。
  const highlighterReady = useHighlighter(!!activeDiff);
  const highlightedDiff = useMemo(() => {
    if (!activeDiff || activeDiff.diff.trim() === BINARY_PLACEHOLDER) return null;
    const lang = languageForPath(activeDiff.path);
    const rawLines = activeDiff.diff.split("\n");
    const oldIndices: number[] = [];
    const oldLines: string[] = [];
    const newIndices: number[] = [];
    const newLines: string[] = [];
    rawLines.forEach((line, index) => {
      const cls = diffLineClass(line);
      if (cls === "is-meta" || cls === "is-hunk" || line === "") return;
      const content = cls === "is-add" || cls === "is-del" || line.startsWith(" ") ? line.slice(1) : line;
      if (cls === "is-del") {
        oldIndices.push(index);
        oldLines.push(content);
      } else if (cls === "is-add") {
        newIndices.push(index);
        newLines.push(content);
      } else {
        // 上下文行：-1 标记旧文侧「只当上下文、不取结果」。
        oldIndices.push(-1);
        oldLines.push(content);
        newIndices.push(index);
        newLines.push(content);
      }
    });
    const result: (string | null)[] = rawLines.map(() => null);
    highlightLines(newLines, lang).forEach((html, i) => {
      result[newIndices[i]] = html;
    });
    highlightLines(oldLines, lang).forEach((html, i) => {
      const at = oldIndices[i];
      if (at >= 0) result[at] = html;
    });
    return result;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- highlighterReady 是 hljs 懒加载就绪信号，引擎到了要重算出带颜色的行
  }, [activeDiff, highlighterReady]);
  const diffMaxCols = useMemo(
    () => (activeDiff && activeDiff.diff.trim() !== BINARY_PLACEHOLDER ? estimateMaxColumns(activeDiff.diff.split("\n")) : 0),
    [activeDiff],
  );
  const anchors = useMemo(() => (activeDiff ? diffLineAnchors(activeDiff.diff) : []), [activeDiff]);
  // 旧/新两列行号（照 Kimi Code 的改动视图），槽宽按最大行号的位数。
  const lineNos = useMemo(() => (activeDiff ? diffLineNumbers(activeDiff.diff) : []), [activeDiff]);
  const lnDigits = useMemo(() => {
    let max = 0;
    for (const no of lineNos) if (no) max = Math.max(max, no.old ?? 0, no.new ?? 0);
    return String(max).length;
  }, [lineNos]);
  const [wrap, toggleWrap] = useWrapPref();
  const staleDiff = useStaleWatch(
    selectedPath,
    activeDiff,
    () => getGitFileDiff(cwd, selectedPath ?? "", !diffBase && fileByPath.get(selectedPath ?? "")?.status === "U", diffBase, sessionCwd),
    (a, b) => a.diff === b.diff && a.truncated === b.truncated,
    live,
    summary,
  );
  // 有攒着的新 diff 就直接换上；没有（手动点的）就丢掉缓存重拉，图片一并重读。
  const refreshSelected = () => {
    if (!selectedPath) return;
    const path = selectedPath;
    setEntries((prev) => {
      const next = new Map(prev);
      if (staleDiff) next.set(path, { loading: false, error: null, data: staleDiff });
      else next.delete(path);
      return next;
    });
    setImages((prev) => {
      if (!prev.has(path)) return prev;
      const next = new Map(prev);
      next.delete(path);
      return next;
    });
  };
  const commentCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const comment of comments) counts.set(comment.path, (counts.get(comment.path) ?? 0) + 1);
    return counts;
  }, [comments]);

  // 正在写评论的那一行（同一时间只开一个框）；换文件就关掉。
  const [editing, setEditing] = useState<DiffLineAnchor | null>(null);
  const [commentDraft, setCommentDraft] = useState("");
  useEffect(() => {
    setEditing(null);
    setCommentDraft("");
  }, [selectedPath]);
  useEffect(() => {
    if (!editing) return;
    escOverrideRef.current = () => setEditing(null);
    return () => { escOverrideRef.current = null; };
  }, [editing, escOverrideRef]);
  const saveComment = () => {
    const text = commentDraft.trim();
    if (!editing || !selectedPath || !text) return;
    onAddComment({ path: selectedPath, side: editing.side, line: editing.line, code: editing.code, text });
    setEditing(null);
    setCommentDraft("");
  };
  const sameAnchor = (a: DiffLineAnchor, b: { side: string; line: number }) => a.side === b.side && a.line === b.line;

  const renderTree = (nodes: FileTreeNode[], depth: number): ReactNode =>
    nodes.map((node) =>
      node.children ? (
        <div key={node.path}>
          <div className="chat-diff-tree-row">
            <button
              type="button"
              className="chat-diff-tree-dir"
              style={{ paddingLeft: `calc(var(--sp-8) + ${depth * 14}px)` }}
              onClick={() => toggleDir(node.path)}
              onContextMenu={(event) => openCtxMenu(event, node.path, true)}
            >
              <svg
                className={collapsed.has(node.path) ? "chat-diff-chev is-collapsed" : "chat-diff-chev"}
                width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"
              >
                <path d="M6 9l6 6 6-6" />
              </svg>
              <FolderIcon />
              <span className="chat-diff-path">{node.name}</span>
            </button>
            {rowMore(node.path, true)}
          </div>
          {!collapsed.has(node.path) && renderTree(node.children, depth + 1)}
        </div>
      ) : (
        <div key={node.path} className="chat-diff-tree-row">
          <button
            type="button"
            className="chat-diff-tree-file"
            style={{ paddingLeft: `calc(var(--sp-8) + ${depth * 14}px)` }}
            onClick={() => node.file && setSelectedPath(node.file.path)}
            onContextMenu={node.file && node.file.status !== "D" ? (event) => openCtxMenu(event, node.file!.path, false) : undefined}
          >
            {node.file && <StatusBadge status={node.file.status} />}
            <span className="chat-diff-path">{node.name}</span>
            {node.file && <LineCounts file={fileByPath.get(node.file.path)} />}
            {node.file && commentCounts.get(node.file.path) && (
              <span className="chat-diff-cmt-count" aria-label={t.chat.reviewComments(commentCounts.get(node.file.path)!)}>{commentCounts.get(node.file.path)}</span>
            )}
          </button>
          {node.file && node.file.status !== "D" && rowMore(node.file.path, false)}
        </div>
      ),
    );

  const fileView = (() => {
    if (!selectedEntry || selectedEntry.loading) return <div className="chat-diff-empty">{t.chat.diffLoading}</div>;
    if (selectedEntry.error) return <div className="chat-diff-empty is-error">{selectedEntry.error}</div>;
    const fileDiff = selectedEntry.data;
    if (!fileDiff || fileDiff.diff.trim() === BINARY_PLACEHOLDER) {
      // 图片不甩「二进制」冷话:渲染工作区当前版本(新增/修改后的样子)。旧版对比
      // 做不了(git 不给二进制 diff),看得到现状已经回答了「这张图是什么」。
      // 已删除的文件工作区没有内容,才落回二进制占位文案。
      const status = summary.files.find((file) => file.path === selectedPath)?.status;
      if (selectedPath && previewKindOf(selectedPath) === "image" && status !== "D") {
        const entry = images.get(selectedPath);
        if (!entry || entry.loading) return <div className="chat-diff-empty">{t.chat.diffLoading}</div>;
        if (entry.error) return <div className="chat-diff-empty is-error">{entry.error}</div>;
        if (!entry.data?.dataUrl) return <div className="chat-diff-empty">{t.chat.diffImageTooLarge}</div>;
        return (
          <div className="chat-diff-scroll chat-diff-imgwrap">
            <img className="chat-diff-img" src={entry.data.dataUrl} alt={selectedPath} />
          </div>
        );
      }
      return <div className="chat-diff-empty">{t.chat.diffBinary}</div>;
    }
    // 空 diff ≠ 出错:清单快照落后于 agent 的提交/撤销时就会出现——给明确解释,
    // 不再渲染成一片看似坏掉的空白(实拍反馈)。
    if (fileDiff.diff.trim() === "") {
      return <div className="chat-diff-empty">{t.chat.diffFileUpToDate}</div>;
    }
    return (
      <>
        <div className="chat-diff-scroll">
          {/* min-width 钉住横向宽度（符号列、两列行号和内边距另加），理由见 estimateMaxColumns。
              换行时不钉，宽度跟着面板走。 */}
          <pre
            className={"chat-diff-text is-diff" + (wrap ? " is-wrap" : "")}
            style={{ "--ln-w": `${lnDigits}ch`, minWidth: wrap ? undefined : `max(100%, ${diffMaxCols + 12 + 2 * lnDigits}ch)` } as CSSProperties}
          >
            {fileDiff.diff.split("\n").map((line, index) => {
              const cls = diffLineClass(line);
              const html = highlightedDiff?.[index];
              // meta/hunk/空行：保持原有整行配色（meta 暗、@@ 主题色），不进符号列结构。
              if (html == null) return <div key={index} className={cls || undefined}>{line || " "}</div>;
              // add/del/上下文行：符号列（+/−/空，不可选中）+ 逐 token 着色的正文。
              const sign = cls === "is-add" ? "+" : cls === "is-del" ? "−" : "";
              const anchor = anchors[index];
              const lineComments = anchor ? comments.filter((comment) => comment.path === selectedPath && sameAnchor(anchor, comment)) : [];
              const isEditing = !!anchor && !!editing && sameAnchor(anchor, editing);
              return (
                <Fragment key={index}>
                  <div className={(cls ? cls + " " : "") + "chat-diff-row"}>
                    {/* 悬停行时左缘出「+」：写一句意见，攒几条再一起插进输入框发给 agent
                        （对照 Codex/Claude 桌面版的 diff 行评论；纯文本，五家 CLI 通吃）。 */}
                    {anchor && (
                      <button
                        type="button"
                        className="chat-diff-cmt-add"
                        aria-label={t.chat.diffCommentAdd}
                        data-tip={t.chat.diffCommentAdd}
                        onClick={() => { setEditing(anchor); setCommentDraft(""); }}
                      >+</button>
                    )}
                    <span className="chat-diff-ln is-old">{lineNos[index]?.old ?? ""}</span>
                    <span className="chat-diff-ln is-new">{lineNos[index]?.new ?? ""}</span>
                    <span className="chat-diff-sign">{sign || " "}</span>
                    <span className="chat-diff-code" dangerouslySetInnerHTML={{ __html: html || " " }} />
                  </div>
                  {lineComments.map((comment) => (
                    <div key={comment.id} className="chat-diff-cmt">
                      <span className="chat-diff-cmt-text">{comment.text}</span>
                      <button type="button" className="chat-diff-cmt-remove" aria-label={t.chat.diffCommentRemove} data-tip={t.chat.diffCommentRemove} onClick={() => onRemoveComment(comment.id)}>×</button>
                    </div>
                  ))}
                  {isEditing && (
                    <div className="chat-diff-cmt is-form">
                      <textarea
                        autoFocus
                        rows={2}
                        value={commentDraft}
                        placeholder={t.chat.diffCommentPlaceholder}
                        onChange={(event) => setCommentDraft(event.target.value)}
                        onKeyDown={(event) => {
                          // IME 合成中的 Enter 是选词，不是提交。
                          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                            event.preventDefault();
                            saveComment();
                          }
                        }}
                      />
                      <div className="chat-diff-cmt-actions">
                        <button type="button" onClick={() => setEditing(null)}>{t.chat.diffCommentCancel}</button>
                        <button type="button" className="is-primary" disabled={!commentDraft.trim()} onClick={saveComment}>{t.chat.diffCommentSave}</button>
                      </div>
                    </div>
                  )}
                </Fragment>
              );
            })}
          </pre>
        </div>
        {fileDiff.truncated && <div className="chat-diff-truncated">{t.chat.diffTruncated}</div>}
      </>
    );
  })();

  if (!summary.gitAvailable) return <div className="chat-diff-empty">{t.chat.diffNoGit}</div>;
  if (!summary.isRepo) return <div className="chat-diff-empty">{t.chat.diffNoRepo}</div>;
  if (summary.files.length === 0) {
    return (
      <div className="chat-diff-empty">
        <div className="chat-diff-clean">
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="9" /><path d="M8 12.5l2.8 2.8L16 10" />
          </svg>
          {t.chat.diffNoChanges}
        </div>
      </div>
    );
  }
  if (selectedPath) {
    return (
      <>
        <div className="chat-diff-filehead">
          <BackButton label={t.chat.diffBack} onClick={() => (onBack ? onBack() : setSelectedPath(null))} />
          {selected && <StatusBadge status={selected.status} />}
          <span className="chat-diff-path" data-tip={selectedPath}>{selectedPath}</span>
          <span className="chat-diff-tools">
            {activeDiff && activeDiff.diff.trim() !== BINARY_PLACEHOLDER && <WrapButton wrap={wrap} onToggle={toggleWrap} />}
            <RefreshButton stale={!!staleDiff} onClick={refreshSelected} />
          </span>
          {selected && selected.status !== "D" && !remoteUi() && <OpenSplitButton cwd={cwd} rel={selectedPath} />}
        </div>
        {fileView}
      </>
    );
  }
  return (
    <>
      {/* 分支名放页签内部而非面板头行：这里整行宽度可用，长分支名截断不挤布局。
          行尾照 Kimi 放文件数与「列表 / 树形」切换。 */}
      <div className="chat-diff-branchline" data-tip={summary.branch ?? undefined}>
          {summary.branch && <>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="9" r="2.5" />
              <path d="M6 8.5v7" /><path d="M18 11.5c0 4-4 4.5-9.2 4.6" />
            </svg>
            <span>{summary.branch}</span>
            {/* 旧后端不下发 ahead/behind（手机页可能先于安装版更新），按缺省处理。 */}
            {!!summary.ahead && <em className="chat-diff-divergence" data-tip={t.chat.diffAheadTip(summary.ahead)}>↑{summary.ahead}</em>}
            {!!summary.behind && <em className="chat-diff-divergence" data-tip={t.chat.diffBehindTip(summary.behind)}>↓{summary.behind}</em>}
          </>}
          <LineCounts file={totals ?? undefined} />
          <span className="chat-diff-filecount">{t.chat.diffFileCount(listedFiles.length)}</span>
          <div className="chat-diff-layout" role="group" aria-label={t.chat.diffLayoutLabel}>
            <button type="button" aria-pressed={layout === "list"} onClick={() => setLayout("list")}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M5 7h14M5 12h14M5 17h14" /></svg>
              {t.chat.diffLayoutList}
            </button>
            <button type="button" aria-pressed={layout === "tree"} onClick={() => setLayout("tree")}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="3" y="3" width="7" height="6" rx="1.5" /><rect x="14" y="10" width="7" height="5" rx="1.5" /><rect x="14" y="17" width="7" height="4" rx="1.5" /><path d="M6.5 9v9.5H14M6.5 12.5H14" /></svg>
              {t.chat.diffLayoutTree}
            </button>
          </div>
      </div>
      <input
        type="text"
        className="chat-diff-search"
        placeholder={t.chat.diffSearchPlaceholder}
        value={search}
        // IME 合成守卫：与侧栏搜索框同款，拼音中间态不下发。
        onChange={(event) => {
          if ((event.nativeEvent as InputEvent).isComposing) return;
          setSearch(event.target.value);
        }}
        onCompositionEnd={(event) => setSearch(event.currentTarget.value)}
      />
      {sessionFiles && (
        <div className="chat-diff-scope" role="group" aria-label={t.chat.diffScopeLabel}>
          <button type="button" aria-pressed={scope === "all"} onClick={() => setScope("all")}>{t.chat.diffScopeAll}</button>
          <button type="button" aria-pressed={scope === "session"} data-tip={t.chat.diffOnlySessionTip} onClick={() => setScope("session")}>
            {t.chat.diffScopeSession(sessionFiles.size)}
          </button>
          <button type="button" aria-pressed={scope === "turn"} data-tip={t.chat.diffOnlyTurnTip} onClick={() => setScope("turn")}>
            {t.chat.diffScopeTurn(turnFiles.size)}
          </button>
        </div>
      )}
      {filterSession && visibleFiles.length === 0
        ? <div className="chat-diff-empty">{scope === "turn" ? t.chat.diffNoTurnChanges : t.chat.diffNoSessionChanges}</div>
        : <div className="chat-diff-tree">{layout === "tree" ? renderTree(tree, 0) : listedFiles.map((file) => (
          <div key={file.path} className="chat-diff-tree-row">
            <button
              type="button"
              className="chat-diff-tree-file is-flat"
              data-tip={file.path}
              onClick={() => setSelectedPath(file.path)}
              onContextMenu={file.status !== "D" ? (event) => openCtxMenu(event, file.path, false) : undefined}
            >
              <StatusBadge status={file.status} />
              <span className="chat-diff-path">{file.path}</span>
              <LineCounts file={file} />
              {commentCounts.get(file.path) && (
                <span className="chat-diff-cmt-count" aria-label={t.chat.reviewComments(commentCounts.get(file.path)!)}>{commentCounts.get(file.path)}</span>
              )}
            </button>
            {file.status !== "D" && rowMore(file.path, false)}
          </div>
        ))}</div>}
      {ctxMenu && (
        <PathContextMenu
          cwd={cwd}
          rel={ctxMenu.rel}
          isDir={ctxMenu.isDir}
          x={ctxMenu.x}
          y={ctxMenu.y}
          onClose={() => setCtxMenu(null)}
        />
      )}
    </>
  );
}

/// 文件/图片缓存的上限。面板折叠时**不卸载**（打开的文件、树展开、diff 缓存全部保留，
/// 见 ChatWindow 的注释），而 ChangesView 有 `[summary]` 重置、FilesView 一个都没有——
/// 一次会话里翻几十个文件加十几张截图，就是几十 MB 只增不减地挂到换会话为止（图片还是
/// 1.33 倍体积的 base64 data URL）。20 条足够覆盖「来回对照几个文件」的实际用法。
const FILE_CACHE_MAX = 20;

/// 写入并按 LRU 裁剪：命中/重写都挪到队尾，超限从队首淘汰。Map 保持插入序，
/// 先 delete 再 set 即可把它变成访问序。
function cappedCache<T>(prev: Map<string, T>, key: string, value: T): Map<string, T> {
  const next = new Map(prev);
  next.delete(key);
  next.set(key, value);
  while (next.size > FILE_CACHE_MAX) {
    const oldest = next.keys().next().value;
    if (oldest === undefined) break;
    next.delete(oldest);
  }
  return next;
}

/** 「文件」页签：可搜索（文件名 + 内容）的懒加载目录树 + 单文件文本查看（返回行 + pre 文本）。 */
export function FilesView({ cwd, live = false }: { cwd: string; /** 面板正显示着：这时才在后台查打开的文件过期没有。 */ live?: boolean }) {
  const t = useT();
  // 文件树当侧栏（照 Kimi Code 的文件页）：宽面板时可收起、放左边或右边、拖动调宽，三样都记住。
  // 窄面板照旧逐级进出，这几样不起作用（CSS 只在 ≥560px 的容器查询里认它们）。
  const [treeOpen, setTreeOpen] = useState(() => readPref(TREE_OPEN_KEY) !== "0");
  const [treeRight, setTreeRight] = useState(() => readPref(TREE_POS_KEY) === "right");
  const [treeWidth, setTreeWidth] = useState(() => clampTreeWidth(Number(readPref(TREE_WIDTH_KEY)) || TREE_WIDTH_DEFAULT));
  const toggleTree = () => setTreeOpen((prev) => {
    savePref(TREE_OPEN_KEY, prev ? "0" : "1");
    return !prev;
  });
  const setTreeSide = (right: boolean) => {
    setTreeRight(right);
    savePref(TREE_POS_KEY, right ? "right" : "left");
  };
  const [posMenu, setPosMenu] = useState<{ x: number; y: number } | null>(null);
  // 拖分隔条调宽：树在右边时往左拖是变宽。
  const resizeDrag = useRef<{ x: number; width: number } | null>(null);
  const onResizeDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    resizeDrag.current = { x: event.clientX, width: treeWidth };
  };
  const onResizeMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = resizeDrag.current;
    if (!drag) return;
    const dx = event.clientX - drag.x;
    setTreeWidth(clampTreeWidth(drag.width + (treeRight ? -dx : dx)));
  };
  const onResizeUp = () => {
    if (!resizeDrag.current) return;
    resizeDrag.current = null;
    savePref(TREE_WIDTH_KEY, String(treeWidth));
  };
  // 键盘调宽同 Kimi：←/→ 每次 16px，按住 Shift 48px。
  const onResizeKey = (event: ReactKeyboardEvent) => {
    const step = event.shiftKey ? 48 : 16;
    const delta = event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
    if (!delta) return;
    event.preventDefault();
    const next = clampTreeWidth(treeWidth + (treeRight ? -delta : delta));
    setTreeWidth(next);
    savePref(TREE_WIDTH_KEY, String(next));
  };
  const [wrap, toggleWrap] = useWrapPref();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);
  // 目录清单与文件文本都按 rel 路径缓存：折叠/回退再展开不重复拉取。
  const [dirs, setDirs] = useState<Map<string, FetchEntry<DirEntryDto[]>>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [files, setFiles] = useState<Map<string, FetchEntry<FileTextDto>>>(new Map());
  // 图片预览按路径缓存（data URL）；与文本缓存分开——同一 svg 两种形态都可能要。
  const [images, setImages] = useState<Map<string, FetchEntry<ImagePreviewDto>>>(new Map());
  // 预览形态文件（md/svg）的「源码」切换；打开新文件回到预览态。
  const [sourceMode, setSourceMode] = useState(false);
  // 名称+内容搜索：防抖后发后端，seq 挡乱序回包；查询清空即回树形态（结果状态一并清）。
  const [search, setSearch] = useState("");
  const [searchState, setSearchState] = useState<FetchEntry<SearchResultDto>>({ loading: false, error: null, data: null });
  const searchSeq = useRef(0);
  // markdown 预览里相对路径图片 → data URL（logo/截图等仓库内引用；http(s) 由
  // FileMarkdown 直用）。按打开的 md 文件所在目录做基准。
  const resolveMdImage = useCallback(
    (src: string) => {
      const baseDir = selectedPath ? selectedPath.split("/").slice(0, -1).join("/") : "";
      const rel = resolveMdRel(baseDir, src);
      if (!rel) return Promise.resolve(null);
      return readImagePreview(cwd, rel)
        .then((preview) => preview.dataUrl ?? null)
        .catch(() => null);
    },
    [cwd, selectedPath],
  );
  // 内容命中组的折叠状态（按 rel 路径记被折叠的组；换关键词后旧路径残留无害）。
  const [collapsedHits, setCollapsedHits] = useState<Set<string>>(new Set());
  const toggleHitGroup = (rel: string) => {
    setCollapsedHits((prev) => {
      const next = new Set(prev);
      if (next.has(rel)) next.delete(rel);
      else next.add(rel);
      return next;
    });
  };
  // 内容命中跳行：打开文件后滚到目标行并常亮标出（再从树里正常打开文件时清除）。
  const [target, setTarget] = useState<{ rel: string; line: number } | null>(null);
  const preRef = useRef<HTMLPreElement>(null);
  // 树行/结果行右键的路径菜单（复制/定位/打开方式）。
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; rel: string; isDir: boolean } | null>(null);
  const openCtxMenu = (event: ReactMouseEvent, rel: string, isDir: boolean) => {
    event.preventDefault();
    setCtxMenu({ x: event.clientX, y: event.clientY, rel, isDir });
  };

  const query = search.trim();
  // 单个 ASCII 字符不触发（后端同样拒：内容命中噪声太大）；CJK 单字放行。
  const searchActive = query.length >= 2 || /[^\x00-\x7f]/.test(query);

  useEffect(() => {
    if (!searchActive) {
      searchSeq.current += 1;
      setSearchState({ loading: false, error: null, data: null });
      return;
    }
    const seq = ++searchSeq.current;
    setSearchState((prev) => ({ ...prev, loading: true }));
    const timer = window.setTimeout(() => {
      searchProjectFiles(cwd, query)
        .then((data) => {
          if (searchSeq.current === seq) setSearchState({ loading: false, error: null, data });
        })
        .catch((error) => {
          if (searchSeq.current === seq) setSearchState({ loading: false, error: formatBackendError(error, t.locale), data: null });
        });
    }, 250);
    return () => window.clearTimeout(timer);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- t.locale 只用来格式化报错文案，切语言不该重跑
  }, [cwd, query, searchActive]);

  // 打开文件的逐行高亮 HTML（useMemo 按文件对象缓存；二进制/未打开为 null）。
  const selectedFile = selectedPath ? files.get(selectedPath)?.data ?? null : null;
  const displayText = useMemo(
    () => (selectedFile && !selectedFile.binary ? prettyJson(selectedFile.relPath, selectedFile.text, selectedFile.truncated) : null),
    [selectedFile],
  );
  // hljs 懒加载（见 highlight.ts）：就绪与否进依赖，引擎到了重算出带颜色的行。
  const highlighterReady = useHighlighter(displayText != null);
  const highlightedFile = useMemo(() => {
    if (!selectedFile || displayText == null) return null;
    return highlightLines(displayText.split("\n"), languageForPath(selectedFile.relPath));
  // eslint-disable-next-line react-hooks/exhaustive-deps -- highlighterReady 是 hljs 懒加载就绪信号，引擎到了要重算出带颜色的行
  }, [selectedFile, displayText, highlighterReady]);
  const fileMaxCols = useMemo(() => (displayText != null ? estimateMaxColumns(displayText.split("\n")) : 0), [displayText]);
  const staleFile = useStaleWatch(
    selectedPath,
    selectedFile,
    () => readFileText(cwd, selectedPath ?? ""),
    (a, b) => a.text === b.text && a.binary === b.binary && a.truncated === b.truncated,
    live,
    null,
  );

  // 跳行滚动：等文件加载 + 高亮渲染完成后再滚（children 与行号一一对应）。
  useEffect(() => {
    if (!target || selectedPath !== target.rel || !highlightedFile) return;
    preRef.current?.children[target.line - 1]?.scrollIntoView({ block: "center" });
  }, [target, selectedPath, highlightedFile]);

  // quiet：刷新已经显示着的目录时不先切回「加载中」，免得整棵树闪一下。
  const fetchDir = (rel: string, quiet = false) => {
    if (!quiet) setDirs((prev) => new Map(prev).set(rel, { loading: true, error: null, data: null }));
    listDirEntries(cwd, rel)
      .then((data) => setDirs((prev) => new Map(prev).set(rel, { loading: false, error: null, data })))
      .catch((error) => setDirs((prev) => new Map(prev).set(rel, { loading: false, error: formatBackendError(error, t.locale), data: null })));
  };

  // 根目录进面板即拉。
  useEffect(() => {
    if (!dirs.has("")) fetchDir("");
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fetchDir 每次渲染新建，按 cwd 与目录缓存判断即可
  }, [cwd, dirs]);

  // 展开时按需拉子目录；上次失败的条目借「重新展开」重试。
  const toggleDir = (rel: string) => {
    const cached = dirs.get(rel);
    if (!expanded.has(rel) && (!cached || cached.error)) fetchDir(rel);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(rel)) next.delete(rel);
      else next.add(rel);
      return next;
    });
  };

  // 根目录和展开着的目录都重拉一遍（agent 新建/删掉的文件）。
  const refreshTree = () => {
    for (const rel of ["", ...expanded]) fetchDir(rel, !!dirs.get(rel)?.data);
  };

  // force：手动刷新。已经有内容的不先切回「加载中」，拉回来直接换。
  const fetchText = (rel: string, force = false) => {
    if (files.has(rel) && !force) return;
    if (!force || !files.get(rel)?.data) setFiles((prev) => cappedCache(prev, rel, { loading: true, error: null, data: null }));
    readFileText(cwd, rel)
      .then((data) => setFiles((prev) => cappedCache(prev, rel, { loading: false, error: null, data })))
      .catch((error) => setFiles((prev) => cappedCache(prev, rel, { loading: false, error: formatBackendError(error, t.locale), data: null })));
  };

  const fetchImage = (rel: string, force = false) => {
    if (images.has(rel) && !force) return;
    if (!force || !images.get(rel)?.data) setImages((prev) => cappedCache(prev, rel, { loading: true, error: null, data: null }));
    readImagePreview(cwd, rel)
      .then((data) => setImages((prev) => cappedCache(prev, rel, { loading: false, error: null, data })))
      .catch((error) => setImages((prev) => cappedCache(prev, rel, { loading: false, error: formatBackendError(error, t.locale), data: null })));
  };

  const openFile = (rel: string, line: number | null = null) => {
    setTarget(line ? { rel, line } : null);
    setSourceMode(false);
    setSelectedPath(rel);
    // 图片走预览通道（读盘转 data URL），其余（含 markdown）走文本通道。
    if (previewKindOf(rel) === "image") fetchImage(rel);
    else fetchText(rel);
  };

  // 点中目录命中：回树形态并逐级展开到该目录（沿途未拉取/拉取失败的目录顺手补拉）。
  const revealDirFromSearch = (rel: string) => {
    setSearch("");
    const ancestors: string[] = [];
    let acc = "";
    for (const part of rel.split("/")) {
      acc = acc ? `${acc}/${part}` : part;
      ancestors.push(acc);
    }
    setExpanded((prev) => {
      const next = new Set(prev);
      for (const path of ancestors) next.add(path);
      return next;
    });
    for (const path of ancestors) {
      const cached = dirs.get(path);
      if (!cached || cached.error) fetchDir(path);
    }
  };

  const indent = (depth: number) => ({ paddingLeft: `calc(var(--sp-8) + ${depth * 14}px)` });
  // 手机页没有路径菜单（全是在桌面上开程序/定位文件的动作），也就不出「…」。
  const rowMore = (rel: string, isDir: boolean) =>
    remoteUi() ? null : (
      <RowMore
        label={t.chat.filesMore}
        open={ctxMenu?.rel === rel}
        onOpen={(x, y) => setCtxMenu({ x, y, rel, isDir })}
      />
    );

  const renderDir = (rel: string, depth: number): ReactNode => {
    const dir = dirs.get(rel);
    if (!dir || dir.loading) {
      return <div className="chat-diff-tree-note" style={indent(depth)}>{t.chat.diffLoading}</div>;
    }
    if (dir.error) {
      return <div className="chat-diff-tree-note is-error" style={indent(depth)}>{dir.error}</div>;
    }
    return (dir.data ?? []).map((entry) =>
      entry.isDir ? (
        <div key={entry.relPath}>
          <div className="chat-diff-tree-row">
            <button
              type="button"
              className="chat-diff-tree-dir"
              style={indent(depth)}
              onClick={() => toggleDir(entry.relPath)}
              onContextMenu={(event) => openCtxMenu(event, entry.relPath, true)}
            >
              <svg
                className={expanded.has(entry.relPath) ? "chat-diff-chev" : "chat-diff-chev is-collapsed"}
                width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"
              >
                <path d="M6 9l6 6 6-6" />
              </svg>
              <FolderIcon />
              <span className="chat-diff-path">{entry.name}</span>
            </button>
            {rowMore(entry.relPath, true)}
          </div>
          {expanded.has(entry.relPath) && renderDir(entry.relPath, depth + 1)}
        </div>
      ) : (
        <div key={entry.relPath} className="chat-diff-tree-row">
          <button
            type="button"
            // 并排时树与预览同屏，正在看的那个文件要在树上标出来。
            className={"chat-diff-tree-file" + (entry.relPath === selectedPath ? " is-selected" : "")}
            aria-current={entry.relPath === selectedPath ? "true" : undefined}
            style={indent(depth)}
            onClick={() => openFile(entry.relPath)}
            onContextMenu={(event) => openCtxMenu(event, entry.relPath, false)}
          >
            <FileIcon />
            <span className="chat-diff-path">{entry.name}</span>
          </button>
          {rowMore(entry.relPath, false)}
        </div>
      ),
    );
  };

  // 搜索结果（VS Code 搜索面板式按文件分组）：
  //  - 名称命中的目录/文件在前：目录点开即展开树、无内容命中的文件点开即查看；
  //  - 带内容命中的组：头行点击折叠/展开（角标 = 该文件命中总行数，可大于展示的行数），
  //    行命中点开后跳到该行并高亮命中词。
  const searchView = (() => {
    if (searchState.error) return <div className="chat-diff-empty is-error">{searchState.error}</div>;
    const groups = searchState.data?.files ?? [];
    if (groups.length === 0) {
      return (
        <div className="chat-diff-empty">
          {searchState.loading ? t.chat.diffLoading : t.chat.diffSearchNoResults}
        </div>
      );
    }
    return (
      <>
        <div className="chat-diff-tree">
          {groups.map((group) => {
            const hasLines = group.lines.length > 0;
            const collapsed = collapsedHits.has(group.relPath);
            return (
              <div key={group.relPath}>
                <div className="chat-diff-tree-row">
                <button
                  type="button"
                  className="chat-diff-tree-file"
                  onClick={() =>
                    group.isDir
                      ? revealDirFromSearch(group.relPath)
                      : hasLines
                        ? toggleHitGroup(group.relPath)
                        : openFile(group.relPath)
                  }
                  onContextMenu={(event) => openCtxMenu(event, group.relPath, group.isDir)}
                >
                  {hasLines && (
                    <svg
                      className={collapsed ? "chat-diff-chev is-collapsed" : "chat-diff-chev"}
                      width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"
                    >
                      <path d="M6 9l6 6 6-6" />
                    </svg>
                  )}
                  {group.isDir ? <FolderIcon /> : <FileIcon />}
                  <span className="chat-diff-path" data-tip={group.relPath}>{group.relPath}</span>
                  {group.totalLineMatches > 0 && (
                    <span className="chat-diff-hit-count">{group.totalLineMatches}</span>
                  )}
                </button>
                {rowMore(group.relPath, group.isDir)}
                </div>
                {hasLines && !collapsed &&
                  group.lines.map((lineHit) => (
                    <button
                      key={lineHit.line}
                      type="button"
                      className="chat-diff-hit-row"
                      onClick={() => openFile(group.relPath, lineHit.line)}
                      onContextMenu={(event) => openCtxMenu(event, group.relPath, false)}
                    >
                      <span className="chat-diff-hit-ln">{lineHit.line}</span>
                      <span className="chat-diff-hit-preview">{markMatch(lineHit.preview, query)}</span>
                    </button>
                  ))}
              </div>
            );
          })}
        </div>
        {searchState.data?.truncated && <div className="chat-diff-truncated">{t.chat.diffSearchTruncated}</div>}
      </>
    );
  })();

  // 文件树与预览两块都渲染，由 CSS 按面板宽度排布（对照 Kimi 的文件页：树在左、预览在右）：
  // 面板窄（默认 400px）时照旧逐级进出，宽到能并排时两块同屏，点一个看一个不用来回返回。
  let preview: ReactNode = <div className="chat-diff-empty">{t.chat.filesPickToPreview}</div>;
  if (selectedPath) {
    const kind = previewKindOf(selectedPath);
    // md 与 svg 提供「预览/源码」切换（svg 本质是文本）；位图没有可读源码，不出切换。
    const showModeToggle = kind === "markdown" || extKeyOf(selectedPath) === "svg";
    const setMode = (source: boolean) => {
      setSourceMode(source);
      if (source) fetchText(selectedPath);
      else if (kind === "image") fetchImage(selectedPath);
    };
    // 源码视图（高亮 + 行号）才有换行和复制全文；图片、md 渲染没有。
    const codeView = !!selectedFile && !selectedFile.binary && (sourceMode || kind === null);
    const refreshFile = () => {
      if (staleFile) {
        setFiles((prev) => cappedCache(prev, selectedPath, { loading: false, error: null, data: staleFile }));
        return;
      }
      if (kind === "image" && !sourceMode) fetchImage(selectedPath, true);
      else fetchText(selectedPath, true);
    };
    const content = (() => {
      // 图片预览通道：独立缓存与占位（过大/失败给文字，不落到「二进制」歧义提示）。
      if (kind === "image" && !sourceMode) {
        const entry = images.get(selectedPath);
        if (!entry || entry.loading) return <div className="chat-diff-empty">{t.chat.diffLoading}</div>;
        if (entry.error) return <div className="chat-diff-empty is-error">{entry.error}</div>;
        const image = entry.data;
        if (!image?.dataUrl) return <div className="chat-diff-empty">{t.chat.diffImageTooLarge}</div>;
        return (
          <div className="chat-diff-scroll chat-diff-imgwrap">
            <img className="chat-diff-img" src={image.dataUrl} alt={selectedPath} />
          </div>
        );
      }
      const entry = files.get(selectedPath);
      if (!entry || entry.loading) return <div className="chat-diff-empty">{t.chat.diffLoading}</div>;
      if (entry.error) return <div className="chat-diff-empty is-error">{entry.error}</div>;
      const file = entry.data;
      if (!file || file.binary) return <div className="chat-diff-empty">{t.chat.diffBinary}</div>;
      // markdown 预览：FileMarkdown（渲染内嵌 HTML + sanitize 白名单 + 仓库内图片
      // 转 data URL），排版样式复用对话区 .chat-md 全套。
      if (kind === "markdown" && !sourceMode) {
        return (
          <>
            <div className="chat-diff-scroll">
              <div className="chat-md chat-diff-mdview">
                <FileMarkdown text={file.text} resolveImage={resolveMdImage} />
              </div>
            </div>
            {file.truncated && <div className="chat-diff-truncated">{t.chat.diffTruncated}</div>}
          </>
        );
      }
      return (
        <>
          <div className="chat-diff-scroll">
            {/* 行号槽宽按最大行号位数走（--ln-w），右对齐 + 右描边，不可选中。
                min-width 钉住横向宽度（+8ch 给行号槽/间距），理由见 estimateMaxColumns。 */}
            <pre
              ref={preRef}
              className={"chat-diff-text is-code" + (wrap ? " is-wrap" : "")}
              style={{ "--ln-w": `${String((highlightedFile ?? []).length).length}ch`, minWidth: wrap ? undefined : `max(100%, ${fileMaxCols + 8}ch)` } as CSSProperties}
            >
              {(highlightedFile ?? []).map((html, index) => (
                <div key={index} className={target && selectedPath === target.rel && index + 1 === target.line ? "is-target" : undefined}>
                  <span className="chat-diff-ln">{index + 1}</span>
                  <span className="chat-diff-code" dangerouslySetInnerHTML={{ __html: html || " " }} />
                </div>
              ))}
            </pre>
          </div>
          {file.truncated && <div className="chat-diff-truncated">{t.chat.diffTruncated}</div>}
        </>
      );
    })();
    preview = (
      <>
        <div className="chat-diff-filehead">
          <BackButton label={t.chat.diffBack} onClick={() => setSelectedPath(null)} />
          <button
            type="button"
            className="chat-diff-iconbtn chat-files-treetoggle"
            aria-pressed={treeOpen}
            aria-label={treeOpen ? t.chat.filesTreeHide : t.chat.filesTreeShow}
            data-tip={treeOpen ? t.chat.filesTreeHide : t.chat.filesTreeShow}
            onClick={toggleTree}
          >
            <TreePaneIcon right={treeRight} />
          </button>
          <span className="chat-diff-path" data-tip={selectedPath}>{selectedPath}</span>
          {showModeToggle && (
            <div className="chat-diff-tabs">
              <button type="button" className={sourceMode ? "" : "is-active"} onClick={() => setMode(false)}>
                {t.chat.diffMdPreview}
              </button>
              <button type="button" className={sourceMode ? "is-active" : ""} onClick={() => setMode(true)}>
                {t.chat.diffMdSource}
              </button>
            </div>
          )}
          <span className="chat-diff-tools">
            {codeView && <WrapButton wrap={wrap} onToggle={toggleWrap} />}
            {codeView && (
              <button
                type="button"
                className="chat-diff-iconbtn"
                aria-label={copied ? t.chat.codeCopied : t.chat.filesCopyAll}
                data-tip={copied ? t.chat.codeCopied : t.chat.filesCopyAll}
                onClick={() => {
                  if (!selectedFile) return;
                  navigator.clipboard?.writeText(selectedFile.text).then(() => setCopied(true), () => {});
                }}
              >
                {copied ? <CheckIcon /> : <CopyIcon />}
              </button>
            )}
            <RefreshButton stale={!!staleFile} onClick={refreshFile} />
          </span>
          <OpenSplitButton cwd={cwd} rel={selectedPath} />
        </div>
        {content}
      </>
    );
  }
  return (
    <div
      className={"chat-files" + (selectedPath ? " has-selection" : "") + (treeRight ? " is-tree-right" : "") + (selectedPath && !treeOpen ? " is-tree-hidden" : "")}
      style={{ "--tree-w": `${treeWidth}px` } as CSSProperties}
    >
      <div className="chat-files-browse">
      <div className="chat-files-head">
        <FolderIcon />
        <span className="chat-files-root" data-tip={cwd}>{folderName(cwd)}</span>
        <button type="button" className="chat-diff-iconbtn" aria-label={t.chat.filesRefreshTree} data-tip={t.chat.filesRefreshTree} onClick={refreshTree}>
          <RefreshIcon />
        </button>
        <button
          type="button"
          className="chat-diff-iconbtn chat-files-posbtn"
          aria-label={t.chat.filesTreePosition}
          data-tip={t.chat.filesTreePosition}
          aria-haspopup="menu"
          aria-expanded={!!posMenu}
          onClick={(event) => {
            if (posMenu) {
              setPosMenu(null);
              return;
            }
            const rect = event.currentTarget.getBoundingClientRect();
            setPosMenu({ x: rect.left, y: rect.bottom + 4 });
          }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
          </svg>
        </button>
      </div>
      <input
        type="text"
        className="chat-diff-search"
        placeholder={t.chat.diffSearchFilesPlaceholder}
        value={search}
        // IME 合成守卫：与侧栏搜索框同款，拼音中间态不下发。
        onChange={(event) => {
          if ((event.nativeEvent as InputEvent).isComposing) return;
          setSearch(event.target.value);
        }}
        onCompositionEnd={(event) => setSearch(event.currentTarget.value)}
      />
      {searchActive ? searchView : <div className="chat-diff-tree">{renderDir("", 0)}</div>}
      </div>
      <div
        className="chat-files-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-label={t.chat.filesTreeResize}
        aria-valuenow={treeWidth}
        aria-valuemin={TREE_WIDTH_MIN}
        aria-valuemax={TREE_WIDTH_MAX}
        tabIndex={0}
        onPointerDown={onResizeDown}
        onPointerMove={onResizeMove}
        onPointerUp={onResizeUp}
        onPointerCancel={onResizeUp}
        onKeyDown={onResizeKey}
      />
      <div className="chat-files-preview">{preview}</div>
      {posMenu && (
        <PathMenu
          x={posMenu.x}
          y={posMenu.y}
          items={[
            { key: "left", label: t.chat.filesTreeLeft, icon: treeRight ? <span className="ctx-check" /> : <CheckIcon />, onSelect: () => setTreeSide(false) },
            { key: "right", label: t.chat.filesTreeRight, icon: treeRight ? <CheckIcon /> : <span className="ctx-check" />, onSelect: () => setTreeSide(true) },
          ]}
          onClose={() => setPosMenu(null)}
        />
      )}
      {ctxMenu && (
        <PathContextMenu
          cwd={cwd}
          rel={ctxMenu.rel}
          isDir={ctxMenu.isDir}
          x={ctxMenu.x}
          y={ctxMenu.y}
          onClose={() => setCtxMenu(null)}
        />
      )}
    </div>
  );
}

/// 改动状态的小方块（对照 Kimi 改动列表的 +）：新增 / 未跟踪「+」、删除「−」，其余用字母。
/// 字母本身不进读屏名字，按状态给一句话。
function StatusBadge({ status }: { status: string }) {
  const t = useT();
  const glyph = status === "A" || status === "U" ? "+" : status === "D" ? "−" : status;
  return (
    <span className={`chat-diff-status is-${status.toLowerCase()}`} role="img" aria-label={t.chat.diffStatusName[status] ?? status} data-tip={t.chat.diffStatusName[status] ?? status}>
      {glyph}
    </span>
  );
}

function BackButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="chat-diff-back" onClick={onClick}>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M15 18l-6-6 6-6" /></svg>
      {label}
    </button>
  );
}

function RefreshIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg className="ctx-check" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </svg>
  );
}

/** 文件树显隐按钮：一个窗格、竖线画在树所在的那一侧。 */
function TreePaneIcon({ right }: { right: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="4" width="18" height="16" rx="2" /><path d={right ? "M15 4v16" : "M9 4v16"} />
    </svg>
  );
}

// 放在文件末尾：两个组件此时都已定义（见 lazyDiff.tsx）。
registerDiffModule({ FilesView, ChangesView });
