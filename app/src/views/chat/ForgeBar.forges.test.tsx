// 「更改」面板在 GitLab / Gitea / Bitbucket / GitHub 企业版仓库上：用平台的叫法（GitLab 是 MR、!12、流水线），
// 没有账号时引导去设置添加、不报错，令牌失效时引导重新添加。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ForgeStatusDto, GitDiffSummaryDto, PullRequestDto, RepoChangesDto, SidebarGitBadgeDto } from "../../api";
import { zh } from "../../i18n/zh";
import { en } from "../../i18n/en";
import { clearForgeCaches, ForgeBar } from "./ForgeBar";
import { SidebarGitBadge } from "./SidebarGitBadge";
import { PullRequestChip } from "./TitleBarTools";
import { forgeText, kindOfUrl } from "./forgeWords";
import { sessionPrsInRepo, sessionPullRequestUrls } from "./sessionPrs";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));
const settingsNav = vi.hoisted(() => ({ openSettingsAt: vi.fn() }));
vi.mock("../../settingsNav", () => settingsNav);

const ROOT = "C:/w/app";
const summary = (over: Partial<GitDiffSummaryDto> = {}): GitDiffSummaryDto => ({
  gitAvailable: true, isRepo: true, branch: "feat/x", ahead: null, behind: null, files: [], upstream: "origin/feat/x", unpushed: 0, ...over,
});
const repo = (): RepoChangesDto => ({ root: ROOT, name: "app", isCwdRepo: true, summary: summary(), base: null, baseLabel: null, committed: [] });
const mr = (over: Partial<PullRequestDto> = {}): PullRequestDto => ({
  number: 42, state: "open", url: "https://gitlab.com/g/app/-/merge_requests/42", title: "Fix login", baseRef: "main", headRef: "feat/x",
  headOid: "abc", mergeable: "MERGEABLE", mergeState: "CLEAN", checksSummary: "fail", forge: "gitlab",
  checks: [{ name: "test", workflow: "test", state: "fail", url: "https://gitlab.com/g/app/-/jobs/1" }],
  ...over,
});
const gitlab = (over: Partial<ForgeStatusDto> = {}): ForgeStatusDto => ({
  gh: "ok", ghTooOld: false, repoUrl: "https://gitlab.com/g/app", forge: "gitlab", forgeHost: "gitlab.com",
  compareUrl: "https://gitlab.com/g/app/-/merge_requests/new?merge_request%5Bsource_branch%5D=feat%2Fx&merge_request%5Btarget_branch%5D=main",
  mergeMethods: ["merge", "squash"], lastMergeMethod: null, pr: null, prDefaults: { title: "Fix login", body: "", base: "main" },
  repoChoices: [], repoRemote: "origin", repoNeedsChoice: false, ...over,
});

let status: ForgeStatusDto = gitlab();
beforeEach(() => {
  status = gitlab();
  settingsNav.openSettingsAt.mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string) => (command === "git_forge_status" ? status : null));
});
afterEach(() => {
  clearForgeCaches();
  cleanup();
});

const renderBar = () => render(<ForgeBar repo={repo()} sessionCwd={null} sessionId={7} active onChanged={() => {}} />);

describe("「更改」面板按平台的叫法", () => {
  it("GitLab：MR !42、流水线失败不能合并、合并方式按项目设置", async () => {
    status = gitlab({ pr: mr() });
    renderBar();
    const card = await screen.findByTestId("scm-pr");
    expect(card.textContent).toContain("!42 Fix login");
    expect(card.textContent).toContain("流水线有失败，不能合并");
    // 顶部汇总与底部提示同一套叫法：流水线 / 作业（审查指出以前顶部写「1 项检查失败」）。
    expect(card.textContent).toContain("流水线 1 项作业失败");
    expect(card.textContent).not.toContain("检查失败");
    expect(within(card).getByTestId("scm-merge").hasAttribute("disabled")).toBe(true);
    // 检查都过了：合并框写「合并 MR !42」，方式是 GitLab 的两种。
    cleanup();
    clearForgeCaches();
    status = gitlab({ pr: mr({ checksSummary: "pass", checks: [] }) });
    renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    expect(await screen.findByText("合并 MR !42")).toBeTruthy();
    expect(screen.getByText(zh.forgeP.gitlabMethods.merge)).toBeTruthy();
    expect(screen.getByText(zh.forgeP.gitlabMethods.squash)).toBeTruthy();
  });

  it("GitLab 没有 MR：开 MR；没有账号：网页上新建、引导去设置添加账号，不报错", async () => {
    renderBar();
    expect(await screen.findByRole("button", { name: "开 MR" })).toBeTruthy();
    cleanup();
    clearForgeCaches();
    status = gitlab({ gh: "no_account" });
    renderBar();
    const notice = await screen.findByTestId("forge-connection-notice");
    expect(notice.textContent).toContain(zh.forgeP.noAccount(zh.forgeWords.gitlab));
    expect(screen.getByRole("button", { name: "在 GitLab 打开" })).toBeTruthy();
    fireEvent.click(within(notice).getByRole("button", { name: zh.forgeP.addAccount(zh.forgeWords.gitlab) }));
    expect(settingsNav.openSettingsAt).toHaveBeenCalledWith("github");
    expect(screen.queryByTestId("forge-error")).toBeNull();
  });

  it("Gitee：能否合并不给原因（UNKNOWN）时不挡合并按钮；没有检查不编造", async () => {
    status = gitlab({ forge: "gitee", forgeHost: "gitee.com", repoUrl: "https://gitee.com/o/app", mergeMethods: ["merge", "squash", "rebase"],
      pr: mr({ number: 316, url: "https://gitee.com/o/app/pulls/316", forge: "gitee", mergeable: "UNKNOWN", mergeState: "UNKNOWN", checks: [], checksSummary: "none" }) });
    renderBar();
    const card = await screen.findByTestId("scm-pr");
    expect(card.textContent).toContain("!316 Fix login");
    expect(card.textContent).toContain(zh.forgeWords.gitee.noCi);
    expect(within(card).getByTestId("scm-merge").hasAttribute("disabled")).toBe(false);
  });

  it("Gitea 回「Please try again later」：不说成检查在跑，说清楚并给网页入口", async () => {
    const url = "https://codeberg.org/o/r/pulls/57";
    status = gitlab({ forge: "gitea", forgeHost: "codeberg.org", repoUrl: "https://codeberg.org/o/r", mergeMethods: ["merge"],
      pr: mr({ number: 57, url, forge: "gitea", checks: [], checksSummary: "none" }) });
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "git_forge_status") return status;
      if (command === "gh_pr_merge") throw `forge/not_mergeable: ${url}`;
      return null;
    });
    renderBar();
    fireEvent.click(await screen.findByTestId("scm-merge"));
    fireEvent.click(await screen.findByTestId("merge-confirm"));
    const error = await screen.findByTestId("forge-error");
    expect(error.textContent).toContain("平台判定暂时不能合并");
    expect(error.textContent).not.toContain(zh.forge.mergeBlockedPending);
    expect(within(error).getByRole("button", { name: "在 Codeberg 打开" })).toBeTruthy();
  });

  it("令牌失效：说清楚并引导重新添加；Codeberg 用站点名", async () => {
    status = gitlab({ gh: "unauthenticated", forge: "gitea", forgeHost: "codeberg.org", repoUrl: "https://codeberg.org/o/r", compareUrl: "https://codeberg.org/o/r/compare/main...feat/x" });
    renderBar();
    const notice = await screen.findByTestId("forge-connection-notice");
    expect(notice.textContent).toContain("Codeberg 账号的令牌已失效");
    expect(within(notice).getByRole("button", { name: zh.forgeP.reloginAction })).toBeTruthy();
    expect(screen.getByRole("button", { name: "在 Codeberg 打开" })).toBeTruthy();
  });

  it("GitHub 的文案与改版前逐字相同", () => {
    const tz = forgeText(zh, "github");
    const w = zh.forgeWords.github;
    expect([tz.openPr, tz.prCreate, tz.mergeTitle(3), tz.checksNone, tz.mergeBlockedFail, tz.repoAt("o/r")]).toEqual([
      zh.forgeP.openPr(w), zh.forgeP.prCreate(w), zh.forgeP.mergeTitle(w, 3), w.noCi, zh.forgeP.mergeBlockedFail(w), zh.forgeP.repoAt(w, "o/r"),
    ]);
    expect(tz.prOpenTip(5, "x")).toBe(zh.forgeP.prOpenTip(w, 5, "x"));
    const te = forgeText(en, "github");
    const we = en.forgeWords.github;
    expect([te.openPr, te.mergeTitle(3), te.mergeBlockedPending, te.prOpenTip(5, "x")]).toEqual([
      en.forgeP.openPr(we), en.forgeP.mergeTitle(we, 3), en.forgeP.mergeBlockedPending(we), en.forgeP.prOpenTip(we, 5, "x"),
    ]);
    expect(forgeText(en, "gitlab").mergeBlockedFail).toBe("Pipeline failed. Can't merge.");
    expect([tz.checksPass(3), tz.checksFail(1), tz.checksPending(2)]).toEqual([zh.forge.checksPass(3), zh.forge.checksFail(1), zh.forge.checksPending(2)]);
    expect([zh.forgeP.checksPass(w, 3), zh.forgeP.checksFail(w, 1)]).toEqual([zh.forge.checksPass(3), zh.forge.checksFail(1)]);
    expect([en.forgeP.checksPass(we, 3), en.forgeP.checksFail(we, 1), en.forgeP.checksPending(we, 2)]).toEqual([en.forge.checksPass(3), en.forge.checksFail(1), en.forge.checksPending(2)]);
    expect(forgeText(en, "gitlab").checksFail(2)).toBe("Pipeline: 2 jobs failed");
  });
});

describe("PR 状态的说法", () => {
  it("「更改」面板、标题栏、侧栏同一说法", () => {
    for (const state of ["open", "draft", "merged", "closed"]) {
      expect(zh.forge.prState[state]).toBe(zh.chat.prState[state]);
      expect(en.forge.prState[state]).toBe(en.chat.prState[state]);
    }
  });
});

describe("侧栏与标题栏按平台的叫法", () => {
  it("GitLab 的 MR 写 !号；没有 forge 字段时按链接认", () => {
    const badge: SidebarGitBadgeDto = { cwd: ROOT, additions: 0, deletions: 0, files: 0, pr: { number: 42, state: "open", checks: "pass", url: "https://gitlab.com/g/app/-/merge_requests/42", title: "x", forge: "gitlab" } };
    render(<SidebarGitBadge badge={badge} />);
    const el = screen.getByTestId("sidebar-git-badge");
    expect(el.textContent).toContain("!42");
    expect(el.getAttribute("aria-label")).toContain("MR !42");
    cleanup();
    render(<PullRequestChip pr={{ number: 9, state: "open", url: "https://bitbucket.org/w/r/pull-requests/9", title: "t" }} />);
    expect(screen.getByRole("button").getAttribute("data-tip")).toBe("打开 Pull Request #9：t");
    cleanup();
    render(<PullRequestChip pr={{ number: 7, state: "merged", url: "https://gitlab.com/g/r/-/merge_requests/7", title: "t" }} />);
    expect(screen.getByRole("button").getAttribute("data-tip")).toBe("打开合并请求 !7：t");
    expect(kindOfUrl("https://codeberg.org/o/r/pulls/3")).toBe("gitea");
    // Gitee：链接与 Gitea 同形，按主机认；编号写 !号，叫法仍是 Pull Request。
    expect(kindOfUrl("https://gitee.com/openharmony/docs/pulls/95569")).toBe("gitee");
    cleanup();
    render(<PullRequestChip pr={{ number: 95569, state: "open", url: "https://gitee.com/openharmony/docs/pulls/95569", title: "t", forge: "gitee" }} />);
    expect(screen.getByRole("button").getAttribute("data-tip")).toBe("打开 Pull Request !95569：t");
  });
});

describe("会话里 agent 动过的 MR / PR", () => {
  it("认 glab mr 与 tea pr 的输出里的链接；GitLab 子组的 MR 链接也认", () => {
    const items = [
      { type: "tool_use", id: "a", detail: { kind: "command", command: "glab mr create --fill" } },
      { type: "tool_result", tool_use_id: "a", is_error: false, text: "https://gitlab.com/g/sub/app/-/merge_requests/42\n" },
      { type: "tool_use", id: "b", detail: { kind: "command", command: "tea pr create" } },
      { type: "tool_result", tool_use_id: "b", is_error: false, text: "https://codeberg.org/o/r/pulls/7" },
      { type: "tool_use", id: "c", detail: { kind: "command", command: "glab mr list" } },
      { type: "tool_result", tool_use_id: "c", is_error: false, text: "https://gitlab.com/g/sub/app/-/merge_requests/1" },
    ] as unknown as Parameters<typeof sessionPullRequestUrls>[0];
    const urls = sessionPullRequestUrls(items);
    expect(urls).toEqual(["https://gitlab.com/g/sub/app/-/merge_requests/42", "https://codeberg.org/o/r/pulls/7"]);
    expect(sessionPrsInRepo(urls, "https://gitlab.com/g/sub/app", null, [], 3, "/-/merge_requests/")).toEqual(["https://gitlab.com/g/sub/app/-/merge_requests/42"]);
    expect(sessionPrsInRepo(urls, "https://codeberg.org/o/r", null, [], 3, "/pulls/")).toEqual(["https://codeberg.org/o/r/pulls/7"]);
  });
});

