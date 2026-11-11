// 焦点规则回归（UI 优化包 1：i1、c3/m11、c13）：隐藏终端不进 Tab 序列、开窗首焦点给 composer、
// Tab 进审批卡先落「拒绝」、Esc 收理由框焦点回到「说明并拒绝」。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { afterEach, describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, respondWithHistory } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { markRemoteUi } from "../remoteMode";

const history = {
  sessionId: 41, title: "审批焦点", status: "running", provider: "claude", cwd: "C:/repo",
  supported: true, offset: 1, reset: false, pendingReview: "approval", ptyManaged: true,
  items: [{ type: "assistant_text", id: "a1", timestamp: null, text: "要跑测试" }],
};
const approval = {
  sessionId: 41, requestId: "req-focus", provider: "claude", toolName: "Bash",
  description: "运行测试", input: "{\"command\":\"cargo test\"}", permissionSuggestions: [],
};

afterEach(() => {
  document.documentElement.removeAttribute("data-im");
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = undefined;
  document.body.classList.remove("remote-ui");
});

/// 按浏览器的默认行为「打字」：焦点在按钮上时空格/回车 = 点击；在输入框里则追加字符，
/// 回车提交所在表单。c3 的误拒就是这条路径：焦点落在「说明并拒绝」，空格把它点开，
/// 后面的字进了理由框，回车即以它为理由拒绝。
function typeLikeBrowser(text: string) {
  for (const ch of text) {
    const el = document.activeElement as HTMLElement | null;
    if (!el) continue;
    if (el.tagName === "BUTTON") {
      if (ch === " " || ch === "\n") fireEvent.click(el);
      continue;
    }
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
      if (ch === "\n") {
        fireEvent.keyDown(el, { key: "Enter" });
        if (el instanceof HTMLInputElement && el.form) fireEvent.submit(el.form);
      } else {
        fireEvent.change(el, { target: { value: el.value + ch } });
      }
    }
  }
}

describe("ChatWindow 焦点规则", () => {
  it("开窗时有待批准请求：首焦点在 composer，直接打字回车不会点开理由框、不会拒绝", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory(history, approval);
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "允许一次" });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
    const composer = screen.getByRole("combobox", { name: "发送消息给 Agent" });
    expect(document.activeElement).toBe(composer);
    typeLikeBrowser("hello world\n");
    expect(screen.queryByPlaceholderText("说明拒绝原因…")).toBeNull();
    const denies = invoke.mock.calls.filter(([command, args]) => command === "resolve_pending_approval" && String(args?.choice).startsWith("deny"));
    expect(denies).toEqual([]);
  });

  it("手机远程不做挂载移焦：焦点既不进卡也不进 composer（避免 WebKit 画焦点环、弹键盘）", async () => {
    markRemoteUi();
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory(history, approval);
    render(<ChatWindow />);
    await screen.findByRole("button", { name: "允许一次" });
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
    expect(document.activeElement).toBe(document.body);
  });

  it("键盘 Tab 进审批卡时首个落点是「拒绝」，不是「说明并拒绝」", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory(history, approval);
    render(<ChatWindow />);
    const deny = await screen.findByRole("button", { name: /^拒绝/ });
    const card = screen.getByRole("alertdialog");
    // input-modality 在 Tab 按下时挂 data-im=kbd；随后浏览器把焦点交给卡里 DOM 序第一个可聚焦元素。
    document.documentElement.setAttribute("data-im", "kbd");
    const first = card.querySelector<HTMLElement>("button")!;
    expect(first).not.toBe(deny);
    act(() => { first.focus(); });
    expect(document.activeElement).toBe(deny);
    // 进卡后在卡内移动不再被改道：Shift+Tab 回到「说明并拒绝」照常可达。
    const reasonButton = screen.getByRole("button", { name: "说明并拒绝" });
    act(() => { reasonButton.focus(); });
    expect(document.activeElement).toBe(reasonButton);
  });

  it("鼠标点卡里的按钮不改道", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory(history, approval);
    render(<ChatWindow />);
    const reasonButton = await screen.findByRole("button", { name: "说明并拒绝" });
    act(() => { reasonButton.focus(); });
    expect(document.activeElement).toBe(reasonButton);
  });

  it("Esc 收起理由框：焦点回到「说明并拒绝」，不掉到 body", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory(history, approval);
    render(<ChatWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "说明并拒绝" }));
    const input = screen.getByPlaceholderText("说明拒绝原因…");
    act(() => { input.focus(); });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByPlaceholderText("说明拒绝原因…")).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("button", { name: "说明并拒绝" })));
  });

  it("对话视图下常驻的托管终端整块 inert，切回终端视图解除", async () => {
    window.history.replaceState({}, "", "/?sessionId=41");
    respondWithHistory({ ...history, pendingReview: null }, null);
    const { container } = render(<ChatWindow />);
    await screen.findByText("要跑测试");
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    const pane = await waitFor(() => {
      const el = container.querySelector(".chat-terminal-pane");
      expect(el).toBeTruthy();
      return el!;
    });
    expect(pane.hasAttribute("inert")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "终端" }));
    await waitFor(() => expect(container.querySelector(".chat-terminal-pane")!.className).toContain("is-background"));
    expect(container.querySelector(".chat-terminal-pane")!.hasAttribute("inert")).toBe(true);
  });
});
