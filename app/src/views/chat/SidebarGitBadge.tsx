/**
 * 侧栏会话行的 PR / CI 状态点与 `+N −M`（UI 优化 #54，对照 Conductor 侧栏的 GitHub 状态、Warp 竖排标签的
 * diff 统计）。多个 worktree 会话并行时，哪个 PR 的 CI 挂了不必逐个点开看。
 *
 * 位置在副行工作区名之后：行首 16px 是 activityTone 的状态点（运行 / 待处理 / 出错），这里不抢那一格，
 * 颜色也只用 PR 四态（同标题栏 `.tb-pr`）和增删色，不与状态点同色系混用。
 */
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { getSidebarGitBadges, type SidebarGitBadgeDto } from "../../api";
import { useT } from "../../i18n";

/** 前端多久问一次。后端 PR 每仓库 60 秒才真查一次 GitHub、改动数 15 秒一算，这里问得再勤也只是读缓存。 */
const POLL_MS = 30_000;
/** 首次取摘要前等这么久（见下方 useEffect）。 */
const FIRST_DELAY_MS = 250;
/** 与后端单次上限一致（forge/badge.rs MAX_CWDS）。 */
const MAX_CWDS = 64;

/**
 * 摘要放在组件外的小仓库里，由每个徽标自己订阅：结果回来时只重渲染有变化的那几个徽标，
 * 侧栏 200 行整表不跟着重渲染（renderItem 没有逐行 memo，整表一次约几十毫秒，见 ChatSidebar.perf.test）。
 */
export class GitBadgeStore {
  private map: ReadonlyMap<string, SidebarGitBadgeDto> = new Map();
  private json = "";
  private listeners = new Set<() => void>();
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };
  get(cwd: string): SidebarGitBadgeDto | undefined { return this.map.get(cwd); }
  set(rows: SidebarGitBadgeDto[]) {
    const json = JSON.stringify(rows);
    // 内容没变就不通知：轮询多半是空转。
    if (json === this.json) return;
    // 逐条复用旧对象：没变的目录引用不变，useSyncExternalStore 判等后那个徽标不重渲染。
    const prev = this.map;
    this.map = new Map(rows.map((row) => {
      const old = prev.get(row.cwd);
      return [row.cwd, old && JSON.stringify(old) === JSON.stringify(row) ? old : row];
    }));
    this.json = json;
    for (const listener of this.listeners) listener();
  }
}

/** 按工作目录取摘要写进仓库。窗口隐藏时不轮询（规格「隐藏 0 次」，后端另按窗口可见性兜一道），回到前台立刻补一次。 */
export function useSidebarGitBadges(cwds: readonly (string | null | undefined)[]): GitBadgeStore {
  const [store] = useState(() => new GitBadgeStore());
  // 列表每几百毫秒就可能换新引用：拼成去重排序的字符串当依赖，目录集合不变就不重起轮询。
  const key = useMemo(() => {
    const unique = [...new Set(cwds.filter((cwd): cwd is string => !!cwd))].sort();
    return unique.slice(0, MAX_CWDS).join("\n");
  }, [cwds]);
  useEffect(() => {
    if (!key) return;
    const list = key.split("\n");
    let alive = true;
    const fetchNow = () => {
      if (document.visibilityState === "hidden") return;
      getSidebarGitBadges(list)
        .then((rows) => { if (alive && Array.isArray(rows)) store.set(rows); })
        // 旧后端没有这条命令、gh 出错：不显示就是了，侧栏照常。
        .catch(() => {});
    };
    // 首取让到列表画出来之后：摘要不是急事，和首屏挤在同一拍会把 200 行的首次渲染拖慢两三成
    //（ChatSidebar.perf.test 实测，摘要与列表同拍到达时中位数 57→80ms）。
    const first = window.setTimeout(fetchNow, FIRST_DELAY_MS);
    const timer = window.setInterval(fetchNow, POLL_MS);
    const onVisible = () => { if (document.visibilityState === "visible") fetchNow(); };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      alive = false;
      window.clearTimeout(first);
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [key, store]);
  return store;
}

/**
 * 会话行副行里的摘要位。wrap：这一行本来没有副行（按工作区分组、又不是 worktree），有摘要时自己开一行。
 */
export function SidebarGitBadgeSlot({ store, cwd, wrap = false }: { store: GitBadgeStore; cwd: string; wrap?: boolean }) {
  const badge = useSyncExternalStore(store.subscribe, () => store.get(cwd));
  if (!badge) return null;
  return wrap
    ? <span className="chat-sidebar-metaline"><SidebarGitBadge badge={badge} /></span>
    : <SidebarGitBadge badge={badge} />;
}

/** 副行里的紧凑摘要：PR 状态点（+ CI 记号）与未提交的增删行数。 */
export function SidebarGitBadge({ badge }: { badge: SidebarGitBadgeDto }) {
  const t = useT();
  const pr = badge.pr;
  const ci = pr?.checks && pr.checks !== "none" ? pr.checks : null;
  const parts = [
    pr ? t.chat.sidebarPr(pr.number, t.chat.prState[pr.state] ?? pr.state) : null,
    ci ? t.chat.sidebarCi(t.forge.checkState[ci] ?? ci) : null,
    badge.files > 0 ? t.chat.uncommitted(badge.files) : null,
  ].filter(Boolean);
  const label = parts.join(" · ");
  const lines = badge.additions + badge.deletions > 0;
  return (
    <span className="chat-sidebar-git" role="img" aria-label={label} data-tip={label} data-testid="sidebar-git-badge">
      {pr && <i className={`chat-sidebar-pr is-${pr.state}`} aria-hidden="true" />}
      {ci && <i className={`chat-sidebar-ci is-${ci}`} aria-hidden="true" />}
      {badge.files > 0 && (lines
        ? <span className="chat-sidebar-diff" aria-hidden="true"><span className="chat-sidebar-add">+{badge.additions}</span><span className="chat-sidebar-del">−{badge.deletions}</span></span>
        // 二进制等数不出行数的：只报文件数，不写误导的 +0 −0（同标题栏分支胶囊）。
        : <span className="chat-sidebar-diff" aria-hidden="true">{badge.files}</span>)}
    </span>
  );
}
