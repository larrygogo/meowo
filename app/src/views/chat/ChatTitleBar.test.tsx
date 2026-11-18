import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
import { ChatTitleBar } from "./ChatTitleBar";

afterEach(cleanup);

const props = {
  leading: null, title: <span>标题</span>, archive: null, end: null, cwd: "C:\\repo", gitSummary: null,
  changesOpen: false, panelOpen: false, terminalOn: false,
  onToggleChanges: () => {}, onTogglePanel: () => {}, onToggleTerminal: () => {}, onError: () => {},
};

describe("标题栏的面板开关", () => {
  it("面板收起时 agent 在操作浏览器：开关上亮点，提示说明原因", () => {
    const { rerender } = render(<ChatTitleBar {...props} />);
    expect(document.querySelector(".chat-panel-agent-dot")).toBeNull();
    rerender(<ChatTitleBar {...props} panelAgentBusy />);
    expect(document.querySelector(".chat-panel-agent-dot")).not.toBeNull();
    expect(screen.getByRole("button", { name: /Agent 正在操作浏览器/ })).toBeTruthy();
  });
});

/** 2026-11-16 实拍：agent 在临时 worktree 里建了 #536，会话目录还停在旧分支上，标题栏按分支查到的是已合并的 #530。
 *  会话里动过 PR 时按链接查最近那个；按链接查不到才退回按分支。 */
describe("标题栏的 PR：会话里 agent 建过的优先", () => {
  const summary = { isRepo: true, branch: "feat/old", files: [], ahead: 0, behind: 0 } as unknown as NonNullable<Parameters<typeof ChatTitleBar>[0]["gitSummary"]>;
  const pr = (number: number, state: string) => ({ number, state, url: `https://github.com/o/r/pull/${number}`, title: `PR ${number}` });

  it("有会话 PR：按链接查，显示新建的那个，不问当前分支", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((command: string) => Promise.resolve(
      command === "git_pull_request_by_url" ? pr(536, "open") : command === "git_pull_request" ? pr(530, "merged") : null));
    render(<ChatTitleBar {...props} gitSummary={summary} sessionPrUrl="https://github.com/o/r/pull/536" />);
    expect(await screen.findByText("#536")).toBeTruthy();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("git_pull_request_by_url", { cwd: props.cwd, url: "https://github.com/o/r/pull/536" });
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("git_pull_request", expect.anything());
  });

  it("按链接查不到（没账号、链接失效）：退回当前分支的 PR", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((command: string) => Promise.resolve(command === "git_pull_request" ? pr(530, "merged") : null));
    render(<ChatTitleBar {...props} gitSummary={summary} sessionPrUrl="https://github.com/o/r/pull/536" />);
    expect(await screen.findByText("#530")).toBeTruthy();
  });

  it("会话里没动过 PR：照旧按分支查", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation((command: string) => Promise.resolve(command === "git_pull_request" ? pr(530, "merged") : null));
    render(<ChatTitleBar {...props} gitSummary={summary} />);
    expect(await screen.findByText("#530")).toBeTruthy();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("git_pull_request_by_url", expect.anything());
  });
});
