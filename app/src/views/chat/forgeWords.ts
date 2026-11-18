// 代码托管平台的叫法：GitHub 是 PR、编号 #12、检查叫 CI；GitLab 是合并请求（MR）、编号 !12、检查叫流水线；
// Gitea / Forgejo 与 Bitbucket 沿用 PR。「更改」面板、侧栏角标、标题栏、评审面板都从这里取词。
//
// GitHub 一律用 forge.* / chat.* 的原句（改版前的文案与测试不动）；别的平台用 forgeP 的模板套平台词。
import type { useT } from "../../i18n";

type T = ReturnType<typeof useT>;
export type ForgeKind = "github" | "gitlab" | "gitea" | "bitbucket";

const KINDS: readonly ForgeKind[] = ["github", "gitlab", "gitea", "bitbucket"];

/** 后端的平台字段 → 平台；没有（旧后端、GitHub）按 GitHub。 */
export function forgeKind(forge: string | null | undefined): ForgeKind {
  return KINDS.includes(forge as ForgeKind) ? (forge as ForgeKind) : "github";
}

export type ForgeText = {
  name: string;
  pr: string;
  num: (n: number) => string;
  sessionPrs: string;
  branchPr: string;
  dismissPrTip: string;
  openPr: string;
  openOnSite: string;
  prFormTitle: (base: string) => string;
  prCreate: string;
  prCreated: (n: number) => string;
  checksNone: string;
  checksPass: (n: number) => string;
  checksFail: (n: number) => string;
  checksPending: (n: number) => string;
  mergeTitle: (n: number) => string;
  mergeBlockedFail: string;
  mergeBlockedPending: string;
  mergeBlockedDraft: string;
  merged: string;
  repoNeedsChoice: string;
  repoAt: (slug: string) => string;
  repoPickTitle: string;
  commentConfirm: string;
  monitorStart: string;
  reviewLoading: string;
  methods: Record<string, string>;
  prOpenTip: (n: number, title: string) => string;
  sidebarPr: (n: number, state: string) => string;
};

/** 这个平台的文案。host 给 codeberg.org 时平台名写 Codeberg（用户认的是站点名）。 */
export function forgeText(t: T, kind: ForgeKind, host?: string | null): ForgeText {
  if (kind === "github") {
    const f = t.forge;
    return {
      name: "GitHub",
      pr: "PR",
      num: (n) => `#${n}`,
      sessionPrs: f.sessionPrs,
      branchPr: f.branchPr,
      dismissPrTip: f.dismissPrTip,
      openPr: f.openPr,
      openOnSite: f.openOnGithub,
      prFormTitle: f.prFormTitle,
      prCreate: f.prCreate,
      prCreated: f.prCreated,
      checksNone: f.checksNone,
      checksPass: f.checksPass,
      checksFail: f.checksFail,
      checksPending: f.checksPending,
      mergeTitle: f.mergeTitle,
      mergeBlockedFail: f.mergeBlockedFail,
      mergeBlockedPending: f.mergeBlockedPending,
      mergeBlockedDraft: f.mergeBlockedDraft,
      merged: f.merged,
      repoNeedsChoice: f.repoNeedsChoice,
      repoAt: f.repoAt,
      repoPickTitle: f.repoPickTitle,
      commentConfirm: f.commentConfirm,
      monitorStart: f.monitorStart,
      reviewLoading: f.reviewLoading,
      methods: f.methods,
      prOpenTip: t.chat.prOpenTip,
      sidebarPr: t.chat.sidebarPr,
    };
  }
  const base = t.forgeWords[kind];
  const w = host?.toLowerCase() === "codeberg.org" ? { ...base, name: "Codeberg" } : base;
  const p = t.forgeP;
  return {
    name: w.name,
    pr: w.pr,
    num: (n) => `${w.num}${n}`,
    sessionPrs: p.sessionPrs(w),
    branchPr: p.branchPr(w),
    dismissPrTip: p.dismissPrTip(w),
    openPr: p.openPr(w),
    openOnSite: p.openOnSite(w),
    prFormTitle: (b) => p.prFormTitle(w, b),
    prCreate: p.prCreate(w),
    prCreated: (n) => p.prCreated(w, n),
    checksNone: w.noCi,
    checksPass: (n) => p.checksPass(w, n),
    checksFail: (n) => p.checksFail(w, n),
    checksPending: (n) => p.checksPending(w, n),
    mergeTitle: (n) => p.mergeTitle(w, n),
    mergeBlockedFail: p.mergeBlockedFail(w),
    mergeBlockedPending: p.mergeBlockedPending(w),
    mergeBlockedDraft: p.mergeBlockedDraft(w),
    merged: p.merged(w),
    repoNeedsChoice: p.repoNeedsChoice(w),
    repoAt: (slug) => p.repoAt(w, slug),
    repoPickTitle: p.repoPickTitle(w),
    commentConfirm: p.commentConfirm(w),
    monitorStart: p.monitorStart(w),
    reviewLoading: p.reviewLoading(w),
    methods: kind === "gitlab" ? p.gitlabMethods : t.forge.methods,
    prOpenTip: (n, title) => p.prOpenTip(w, n, title),
    sidebarPr: (n, state) => p.sidebarPr(w, n, state),
  };
}

/** 会话时间线里 agent 动过的 PR / MR 链接的路径段（仓库首页之后）：GitHub `/pull/`、GitLab `/-/merge_requests/`、
 *  Gitea `/pulls/`、Bitbucket `/pull-requests/`。 */
export function prPathSegment(kind: ForgeKind): string {
  switch (kind) {
    case "gitlab": return "/-/merge_requests/";
    case "gitea": return "/pulls/";
    case "bitbucket": return "/pull-requests/";
    default: return "/pull/";
  }
}

/** 从 PR / MR 链接认平台（侧栏、标题栏只拿到链接与 forge 字段时兜底）。 */
export function kindOfUrl(url: string): ForgeKind {
  if (url.includes("/-/merge_requests/")) return "gitlab";
  if (url.includes("/pull-requests/")) return "bitbucket";
  if (/\/pulls\/\d+/.test(url)) return "gitea";
  return "github";
}
