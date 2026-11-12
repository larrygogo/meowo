/// 「更改」页签：照 VS Code 源代码管理做（用户要求 2026-11-09，附 VS Code 截图）。
///
/// 此前按仓库分组、每组再分「未提交 / 已提交未合并 · 相对 main」两段，用户看不懂在分什么，
/// 也不能直接操作。现在顶部是「存储库」列表（只显示分支与 ↑↓，不同步、不切分支——推送是对外
/// 动作、切分支会换掉 agent 脚下的文件，用户定），点选一个仓库，下面是它的提交框、「暂存的更改」、
/// 「更改」，最下面是折叠的「本分支已提交（未合入 main）」（agent 多半自己提交，去掉它就看不到
/// 这一轮改了什么，用户定）。选中的仓库与「文件」页签共用（会话开在主目录、改动在 worktree 里，
/// 两个页签要一起切过去）。
///
/// 点文件进单文件 diff（ChangesView，评论照旧），返回回到列表。打开的文件记在这里而不是
/// ChangesView 里：5 秒轮询换了对象、子组件重挂也不会把正在看的文件踢回列表（做空复现过）。
///
/// 文件多的未跟踪目录后端只给一条（path 以 / 结尾，untrackedFiles 是文件数）：显示成「目录名/ · N 个文件」，
/// 点开才拉其中的文件（不进 5 秒轮询）。列表做虚拟滚动：用户实测一个没进 .gitignore 的构建目录有
/// 一万五千多个文件，整份平铺挂进 DOM 面板就卡死了。

import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent, type MutableRefObject, type ReactNode } from "react";
import { observeElementRect, useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";
import { getUntrackedDirFiles, gitCommit, gitDiscard, gitIgnoreDir, gitStage, gitUnstage, openPathWith, type RepoChangesDto } from "../../api";
import type { GitChangedFileDto } from "../../generated/contracts/GitChangedFileDto";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { remoteUi } from "../../remoteMode";
import { AppModal } from "../AppModal";
import type { ReviewComment } from "./diffComments";
import { buildFileTree, changedFileCount, normPath, type FileTreeNode } from "./gitDiff";
import { FileIcon, FolderIcon, LineCounts, readDiffLayout, saveDiffLayout, touchedInRoot } from "./diffShared";
import { OpenIcon, PathContextMenu, RowMore } from "./pathMenu";
// ChangesView 住在 GitDiffView 里，静态引会把整个改动面板并进 SidePanel / 对话窗 chunk（见 lazyDiff.tsx）。
import { LazyChangesView as ChangesView } from "./lazyDiff";
import { ForgeBar } from "./ForgeBar";
import { Icon } from "../../icons/Icon";
import { icGitBranch, icMenu, icMinus, icUndo } from "../../icons/glyphs.chat";
import { icCheck, icPlus } from "../../icons/glyphs";
import { icBan, icBook, icChevronRight, icTree } from "../../icons/glyphs.scm";

/// 旧后端（手机页可能先于安装版更新）不下发两个新字段：按「全部是未暂存」处理。
type ScmFile = GitChangedFileDto & { indexStatus?: string | null; worktreeStatus?: string | null; untrackedFiles?: number | null };
type GroupKind = "staged" | "work" | "committed";
/// 一行：letter 是这一组里该显示的状态字母（同一文件在两组里字母可能不同，MM 之类）。
type Row = { file: ScmFile; letter: string };

export function stagedRows(files: readonly ScmFile[]): Row[] {
  return files.filter((file) => !!file.indexStatus).map((file) => ({ file, letter: file.indexStatus! }));
}

export function workRows(files: readonly ScmFile[]): Row[] {
  return files.flatMap((file) => {
    if (file.indexStatus === undefined && file.worktreeStatus === undefined) return [{ file, letter: file.status }];
    return file.worktreeStatus ? [{ file, letter: file.worktreeStatus }] : [];
  });
}

const sameRoot = (a: string, b: string) => normPath(a).toLowerCase() === normPath(b).toLowerCase();

/// 折叠的未跟踪目录条目（后端 git.rs fold_untracked_dirs）。
export const isDirEntry = (file: { path: string }) => file.path.endsWith("/");
/// 点开的折叠目录里的文件（按仓库根 + 目录记）。req：只认最后一次请求的结果，写操作后重拉时旧结果晚到不覆盖。
type DirState = { req: number; loading: boolean; error: string | null; files: readonly ScmFile[] };
const dirKey = (root: string, path: string) => `${normPath(root).toLowerCase()}\u0000${path}`;

/// 一行的高度：CSS 里 .scm-row 是 28px（与文件树、会话信息栏的行同高）。虚拟列表按它估位置，再按实测修正（手机触屏样式可能更高）。
const ROW_H = 28;
const NOTE_H = 28;
type VRow = { key: string; size: number; node: ReactNode };

/// 选中的仓库：记着的那个还在就用它，否则工作目录所在仓库，再否则第一个。
export function pickRepo(repos: readonly RepoChangesDto[], selected: string | null): RepoChangesDto | null {
  return (selected ? repos.find((repo) => sameRoot(repo.root, selected)) : undefined)
    ?? repos.find((repo) => repo.isCwdRepo)
    ?? repos[0]
    ?? null;
}

export function SourceControl({ cwd, repos, selectedRoot, onSelectRoot, onChanged, comments, touchedPaths, turnTouchedPaths, onAddComment, onRemoveComment, escOverrideRef, live = false, sessionId = null, visible = true }: {
  /** 会话工作目录（手机桥按它收口）。 */
  cwd: string;
  repos: readonly RepoChangesDto[];
  /** 与「文件」页签共用的选中仓库根；null = 默认。 */
  selectedRoot: string | null;
  onSelectRoot: (root: string) => void;
  /** 写操作完成后让上层立刻重拉清单（不等 5 秒轮询）。 */
  onChanged: () => void;
  comments: readonly ReviewComment[];
  touchedPaths: readonly string[];
  turnTouchedPaths: readonly string[];
  onAddComment: (comment: Omit<ReviewComment, "id" | "dir">, dir: string) => void;
  onRemoveComment: (id: string) => void;
  escOverrideRef: MutableRefObject<(() => void) | null>;
  live?: boolean;
  /** 会话 id：PR 合并后查它的 worktree 能不能清理。 */
  sessionId?: number | null;
  /** 面板可见：收起时推送 / PR 区块不轮询 GitHub。 */
  visible?: boolean;
}) {
  const t = useT();
  const repo = pickRepo(repos, selectedRoot);
  const [open, setOpen] = useState<{ root: string; kind: GroupKind; path: string } | null>(null);
  const [search, setSearch] = useState("");
  const [scopeChoice, setScope] = useState<"all" | "session" | "turn">("all");
  const [layout, setLayoutState] = useState<"list" | "tree">(readDiffLayout);
  const setLayout = (next: "list" | "tree") => { setLayoutState(next); saveDiffLayout(next); };
  const [folded, setFolded] = useState<ReadonlySet<string>>(() => new Set(["committed"]));
  const [collapsedDirs, setCollapsedDirs] = useState<ReadonlySet<string>>(new Set());
  // 提交信息按仓库各记一份：切到别的 worktree 再回来，写了一半的不丢。
  const [messages, setMessages] = useState<ReadonlyMap<string, string>>(new Map());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // 打开确认框时钉住仓库根与文件：确认前轮询可能把选中的 worktree 挤出列表，pickRepo 退回主仓库，
  // 按渲染时的 root 放弃就会删到主仓库的同名文件（做空复现）。
  const [confirmDiscard, setConfirmDiscard] = useState<{ root: string; name: string; files: readonly ScmFile[] } | null>(null);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; rel: string } | null>(null);
  // 滚动容器放 state 不放 ref：虚拟列表在子组件里，子组件的 layout effect 先于父元素的 ref 挂上，
  // 用 ref 时首次挂载拿到 null、滚动监听要等下一次重渲染才接上（jsdom 里实测滚动无反应）。
  const [scrollEl, setScrollEl] = useState<HTMLDivElement | null>(null);
  const [expandedDirs, setExpandedDirs] = useState<ReadonlySet<string>>(new Set());
  const [dirFiles, setDirFiles] = useState<ReadonlyMap<string, DirState>>(new Map());
  const reqSeq = useRef(0);

  // 点开的折叠目录还没拉（或写操作后清掉了）就拉一次。只在这几种时候跑，不跟 5 秒轮询（轮询换了 repo
  // 对象会进来，但拉过的不再拉）。目录已不在清单里（被放弃、加进 .gitignore、里面有文件被暂存）就不拉。
  const sessionCwdOf = (r: string) => (r === cwd ? null : cwd);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 刻意每次渲染都检查，靠 dirFiles 里已有或在途的条目去重，不会重复拉
  useEffect(() => {
    if (!repo) return;
    const r = repo.root;
    const present = new Set(repo.summary.files.filter(isDirEntry).map((file) => dirKey(r, file.path)));
    for (const k of expandedDirs) {
      if (!present.has(k) || dirFiles.has(k)) continue;
      const path = k.slice(k.indexOf("\u0000") + 1);
      const req = ++reqSeq.current;
      const settle = (state: Omit<DirState, "req">) =>
        setDirFiles((prev) => (prev.get(k)?.req === req ? new Map(prev).set(k, { req, ...state }) : prev));
      setDirFiles((prev) => new Map(prev).set(k, { req, loading: true, error: null, files: [] }));
      getUntrackedDirFiles(r, path, sessionCwdOf(r))
        .then((files) => settle({ loading: false, error: null, files }))
        .catch((e) => settle({ loading: false, error: formatBackendError(e, t.locale), files: [] }));
    }
  });

  if (!repo) return <p className="side-panel-empty">{t.chat.diffNoRepo}</p>;
  const root = repo.root;
  const key = normPath(root).toLowerCase();
  // 手机桥逐字校验会话目录：仓库根不是会话目录本身时带上它（同 RepoChanges 的做空结论）。
  const sessionCwd = root === cwd ? null : cwd;
  const files = repo.summary.files as ScmFile[];
  const staged = stagedRows(files);
  const work = workRows(files);
  const committed: Row[] = repo.base ? repo.committed.map((file) => ({ file, letter: file.status })) : [];

  const repoComments = comments.filter((comment) => sameRoot(comment.dir, root));
  // 筛选按钮只在认得出本会话改了什么时出现；按钮没了（/clear 换代、编辑记录滑出加载窗口）就退回「全部」，
  // 不然列表一片空、提交被禁、又找不到清除筛选的入口（做空第 2 轮复现）。
  const hasSession = touchedPaths.length > 0;
  const scope = hasSession ? scopeChoice : "all";

  if (open && sameRoot(open.root, root)) {
    // 目录条目不是文件，不进单文件 diff 的清单；从折叠目录里点开的文件不在摘要里，补进去（ChangesView 靠清单认
    // 未跟踪、决定读盘）。只补点开的这一个：目录里可能有上万个文件，整份塞进去它的文件列表又要卡。
    const fromDir = open.kind === "work" && !files.some((file) => file.path === open.path)
      ? [...dirFiles.values()].flatMap((state) => state.files).find((file) => file.path === open.path)
      : undefined;
    const summary = open.kind === "committed"
      ? { ...repo.summary, files: repo.committed, ahead: null, behind: null }
      : { ...repo.summary, files: [...files.filter((file) => !isDirEntry(file)), ...(fromDir ? [fromDir] : [])] };
    return (
      <ChangesView
        key={`open\u0000${key}\u0000${open.kind}`}
        cwd={root}
        summary={summary}
        diffBase={open.kind === "committed" ? repo.base : null}
        sessionCwd={sessionCwd}
        initialPath={open.path}
        onBack={() => setOpen(null)}
        onSelectPath={(path) => { if (path) setOpen({ root, kind: open.kind, path }); }}
        comments={repoComments}
        touchedPaths={touchedPaths}
        turnTouchedPaths={turnTouchedPaths}
        onAddComment={(comment) => onAddComment(comment, root)}
        onRemoveComment={onRemoveComment}
        escOverrideRef={escOverrideRef}
        live={live}
      />
    );
  }

  const run = async (action: () => Promise<unknown>, after?: (result: unknown) => void) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = await action();
      after?.(result);
    } catch (e) {
      setError(formatBackendError(e, t.locale));
    } finally {
      setBusy(false);
      // 点开的折叠目录里的文件可能刚被暂存 / 放弃：清掉让上面的 effect 重拉。
      setDirFiles(new Map());
      onChanged();
    }
  };
  const stage = (rows: readonly Row[]) => run(() => gitStage(root, rows.map((row) => row.file.path), sessionCwd));
  const unstage = (rows: readonly Row[]) => run(() => gitUnstage(root, rows.map((row) => row.file.path), sessionCwd));
  const askDiscard = (list: readonly ScmFile[]) => setConfirmDiscard({ root, name: repo.name, files: list });
  const ignoreDir = (path: string) => run(() => gitIgnoreDir(root, path));
  const toggleDirEntry = (path: string) => {
    const k = dirKey(root, path);
    setExpandedDirs((prev) => toggle(prev, k));
    // 收起时丢掉清单：再点开是现拉的。
    setDirFiles((prev) => {
      if (!prev.has(k)) return prev;
      const next = new Map(prev);
      next.delete(k);
      return next;
    });
  };
  const confirmAndDiscard = () => {
    const pending = confirmDiscard;
    setConfirmDiscard(null);
    if (!pending) return;
    // 仓库已经不在列表里（worktree 被删、被挤出上限）：作废这次确认，不拿别的仓库顶上。
    if (!repos.some((item) => sameRoot(item.root, pending.root))) {
      setError(t.chat.scmRepoGone(pending.name));
      return;
    }
    const target = pending.root;
    void run(() => gitDiscard(target, pending.files.map((file) => file.path), target === cwd ? null : cwd));
  };
  const message = messages.get(key) ?? "";
  const setMessage = (value: string) => setMessages((prev) => new Map(prev).set(key, value));
  const commitAll = staged.length === 0;
  // 开着搜索或「本会话 / 本轮」筛选时不许「提交全部」：列表只露出一部分，全部提交会带上看不见的文件
  //（做空复现：筛到只剩 agent 改的那个，隐藏的 secret.env 一起进了提交）。先暂存要提交的，或清掉筛选。
  // 「提交」提交整个暂存区，「提交全部」提交整个「更改」组：筛选藏起了其中任何一个都不许提交（做空第 2 轮：
  // 只拦了「提交全部」，筛选下点「提交」把被筛掉的暂存文件也提交了）。
  const query = search.trim().toLowerCase();
  const sessionSet = scope === "turn" ? turnTouchedPaths : touchedPaths;
  const matchQuery = (path: string) => !query || path.toLowerCase().includes(query);
  const matchScope = (path: string) => scope === "all" || touchedInRoot(sessionSet, root, path);
  const openDir = (row: Row) => (isDirEntry(row.file) && expandedDirs.has(dirKey(root, row.file.path)) ? dirFiles.get(dirKey(root, row.file.path)) : undefined);
  // 点开的折叠目录里显示哪些文件：目录名就命中了搜索就全显示，否则只显示命中的（本会话筛选逐个算）。
  const shownKids = (row: Row) => (openDir(row)?.files ?? []).filter((kid) => matchScope(kid.path) && (matchQuery(row.file.path) || matchQuery(kid.path)));
  const filter = (rows: readonly Row[]) => rows.filter((row) => matchScope(row.file.path)
    && (matchQuery(row.file.path) || (isDirEntry(row.file) && shownKids(row).length > 0)));
  // 折叠目录在筛选下只露出了一部分：目录名没命中搜索（只显示命中的文件），或开着「本会话 / 本轮」
  //（matchScope 对目录是前缀匹配，会话只写过其中一个也算命中）。这时整个目录不能当一个「看得见的条目」
  // 交给提交或整组操作，否则目录里看不见的文件会被一起提交或放弃（做空第 3 轮复现：搜 keep 只露出
  // keep.js，「提交全部」发出去的是 paths:["out/"]，60 个文件全进了提交）。
  const partialDir = (row: Row) => isDirEntry(row.file) && (scope !== "all" || !matchQuery(row.file.path));
  const hidesFiles = (rows: readonly Row[]) => filter(rows).length < rows.length || rows.some(partialDir);
  const commitBlockedByFilter = hidesFiles(commitAll ? work : staged);
  // 整组操作的对象：只露出一部分的折叠目录换成其中看得见的文件（没点开就一个都不算）。
  const actionable = (shown: readonly Row[]): Row[] =>
    shown.flatMap((row) => (partialDir(row) ? shownKids(row).map((kid) => ({ file: kid, letter: "U" }) as Row) : [row]));
  const canCommit = !busy && !!message.trim() && (staged.length > 0 || work.length > 0) && !commitBlockedByFilter;
  const commit = () => {
    if (!canCommit) return;
    void run(() => gitCommit(root, message, commitAll, sessionCwd, commitAll ? work.map((row) => row.file.path) : undefined), (sha) => {
      setMessage("");
      setNotice(t.chat.scmCommitted1(String(sha)));
    });
  };

  const commentCount = (path: string) => repoComments.filter((comment) => comment.path === path).length;
  const onRowContext = (event: ReactMouseEvent, file: ScmFile) => {
    if (remoteUi() || file.status === "D") return;
    event.preventDefault();
    setCtxMenu({ x: event.clientX, y: event.clientY, rel: file.path });
  };

  const rowActions = (kind: GroupKind, row: Row) => {
    if (kind === "committed") return null;
    const deleted = row.letter === "D";
    // 冲突文件（"!"）不给暂存 / 放弃：暂存等于告诉 git 已解决，后端也会拒（git/conflict）。
    const conflict = row.letter === "!";
    return (
      <span className="scm-row-actions">
        {!remoteUi() && !deleted && (
          <button type="button" className="scm-icon-btn" aria-label={t.chat.scmOpenFile} data-tip={t.chat.scmOpenFile} disabled={busy}
            onClick={() => void openPathWith(root, row.file.path, "default").catch(() => {})}>
            <OpenIcon />
          </button>
        )}
        {kind === "work" && !conflict && (
          <button type="button" className="scm-icon-btn" aria-label={t.chat.scmDiscard} data-tip={t.chat.scmDiscard} disabled={busy}
            onClick={() => askDiscard([row.file])}>
            <DiscardIcon />
          </button>
        )}
        {conflict ? null : kind === "work"
          ? <button type="button" className="scm-icon-btn" aria-label={t.chat.scmStage} data-tip={t.chat.scmStage} disabled={busy} onClick={() => void stage([row])}><PlusIcon /></button>
          : <button type="button" className="scm-icon-btn" aria-label={t.chat.scmUnstage} data-tip={t.chat.scmUnstage} disabled={busy} onClick={() => void unstage([row])}><MinusIcon /></button>}
      </span>
    );
  };

  /// dirLabel：折叠目录里的文件只显示目录内的相对位置（上级目录在折叠行上已经写了）。
  const fileRow = (kind: GroupKind, row: Row, depth: number, showDir: boolean, dirLabel?: string) => {
    const slash = row.file.path.lastIndexOf("/");
    const name = row.file.path.slice(slash + 1);
    const dir = dirLabel ?? (slash > 0 ? row.file.path.slice(0, slash) : "");
    const count = commentCount(row.file.path);
    return (
      <div key={`${kind}\u0000${row.file.path}`} className="scm-row" data-testid={`scm-row-${kind}`} style={{ paddingLeft: 8 + depth * 12 }}>
        <button type="button" className="scm-row-main" data-tip={row.file.path}
          onClick={() => setOpen({ root, kind, path: row.file.path })}
          onContextMenu={(event) => onRowContext(event, row.file)}>
          <FileIcon />
          <span className={"scm-name" + (row.letter === "D" ? " is-deleted" : "")}>{name}</span>
          {showDir && dir && <span className="scm-dir">{dir}</span>}
          {count > 0 && <span className="chat-diff-cmt-count" aria-label={t.chat.reviewComments(count)}>{count}</span>}
        </button>
        {rowActions(kind, row)}
        {kind !== "committed" && !remoteUi() && row.letter !== "D" && (
          <RowMore label={t.chat.filesMore} open={ctxMenu?.rel === row.file.path} onOpen={(x, y) => setCtxMenu({ x, y, rel: row.file.path })} />
        )}
        <span className="scm-letters">
          <LineCounts file={row.file} />
          <span className={`scm-status is-${row.letter === "!" ? "conflict" : row.letter.toLowerCase()}`} data-tip={t.chat.diffStatusName[row.letter] ?? row.letter}>{row.letter}</span>
        </span>
      </div>
    );
  };

  /// 折叠的未跟踪目录一行：「目录名/ · N 个文件」，点一下展开 / 收起其中的文件。
  const dirRow = (kind: GroupKind, row: Row, depth: number, showDir: boolean) => {
    const trimmed = row.file.path.slice(0, -1);
    const slash = trimmed.lastIndexOf("/");
    const parent = slash > 0 ? trimmed.slice(0, slash) : "";
    const isOpen = expandedDirs.has(dirKey(root, row.file.path));
    const count = row.file.untrackedFiles;
    return (
      <div className="scm-row is-folded" data-testid={`scm-row-${kind}`} style={{ paddingLeft: 8 + depth * 12 }}>
        <button type="button" className="scm-row-main" data-tip={row.file.path} aria-expanded={isOpen}
          onClick={() => toggleDirEntry(row.file.path)}>
          <FolderIcon />
          <span className="scm-name">{trimmed.slice(slash + 1)}/</span>
          {count != null && <span className="scm-dir-count">· {t.chat.scmDirFiles(count)}</span>}
          {showDir && parent && <span className="scm-dir">{parent}</span>}
        </button>
        {kind === "work" && (
          <span className="scm-row-actions">
            {/* 改的是仓库里的 .gitignore，手机端不给（remote.rs 拒绝名单）。 */}
            {!remoteUi() && (
              <button type="button" className="scm-icon-btn" aria-label={t.chat.scmIgnoreDir} data-tip={t.chat.scmIgnoreDir} disabled={busy}
                onClick={() => void ignoreDir(row.file.path)}>
                <IgnoreIcon />
              </button>
            )}
            <button type="button" className="scm-icon-btn" aria-label={t.chat.scmDiscard} data-tip={t.chat.scmDiscard} disabled={busy}
              onClick={() => askDiscard([row.file])}>
              <DiscardIcon />
            </button>
            <button type="button" className="scm-icon-btn" aria-label={t.chat.scmStage} data-tip={t.chat.scmStage} disabled={busy} onClick={() => void stage([row])}><PlusIcon /></button>
          </span>
        )}
        <span className="scm-letters">
          <span className="scm-status is-u" data-tip={t.chat.diffStatusName.U ?? "U"}>U</span>
        </span>
      </div>
    );
  };

  /// 一个变更条目展开成虚拟列表的若干行：普通文件一行；折叠目录一行，点开了再跟上其中的文件。
  const rowItems = (kind: GroupKind, row: Row, depth: number, showDir: boolean): VRow[] => {
    const id = `${kind}\u0000${row.file.path}`;
    if (!isDirEntry(row.file)) return [{ key: id, size: ROW_H, node: fileRow(kind, row, depth, showDir) }];
    const head: VRow = { key: id, size: ROW_H, node: dirRow(kind, row, depth, showDir) };
    const state = openDir(row);
    if (!expandedDirs.has(dirKey(root, row.file.path))) return [head];
    const pad = { paddingLeft: 8 + (depth + 1) * 12 };
    if (!state || state.loading) return [head, { key: `${id}\u0000loading`, size: NOTE_H, node: <div className="scm-empty" style={pad}>{t.chat.scmDirLoading}</div> }];
    if (state.error) return [head, { key: `${id}\u0000error`, size: NOTE_H, node: <div className="scm-empty is-error" role="alert" style={pad}>{state.error}</div> }];
    return [head, ...shownKids(row).map((kid) => {
      const rel = kid.path.slice(row.file.path.length);
      const slash = rel.lastIndexOf("/");
      return { key: `${kind}\u0000${kid.path}`, size: ROW_H, node: fileRow(kind, { file: kid, letter: "U" }, depth + 1, true, slash > 0 ? rel.slice(0, slash) : "") };
    })];
  };

  const renderTree = (kind: GroupKind, nodes: FileTreeNode[], rows: ReadonlyMap<string, Row>, depth: number): VRow[] =>
    nodes.flatMap((node) => {
      if (node.children) {
        const id = `${kind}\u0000${node.path}`;
        const closed = collapsedDirs.has(id);
        return [
          {
            key: id,
            size: ROW_H,
            node: (
              <div className="scm-row is-dir" style={{ paddingLeft: 8 + depth * 12 }}>
                <button type="button" className="scm-row-main" aria-expanded={!closed} onClick={() => setCollapsedDirs((prev) => toggle(prev, id))}>
                  <FolderIcon />
                  <span className="scm-name">{node.name}</span>
                </button>
              </div>
            ),
          },
          ...(closed ? [] : renderTree(kind, node.children, rows, depth + 1)),
        ];
      }
      const row = node.file ? rows.get(node.file.path) : undefined;
      return row ? rowItems(kind, row, depth, false) : [];
    });

  // headActions 拿到的是当前显示的行：整组操作只作用于看得见的文件（做空复现：「本会话」筛选下点
  // 「全部放弃」，13 个文件全被放弃，其中 12 个不是本会话改的）。
  const group = (kind: GroupKind, title: string, rows: readonly Row[], headActions: ((shown: readonly Row[]) => ReactNode) | null) => {
    const shown = filter(rows);
    if (rows.length === 0) return null;
    const isFolded = folded.has(kind);
    // 树形里折叠目录是一片叶子：按去掉尾斜杠的路径建树（不然 buildFileTree 切出一个空名的文件）。
    const treePath = (row: Row) => (isDirEntry(row.file) ? row.file.path.slice(0, -1) : row.file.path);
    const byPath = new Map(shown.map((row) => [treePath(row), row]));
    return (
      <section className="scm-group" data-testid={`scm-group-${kind}`}>
        <div className="scm-group-head">
          <button type="button" className="scm-group-toggle" aria-expanded={!isFolded} onClick={() => setFolded((prev) => toggle(prev, kind))}>
            <Chevron open={!isFolded} />
            <span>{title}</span>
          </button>
          {headActions && actionable(shown).length > 0 && <span className="scm-group-actions">{headActions(actionable(shown))}</span>}
          {/* 计数徽标坐在与各行「统计 + 状态字母」同宽的尾列里：组头的放弃/暂存才能和各行的严格同列。 */}
          <span className="scm-letters"><span className="scm-count">{changedFileCount(rows.map((row) => row.file))}</span></span>
        </div>
        {!isFolded && (shown.length === 0
          ? <div className="scm-empty">{query ? t.chat.diffSearchNoResults : scope === "turn" ? t.chat.diffNoTurnChanges : t.chat.diffNoSessionChanges}</div>
          : <VirtualRows scrollEl={scrollEl} items={layout === "tree"
            ? renderTree(kind, buildFileTree(shown.map((row) => ({ path: treePath(row), status: row.letter }))), byPath, 0)
            : shown.flatMap((row) => rowItems(kind, row, 0, true))} />)}
      </section>
    );
  };

  const nothing = staged.length === 0 && work.length === 0 && committed.length === 0;

  return (
    <div className="scm" data-testid="source-control" ref={setScrollEl}>
      <section className="scm-repos" aria-label={t.chat.scmRepos}>
        <div className="scm-section-title">{t.chat.scmRepos}</div>
        {repos.map((item) => {
          const selected = sameRoot(item.root, root);
          const dirty = item.summary.files.length > 0;
          return (
            <button key={normPath(item.root)} type="button" className={"scm-repo" + (selected ? " is-selected" : "")} aria-pressed={selected}
              data-tip={item.root} onClick={() => { setOpen(null); setError(null); setNotice(null); onSelectRoot(item.root); }}>
              <RepoIcon />
              <span className="scm-repo-name">{item.name}</span>
              <span className="scm-repo-branch">
                <BranchIcon />
                <span>{item.summary.branch ?? "HEAD"}</span>
                {dirty && <span className="scm-dirty" aria-label={t.chat.scmDirty}>*</span>}
              </span>
              {!!item.summary.ahead && <em className="scm-div" data-tip={t.chat.diffAheadTip(item.summary.ahead)}>↑{item.summary.ahead}</em>}
              {!!item.summary.behind && <em className="scm-div" data-tip={t.chat.diffBehindTip(item.summary.behind)}>↓{item.summary.behind}</em>}
            </button>
          );
        })}
      </section>

      <div className="scm-commit">
        <textarea
          className="scm-commit-input"
          rows={1}
          value={message}
          // 手机上没有 Ctrl+Enter，占位符不写快捷键；分支名上一行已经有了（UI 调研 m12，SE 上还会折成两行被截）。
          placeholder={remoteUi() ? t.chat.scmCommitPlaceholderRemote : t.chat.scmCommitPlaceholder(repo.summary.branch)}
          aria-label={remoteUi() ? t.chat.scmCommitPlaceholderRemote : t.chat.scmCommitPlaceholder(repo.summary.branch)}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !event.nativeEvent.isComposing) {
              event.preventDefault();
              commit();
            }
          }}
        />
        <button type="button" className="scm-commit-btn btn-primary" disabled={!canCommit}
          data-tip={commitBlockedByFilter ? t.chat.scmCommitFiltered : commitAll ? t.chat.scmCommitAllTip : undefined} onClick={commit}>
          <CheckIcon />
          {commitAll ? t.chat.scmCommitAll : t.chat.scmCommit}
        </button>
        {error && <div className="scm-error" role="alert">{error}</div>}
        {notice && <div className="scm-notice" role="status">{notice}</div>}
        {commitBlockedByFilter && !!message.trim() && <div className="scm-notice">{t.chat.scmCommitFiltered}</div>}
      </div>
      {/* 推送 / PR / CI / 合并（二期包 5）。key 按仓库：切仓库时轮询、表单、提示都换成那个仓库的。 */}
      <ForgeBar key={key} repo={repo} sessionCwd={sessionCwd} sessionId={sessionId} active={visible} onChanged={onChanged} />

      <div className="scm-tools">
        <input
          type="text"
          className="chat-diff-search"
          placeholder={t.chat.diffSearchPlaceholder}
          value={search}
          onChange={(event) => {
            if ((event.nativeEvent as InputEvent).isComposing) return;
            setSearch(event.target.value);
          }}
          onCompositionEnd={(event) => setSearch(event.currentTarget.value)}
        />
        <div className="chat-diff-layout" role="group" aria-label={t.chat.diffLayoutLabel}>
          <button type="button" aria-pressed={layout === "list"} aria-label={t.chat.diffLayoutList} data-tip={t.chat.diffLayoutList} onClick={() => setLayout("list")}>
            <Icon glyph={icMenu} size={12} />
          </button>
          <button type="button" aria-pressed={layout === "tree"} aria-label={t.chat.diffLayoutTree} data-tip={t.chat.diffLayoutTree} onClick={() => setLayout("tree")}>
            <Icon glyph={icTree} size={12} />
          </button>
        </div>
      </div>
      {hasSession && (
        // 外面套一层进居中栏（.scm-scope 与 .scm-tools 同列限宽）：切换本身是 fit-content 的小控件，直接挂在
        // .scm 下会贴着面板最左边，面板最大化时和搜索框、文件列表隔着半个屏幕（2026-11-12 实拍）。
        <div className="scm-scope"><div className="chat-diff-scope" role="group" aria-label={t.chat.diffScopeLabel}>
          <button type="button" aria-pressed={scope === "all"} onClick={() => setScope("all")}>{t.chat.diffScopeAll}</button>
          <button type="button" aria-pressed={scope === "session"} data-tip={t.chat.diffOnlySessionTip} onClick={() => setScope("session")}>
            {t.chat.diffScopeSession(files.filter((file) => touchedInRoot(touchedPaths, root, file.path)).length)}
          </button>
          <button type="button" aria-pressed={scope === "turn"} data-tip={t.chat.diffOnlyTurnTip} onClick={() => setScope("turn")}>
            {t.chat.diffScopeTurn(files.filter((file) => touchedInRoot(turnTouchedPaths, root, file.path)).length)}
          </button>
        </div></div>
      )}

      <div className="scm-groups">
        {nothing && <div className="scm-empty is-clean">{t.chat.scmNoChanges}</div>}
        {group("staged", t.chat.scmStaged, staged, (shown) => (
          <button type="button" className="scm-icon-btn" aria-label={t.chat.scmUnstageAll} data-tip={t.chat.scmUnstageAll} disabled={busy} onClick={() => void unstage(shown)}><MinusIcon /></button>
        ))}
        {group("work", t.chat.scmChanges, work, (shown) => <>
          <button type="button" className="scm-icon-btn" aria-label={t.chat.scmDiscardAll} data-tip={t.chat.scmDiscardAll} disabled={busy} onClick={() => askDiscard(shown.filter((row) => row.letter !== "!").map((row) => row.file))}><DiscardIcon /></button>
          <button type="button" className="scm-icon-btn" aria-label={t.chat.scmStageAll} data-tip={t.chat.scmStageAll} disabled={busy} onClick={() => void stage(shown.filter((row) => row.letter !== "!"))}><PlusIcon /></button>
        </>)}
        {group("committed", t.chat.scmCommitted(repo.baseLabel ?? "main"), committed, null)}
        {/* 没选中过的仓库后端只给了文件清单：行数和「本分支已提交」正在拉。 */}
        {repo.detailed === false && <div className="scm-empty">{t.chat.scmLoadingDetail}</div>}
      </div>

      {ctxMenu && <PathContextMenu cwd={root} rel={ctxMenu.rel} isDir={false} x={ctxMenu.x} y={ctxMenu.y} onClose={() => setCtxMenu(null)} />}
      {confirmDiscard && (
        <AppModal label={t.chat.scmDiscardTitle(changedFileCount(confirmDiscard.files))} onClose={() => setConfirmDiscard(null)}>
          <div className="chat-modal-title">{t.chat.scmDiscardTitle(changedFileCount(confirmDiscard.files))}</div>
          <div className="scm-discard-repo">{t.chat.scmDiscardIn(confirmDiscard.name)}</div>
          <ul className="scm-discard-list">
            {confirmDiscard.files.slice(0, 8).map((file) => (
              <li key={file.path}>{file.path}{file.untrackedFiles != null && <span className="is-count"> · {t.chat.scmDirFiles(file.untrackedFiles)}</span>}</li>
            ))}
            {confirmDiscard.files.length > 8 && <li className="is-more">{t.chat.scmDiscardMore(confirmDiscard.files.length - 8)}</li>}
          </ul>
          <div className="scm-discard-note">{t.chat.scmDiscardNote}</div>
          {confirmDiscard.files.some(isDirEntry) && <div className="scm-discard-note">{t.chat.scmDiscardDirNote}</div>}
          <div className="chat-modal-actions">
            <button type="button" className="ns-btn" autoFocus onClick={() => setConfirmDiscard(null)}>{t.newSession.cancel}</button>
            <button type="button" className="ns-btn is-danger" onClick={confirmAndDiscard}>{t.chat.scmDiscardConfirm}</button>
          </div>
        </AppModal>
      )}
    </div>
  );
}

const toggle = (set: ReadonlySet<string>, id: string) => {
  const next = new Set(set);
  if (next.has(id)) next.delete(id); else next.add(id);
  return next;
};

/// 滚动容器还没布局（面板刚挂上、jsdom）时量到 0 高：按这个高度先排一屏，不然整组一行都不渲染。
const FALLBACK_RECT = { width: 320, height: 800 };
function observeRect(instance: Virtualizer<HTMLDivElement, Element>, cb: (rect: { width: number; height: number }) => void) {
  return observeElementRect(instance, (rect) => cb(rect.height > 0 ? rect : { width: rect.width || FALLBACK_RECT.width, height: FALLBACK_RECT.height }));
}

/// 一组的行做虚拟滚动：只挂可视区附近的行。滚动容器是整个 .scm（存储库、提交框、各组一起滚，和改前一样），
/// 每组各一个 virtualizer，scrollMargin 是这组列表在滚动内容里的起点（上面的提交框会变高，每次渲染后重量）。
function VirtualRows({ items, scrollEl }: { items: readonly VRow[]; scrollEl: HTMLDivElement | null }) {
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const [margin, setMargin] = useState(0);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 刻意每次渲染后重量列表起点（上方提交框会变高），值没变不 setState，不会循环
  useLayoutEffect(() => {
    const body = bodyRef.current;
    if (!body || !scrollEl) return;
    // 沿 offsetParent 累加到滚动容器（.scm 是 position: relative）：offsetTop 是内容坐标，不随滚动变，
    // 不用 getBoundingClientRect 再加 scrollTop 那种和滚动时刻绑在一起的算法。
    let next = 0;
    for (let el: HTMLElement | null = body; el && el !== scrollEl; el = el.offsetParent as HTMLElement | null) next += el.offsetTop;
    if (next !== margin) setMargin(next);
  });
  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollEl,
    estimateSize: (index) => items[index]?.size ?? ROW_H,
    // key 用路径：轮询后清单变了，按 index 复用会把别的行的测量高度、焦点套到新行上（同 Sticker.tsx 的教训）。
    getItemKey: (index) => items[index]?.key ?? index,
    overscan: 12,
    scrollMargin: margin,
    initialRect: FALLBACK_RECT,
    observeElementRect: observeRect,
    // 实测量不出（没布局）就用估计值，量到 0 会让所有行挤在一处、全被判成可见。
    measureElement: (el) => el.getBoundingClientRect().height || items[Number(el.getAttribute("data-index"))]?.size || ROW_H,
  });
  return (
    <div ref={bodyRef} className="scm-vlist" style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((item) => (
        <div key={item.key} data-index={item.index} ref={virtualizer.measureElement} className="scm-vitem"
          style={{ transform: `translateY(${item.start - margin}px)` }}>
          {items[item.index]?.node}
        </div>
      ))}
    </div>
  );
}

function IgnoreIcon() {
  return (
    <Icon glyph={icBan} size={14} />
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <Icon glyph={icChevronRight} size={12} className={"scm-chevron" + (open ? " is-open" : "")} />
  );
}

function PlusIcon() {
  return <Icon glyph={icPlus} size={14} />;
}

function MinusIcon() {
  return <Icon glyph={icMinus} size={14} />;
}

function DiscardIcon() {
  return (
    <Icon glyph={icUndo} size={14} />
  );
}

function CheckIcon() {
  return <Icon glyph={icCheck} size={14} />;
}

function RepoIcon() {
  return (
    <Icon glyph={icBook} size={12} />
  );
}

function BranchIcon() {
  return (
    <Icon glyph={icGitBranch} size={12} />
  );
}
