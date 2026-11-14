// 侧栏「用起来」走查（2026-11-12，research-walkthrough.md S-1～S-9）的回归测试。
// 兄弟场景：单条（右键菜单）与批量（多选操作条）两条「标记完成」入口都测；对话窗标题菜单的同一
// 确认在 ChatWindow.session.test.tsx；会话管理页的单条/批量在 chat/SessionAdmin.test.tsx。
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), message: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import { SIDEBAR_SORT_MODE_KEY } from "./sticker/helpers";
import type { LiveSession } from "../api";
import { zh } from "../i18n/zh";

function session(id: number, title: string, extra: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id, cc_session_id: `cc-${id}`, status: "ended" },
    project_name: "meowo",
    task_title: title,
    connected: false,
    pending_review: null,
    cwd: "C:/w/meowo",
    provider: "claude",
    ...extra,
  } as unknown as LiveSession;
}
const running = (id: number, title: string) =>
  session(id, title, { connected: true, session: { id, cc_session_id: `cc-${id}`, status: "running" } } as Partial<LiveSession>);

type Handler = (args: Record<string, unknown> | undefined) => unknown;
let handlers: Record<string, Handler> = {};
let confirmAnswer = true;
const confirms: string[] = [];
const archived = () => invoke.mock.calls.filter(([c]) => c === "set_archived").map(([, a]) => (a as { sessionId: number }).sessionId);

function setup(items: LiveSession[], activeId = 99) {
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_live_sessions_page") return Promise.resolve(items);
    if (command === "get_settings") return Promise.resolve({ card_menu_mode: "context" });
    if (command === "confirm_dialog") { confirms.push(String(args?.message)); return Promise.resolve(confirmAnswer); }
    const handler = handlers[command];
    return handler ? Promise.resolve().then(() => handler(args)) : Promise.resolve();
  });
  const onSelect = vi.fn();
  render(<ChatSidebar activeId={activeId} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
  return { onSelect };
}
const row = async (name: string) => (await screen.findByRole("button", { name: new RegExp(name) })) as HTMLElement;
const tick = (ms = 10) => act(() => new Promise((r) => setTimeout(r, ms)));

afterEach(cleanup);
beforeEach(() => {
  invoke.mockReset();
  localStorage.clear();
  handlers = {};
  confirmAnswer = true;
  confirms.length = 0;
});

describe("S-1 标记完成作用于正在运行的会话先确认", () => {
  it("单条：在跑的先问，取消就什么都不做", async () => {
    setup([running(1, "在跑的"), session(2, "旧任务")]);
    await tick();
    confirmAnswer = false;
    fireEvent.contextMenu(await row("在跑的"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    await waitFor(() => expect(confirms).toEqual([zh.chat.markDoneRunningConfirm]));
    await tick();
    expect(archived()).toEqual([]);
    // 取消后还在列表里。
    expect(screen.getByRole("button", { name: /在跑的/ })).toBeTruthy();
  });

  it("单条：确认后才写 set_archived", async () => {
    setup([running(1, "在跑的"), session(2, "旧任务")]);
    await tick();
    fireEvent.contextMenu(await row("在跑的"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    await waitFor(() => expect(archived()).toEqual([1]));
    expect(confirms).toHaveLength(1);
  });

  it("单条：停在提示符上的不打扰", async () => {
    setup([running(1, "在跑的"), session(2, "旧任务")]);
    await tick();
    fireEvent.contextMenu(await row("旧任务"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    await waitFor(() => expect(archived()).toEqual([2]));
    expect(confirms).toEqual([]);
  });

  it("批量：选中里有在跑的，确认框写明几个会被结束", async () => {
    setup([running(1, "在跑的"), running(2, "也在跑"), session(3, "旧任务")]);
    await tick();
    fireEvent.contextMenu(await row("旧任务"));
    fireEvent.click(screen.getByRole("menuitem", { name: "多选" }));
    fireEvent.click(await row("在跑的"));
    fireEvent.click(await row("也在跑"));
    confirmAnswer = false;
    fireEvent.click(screen.getByRole("button", { name: "标记完成" }));
    await waitFor(() => expect(confirms).toEqual([zh.chat.markDoneRunningBatchConfirm(2)]));
    await tick();
    expect(archived()).toEqual([]);
    // 取消后选中集还在，用户可以去掉在跑的那几条再点。
    expect(screen.getByText("已选 3 条")).toBeTruthy();
  });

  it("批量：全是不在跑的不弹框", async () => {
    setup([running(1, "在跑的"), session(2, "旧一"), session(3, "旧二")]);
    await tick();
    fireEvent.contextMenu(await row("旧一"));
    fireEvent.click(screen.getByRole("menuitem", { name: "多选" }));
    fireEvent.click(await row("旧二"));
    fireEvent.click(screen.getByRole("button", { name: "标记完成" }));
    await waitFor(() => expect(archived()).toEqual([2, 3]));
    expect(confirms).toEqual([]);
  });
});

describe("S-2/S-3 焦点不掉到 body", () => {
  it("键盘菜单标记完成后焦点落到下一条", async () => {
    setup([session(1, "甲"), session(2, "乙"), session(3, "丙")]);
    await tick();
    const yi = await row("乙");
    yi.focus();
    fireEvent.contextMenu(yi);
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    await waitFor(() => expect(archived()).toEqual([2]));
    await tick();
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("3");
  });

  it("批量标记完成后焦点落到被摘掉那批之后的第一条", async () => {
    setup([session(1, "甲"), session(2, "乙"), session(3, "丙"), session(4, "丁")]);
    await tick();
    fireEvent.contextMenu(await row("乙"));
    fireEvent.click(screen.getByRole("menuitem", { name: "多选" }));
    fireEvent.click(await row("丙"));
    const button = screen.getByRole("button", { name: "标记完成" });
    button.focus();
    fireEvent.click(button);
    await waitFor(() => expect(archived()).toEqual([2, 3]));
    await tick();
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("4");
  });

  it("最后一条也被摘掉时焦点退到搜索框", async () => {
    setup([session(1, "甲")]);
    await tick();
    const jia = await row("甲");
    jia.focus();
    fireEvent.contextMenu(jia);
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    await waitFor(() => expect(archived()).toEqual([1]));
    await tick();
    expect(document.activeElement?.tagName).toBe("INPUT");
    expect(document.activeElement?.getAttribute("aria-label")).toBe(zh.chat.sidebarSearch);
  });

  it("行内重命名回车提交、Esc 取消后焦点回到那一行", async () => {
    setup([session(1, "甲"), session(2, "乙")]);
    await tick();
    fireEvent.contextMenu(await row("乙"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "重命名" }));
    const input = document.querySelector<HTMLInputElement>(".chat-sidebar-edit")!;
    fireEvent.change(input, { target: { value: "乙改" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await tick();
    expect(invoke).toHaveBeenCalledWith("rename_session", expect.objectContaining({ title: "乙改" }));
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("2");

    fireEvent.contextMenu(await row("乙"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "重命名" }));
    fireEvent.keyDown(document.querySelector(".chat-sidebar-edit")!, { key: "Escape" });
    await tick();
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("2");
  });
});

describe("S-4/S-5 失败提示有原因、不自动消失", () => {
  it("重命名失败：草稿带回编辑框，提示带原因，5 秒后还在，× 关闭", async () => {
    handlers.rename_session = () => { throw new Error("会话不存在"); };
    setup([session(1, "甲"), session(2, "修支付回调的重复扣款")]);
    await tick();
    fireEvent.contextMenu(await row("修支付回调"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "重命名" }));
    fireEvent.change(document.querySelector(".chat-sidebar-edit")!, { target: { value: "支付回调去重" } });
    fireEvent.keyDown(document.querySelector(".chat-sidebar-edit")!, { key: "Enter" });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(zh.chat.sidebarRenameFailed);
    expect(alert.textContent).toContain("会话不存在");
    const input = document.querySelector<HTMLInputElement>(".chat-sidebar-edit");
    expect(input?.value).toBe("支付回调去重");
    expect(document.activeElement).toBe(input);
    // 原先 4 秒就清掉了。
    await tick(5_000);
    expect(screen.getByRole("alert").textContent).toContain(zh.chat.sidebarRenameFailed);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.close }));
    expect(screen.queryByRole("alert")).toBeNull();
  }, 10_000);

  it("标记完成失败：说清动作和原因，会话拉回列表", async () => {
    handlers.set_archived = () => { throw new Error("worktree/busy"); };
    setup([session(1, "甲"), session(2, "乙")]);
    await tick();
    fireEvent.contextMenu(await row("乙"));
    fireEvent.click(await screen.findByRole("menuitem", { name: "标记完成" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(zh.chat.sidebarArchiveFailed);
    expect(alert.textContent).toContain("目录还被占用");
    expect(alert.textContent).not.toContain("请稍后再试");
  });

  it("对话内容搜索失败给「重试」", async () => {
    let calls = 0;
    handlers.search_chat_transcripts = () => { calls += 1; if (calls === 1) throw new Error("boom"); return []; };
    setup([session(1, "甲")]);
    await tick();
    fireEvent.change(screen.getByRole("textbox", { name: zh.chat.sidebarSearch }), { target: { value: "schema" } });
    await tick(350);
    fireEvent.click(await screen.findByRole("button", { name: zh.chat.searchTranscripts }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain(zh.chat.sidebarSearchFailed);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.sidebarRetry }));
    await waitFor(() => expect(calls).toBe(2));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });
});

describe("S-6～S-9 键盘", () => {
  it("S-6 按最近活动排序时 Ctrl+Shift+↓ 不挪焦点，提示并可一键改手动排序", async () => {
    localStorage.setItem(SIDEBAR_SORT_MODE_KEY, "recent");
    setup([session(1, "甲"), session(2, "乙")]);
    await tick();
    const jia = await row("甲");
    jia.focus();
    fireEvent.keyDown(jia, { key: "ArrowDown", ctrlKey: true, shiftKey: true });
    expect(document.activeElement).toBe(jia);
    expect(screen.getByRole("alert").textContent).toContain(zh.chat.sidebarReorderNeedsManual);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.sidebarSwitchToManual }));
    expect(localStorage.getItem(SIDEBAR_SORT_MODE_KEY)).toBe("manual");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("S-7 键盘挪位后读屏播报新位置", async () => {
    setup([session(3, "丙"), session(2, "乙"), session(1, "甲")]);
    await tick();
    const yi = await row("乙");
    yi.focus();
    fireEvent.keyDown(yi, { key: "ArrowUp", ctrlKey: true, shiftKey: true });
    expect(document.querySelector(".chat-sidebar-sronly")?.textContent).toBe(zh.chat.sidebarMoved(1, 3));
  });

  it("S-8 搜索框按 ↓ 进第一条结果，Enter 打开第一条命中", async () => {
    const { onSelect } = setup([session(1, "甲"), session(2, "乙")]);
    await tick();
    const box = screen.getByRole("textbox", { name: zh.chat.sidebarSearch });
    box.focus();
    fireEvent.keyDown(box, { key: "ArrowDown" });
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("1");
    box.focus();
    fireEvent.change(box, { target: { value: "甲" } });
    // 防抖未落、结果还是旧词的：回车不打开。
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSelect).not.toHaveBeenCalled();
    await tick(350);
    await tick();
    fireEvent.keyDown(box, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(1);
  });

  it("S-9 多选态 Shift+↓ 从锚点连选", async () => {
    setup([session(1, "甲"), session(2, "乙"), session(3, "丙"), session(4, "丁")]);
    await tick();
    fireEvent.contextMenu(await row("甲"));
    fireEvent.click(screen.getByRole("menuitem", { name: "多选" }));
    const jia = await row("甲");
    jia.focus();
    fireEvent.keyDown(jia, { key: "ArrowDown", shiftKey: true });
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown", shiftKey: true });
    expect(screen.getByText("已选 3 条")).toBeTruthy();
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("3");
  });
});
