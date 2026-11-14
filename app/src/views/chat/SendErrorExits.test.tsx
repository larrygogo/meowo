// 错误条出路（走查 C-4）与移除后焦点去处（走查 C-10）的单元回归。
// 兄弟场景（桌面 / 手机）：桌面「去终端页」；手机远程没有终端页，同一个出路换成就地展开的终端
// 画面与按键（RemoteTerminalKeys），不许出现点了没反应的「去终端页」。
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn(() => Promise.resolve(["❯ 1. Yes", "  2. No"])) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { zh } from "../../i18n/zh";
import { markRemoteUi } from "../../remoteMode";
import { SendErrorExits } from "./SendErrorExits";
import { focusAfterRemoval } from "./focusAfterRemoval";

afterEach(() => {
  cleanup();
  invoke.mockClear();
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
});

const labels = () => screen.getAllByRole("button").map((button) => button.textContent);

describe("SendErrorExits", () => {
  it("桌面：重试先把焦点交给输入框再重做；看终端去终端页；收起也回输入框", () => {
    const retry = vi.fn();
    const onGoTerminal = vi.fn();
    const onDismiss = vi.fn();
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    render(<SendErrorExits exit={{ retry, terminal: true }} sessionId={1} onGoTerminal={onGoTerminal} onDismiss={onDismiss} focusComposer={() => composer.focus()} />);
    expect(labels()).toEqual([zh.chat.errorRetry, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.errorRetry }));
    expect(retry).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(composer);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.goTerminal }));
    expect(onGoTerminal).toHaveBeenCalledTimes(1);
    composer.blur();
    fireEvent.click(screen.getByRole("button", { name: zh.chat.slashMenuDismiss }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(composer);
    composer.remove();
  });

  it("没有出路的错误也有「收起」；按钮文案可按场景换（残图拦截说「已清理残图」，撤回残留说「清空终端输入框」）", () => {
    const { rerender } = render(<SendErrorExits exit={{}} sessionId={1} onGoTerminal={() => {}} onDismiss={() => {}} focusComposer={() => {}} />);
    expect(labels()).toEqual([zh.chat.slashMenuDismiss]);
    rerender(<SendErrorExits exit={{ terminal: true, retry: () => {}, retryLabel: zh.chat.undoClearInput }} dismissLabel={zh.chat.imageDraftCleared} sessionId={1} onGoTerminal={() => {}} onDismiss={() => {}} focusComposer={() => {}} />);
    expect(labels()).toEqual([zh.chat.undoClearInput, zh.chat.goTerminal, zh.chat.imageDraftCleared]);
  });

  it("手机远程：没有「去终端页」，换成就地展开终端画面与按键；也不抢焦点（免得弹键盘）", async () => {
    markRemoteUi();
    const onGoTerminal = vi.fn();
    const focusComposer = vi.fn();
    render(<SendErrorExits exit={{ terminal: true, retry: () => {} }} sessionId={7} onGoTerminal={onGoTerminal} onDismiss={() => {}} focusComposer={focusComposer} />);
    expect(screen.queryByRole("button", { name: zh.chat.goTerminal })).toBeNull();
    const keys = screen.getByRole("button", { name: zh.chat.remoteOpenKeys });
    expect(keys.getAttribute("aria-expanded")).toBe("false");
    await act(async () => { fireEvent.click(keys); });
    expect(keys.getAttribute("aria-expanded")).toBe("true");
    expect(await screen.findByLabelText(zh.chat.remoteScreenLabel)).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("managed_terminal_screen", expect.objectContaining({ sessionId: 7 }));
    fireEvent.click(screen.getByRole("button", { name: zh.chat.errorRetry }));
    expect(focusComposer).not.toHaveBeenCalled();
    expect(onGoTerminal).not.toHaveBeenCalled();
  });

  // 终端草稿读不全时由人选择（做空①）：两个选择排在最前；会丢内容的那个要点两次。
  it("由人决定的选择：直接执行的点一次，带确认的第一下只换文案、第二下才执行", () => {
    const keep = vi.fn();
    const discard = vi.fn();
    const actions = [
      { label: zh.chat.composerSendWithDraft, run: keep },
      { label: zh.chat.composerDiscardDraft, confirmLabel: zh.chat.composerDiscardConfirm, run: discard },
    ];
    render(<SendErrorExits exit={{ terminal: true, actions }} sessionId={1} onGoTerminal={() => {}} onDismiss={() => {}} focusComposer={() => {}} />);
    expect(labels()).toEqual([zh.chat.composerSendWithDraft, zh.chat.composerDiscardDraft, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerDiscardDraft }));
    expect(discard).not.toHaveBeenCalled();
    expect(labels()).toEqual([zh.chat.composerSendWithDraft, zh.chat.composerDiscardConfirm, zh.chat.goTerminal, zh.chat.slashMenuDismiss]);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerDiscardConfirm }));
    expect(discard).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: zh.chat.composerSendWithDraft }));
    expect(keep).toHaveBeenCalledTimes(1);
  });

  it("手机远程：两个选择同样在，看终端画面是就地展开", () => {
    markRemoteUi();
    const actions = [
      { label: zh.chat.composerSendWithDraft, run: () => {} },
      { label: zh.chat.composerDiscardDraft, confirmLabel: zh.chat.composerDiscardConfirm, run: () => {} },
    ];
    render(<SendErrorExits exit={{ terminal: true, actions }} sessionId={7} onGoTerminal={() => {}} onDismiss={() => {}} focusComposer={() => {}} />);
    expect(labels()).toEqual([zh.chat.composerSendWithDraft, zh.chat.composerDiscardDraft, zh.chat.remoteOpenKeys, zh.chat.slashMenuDismiss]);
  });
});

describe("focusAfterRemoval", () => {
  const list = () => {
    const ul = document.createElement("div");
    ul.className = "chat-attachments";
    for (const name of ["a", "b", "c"]) {
      const button = document.createElement("button");
      button.setAttribute("data-attachment-remove", "");
      button.textContent = name;
      ul.appendChild(button);
    }
    document.body.appendChild(ul);
    return ul;
  };
  const frame = () => new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
  const opts = (fallback: HTMLElement | null) => ({ list: ".chat-attachments", item: "[data-attachment-remove]", fallback: () => fallback });

  it("删中间一项：焦点落原位置（即下一项）；删末项落上一项；删光交给 fallback", async () => {
    const ul = list();
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    const [, b, c] = [...ul.children] as HTMLElement[];
    b.focus();
    focusAfterRemoval(b, opts(composer));
    b.remove();
    await frame();
    expect(document.activeElement).toBe(c);
    focusAfterRemoval(c, opts(composer));
    c.remove();
    await frame();
    expect(document.activeElement?.textContent).toBe("a");
    const a = document.activeElement as HTMLElement;
    focusAfterRemoval(a, opts(composer));
    ul.remove();
    await frame();
    expect(document.activeElement).toBe(composer);
    composer.remove();
  });

  it("被点的按钮没持有焦点（WebKit 鼠标点按钮不聚焦）：不动焦点，免得手机上弹键盘", async () => {
    const ul = list();
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    const first = ul.children[0] as HTMLElement;
    focusAfterRemoval(first, opts(composer));
    first.remove();
    await frame();
    expect(document.activeElement).toBe(document.body);
    ul.remove();
    composer.remove();
  });
});
