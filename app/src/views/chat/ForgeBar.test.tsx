// 「更改」面板的推送 / PR / CI / 合并（二期包 5，验收 #19–#22、#24、#25）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ForgeStatusDto, GitDiffSummaryDto, PullRequestDto, RepoChangesDto } from "../../api";
import { formatBackendError } from "../../i18n/errors";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import { ForgeBar, FORGE_POLL_MS } from "./ForgeBar";
import { ArchivedWorktreePrompt } from "../../worktreeCleanup";
import { SESSION_ARCHIVED_EVENT } from "../../hooks/useSessionActions";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));
let remote = false;
vi.mock("../../remoteMode", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../remoteMode")>()),
  remoteUi: () => remote,
}));

const ROOT = "C:/w/meowo-wt";
const summary = (over: Partial<GitDiffSummaryDto> = {}): GitDiffSummaryDto => ({
  gitAvailable: true, isRepo: true, branch: "meowo/drill", ahead: null, behind: null, files: [], upstream: undefined, unpushed: 2, ...over,
});
const repo = (over: Partial<GitDiffSummaryDto> = {}): RepoChangesDto => ({
  root: ROOT, name: "meowo-wt", isCwdRepo: false, summary: summary(over), base: null, baseLabel: null, committed: [],
});
const HEAD = "1111111111111111111111111111111111111111";
const pr = (over: Partial<PullRequestDto> = {}): PullRequestDto => ({
  number: 12, state: "open", url: "https://github.com/o/r/pull/12", title: "feat: drill", baseRef: "main", headRef: "meowo/drill",
  headOid: HEAD, mergeable: "MERGEABLE", mergeState: "CLEAN", checksSummary: "pass",
  checks: [
    { name: "frontend", workflow: "CI", state: "pass", url: "https://github.com/o/r/actions/runs/1" },
    { name: "check (macos-latest)", workflow: "CI", state: "pass", url: null },
  ],
  ...over,
});
const forge = (over: Partial<ForgeStatusDto> = {}): ForgeStatusDto => ({
  gh: "ok", ghTooOld: false, repoUrl: "https://github.com/o/r", compareUrl: "https://github.com/o/r/compare/main...meowo/drill?expand=1",
  mergeMethods: ["squash", "rebase"], lastMergeMethod: "rebase", pr: null,
  prDefaults: { title: "feat: drill", body: "- feat: drill", base: "main" }, ...over,
});

type Handler = (command: string, args: Record<string, unknown>) => unknown;
let handler: Handler = () => null;
let status: ForgeStatusDto = forge();
const calls = (command: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === command);

beforeEach(() => {
  remote = false;
  status = forge();
  handler = () => null;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const a = (args ?? {}) as Record<string, unknown>;
    const custom = handler(command, a);
    if (custom !== null) {
      if (custom instanceof Error) throw custom.message;
      return custom;
    }
    if (command === "git_forge_status") return status;
    if (command === "git_push_start") return "push-1";
    if (command === "git_job_status") return { state: "ok", push: { remote: "origin", branch: "meowo/drill", setUpstream: true, pushed: 2 }, error: null };
    return null;
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
});

function renderBar(over: Partial<GitDiffSummaryDto> = {}, extra: Partial<Parameters<typeof ForgeBar>[0]> = {}) {
  const onChanged = vi.fn();
  const utils = render(<ForgeBar repo={repo(over)} sessionCwd="C:/w/meowo" sessionId={7} active onChanged={onChanged} {...extra} />);
  return { ...utils, onChanged };
}

describe("#19 推送", () => {
  it("显示未推提交数；没上游时提示会设上游；点了走后台任务并带上会话目录", async () => {
    const { onChanged } = renderBar();
    const button = await screen.findByRole("button", { name: /推送 ↑2/ });
    expect(button.getAttribute("data-tip")).toBe(zh.forge.publishTip);
    fireEvent.click(button);
    await screen.findByText(zh.forge.pushed(2, "origin/meowo/drill"));
    expect(calls("git_push_start")[0][1]).toEqual({ cwd: ROOT, sessionCwd: "C:/w/meowo" });
    expect(calls("git_job_status")[0][1]).toEqual({ jobId: "push-1" });
    expect(onChanged).toHaveBeenCalled();
  });

  // 做空审查：上游是 origin/main 的功能分支，后端改推同名分支并新建上游，提示要据实写出推去哪。
  it("上游不是同名分支：提示写明推到 origin/<分支> 并新建", async () => {
    renderBar({ upstream: undefined, pushTarget: "origin/meowo/drill", unpushed: 2 });
    const button = await screen.findByRole("button", { name: /推送 ↑2/ });
    expect(button.getAttribute("data-tip")).toBe(zh.forge.pushNewTip("origin/meowo/drill"));
    expect(zh.forge.pushNewTip("origin/x")).toBe("推送到 origin/x（新建）");
    expect(en.forge.pushNewTip("origin/x")).toBe("Push to origin/x (new branch)");
  });

  it("有上游时写明推到哪；没有未推提交不显示按钮", async () => {
    const { unmount } = renderBar({ upstream: "origin/meowo/drill", unpushed: 1, ahead: 1 });
    expect((await screen.findByRole("button", { name: /推送 ↑1/ })).getAttribute("data-tip")).toBe(zh.forge.pushTip("origin/meowo/drill"));
    unmount();
    renderBar({ upstream: "origin/meowo/drill", unpushed: 0, ahead: 0 });
    await waitFor(() => expect(calls("git_forge_status").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button", { name: /推送/ })).toBeNull();
  });

  it("认证失败、被拒、被保护分支、钩子等错误给中英文案", async () => {
    handler = (command) => (command === "git_job_status" ? { state: "failed", push: null, error: "git/push_auth" } : null);
    renderBar();
    fireEvent.click(await screen.findByRole("button", { name: /推送 ↑2/ }));
    expect((await screen.findByRole("alert")).textContent).toContain("推送认证失败，检查 git 凭据");
    const cases: [string, string, string][] = [
      ["git/push_auth", "推送认证失败，检查 git 凭据", "Push failed to sign in. Check your git credentials."],
      ["git/push_rejected", "远端有新提交，先拉取再推", "The remote has new commits. Pull first."],
      ["git/push_protected: GH006: Protected branch update failed", "分支受保护，推送被拒: GH006: Protected branch update failed", "The branch is protected. Push rejected.: GH006: Protected branch update failed"],
      ["git/push_hook: lint failed", "推送钩子没通过: lint failed", "A pre-push hook failed: lint failed"],
      ["git/no_remote", "没有远端仓库，推不了", "No remote to push to"],
      ["git/push_network", "连不上远端，检查网络", "Can't reach the remote. Check the network."],
      ["git/push_timeout", "推送超时", "Push timed out"],
    ];
    for (const [raw, zhText, enText] of cases) {
      expect(formatBackendError(raw, "zh")).toBe(zhText);
      expect(formatBackendError(raw, "en")).toBe(enText);
    }
  });
});

describe("#20 开 PR", () => {
  it("有 gh：表单带默认标题正文，提交走 gh_pr_create", async () => {
    renderBar();
    fireEvent.click(await screen.findByRole("button", { name: zh.forge.openPr }));
    const dialog = await screen.findByRole("dialog");
    expect((within(dialog).getByLabelText(zh.forge.prTitle) as HTMLInputElement).value).toBe("feat: drill");
    expect((within(dialog).getByLabelText(zh.forge.prBody) as HTMLTextAreaElement).value).toBe("- feat: drill");
    expect(within(dialog).getByText(zh.forge.prPushFirst(2))).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: zh.forge.prCreate }));
    await waitFor(() => expect(calls("gh_pr_create").length).toBe(1));
    expect(calls("gh_pr_create")[0][1]).toEqual({ cwd: ROOT, title: "feat: drill", body: "- feat: drill", base: "main", draft: false, sessionCwd: "C:/w/meowo" });
  });

  it("没装 gh：先推送再打开 compare 网页，并说明看不到 CI", async () => {
    status = forge({ gh: "missing", mergeMethods: [] });
    renderBar();
    expect(await screen.findByText(zh.forge.ghMissing)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.forge.openOnGithub }));
    await waitFor(() => expect(calls("open_link").length).toBe(1));
    expect(calls("open_link")[0][1]).toEqual({ url: "https://github.com/o/r/compare/main...meowo/drill?expand=1" });
    expect(calls("git_push_start").length).toBe(1);
    expect(calls("gh_pr_create").length).toBe(0);
  });
});

describe("#21 CI 检查与轮询", () => {
  it("显示每一项检查的结果", async () => {
    status = forge({
      pr: pr({
        checksSummary: "fail",
        checks: [
          { name: "frontend", workflow: "CI", state: "pass", url: null },
          { name: "ESLint v7", workflow: "Lint", state: "fail", url: null },
          { name: "e2e", workflow: "CI", state: "pending", url: null },
          { name: "discord", workflow: "Notify", state: "skipped", url: null },
        ],
      }),
    });
    renderBar();
    const list = await screen.findByRole("list", { name: zh.forge.checks });
    const items = within(list).getAllByRole("listitem");
    expect(items.map((li) => [li.textContent, li.querySelector("[role=img]")?.getAttribute("aria-label")])).toEqual([
      ["frontendCI", "通过"], ["ESLint v7Lint", "失败"], ["e2eCI", "进行中"], ["discordNotify", "跳过"],
    ]);
    expect(screen.getByText(zh.forge.checksFail(1))).toBeTruthy();
  });

  it("可见时每 60 秒一次，隐藏时 0 次，回到可见补一次；面板收起不查", async () => {
    vi.useFakeTimers();
    let state: DocumentVisibilityState = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
    renderBar();
    await act(async () => {});
    expect(calls("git_forge_status").length).toBe(1);
    await act(async () => { vi.advanceTimersByTime(FORGE_POLL_MS - 1); });
    expect(calls("git_forge_status").length).toBe(1);
    await act(async () => { vi.advanceTimersByTime(1); });
    expect(calls("git_forge_status").length).toBe(2);
    state = "hidden";
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => { vi.advanceTimersByTime(FORGE_POLL_MS * 5); });
    expect(calls("git_forge_status").length).toBe(2);
    state = "visible";
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => {});
    expect(calls("git_forge_status").length).toBe(3);
    // 聚焦距上次不满 60 秒：不查。
    window.dispatchEvent(new Event("focus"));
    await act(async () => {});
    expect(calls("git_forge_status").length).toBe(3);
    cleanup();
    vi.mocked(invoke).mockClear();
    renderBar({}, { active: false });
    await act(async () => { vi.advanceTimersByTime(FORGE_POLL_MS * 3); });
    expect(calls("git_forge_status").length).toBe(0);
  });
});

describe("#22 合并", () => {
  it("确认框写明目标分支与方式，只列允许的、默认上次的；合并带头提交；合并后提示清理不自动删", async () => {
    status = forge({ pr: pr() });
    handler = (command) => {
      if (command === "gh_pr_merge") return { merged: true, queued: false };
      if (command === "worktree_status") return { path: ROOT, branch: "meowo/drill", prNumber: 12, eligible: true, reason: null, count: null, ignored: [] };
      return null;
    };
    renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByTestId("merge-into").textContent).toBe("把 meowo/drill 合进 main");
    const radios = within(dialog).getAllByRole("radio") as HTMLInputElement[];
    expect(radios.map((r) => [r.value, r.checked])).toEqual([["squash", false], ["rebase", true]]);
    expect(within(dialog).queryByText(zh.forge.methods.merge)).toBeNull();
    expect(within(dialog).getByText(zh.forge.mergeCi(zh.forge.checksPass(2)))).toBeTruthy();
    fireEvent.click(radios[0]);
    fireEvent.click(within(dialog).getByTestId("merge-confirm"));
    await waitFor(() => expect(calls("gh_pr_merge").length).toBe(1));
    expect(calls("gh_pr_merge")[0][1]).toEqual({ cwd: ROOT, number: 12, method: "squash", expectedHeadOid: HEAD, sessionCwd: "C:/w/meowo" });
    expect(await screen.findByText(zh.forge.cleanupHint)).toBeTruthy();
    expect(screen.getByRole("button", { name: zh.worktree.remove })).toBeTruthy();
    expect(calls("worktree_status")[0][1]).toEqual({ sessionId: 7 });
    expect(calls("remove_session_worktree").length).toBe(0);
  });

  it("CI 失败或在跑时不能点；冲突引导去网页", async () => {
    status = forge({ pr: pr({ checksSummary: "fail" }) });
    const { unmount } = renderBar();
    expect((await screen.findByTestId("scm-merge") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getAllByText(zh.forge.mergeBlockedFail).length).toBeGreaterThan(0);
    unmount();
    status = forge({ pr: pr({ checksSummary: "pending" }) });
    const second = renderBar();
    expect((await screen.findByTestId("scm-merge") as HTMLButtonElement).disabled).toBe(true);
    second.unmount();
    status = forge({ pr: pr({ mergeable: "UNKNOWN" }) });
    const third = renderBar();
    expect((await screen.findByTestId("scm-merge") as HTMLButtonElement).disabled).toBe(true);
    third.unmount();
    status = forge({ pr: pr({ mergeable: "CONFLICTING", mergeState: "DIRTY" }) });
    renderBar();
    fireEvent.click(await screen.findByRole("button", { name: zh.forge.mergeConflict }));
    await waitFor(() => expect(calls("open_link")[0]?.[1]).toEqual({ url: "https://github.com/o/r/pull/12" }));
    expect(screen.queryByTestId("scm-merge")).toBeNull();
  });

  // 做空审查：草稿 PR 的合并按钮是灰的，却没有任何原因。
  it("草稿 PR：合并按钮不能点，并说明原因", async () => {
    status = forge({ pr: pr({ state: "draft" }) });
    renderBar();
    expect((await screen.findByTestId("scm-merge") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getAllByText(zh.forge.mergeBlockedDraft).length).toBeGreaterThan(0);
    for (const text of [zh.forge.mergeBlockedDraft, en.forge.mergeBlockedDraft]) {
      expect(text).toBeTruthy();
      expect(text).not.toMatch(/[—–;；]/);
    }
    expect(zh.forge.mergeBlockedDraft.length).toBeLessThanOrEqual(20);
  });

  it("合并时后端说有冲突：错误旁边给网页入口；进合并队列时说已入队不说已合并", async () => {
    status = forge({ pr: pr() });
    handler = (command) => (command === "gh_pr_merge" ? new Error("gh/merge_conflict: https://github.com/o/r/pull/12") : null);
    const { unmount } = renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    fireEvent.click(within(await screen.findByRole("dialog")).getByTestId("merge-confirm"));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("有冲突，去网页处理");
    fireEvent.click(within(alert).getByRole("button", { name: zh.forge.openOnGithub }));
    await waitFor(() => expect(calls("open_link").length).toBe(1));
    unmount();
    handler = (command) => (command === "gh_pr_merge" ? { merged: false, queued: true } : null);
    renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    fireEvent.click(within(await screen.findByRole("dialog")).getByTestId("merge-confirm"));
    expect(await screen.findByText(zh.forge.queued)).toBeTruthy();
    expect(screen.queryByText(zh.forge.merged)).toBeNull();
    expect(calls("worktree_status").length).toBe(0);
  });
});

describe("#25 手机", () => {
  it("合并多一道确认，CI 不全绿时不能点", async () => {
    remote = true;
    status = forge({ pr: pr() });
    handler = (command) => (command === "gh_pr_merge" ? { merged: true, queued: false } : null);
    const { unmount } = renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByTestId("merge-confirm"));
    expect(within(dialog).getByText(zh.forge.mergeAgainNote)).toBeTruthy();
    expect(within(dialog).getByTestId("merge-confirm").textContent).toBe(zh.forge.mergeConfirmAgain);
    expect(calls("gh_pr_merge").length).toBe(0);
    fireEvent.click(within(dialog).getByTestId("merge-confirm"));
    await waitFor(() => expect(calls("gh_pr_merge").length).toBe(1));
    unmount();
    status = forge({ pr: pr({ checksSummary: "pending" }) });
    renderBar();
    expect((await screen.findByTestId("scm-merge") as HTMLButtonElement).disabled).toBe(true);
  });

  it("手机页新增文案不出现让用户去电脑操作的出口", () => {
    const texts: string[] = [];
    const walk = (value: unknown) => {
      if (typeof value === "string") texts.push(value);
      else if (typeof value === "function") texts.push(String((value as (...a: unknown[]) => unknown)(1, "x", "y")));
      else if (value && typeof value === "object") Object.values(value).forEach(walk);
    };
    walk(zh.forge);
    walk(en.forge);
    const codes = ["git/no_remote", "git/detached", "git/push_auth", "git/push_rejected", "git/push_protected", "git/push_hook", "git/push_timeout", "git/push_network", "git/push_failed", "git/job_not_found",
      "gh/not_installed", "gh/not_authed", "gh/too_old", "gh/checks_failed", "gh/checks_pending", "gh/merge_conflict", "gh/head_changed", "gh/method_not_allowed", "gh/review_required", "gh/merge_blocked", "gh/pr_exists", "gh/failed"];
    for (const code of codes) texts.push(formatBackendError(code, "zh"), formatBackendError(code, "en"));
    const bad = texts.filter((text) => /电脑|桌面|终端|computer|desktop|terminal/i.test(text));
    expect(bad).toEqual([]);
  });
});

describe("#24 归档后提示清理", () => {
  it("归档的会话 worktree 可清理时提示，不自动删；不可清理不提示", async () => {
    let eligible = true;
    handler = (command) => (command === "worktree_status" ? { path: ROOT, branch: "meowo/drill", prNumber: 12, eligible, reason: eligible ? null : "dirty", count: 1, ignored: [] } : null);
    const { unmount } = render(<ArchivedWorktreePrompt t={zh} className="x" />);
    act(() => { window.dispatchEvent(new CustomEvent(SESSION_ARCHIVED_EVENT, { detail: { ids: [7] } })); });
    expect((await screen.findByTestId("archived-worktree-prompt")).textContent).toContain(zh.forge.cleanupHint);
    expect(await screen.findByRole("button", { name: zh.worktree.remove })).toBeTruthy();
    expect(calls("remove_session_worktree").length).toBe(0);
    unmount();
    eligible = false;
    render(<ArchivedWorktreePrompt t={zh} className="x" />);
    act(() => { window.dispatchEvent(new CustomEvent(SESSION_ARCHIVED_EVENT, { detail: { ids: [8] } })); });
    await waitFor(() => expect(calls("worktree_status").some(([, a]) => (a as { sessionId: number }).sessionId === 8)).toBe(true));
    expect(screen.queryByTestId("archived-worktree-prompt")).toBeNull();
  });
});
