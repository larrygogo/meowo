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

import { useState, type MouseEvent as ReactMouseEvent, type MutableRefObject, type ReactNode } from "react";
import { gitCommit, gitDiscard, gitStage, gitUnstage, openPathWith, type RepoChangesDto } from "../../api";
import type { GitChangedFileDto } from "../../generated/contracts/GitChangedFileDto";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { remoteUi } from "../../remoteMode";
import { AppModal } from "../AppModal";
import type { ReviewComment } from "./diffComments";
import { buildFileTree, normPath, type FileTreeNode } from "./gitDiff";
import {
  ChangesView,
  FileIcon,
  FolderIcon,
  LineCounts,
  OpenIcon,
  PathContextMenu,
  RowMore,
  readDiffLayout,
  saveDiffLayout,
  touchedInRoot,
} from "./GitDiffView";

/// 旧后端（手机页可能先于安装版更新）不下发两个新字段：按「全部是未暂存」处理。
type ScmFile = GitChangedFileDto & { indexStatus?: string | null; worktreeStatus?: string | null };
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

/// 选中的仓库：记着的那个还在就用它，否则工作目录所在仓库，再否则第一个。
export function pickRepo(repos: readonly RepoChangesDto[], selected: string | null): RepoChangesDto | null {
  return (selected ? repos.find((repo) => sameRoot(repo.root, selected)) : undefined)
    ?? repos.find((repo) => repo.isCwdRepo)
    ?? repos[0]
    ?? null;
}

export function SourceControl({ cwd, repos, selectedRoot, onSelectRoot, onChanged, comments, touchedPaths, turnTouchedPaths, onAddComment, onRemoveComment, escOverrideRef, live = false }: {
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
    const summary = open.kind === "committed"
      ? { ...repo.summary, files: repo.committed, ahead: null, behind: null }
      : repo.summary;
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
      onChanged();
    }
  };
  const stage = (rows: readonly Row[]) => run(() => gitStage(root, rows.map((row) => row.file.path), sessionCwd));
  const unstage = (rows: readonly Row[]) => run(() => gitUnstage(root, rows.map((row) => row.file.path), sessionCwd));
  const askDiscard = (list: readonly ScmFile[]) => setConfirmDiscard({ root, name: repo.name, files: list });
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
  const query0 = search.trim().toLowerCase();
  const visibleNow = (rows: readonly Row[]) => rows.filter((row) =>
    (!query0 || row.file.path.toLowerCase().includes(query0))
    && (scope === "all" || touchedInRoot(scope === "turn" ? turnTouchedPaths : touchedPaths, root, row.file.path)));
  const commitBlockedByFilter = commitAll ? visibleNow(work).length < work.length : visibleNow(staged).length < staged.length;
  const canCommit = !busy && !!message.trim() && (staged.length > 0 || work.length > 0) && !commitBlockedByFilter;
  const commit = () => {
    if (!canCommit) return;
    void run(() => gitCommit(root, message, commitAll, sessionCwd, commitAll ? work.map((row) => row.file.path) : undefined), (sha) => {
      setMessage("");
      setNotice(t.chat.scmCommitted1(String(sha)));
    });
  };

  const query = search.trim().toLowerCase();
  const sessionSet = scope === "turn" ? turnTouchedPaths : touchedPaths;
  const filter = (rows: readonly Row[]) => rows.filter((row) =>
    (!query || row.file.path.toLowerCase().includes(query))
    && (scope === "all" || touchedInRoot(sessionSet, root, row.file.path)));
  const commentCount = (path: string) => repoComments.filter((comment) => comment.path === path).length;
  const toggle = (set: ReadonlySet<string>, id: string) => {
    const next = new Set(set);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  };
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

  const fileRow = (kind: GroupKind, row: Row, depth: number, showDir: boolean) => {
    const slash = row.file.path.lastIndexOf("/");
    const name = row.file.path.slice(slash + 1);
    const dir = slash > 0 ? row.file.path.slice(0, slash) : "";
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

  const renderTree = (kind: GroupKind, nodes: FileTreeNode[], rows: ReadonlyMap<string, Row>, depth: number): ReactNode[] =>
    nodes.flatMap((node) => {
      if (node.children) {
        const id = `${kind}\u0000${node.path}`;
        const closed = collapsedDirs.has(id);
        return [
          <div key={id} className="scm-row is-dir" style={{ paddingLeft: 8 + depth * 12 }}>
            <button type="button" className="scm-row-main" aria-expanded={!closed} onClick={() => setCollapsedDirs((prev) => toggle(prev, id))}>
              <FolderIcon />
              <span className="scm-name">{node.name}</span>
            </button>
          </div>,
          ...(closed ? [] : renderTree(kind, node.children, rows, depth + 1)),
        ];
      }
      const row = node.file ? rows.get(node.file.path) : undefined;
      return row ? [fileRow(kind, row, depth, false)] : [];
    });

  // headActions 拿到的是当前显示的行：整组操作只作用于看得见的文件（做空复现：「本会话」筛选下点
  // 「全部放弃」，13 个文件全被放弃，其中 12 个不是本会话改的）。
  const group = (kind: GroupKind, title: string, rows: readonly Row[], headActions: ((shown: readonly Row[]) => ReactNode) | null) => {
    const shown = filter(rows);
    if (rows.length === 0) return null;
    const isFolded = folded.has(kind);
    const byPath = new Map(shown.map((row) => [row.file.path, row]));
    return (
      <section className="scm-group" data-testid={`scm-group-${kind}`}>
        <div className="scm-group-head">
          <button type="button" className="scm-group-toggle" aria-expanded={!isFolded} onClick={() => setFolded((prev) => toggle(prev, kind))}>
            <Chevron open={!isFolded} />
            <span>{title}</span>
          </button>
          {headActions && shown.length > 0 && <span className="scm-group-actions">{headActions(shown)}</span>}
          <span className="scm-count">{rows.length}</span>
        </div>
        {!isFolded && (shown.length === 0
          ? <div className="scm-empty">{query ? t.chat.diffSearchNoResults : scope === "turn" ? t.chat.diffNoTurnChanges : t.chat.diffNoSessionChanges}</div>
          : layout === "tree"
            ? renderTree(kind, buildFileTree(shown.map((row) => ({ path: row.file.path, status: row.letter }))), byPath, 0)
            : shown.map((row) => fileRow(kind, row, 0, true)))}
      </section>
    );
  };

  const nothing = staged.length === 0 && work.length === 0 && committed.length === 0;

  return (
    <div className="scm" data-testid="source-control">
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
          placeholder={t.chat.scmCommitPlaceholder(repo.summary.branch)}
          aria-label={t.chat.scmCommitPlaceholder(repo.summary.branch)}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !event.nativeEvent.isComposing) {
              event.preventDefault();
              commit();
            }
          }}
        />
        <button type="button" className="scm-commit-btn" disabled={!canCommit}
          data-tip={commitBlockedByFilter ? t.chat.scmCommitFiltered : commitAll ? t.chat.scmCommitAllTip : undefined} onClick={commit}>
          <CheckIcon />
          {commitAll ? t.chat.scmCommitAll : t.chat.scmCommit}
        </button>
        {error && <div className="scm-error" role="alert">{error}</div>}
        {notice && <div className="scm-notice" role="status">{notice}</div>}
        {commitBlockedByFilter && !!message.trim() && <div className="scm-notice">{t.chat.scmCommitFiltered}</div>}
      </div>

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
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M5 7h14M5 12h14M5 17h14" /></svg>
          </button>
          <button type="button" aria-pressed={layout === "tree"} aria-label={t.chat.diffLayoutTree} data-tip={t.chat.diffLayoutTree} onClick={() => setLayout("tree")}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><rect x="3" y="3" width="7" height="6" rx="1.5" /><rect x="14" y="10" width="7" height="5" rx="1.5" /><rect x="14" y="17" width="7" height="4" rx="1.5" /><path d="M6.5 9v9.5H14M6.5 12.5H14" /></svg>
          </button>
        </div>
      </div>
      {hasSession && (
        <div className="chat-diff-scope" role="group" aria-label={t.chat.diffScopeLabel}>
          <button type="button" aria-pressed={scope === "all"} onClick={() => setScope("all")}>{t.chat.diffScopeAll}</button>
          <button type="button" aria-pressed={scope === "session"} data-tip={t.chat.diffOnlySessionTip} onClick={() => setScope("session")}>
            {t.chat.diffScopeSession(files.filter((file) => touchedInRoot(touchedPaths, root, file.path)).length)}
          </button>
          <button type="button" aria-pressed={scope === "turn"} data-tip={t.chat.diffOnlyTurnTip} onClick={() => setScope("turn")}>
            {t.chat.diffScopeTurn(files.filter((file) => touchedInRoot(turnTouchedPaths, root, file.path)).length)}
          </button>
        </div>
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
        <AppModal label={t.chat.scmDiscardTitle(confirmDiscard.files.length)} onClose={() => setConfirmDiscard(null)}>
          <div className="chat-modal-title">{t.chat.scmDiscardTitle(confirmDiscard.files.length)}</div>
          <div className="scm-discard-repo">{t.chat.scmDiscardIn(confirmDiscard.name)}</div>
          <ul className="scm-discard-list">
            {confirmDiscard.files.slice(0, 8).map((file) => <li key={file.path}>{file.path}</li>)}
            {confirmDiscard.files.length > 8 && <li className="is-more">{t.chat.scmDiscardMore(confirmDiscard.files.length - 8)}</li>}
          </ul>
          <div className="scm-discard-note">{t.chat.scmDiscardNote}</div>
          <div className="chat-modal-actions">
            <button type="button" className="ns-btn" autoFocus onClick={() => setConfirmDiscard(null)}>{t.newSession.cancel}</button>
            <button type="button" className="ns-btn is-danger" onClick={confirmAndDiscard}>{t.chat.scmDiscardConfirm}</button>
          </div>
        </AppModal>
      )}
    </div>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg className={"scm-chevron" + (open ? " is-open" : "")} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

function PlusIcon() {
  return <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>;
}

function MinusIcon() {
  return <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M5 12h14" /></svg>;
}

function DiscardIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M9 14L4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
    </svg>
  );
}

function CheckIcon() {
  return <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>;
}

function RepoIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M4 19.5V5a2 2 0 0 1 2-2h13v15H6.5A2.5 2.5 0 0 0 4 20.5z" /><path d="M8 7h7" />
    </svg>
  );
}

function BranchIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="6" cy="6" r="2.5" /><circle cx="6" cy="18" r="2.5" /><circle cx="18" cy="9" r="2.5" />
      <path d="M6 8.5v7" /><path d="M18 11.5c0 4-4 4.5-9.2 4.6" />
    </svg>
  );
}
