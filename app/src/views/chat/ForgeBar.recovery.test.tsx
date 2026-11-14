// 推送 / 开 PR 的出路与误操作保护（2026-11-12 walkthrough P-1 / P-8 / P-14）。
// 兄弟场景：推送被拒 → 拉取并重试；拉取冲突 → 交给 agent；拉取缺身份 → 设置身份后接着拉；
// 推送失败后轮询看到已推上去 → 红字自己收掉；开 PR 表单 Esc / 点遮罩只收起不丢、创建中关不掉、成功有回音。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ForgeStatusDto, GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { zh } from "../../i18n/zh";
import { ForgeBar } from "./ForgeBar";
import { PanelPromptContext } from "./panelPrompt";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const ROOT = "C:/w/meowo-wt";
const summary = (over: Partial<GitDiffSummaryDto> = {}): GitDiffSummaryDto => ({
  gitAvailable: true, isRepo: true, branch: "meowo/drill", ahead: null, behind: null, files: [], upstream: "origin/meowo/drill", unpushed: 2,
  pushTarget: "origin/meowo/drill", ...over,
});
const repo = (over: Partial<GitDiffSummaryDto> = {}): RepoChangesDto => ({
  root: ROOT, name: "meowo-wt", isCwdRepo: false, summary: summary(over), base: null, baseLabel: null, committed: [],
});
const forge = (over: Partial<ForgeStatusDto> = {}): ForgeStatusDto => ({
  gh: "ok", ghTooOld: false, repoUrl: "https://github.com/o/r", compareUrl: null,
  mergeMethods: ["squash"], lastMergeMethod: null, pr: null,
  prDefaults: { title: "feat: drill", body: "- feat: drill", base: "main" }, repoChoices: [], repoRemote: "origin", repoNeedsChoice: false, ...over,
});
const calls = (command: string) => vi.mocked(invoke).mock.calls.filter(([c]) => c === command);

type Handler = (command: string, args: Record<string, unknown>) => unknown;
let handler: Handler = () => undefined;
beforeEach(() => {
  handler = () => undefined;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const custom = handler(command, (args ?? {}) as Record<string, unknown>);
    if (custom !== undefined) return custom;
    if (command === "git_forge_status") return forge();
    if (command === "git_push_start") return "push-1";
    if (command === "git_pull_start") return "pull-1";
    return null;
  });
});
afterEach(cleanup);

const pushOk = { state: "ok", push: { remote: "origin", branch: "meowo/drill", setUpstream: false, pushed: 3 }, pull: null, error: null };
const pullOk = { state: "ok", push: null, pull: { remote: "origin", branch: "meowo/drill", pulled: 1, mode: "merge" }, error: null };
const failed = (error: string) => ({ state: "failed", push: null, pull: null, error });

function renderBar(over: Partial<GitDiffSummaryDto> = {}, insert?: (text: string) => void) {
  const ui = (r: RepoChangesDto) => (
    <PanelPromptContext.Provider value={insert ?? null}>
      <ForgeBar repo={r} sessionCwd="C:/w/meowo" sessionId={7} active onChanged={() => {}} />
    </PanelPromptContext.Provider>
  );
  const utils = render(ui(repo(over)));
  return { ...utils, rerender: (next: Partial<GitDiffSummaryDto>) => utils.rerender(ui(repo(next))) };
}

describe("推送被拒的出路（P-1）", () => {
  it("远端有新提交：红字旁有「拉取并重试」，点了先拉再推，成功后说清楚", async () => {
    let pushes = 0;
    handler = (command, args) => {
      if (command !== "git_job_status") return undefined;
      if (args.jobId === "pull-1") return pullOk;
      pushes += 1;
      return pushes === 1 ? failed("git/push_rejected") : pushOk;
    };
    renderBar();
    fireEvent.click(await screen.findByTestId("forge-push"));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("远端有新提交，先拉取再推");
    fireEvent.click(within(alert).getByTestId("forge-pull-retry"));
    await waitFor(() => expect(screen.getByTestId("forge-notice").textContent).toBe("已拉取 1 个提交并推送到 origin/meowo/drill"));
    expect(calls("git_pull_start")[0][1]).toEqual({ cwd: ROOT, sessionCwd: "C:/w/meowo" });
    expect(calls("git_push_start")).toHaveLength(2);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  // 做空第一轮 #3（2026-11-12）：「拉取并重试」「×」点完红字先被清掉，按钮跟着卸载，焦点以前掉到 body。
  // 现在 PanelMessage 卸载前把焦点交给条前面最近的控件（这里是「开 PR」，没有它时是推送按钮）。
  it("红字上的「拉取并重试」「×」点完焦点交给条前面的控件，不掉到 body", async () => {
    handler = (command) => (command === "git_job_status" ? failed("git/push_rejected") : undefined);
    renderBar();
    fireEvent.click(await screen.findByTestId("forge-push"));
    const retry = within(await screen.findByRole("alert")).getByTestId("forge-pull-retry");
    retry.focus();
    fireEvent.click(retry);
    expect(document.activeElement).toBe(screen.getByTestId("forge-open-pr"));
    const close = within(await screen.findByRole("alert")).getByRole("button", { name: "关闭" });
    close.focus();
    fireEvent.click(close);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(document.activeElement).toBe(screen.getByTestId("forge-open-pr"));
  });

  it("拉取有冲突：说明没合并，给「交给 agent」，放进输入框的话里写明推到哪", async () => {
    handler = (command, args) => {
      if (command !== "git_job_status") return undefined;
      return args.jobId === "pull-1" ? failed("git/pull_conflict") : failed("git/push_rejected");
    };
    const insert = vi.fn();
    renderBar({}, insert);
    fireEvent.click(await screen.findByTestId("forge-push"));
    fireEvent.click(within(await screen.findByRole("alert")).getByTestId("forge-pull-retry"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("远端改动和本地冲突，没有合并"));
    fireEvent.click(screen.getByTestId("forge-ask-agent"));
    expect(insert).toHaveBeenCalledWith(zh.forge.pullConflictPrompt("origin/meowo/drill"));
  });

  it("拉取要写合并提交但没配身份：「设置身份」填完接着拉、接着推", async () => {
    let identity = false;
    handler = (command, args) => {
      if (command === "git_set_identity") {
        identity = true;
        return null;
      }
      if (command !== "git_job_status") return undefined;
      if (args.jobId === "pull-1") return identity ? pullOk : failed("git/identity");
      return pushOk;
    };
    renderBar();
    // 直接从被拒开始：第一次推送就被拒。
    let first = true;
    const base = handler;
    handler = (command, args) => {
      if (command === "git_job_status" && args.jobId === "push-1" && first) {
        first = false;
        return failed("git/push_rejected");
      }
      return base(command, args);
    };
    fireEvent.click(await screen.findByTestId("forge-push"));
    fireEvent.click(within(await screen.findByRole("alert")).getByTestId("forge-pull-retry"));
    fireEvent.click(await screen.findByTestId("forge-identity"));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox", { name: "名字" }), { target: { value: "Ada" } });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "邮箱" }), { target: { value: "ada@example.com" } });
    fireEvent.click(within(dialog).getByTestId("identity-save"));
    await waitFor(() => expect(screen.getByTestId("forge-notice").textContent).toBe("已拉取 1 个提交并推送到 origin/meowo/drill"));
    expect(calls("git_pull_start")).toHaveLength(2);
  });

  it("推送失败后轮询看到已经推上去了（agent 自己推了）：红字自己收掉（P-14）", async () => {
    handler = (command) => (command === "git_job_status" ? failed("git/push_network") : undefined);
    const { rerender } = renderBar();
    fireEvent.click(await screen.findByTestId("forge-push"));
    expect(await screen.findByRole("alert")).toBeTruthy();
    rerender({ unpushed: 0 });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });
});

describe("开 PR 表单不丢内容（P-8）", () => {
  it("Esc、点遮罩只收起，再点「开 PR」写过的内容还在；点「取消」才丢", async () => {
    renderBar();
    fireEvent.click(await screen.findByTestId("forge-open-pr"));
    let dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText(zh.forge.prBody), { target: { value: "手写的说明" } });
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByTestId("forge-open-pr"));
    dialog = screen.getByRole("dialog");
    expect((within(dialog).getByLabelText(zh.forge.prBody) as HTMLTextAreaElement).value).toBe("手写的说明");
    const overlay = dialog.parentElement as HTMLElement;
    fireEvent.mouseDown(overlay);
    fireEvent.click(overlay);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByTestId("forge-open-pr"));
    dialog = screen.getByRole("dialog");
    expect((within(dialog).getByLabelText(zh.forge.prBody) as HTMLTextAreaElement).value).toBe("手写的说明");
    fireEvent.click(within(dialog).getByRole("button", { name: zh.newSession.cancel }));
    fireEvent.click(screen.getByTestId("forge-open-pr"));
    expect((within(screen.getByRole("dialog")).getByLabelText(zh.forge.prBody) as HTMLTextAreaElement).value).toBe("- feat: drill");
  });

  it("创建中 Esc 关不掉；成功后表单收起并说「已创建 PR #N」带链接", async () => {
    let finish: (value: unknown) => void = () => {};
    handler = (command) => (command === "gh_pr_create" ? new Promise((resolve) => { finish = resolve; }) : undefined);
    renderBar();
    fireEvent.click(await screen.findByTestId("forge-open-pr"));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByTestId("pr-create"));
    await waitFor(() => expect(calls("gh_pr_create")).toHaveLength(1));
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBe(dialog);
    fireEvent.click(within(dialog).getByRole("button", { name: zh.newSession.cancel }));
    expect(screen.getByRole("dialog")).toBe(dialog);
    finish({ number: 42, state: "open", url: "https://github.com/o/r/pull/42", title: "feat: drill" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const notice = screen.getByTestId("forge-notice");
    expect(notice.textContent).toContain("已创建 PR #42");
    expect(within(notice).getByRole("button", { name: zh.forge.openOnGithub })).toBeTruthy();
  });
});
