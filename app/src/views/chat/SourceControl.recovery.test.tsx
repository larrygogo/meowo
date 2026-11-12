// 「更改」面板第三轮（2026-11-12 walkthrough P-1/P-3/P-4/P-5/P-10/P-12/P-14）：失败有出路、进行中有字、
// 放弃可撤销、键盘操作后焦点不掉到 body、方向键在行间移动。
// 兄弟场景：暂存 / 取消暂存 / 放弃 / 加入 .gitignore 共用同一个 run 与焦点计划；单个行按钮与组头按钮；
// 失败出路覆盖 git/identity（设置身份后重做）、git/conflict（交给 agent）、git/locked（重试）。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { SourceControl } from "./SourceControl";
import { PanelPromptContext } from "./panelPrompt";
import { markRemoteUi } from "../../remoteMode";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}));

const file = (path: string, indexStatus: string | null, worktreeStatus: string | null) => ({
  path, status: indexStatus ?? worktreeStatus ?? "M", additions: 1 as number | null, deletions: 0 as number | null, indexStatus, worktreeStatus,
});
const summary = (files: ReturnType<typeof file>[]): GitDiffSummaryDto =>
  ({ gitAvailable: true, isRepo: true, branch: "main", ahead: 0, behind: 0, files });
const repo = (files: ReturnType<typeof file>[]): RepoChangesDto => ({
  root: "C:/w/meowo", name: "meowo", isCwdRepo: true, summary: summary(files), base: null, baseLabel: null, committed: [],
});
const isOff = (el: Element) => el.getAttribute("aria-disabled") === "true";

type Handler = (command: string, args: Record<string, unknown>) => unknown;
let handler: Handler = () => [];
beforeEach(() => {
  handler = () => [];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => handler(command, (args ?? {}) as Record<string, unknown>));
});
afterEach(cleanup);

function renderScm(repos: RepoChangesDto[], insert?: (text: string) => void) {
  const props = {
    cwd: "C:/w/meowo", selectedRoot: null, onSelectRoot: () => {}, onChanged: vi.fn(), comments: [], touchedPaths: [], turnTouchedPaths: [],
    onAddComment: () => {}, onRemoveComment: () => {}, escOverrideRef: { current: null },
  };
  const ui = (list: RepoChangesDto[]) => (
    <PanelPromptContext.Provider value={insert ?? null}>
      <SourceControl {...props} repos={list} />
    </PanelPromptContext.Provider>
  );
  const utils = render(ui(repos));
  return { ...utils, rerender: (list: RepoChangesDto[]) => utils.rerender(ui(list)) };
}

const typeMessage = (text: string) => fireEvent.change(screen.getByRole("textbox", { name: /^消息/ }), { target: { value: text } });
const commitButton = () => screen.getByRole("button", { name: /^(提交|正在提交)/ });

describe("放弃可以撤销（P-5）", () => {
  it("放弃已跟踪文件后原位给「撤销」，点了把撤销号交给后端", async () => {
    handler = (command) => (command === "git_discard" ? "discard-1" : []);
    renderScm([repo([file("a.ts", null, "M")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    const notice = await screen.findByTestId("scm-notice");
    expect(notice.textContent).toContain("已放弃这个文件的更改");
    fireEvent.click(within(notice).getByTestId("scm-undo"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard_undo", { cwd: "C:/w/meowo", undoId: "discard-1" }));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toContain("已撤销放弃"));
  });

  it("只放弃了未跟踪文件（后端不给撤销号）：只说进了回收站，没有「撤销」", async () => {
    handler = (command) => (command === "git_discard" ? null : []);
    renderScm([repo([file("new.txt", null, "U")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    const notice = await screen.findByTestId("scm-notice");
    expect(notice.textContent).toBe("已移到回收站");
    expect(within(notice).queryByTestId("scm-undo")).toBeNull();
  });
});

// 做空第一轮 #1 / #2 / #3（2026-11-12）。兄弟场景：单个放弃与组头「全部放弃」走同一个 confirmAndDiscard；
// 「更改」面板三种提示条（失败、结果、筛选拦提交）都挂 focusAfter；ForgeBar 走 PanelMessage 的默认落点
//（ForgeBar.recovery.test）；ReviewBar 撤销条 × 交给输入框（ReviewBar.test）。手机页同一个 SourceControl。
describe("做空第一轮：混合放弃、放弃到一半、撤销重试、提示条焦点", () => {
  const discardAll = () => {
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "全部放弃" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
  };

  it("混合放弃：提示分开数已放弃的和进回收站的，撤销后说回收站里还有几个", async () => {
    handler = (command) => (command === "git_discard" ? "discard-1" : []);
    renderScm([repo([file("a.ts", null, "M"), file("new.txt", null, "U")])]);
    discardAll();
    const notice = await screen.findByTestId("scm-notice");
    expect(notice.textContent).toContain("已放弃 1 个，1 个移到回收站");
    fireEvent.click(within(notice).getByTestId("scm-undo"));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已撤销放弃，1 个仍在回收站"));
  });

  it("放弃到一半失败：说只放弃了一部分和原因，给「撤销」把已放弃的写回去", async () => {
    handler = (command) => {
      if (command === "git_discard") throw "git/discard_partial:discard-18a-3:git/locked";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M"), file("b.ts", null, "M")])]);
    discardAll();
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("只放弃了一部分。另一个 git 操作正在进行");
    expect(alert.textContent).not.toContain("discard-18a-3");
    expect(within(alert).queryByRole("button", { name: "重试" })).toBeNull();
    fireEvent.click(within(alert).getByTestId("scm-undo"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_discard_undo", { cwd: "C:/w/meowo", undoId: "discard-18a-3" }));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已撤销放弃"));
  });

  it("撤销写一半失败：给「重试」再撤一次；文件之后改过的不给重试", async () => {
    let undoFails = 1;
    handler = (command) => {
      if (command === "git_discard") return "discard-1";
      if (command === "git_discard_undo" && undoFails-- > 0) throw "git/failed: 拒绝访问。 (os error 5)";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(await screen.findByTestId("scm-notice")).getByTestId("scm-undo"));
    fireEvent.click(within(await screen.findByRole("alert")).getByTestId("scm-undo-retry"));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已撤销放弃"));
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "git_discard_undo")).toHaveLength(2);

    cleanup();
    handler = (command) => {
      if (command === "git_discard") return "discard-2";
      if (command === "git_discard_undo") throw "git/undo_changed";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    fireEvent.click(within(await screen.findByTestId("scm-notice")).getByTestId("scm-undo"));
    expect(within(await screen.findByRole("alert")).queryByTestId("scm-undo-retry")).toBeNull();
  });

  it("提示条上的「撤销」「×」点完焦点交给提交框，不掉到 body", async () => {
    let locked = true;
    handler = (command) => {
      if (command === "git_discard") return "discard-1";
      if (command === "git_stage" && locked) throw "git/locked";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M"), file("b.ts", null, "M")])]);
    const input = screen.getByRole("textbox", { name: /^消息/ });
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getAllByRole("button", { name: "放弃更改" })[0]);
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "放弃更改" }));
    const undo = within(await screen.findByTestId("scm-notice")).getByTestId("scm-undo");
    undo.focus();
    fireEvent.click(undo);
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已撤销放弃"));
    expect(document.activeElement).toBe(input);

    fireEvent.click(within(screen.getByTestId("scm-group-work")).getAllByRole("button", { name: "暂存更改" })[0]);
    const close = within(await screen.findByRole("alert")).getByRole("button", { name: "关闭" });
    close.focus();
    fireEvent.click(close);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(document.activeElement).toBe(input);

    fireEvent.click(within(screen.getByTestId("scm-group-work")).getAllByRole("button", { name: "暂存更改" })[0]);
    const retry = within(await screen.findByRole("alert")).getByRole("button", { name: "重试" });
    locked = false;
    retry.focus();
    fireEvent.click(retry);
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(document.activeElement).not.toBe(document.body);
  });
});

// 做空第二轮 N3（2026-11-12）：手机远程关提示条不聚焦提交框（会弹软键盘），退回条前面的按钮。
describe("手机远程的提示条焦点", () => {
  afterEach(() => {
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
    document.body.classList.remove("remote-ui");
  });
  it("× 关掉失败提示后焦点不进提交框，也不掉到 body", async () => {
    markRemoteUi();
    handler = (command) => {
      if (command === "git_stage") throw "git/locked";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M")])]);
    const input = document.querySelector("textarea.scm-commit-input");
    expect(input).not.toBeNull();
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getAllByRole("button", { name: "暂存更改" })[0]);
    const close = within(await screen.findByRole("alert")).getByRole("button", { name: "关闭" });
    close.focus();
    fireEvent.click(close);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(document.activeElement).not.toBe(input);
    expect(document.activeElement).not.toBe(document.body);
  });
});

describe("失败有出路（P-1 / P-14）", () => {
  it("缺 git 身份：「设置身份」就地填写，保存后自动重做提交", async () => {
    let identity = false;
    handler = (command) => {
      if (command === "git_commit") {
        if (!identity) throw "git/identity";
        return "abc1234";
      }
      if (command === "git_set_identity") {
        identity = true;
        return null;
      }
      return [];
    };
    renderScm([repo([file("a.ts", "M", null)])]);
    typeMessage("修好了");
    fireEvent.click(commitButton());
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).not.toContain("终端");
    fireEvent.click(within(alert).getByTestId("scm-identity"));
    const dialog = screen.getByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox", { name: "名字" }), { target: { value: "Ada" } });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "邮箱" }), { target: { value: "ada@example.com" } });
    fireEvent.click(within(dialog).getByTestId("identity-save"));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_set_identity", { cwd: "C:/w/meowo", name: "Ada", email: "ada@example.com" }));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已提交 abc1234"));
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "git_commit")).toHaveLength(2);
  });

  it("有冲突：「交给 agent」把现成的一句话放进输入框；没人接（单测、老面板）就不出按钮", async () => {
    handler = (command) => {
      if (command === "git_commit") throw "git/conflict";
      return [];
    };
    const insert = vi.fn();
    renderScm([repo([file("a.ts", "M", null)])], insert);
    typeMessage("x");
    fireEvent.click(commitButton());
    const alert = await screen.findByRole("alert");
    fireEvent.click(within(alert).getByRole("button", { name: "交给 agent" }));
    expect(insert).toHaveBeenCalledWith("仓库里有未解决的合并冲突，请解决冲突后提交。");
    cleanup();
    renderScm([repo([file("a.ts", "M", null)])]);
    typeMessage("x");
    fireEvent.click(commitButton());
    expect(within(await screen.findByRole("alert")).queryByRole("button", { name: "交给 agent" })).toBeNull();
  });

  it("被别的 git 操作锁住：「重试」原样再跑；红字有关闭按钮", async () => {
    let locked = true;
    handler = (command) => {
      if (command === "git_stage" && locked) throw "git/locked";
      return [];
    };
    renderScm([repo([file("a.ts", null, "M")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "暂存更改" }));
    const alert = await screen.findByRole("alert");
    locked = false;
    fireEvent.click(within(alert).getByRole("button", { name: "重试" }));
    await waitFor(() => expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "git_stage")).toHaveLength(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    locked = true;
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "暂存更改" }));
    fireEvent.click(within(await screen.findByRole("alert")).getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("进行中（P-3 / P-4）", () => {
  it("提交进行中：按钮写「正在提交…」、行按钮 aria-disabled 而不是 disabled，焦点不被踢走；重复点不再发", async () => {
    let finish: (sha: string) => void = () => {};
    handler = (command) => (command === "git_commit" ? new Promise<string>((resolve) => { finish = resolve; }) : []);
    renderScm([repo([file("a.ts", "M", null), file("b.ts", null, "M")])]);
    typeMessage("x");
    const button = commitButton();
    button.focus();
    fireEvent.click(button);
    await waitFor(() => expect(commitButton().textContent).toBe("正在提交…"));
    expect(document.activeElement).toBe(button);
    const stage = within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "暂存更改" });
    expect(isOff(stage)).toBe(true);
    expect((stage as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(stage);
    fireEvent.click(commitButton());
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "git_stage" || c === "git_commit")).toHaveLength(1);
    await act(async () => finish("abc1234"));
    await waitFor(() => expect(screen.getByTestId("scm-notice").textContent).toBe("已提交 abc1234"));
    // 成功后信息清空、按钮变成不能提交，但焦点还在按钮上（以前 disabled 把它踢到 body）。
    expect(isOff(commitButton())).toBe(true);
    expect(document.activeElement).toBe(commitButton());
  });

  it("暂存进行中有「正在暂存…」", async () => {
    let finish: () => void = () => {};
    handler = (command) => (command === "git_stage" ? new Promise<void>((resolve) => { finish = resolve; }) : []);
    renderScm([repo([file("a.ts", null, "M")])]);
    fireEvent.click(within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "暂存更改" }));
    expect(await screen.findByText("正在暂存…")).toBeTruthy();
    await act(async () => finish());
    await waitFor(() => expect(screen.queryByText("正在暂存…")).toBeNull());
  });
});

describe("焦点不掉到 body（P-3）", () => {
  it("暂存后这一行挪去了「暂存的更改」：焦点落到原位置的下一行", async () => {
    const before = [repo([file("a.ts", null, "M"), file("b.ts", null, "M")])];
    const { rerender } = renderScm(before);
    const rowA = within(screen.getByTestId("scm-group-work")).getByText("a.ts").closest(".scm-row") as HTMLElement;
    const stage = within(rowA).getByRole("button", { name: "暂存更改" });
    stage.focus();
    fireEvent.click(stage);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("git_stage", { cwd: "C:/w/meowo", paths: ["a.ts"] }));
    await waitFor(() => expect(isOff(stage)).toBe(false));
    // 轮询拿到新清单：a.ts 进了暂存组，原来那个按钮被卸载。
    rerender([repo([file("a.ts", "M", null), file("b.ts", null, "M")])]);
    const rowB = within(screen.getByTestId("scm-group-work")).getByText("b.ts").closest(".scm-row-main");
    expect(document.activeElement).toBe(rowB);
  });

  it("组空了：焦点落到组头；方向键在行之间移动", async () => {
    const { rerender } = renderScm([repo([file("a.ts", null, "M")])]);
    const stage = within(screen.getByTestId("scm-group-work")).getByRole("button", { name: "暂存更改" });
    stage.focus();
    fireEvent.click(stage);
    await waitFor(() => expect(isOff(stage)).toBe(false));
    rerender([repo([file("a.ts", "M", null), file("c.ts", "M", null)])]);
    // 「更改」组整个没了：落到另一组的第一行（或提交框），总之不是 body。
    expect(document.activeElement).not.toBe(document.body);
    const rows = Array.from(document.querySelectorAll<HTMLElement>(".scm-groups .scm-row-main, .scm-groups .scm-group-toggle"));
    rows[0].focus();
    fireEvent.keyDown(rows[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(rows[1]);
    fireEvent.keyDown(rows[1], { key: "End" });
    expect(document.activeElement).toBe(rows[rows.length - 1]);
    fireEvent.keyDown(rows[rows.length - 1], { key: "Home" });
    expect(document.activeElement).toBe(rows[0]);
  });
});

describe("筛选拦住提交（P-10）", () => {
  it("提示后面有「清除筛选」，点了清掉搜索，提交恢复可用", () => {
    renderScm([repo([file("a.ts", null, "M"), file("b.ts", null, "M")])]);
    fireEvent.change(screen.getByPlaceholderText("按名称搜索文件…"), { target: { value: "a.ts" } });
    typeMessage("x");
    expect(isOff(commitButton())).toBe(true);
    fireEvent.click(screen.getByTestId("scm-clear-filter"));
    expect((screen.getByPlaceholderText("按名称搜索文件…") as HTMLInputElement).value).toBe("");
    expect(isOff(commitButton())).toBe(false);
  });
});
