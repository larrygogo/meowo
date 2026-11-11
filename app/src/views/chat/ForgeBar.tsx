/// 「更改」面板的推送 / PR / CI / 合并区块（二期包 5，后端 forge.rs）。挂在提交框下面，只对选中的仓库。
///
/// - 推送：显示还有几个提交没推（RepoChangesDto.summary.unpushed，本地计数），没上游时后端自动设上游。
/// - 装了 gh：显示 PR 与每一项检查，能开 PR、能合并。没装或没登录：开 PR 改成打开 GitHub 的 compare 网页，
///   不显示 CI、不能在应用里合并（leader 关卡 Q-no-gh 选 1）。
/// - 轮询只在面板可见、窗口可见时进行，每 60 秒一次；后端按仓库缓存 60 秒，桌面和手机共用（规格「每仓库
///   ≥60s 一次、隐藏时 0 次」）。
/// - 合并确认框写明合进哪、用什么方式（只列仓库允许的，默认上次用的），CI 有失败或在跑时不能点，有冲突时
///   引导去网页。手机上多一道「确认合并」。合并后会话所在 worktree 可以清理就提示，不自动删。
/// - 多个 GitHub 远端、gh 没定默认仓库时（做空第二轮）：在这里列出远端让用户选 PR 开在哪个仓库，后端按仓库
///   记住；没选之前开 PR / 合并不可用。不让用户去终端跑 gh repo set-default（产品标准：别让用户去终端收拾）。

import { useCallback, useEffect, useRef, useState } from "react";
import {
  forgeChooseRepo,
  getForgeStatus,
  ghPrCreate,
  ghPrMerge,
  gitPush,
  openLink,
  worktreeStatus,
  type ForgeStatusDto,
  type PullRequestDto,
  type RepoChangesDto,
  type WorktreeStatusDto,
} from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { remoteUi } from "../../remoteMode";
import { runWorktreeRemoval } from "../../worktreeCleanup";
import { AppModal } from "../AppModal";
import { normPath } from "./gitDiff";

/// 轮询间隔：每个仓库最多 60 秒查一次 GitHub。
export const FORGE_POLL_MS = 60_000;

const sameRoot = (a: string, b: string) => normPath(a).toLowerCase() === normPath(b).toLowerCase();

/// 拉 forge 状态：挂载时一次，之后可见时每 60 秒一次；窗口隐藏时不拉，回到可见或重新聚焦时距上次满 60 秒才补一次
///（聚焦时带 force，顺手让后端重查 gh 登录状态）。active=false（面板收起）时整个停掉。
export function useForgeStatus(root: string, sessionCwd: string | null, active: boolean) {
  const [forge, setForge] = useState<ForgeStatusDto | null>(null);
  const lastRef = useRef(0);
  const reqRef = useRef(0);
  const load = useCallback((force: boolean) => {
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
    lastRef.current = Date.now();
    const req = ++reqRef.current;
    getForgeStatus(root, force, sessionCwd)
      .then((dto) => { if (req === reqRef.current) setForge(dto); })
      .catch(() => {});
  }, [root, sessionCwd]);
  useEffect(() => {
    setForge(null);
    if (!active) return;
    load(false);
    const timer = window.setInterval(() => load(false), FORGE_POLL_MS);
    const stale = () => Date.now() - lastRef.current >= FORGE_POLL_MS;
    const onVisible = () => { if (document.visibilityState === "visible" && stale()) load(false); };
    const onFocus = () => { if (stale()) load(true); };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onFocus);
      reqRef.current += 1;
    };
  }, [active, load]);
  return { forge, refresh: load };
}

type Busy = "push" | "pr" | "merge" | "repo" | null;

/// 合并按钮能不能点、不能点时说什么。
export function mergeBlock(t: ReturnType<typeof useT>, forge: ForgeStatusDto, pr: PullRequestDto): string | null {
  // 草稿 PR GitHub 不让合并：按钮灰着要说清楚为什么（做空审查：以前只是灰着，没有原因）。
  if (pr.state === "draft") return t.forge.mergeBlockedDraft;
  if (forge.repoNeedsChoice) return t.forge.repoNeedsChoice;
  if (forge.ghTooOld) return t.forge.mergeTooOld;
  if (forge.mergeMethods.length === 0) return t.forge.mergeNoMethods;
  if (pr.checksSummary === "fail") return t.forge.mergeBlockedFail;
  // GitHub 还在算能不能合（UNKNOWN）同「检查在跑」：等一轮再合（定案设计）。
  if (pr.checksSummary === "pending" || pr.mergeable === "UNKNOWN") return t.forge.mergeBlockedPending;
  return null;
}

const isConflict = (pr: PullRequestDto) => pr.mergeable === "CONFLICTING" || pr.mergeState === "DIRTY";

export function ForgeBar({ repo, sessionCwd, sessionId, active, onChanged }: {
  repo: RepoChangesDto;
  /** 仓库根不是会话目录本身时带上会话目录（手机桥按它收口）。 */
  sessionCwd: string | null;
  /** 合并后查会话所在 worktree 能不能清理（worktree_status 只认会话 id）。没有就不提示。 */
  sessionId?: number | null;
  /** 面板可见（收起时不轮询）。 */
  active: boolean;
  onChanged: () => void;
}) {
  const t = useT();
  const root = repo.root;
  const { forge, refresh } = useForgeStatus(root, sessionCwd, active);
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorLink, setErrorLink] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [prForm, setPrForm] = useState<{ title: string; body: string; draft: boolean } | null>(null);
  const [mergeDlg, setMergeDlg] = useState<{ pr: PullRequestDto; method: string; step: 1 | 2 } | null>(null);
  const [cleanup, setCleanup] = useState<WorktreeStatusDto | null>(null);
  const [cleanState, setCleanState] = useState<"idle" | "busy">("idle");
  const [repoDlg, setRepoDlg] = useState<string | null>(null);

  const summary = repo.summary;
  const branch = summary.branch;
  // 旧后端没有 unpushed：有上游时 ahead 就是未推数。
  const unpushed = summary.unpushed ?? (summary.upstream ? summary.ahead : null) ?? 0;
  // 推去哪以后端的判定为准（git_target::resolve）：pushDefault 指向 fork 时上游是 origin/feat，推的却是
  // fork/feat（做空第二轮）。旧后端没有 pushTarget 时退回上游。
  const pushTarget = summary.pushTarget ?? summary.upstream ?? null;
  // 没上游、或上游是默认分支（后端按没上游下发）：推送会新建远端分支并设为上游。
  const pushNew = !summary.upstream;

  const fail = (e: unknown) => {
    const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
    setError(formatBackendError(e, t.locale));
    // 冲突带着网页链接：给一个「在 GitHub 打开」去处理。
    const link = raw.startsWith("gh/merge_conflict: ") ? raw.slice("gh/merge_conflict: ".length).trim() : "";
    setErrorLink(link.startsWith("https://") ? link : null);
  };
  const begin = (what: Busy) => {
    setBusy(what);
    setError(null);
    setErrorLink(null);
    setNotice(null);
  };

  const push = async () => {
    begin("push");
    try {
      const result = await gitPush(root, sessionCwd);
      setNotice(t.forge.pushed(result.pushed, `${result.remote}/${result.branch}`));
      refresh(true);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(null);
      onChanged();
    }
  };

  // 没装 gh：有没推的提交先推，不然 compare 网页上看不到这些提交。
  const openCompare = async (url: string) => {
    if (unpushed > 0) {
      begin("push");
      try {
        await gitPush(root, sessionCwd);
      } catch (e) {
        fail(e);
        setBusy(null);
        return;
      }
      setBusy(null);
      onChanged();
    }
    void openLink(url).catch(() => {});
  };

  const createPr = async () => {
    if (!prForm || !forge?.prDefaults) return;
    const form = prForm;
    begin("pr");
    try {
      await ghPrCreate(root, { title: form.title, body: form.body, base: forge.prDefaults.base, draft: form.draft }, sessionCwd);
      setPrForm(null);
    } catch (e) {
      // 已经有 PR：关掉表单，刷新就能看到它。
      if (String(e).startsWith("gh/pr_exists")) setPrForm(null);
      // 状态刷新前别处改了远端：后端说要先选仓库，直接把选择框打开。
      if (String(e) === "gh/ambiguous_repo") openRepoPicker();
      fail(e);
    } finally {
      setBusy(null);
      refresh(true);
      onChanged();
    }
  };

  const doMerge = async () => {
    if (!mergeDlg) return;
    const { pr, method } = mergeDlg;
    // 手机上多一道「确认合并」（leader 定：手机合并要二次确认）。
    if (remoteUi() && mergeDlg.step === 1) {
      setMergeDlg({ ...mergeDlg, step: 2 });
      return;
    }
    setMergeDlg(null);
    begin("merge");
    try {
      const result = await ghPrMerge(root, pr.number, method, pr.headOid ?? "", sessionCwd);
      setNotice(result.merged ? t.forge.merged : result.queued ? t.forge.queued : null);
      if (result.merged && sessionId != null) {
        // 合并后提示清理，不自动删（规格）。只提示本会话自己的 worktree，且必须正是刚合并的这个仓库。
        worktreeStatus(sessionId)
          .then((status) => { if (status.eligible && status.path && sameRoot(status.path, root)) setCleanup(status); })
          .catch(() => {});
      }
    } catch (e) {
      if (String(e) === "gh/ambiguous_repo") openRepoPicker();
      fail(e);
    } finally {
      setBusy(null);
      refresh(true);
      onChanged();
    }
  };

  const openRepoPicker = () => {
    if (!forge || forge.repoChoices.length === 0) return;
    setPrForm(null);
    setRepoDlg(forge.repoRemote ?? forge.repoChoices[0].remote);
  };
  const chooseRepo = async () => {
    if (!repoDlg) return;
    const remote = repoDlg;
    setRepoDlg(null);
    begin("repo");
    try {
      await forgeChooseRepo(root, remote, sessionCwd);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(null);
      refresh(true);
    }
  };

  const clean = () => {
    if (!cleanup || sessionId == null) return;
    setCleanState("busy");
    void runWorktreeRemoval(t, sessionId, cleanup).then((outcome) => {
      setCleanState("idle");
      if (outcome.kind === "removed") {
        setCleanup(null);
        setNotice(outcome.branchDeleted ? t.worktree.removed : t.worktree.removedKeepBranch);
        onChanged();
      } else if (outcome.kind === "failed") {
        setError(outcome.message);
      }
    });
  };

  const pr = forge?.gh === "ok" ? forge.pr ?? null : null;
  const ghOk = forge?.gh === "ok";
  const offGh = forge?.gh === "missing" || forge?.gh === "unauthenticated";
  const showPush = !!branch && unpushed > 0;
  const showPr = !!forge && !!branch && (forge.gh === "ok" || offGh);
  if (!showPush && !showPr && !error && !notice && !cleanup) return null;

  const checkCounts = (p: PullRequestDto) => ({
    total: p.checks?.length ?? 0,
    fail: p.checks?.filter((c) => c.state === "fail").length ?? 0,
    pending: p.checks?.filter((c) => c.state === "pending").length ?? 0,
  });
  const checksLine = (p: PullRequestDto) => {
    const n = checkCounts(p);
    switch (p.checksSummary) {
      case "none": return t.forge.checksNone;
      case "fail": return t.forge.checksFail(n.fail);
      case "pending": return t.forge.checksPending(n.pending);
      case "pass": return t.forge.checksPass(n.total);
      default: return "";
    }
  };
  const block = pr && forge ? mergeBlock(t, forge, pr) : null;
  const openMerge = (p: PullRequestDto) => {
    if (!forge) return;
    const allowed = forge.mergeMethods;
    const method = forge.lastMergeMethod && allowed.includes(forge.lastMergeMethod) ? forge.lastMergeMethod : allowed[0];
    setMergeDlg({ pr: p, method, step: 1 });
  };

  return (
    <div className="scm-forge" data-testid="scm-forge">
      {showPush && (
        <button type="button" className="scm-forge-btn" disabled={busy !== null}
          data-tip={pushTarget ? (pushNew ? t.forge.pushNewTip(pushTarget) : t.forge.pushTip(pushTarget)) : t.forge.publishTip} onClick={() => void push()}>
          <PushIcon />
          {busy === "push" ? t.forge.pushing : t.forge.push(unpushed)}
        </button>
      )}

      {showPr && pr && (
        <div className="scm-pr" data-testid="scm-pr">
          <button type="button" className={`scm-pr-head is-${pr.state}`} data-tip={pr.url} onClick={() => void openLink(pr.url).catch(() => {})}>
            <span className="scm-pr-state">{t.forge.prState[pr.state] ?? pr.state}</span>
            <span className="scm-pr-title">#{pr.number} {pr.title}</span>
          </button>
          {pr.state !== "merged" && pr.state !== "closed" && (
            <>
              <div className={`scm-checks-line is-${pr.checksSummary ?? "none"}`}>
                <span>{checksLine(pr)}</span>
                <button type="button" className="scm-icon-btn" aria-label={t.forge.refresh} data-tip={t.forge.refresh} onClick={() => refresh(true)}><RefreshIcon /></button>
              </div>
              {(pr.checks?.length ?? 0) > 0 && (
                <ul className="scm-checks" aria-label={t.forge.checks}>
                  {pr.checks!.map((check, i) => (
                    <li key={`${check.workflow ?? ""}\u0000${check.name}\u0000${i}`} className={`scm-check is-${check.state}`}>
                      <CheckStateIcon state={check.state} label={t.forge.checkState[check.state] ?? check.state} />
                      {check.url
                        ? <button type="button" className="scm-check-name" data-tip={check.url} onClick={() => void openLink(check.url!).catch(() => {})}>{check.name}</button>
                        : <span className="scm-check-name">{check.name}</span>}
                      {check.workflow && <span className="scm-check-wf">{check.workflow}</span>}
                    </li>
                  ))}
                </ul>
              )}
              {pr.queued
                ? <div className="scm-notice" role="status">{t.forge.queued}</div>
                : isConflict(pr)
                  ? <button type="button" className="scm-forge-btn is-ghost" onClick={() => void openLink(pr.url).catch(() => {})}>{t.forge.mergeConflict}</button>
                  : (
                    <button type="button" className="scm-forge-btn" data-testid="scm-merge" disabled={busy !== null || !!block || pr.state === "draft"}
                      data-tip={block ?? undefined} onClick={() => openMerge(pr)}>
                      {busy === "merge" ? t.forge.merging : t.forge.merge}
                    </button>
                  )}
              {block && !pr.queued && !isConflict(pr) && <div className="scm-notice">{block}</div>}
            </>
          )}
        </div>
      )}

      {showPr && forge && forge.repoChoices.length > 0 && (
        forge.repoNeedsChoice
          ? (
            <div className="scm-notice scm-cleanup" data-testid="scm-repo-choice">
              <span>{t.forge.repoNeedsChoice}</span>
              <button type="button" className="scm-link-btn" disabled={busy !== null} onClick={openRepoPicker}>{t.forge.repoPick}</button>
            </div>
          )
          : (
            <div className="scm-notice scm-cleanup" data-testid="scm-repo-choice">
              <span>{t.forge.repoAt(forge.repoChoices.find((c) => c.remote === forge.repoRemote)?.slug ?? forge.repoRemote ?? "")}</span>
              <button type="button" className="scm-link-btn" disabled={busy !== null} onClick={openRepoPicker}>{t.forge.repoChange}</button>
            </div>
          )
      )}

      {showPr && !pr && forge?.prDefaults && (
        ghOk
          ? (
            <button type="button" className="scm-forge-btn is-ghost" disabled={busy !== null}
              onClick={() => forge.repoNeedsChoice ? openRepoPicker() : setPrForm({ title: forge.prDefaults!.title, body: forge.prDefaults!.body, draft: false })}>
              {t.forge.openPr}
            </button>
          )
          : forge.compareUrl && (
            <button type="button" className="scm-forge-btn is-ghost" disabled={busy !== null} data-tip={forge.compareUrl}
              onClick={() => void openCompare(forge.compareUrl!)}>
              {t.forge.openOnGithub}
            </button>
          )
      )}
      {showPr && offGh && <div className="scm-notice">{forge?.gh === "missing" ? t.forge.ghMissing : t.forge.ghUnauthed}</div>}

      {error && (
        <div className="scm-error" role="alert">
          {error}
          {errorLink && <button type="button" className="scm-link-btn" onClick={() => void openLink(errorLink).catch(() => {})}>{t.forge.openOnGithub}</button>}
        </div>
      )}
      {notice && <div className="scm-notice" role="status">{notice}</div>}
      {cleanup && sessionId != null && (
        <div className="scm-notice scm-cleanup" role="status">
          <span>{t.forge.cleanupHint}</span>
          <button type="button" className="scm-link-btn" disabled={cleanState === "busy"} onClick={clean}>
            {cleanState === "busy" ? t.worktree.removing : t.worktree.remove}
          </button>
        </div>
      )}

      {prForm && forge?.prDefaults && (
        <AppModal label={t.forge.prFormTitle(forge.prDefaults.base)} onClose={() => setPrForm(null)}>
          <div className="chat-modal-title">{t.forge.prFormTitle(forge.prDefaults.base)}</div>
          <label className="ns-field">
            <span className="ns-label">{t.forge.prTitle}</span>
            <input className="ns-input" value={prForm.title} autoFocus onChange={(e) => setPrForm({ ...prForm, title: e.target.value })} />
          </label>
          <label className="ns-field">
            <span className="ns-label">{t.forge.prBody}</span>
            <textarea className="ns-input scm-pr-body" rows={5} value={prForm.body} onChange={(e) => setPrForm({ ...prForm, body: e.target.value })} />
          </label>
          <label className="scm-pr-draft">
            <input type="checkbox" checked={prForm.draft} onChange={(e) => setPrForm({ ...prForm, draft: e.target.checked })} />
            {t.forge.prDraft}
          </label>
          {unpushed > 0 && <div className="scm-discard-repo">{t.forge.prPushFirst(unpushed)}</div>}
          <div className="chat-modal-actions">
            <button type="button" className="ns-btn" onClick={() => setPrForm(null)}>{t.newSession.cancel}</button>
            <button type="button" className="ns-btn btn-primary" disabled={busy !== null || !prForm.title.trim()} onClick={() => void createPr()}>
              {busy === "pr" ? t.forge.prCreating : t.forge.prCreate}
            </button>
          </div>
        </AppModal>
      )}

      {repoDlg && forge && (
        <AppModal label={t.forge.repoPickTitle} onClose={() => setRepoDlg(null)}>
          <div className="chat-modal-title">{t.forge.repoPickTitle}</div>
          <div className="ns-field" role="radiogroup" aria-label={t.forge.repoPickTitle}>
            {forge.repoChoices.map((choice) => (
              <label key={choice.remote} className="scm-merge-method">
                <input type="radio" name="forge-repo" value={choice.remote} checked={repoDlg === choice.remote}
                  onChange={() => setRepoDlg(choice.remote)} />
                {choice.slug} <span className="scm-check-wf">{choice.remote}</span>
              </label>
            ))}
          </div>
          <div className="scm-discard-repo">{t.forge.repoPickNote}</div>
          <div className="chat-modal-actions">
            <button type="button" className="ns-btn" onClick={() => setRepoDlg(null)}>{t.newSession.cancel}</button>
            <button type="button" className="ns-btn btn-primary" data-testid="repo-confirm" onClick={() => void chooseRepo()}>{t.forge.repoConfirm}</button>
          </div>
        </AppModal>
      )}

      {mergeDlg && forge && (
        <AppModal label={t.forge.mergeTitle(mergeDlg.pr.number)} onClose={() => setMergeDlg(null)}>
          <div className="chat-modal-title">{t.forge.mergeTitle(mergeDlg.pr.number)}</div>
          <div className="scm-merge-into" data-testid="merge-into">{t.forge.mergeInto(mergeDlg.pr.headRef ?? branch ?? "", mergeDlg.pr.baseRef ?? forge.prDefaults?.base ?? "main")}</div>
          <div className="ns-field" role="radiogroup" aria-label={t.forge.mergeMethod}>
            <span className="ns-label">{t.forge.mergeMethod}</span>
            {forge.mergeMethods.map((method) => (
              <label key={method} className="scm-merge-method">
                <input type="radio" name="merge-method" value={method} checked={mergeDlg.method === method}
                  onChange={() => setMergeDlg({ ...mergeDlg, method, step: 1 })} />
                {t.forge.methods[method] ?? method}
              </label>
            ))}
          </div>
          <div className={`scm-checks-line is-${mergeDlg.pr.checksSummary ?? "none"}`}>{t.forge.mergeCi(checksLine(mergeDlg.pr))}</div>
          {mergeDlg.step === 2 && <div className="scm-discard-note" role="alert">{t.forge.mergeAgainNote}</div>}
          <div className="chat-modal-actions">
            <button type="button" className="ns-btn" autoFocus onClick={() => setMergeDlg(null)}>{t.newSession.cancel}</button>
            <button type="button" className="ns-btn btn-primary" data-testid="merge-confirm" disabled={!!mergeBlock(t, forge, mergeDlg.pr)} onClick={() => void doMerge()}>
              {mergeDlg.step === 2 ? t.forge.mergeConfirmAgain : t.forge.merge}
            </button>
          </div>
        </AppModal>
      )}
    </div>
  );
}

function CheckStateIcon({ state, label }: { state: string; label: string }) {
  const path = state === "pass" ? "M5 12.5l4.5 4.5L19 7.5" : state === "fail" ? "M6 6l12 12M18 6L6 18" : state === "skipped" ? "M5 12h14" : "M12 7v5l3 2";
  return (
    <svg className="scm-check-icon" role="img" aria-label={label} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round">
      {state === "pending" && <circle cx="12" cy="12" r="8.5" />}
      <path d={path} />
    </svg>
  );
}

function PushIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 19V5" /><path d="M6 11l6-6 6 6" />
    </svg>
  );
}

function RefreshIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" />
    </svg>
  );
}
