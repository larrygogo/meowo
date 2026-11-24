import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { ChatActionsContext, type ChatActions } from "./chatActions";
import { ChoiceMessage, parseChoicesReceipt } from "./ChoiceCard";
import { isImageMessage } from "./imageTools";
import { parseViewReceipt, ViewMessage } from "./ViewCard";
import { formatChoiceAnswer } from "./choiceReply";
import { zh } from "../../i18n/zh";

// 页面里点出来的外部链接走 api.openLink（系统浏览器）：换成计数，看越过门的有几个。
const { openLink } = vi.hoisted(() => ({ openLink: vi.fn(async () => {}) }));
vi.mock("../../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../../api")>()), openLink }));

afterEach(cleanup);

// 测试环境没有 Tauri：convertFileSrc 按 Windows 的 http://<协议>.localhost/ 形态给一个地址即可。
beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    convertFileSrc: (path: string, protocol = "asset") => `http://${protocol}.localhost/${encodeURIComponent(path)}`,
  };
});

type ToolUse = Extract<ChatItem, { type: "tool_use" }>;
type ToolResult = Extract<ChatItem, { type: "tool_result" }>;
const ID = "a".repeat(32);
const call = (id = "v1"): ToolUse => ({ type: "tool_use", id, timestamp: "t1", name: "mcp__meowo__show_html", kind: "other", summary: "" } as ToolUse);
const result = (text: string, isError = false): ToolResult => ({ type: "tool_result", id: "r1", timestamp: null, tool_use_id: "v1", text, is_error: isError });
const receipt = (extra: Record<string, unknown> = {}) => `视图已显示给用户。\n[View: ${JSON.stringify({ title: "动效对比", id: ID, bytes: 2048, height: null, ...extra })}]`;

function mount(text = receipt(), actions: Partial<ChatActions> = {}) {
  const value: ChatActions = { sendText: vi.fn(async () => true), fillPrompt: vi.fn(), ...actions };
  render(<ChatActionsContext.Provider value={value}><ViewMessage item={call()} result={result(text)} /></ChatActionsContext.Provider>);
  const frame = document.querySelector("iframe.chat-view-frame") as HTMLIFrameElement;
  return { value, frame };
}

/** 用户点进 iframe：焦点落到 iframe 上、父窗口失焦（宿主在这一刻给这块页面一次动作机会）。 */
function clickInto(frame: HTMLIFrameElement) {
  act(() => {
    frame.focus();
    window.dispatchEvent(new Event("blur"));
  });
}

function post(source: MessageEventSource | null, data: unknown) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, source }));
  });
}

describe("视图卡回执", () => {
  it("取出标题、指纹、高度；指纹不是 32 位十六进制就不认", () => {
    expect(parseViewReceipt(receipt({ height: 300 }))).toEqual({ title: "动效对比", id: ID, bytes: 2048, height: 300 });
    expect(parseViewReceipt(receipt({ id: "../../etc/passwd" }))).toBeNull();
    expect(parseViewReceipt(receipt({ id: ID.toUpperCase() }))).toBeNull();
    expect(parseViewReceipt("[View: 不是 JSON]")).toBeNull();
  });

  it("排在正文里，不收进过程块", () => {
    expect(isImageMessage({ kind: "other", name: "mcp__meowo__show_html" })).toBe(true);
  });

  it("方案卡里的 view 只认合规指纹", () => {
    const text = `x\n[Choices: ${JSON.stringify({ question: "q", options: [{ label: "a", view: ID }, { label: "b", view: "C:/evil.html" }] })}]`;
    const parsed = parseChoicesReceipt(text)!;
    expect(parsed.options[0].view).toBe(ID);
    expect(parsed.options[1].view).toBeNull();
  });
});

describe("视图卡沙箱与消息", () => {
  it("iframe 只给 allow-scripts，地址走 meowo-view 协议并带主题", () => {
    const { frame } = mount();
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.src).toBe(`http://meowo-view.localhost/${ID}.html?theme=dark`);
    expect(frame.getAttribute("referrerpolicy")).toBe("no-referrer");
  });

  it("页面报的高度生效，但不超过半屏上限", () => {
    const { frame } = mount();
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 333 } });
    expect(frame.style.height).toBe("333px");
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 99999 } });
    expect(parseInt(frame.style.height, 10)).toBeLessThan(99999);
    expect(parseInt(frame.style.height, 10)).toBe(Math.max(240, Math.round(window.innerHeight * 0.55)));
  });

  it("agent 给了 height 就固定，不跟页面报的走", () => {
    const { frame } = mount(receipt({ height: 260 }));
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 120 } });
    expect(frame.style.height).toBe("260px");
  });

  it("meowo.send 只放进输入框，不替用户发送；超长截断", () => {
    vi.useFakeTimers({ toFake: ["Date", "performance"] });
    const { frame, value } = mount();
    clickInto(frame);
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/message", params: { role: "user", content: { type: "text", text: "  就用「弹一下」 " } } });
    expect(value.fillPrompt).toHaveBeenCalledWith("就用「弹一下」");
    expect(value.sendText).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1500);
    clickInto(frame);
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text: "长".repeat(5000) } } });
    expect((value.fillPrompt as ReturnType<typeof vi.fn>).mock.calls[1][0]).toHaveLength(2000);
    vi.useRealTimers();
  });

  it("用户没在用这块页面时不收：没点进来、点进来前很久发的、点一次连发", () => {
    vi.useFakeTimers({ toFake: ["Date", "performance"] });
    const { frame, value } = mount();
    const message = { jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text: "加载就塞" } } };
    post(frame.contentWindow, message);
    expect(value.fillPrompt).not.toHaveBeenCalled();
    // 隔了很久用户才点进来：之前压着的那条已过期，不能借这次点击放出去。
    vi.advanceTimersByTime(5000);
    clickInto(frame);
    expect(value.fillPrompt).not.toHaveBeenCalled();
    post(frame.contentWindow, message);
    post(frame.contentWindow, message);
    post(frame.contentWindow, message);
    expect(value.fillPrompt).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("消息比父窗口失焦先到（跨进程顺序不定）也认：压住等失焦；点击留下的机会 1.5 秒后作废", () => {
    vi.useFakeTimers({ toFake: ["Date", "performance"] });
    const { frame, value } = mount();
    const message = (text: string) => ({ jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text } } });
    post(frame.contentWindow, message("先到的消息"));
    expect(value.fillPrompt).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    clickInto(frame);
    expect(value.fillPrompt).toHaveBeenCalledWith("先到的消息");
    // 再点一次，但页面攒着不用，过 2 秒再自己发：不认。
    vi.advanceTimersByTime(1200);
    clickInto(frame);
    vi.advanceTimersByTime(2000);
    post(frame.contentWindow, message("攒着的"));
    expect(value.fillPrompt).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("点一次只能开一个链接：开完焦点收回，后面的不放行（审查第 2 轮 linkSpam）", () => {
    vi.useFakeTimers({ toFake: ["Date", "performance"] });
    openLink.mockClear();
    const { frame } = mount();
    clickInto(frame);
    for (let i = 0; i < 5; i += 1) {
      post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/open-link", params: { url: `https://example.com/${i}` } });
      vi.advanceTimersByTime(1100);
    }
    expect(document.activeElement).not.toBe(frame);
    expect(openLink).toHaveBeenCalledTimes(1);
    expect(openLink).toHaveBeenCalledWith("https://example.com/0");
    vi.useRealTimers();
  });

  it("换主题时告诉页面，不重载", async () => {
    const { frame } = mount();
    const spy = vi.spyOn(frame.contentWindow!, "postMessage");
    const src = frame.src;
    // 主题经 MutationObserver 通知，回调是异步的。
    await act(async () => { document.documentElement.setAttribute("data-theme", "light"); await Promise.resolve(); });
    expect(spy).toHaveBeenCalledWith({ jsonrpc: "2.0", method: "ui/notifications/host-context-changed", params: { theme: "light" } }, "*");
    expect(frame.src).toBe(src);
    await act(async () => { document.documentElement.removeAttribute("data-theme"); await Promise.resolve(); });
  });

  it("别处来的消息（伪造来源、父窗口自己、格式不对）一律不理", () => {
    const { frame, value } = mount();
    const message = { jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text: "伪造" } } };
    post(window, message);
    post(null, message);
    post(frame.contentWindow, { method: "ui/message", params: message.params });
    post(frame.contentWindow, "ui/message");
    post(window, { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 400 } });
    expect(value.fillPrompt).not.toHaveBeenCalled();
    expect(frame.style.height).toBe("200px");
  });

  it("快照被清理：卡片换成失效说明，不留空白框", () => {
    const { frame } = mount();
    post(frame.contentWindow, { jsonrpc: "2.0", method: "meowo/view-missing", params: {} });
    expect(document.querySelector("iframe")).toBeNull();
    expect(screen.getByText("页面已被清理，可以让 AI 重新发一次")).toBeTruthy();
  });

  it("放大后 Esc 收回", () => {
    mount();
    fireEvent.click(screen.getByLabelText("放大"));
    expect(document.querySelector(".chat-view.is-zoomed")).not.toBeNull();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector(".chat-view.is-zoomed")).toBeNull();
  });

  it("焦点在页面里按 Esc（桥转告 meowo/escape）也收回放大；没放大时不理", () => {
    const { frame } = mount();
    post(frame.contentWindow, { jsonrpc: "2.0", method: "meowo/escape", params: {} });
    fireEvent.click(screen.getByLabelText("放大"));
    clickInto(frame);
    post(frame.contentWindow, { jsonrpc: "2.0", method: "meowo/escape", params: {} });
    expect(document.querySelector(".chat-view.is-zoomed")).toBeNull();
    expect(document.querySelector("iframe.chat-view-frame")).toBe(frame);
  });

  it("回执是失败时给原因，不出 iframe", () => {
    render(<ViewMessage item={call()} result={result("没给 title：写一个几个字的标题", true)} />);
    expect(document.querySelector("iframe")).toBeNull();
    expect(screen.getByText(/没给 title/)).toBeTruthy();
  });
});

describe("方案卡里的 HTML 方案", () => {
  const choicesReceipt = `方案卡已显示给用户。
[Choices: ${JSON.stringify({ question: "导出按钮用哪种？", options: [
    { label: "实心", description: null, image: null, view: "3".repeat(32) },
    { label: "胶囊", description: "更醒目", image: null, view: "4".repeat(32) },
  ] })}]`;
  const choiceCall = { type: "tool_use", id: "c1", timestamp: "t2", name: "mcp__meowo__offer_choices", kind: "other", summary: "" } as ToolUse;
  const choiceResult: ToolResult = { type: "tool_result", id: "rc1", timestamp: null, tool_use_id: "c1", text: choicesReceipt, is_error: false };

  it("缩略图不可交互，也不接收页面消息", () => {
    const fillPrompt = vi.fn();
    render(<ChatActionsContext.Provider value={{ sendText: vi.fn(async () => true), fillPrompt }}><ChoiceMessage item={choiceCall} result={choiceResult} followUps={[]} sessionId={1} /></ChatActionsContext.Provider>);
    const thumb = document.querySelector("iframe.chat-view-thumb-frame") as HTMLIFrameElement;
    expect(thumb.getAttribute("sandbox")).toBe("allow-scripts");
    expect(thumb.style.pointerEvents).toBe("none");
    expect(thumb.tabIndex).toBe(-1);
    post(thumb.contentWindow, { jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text: "缩略图塞字" } } });
    expect(fillPrompt).not.toHaveBeenCalled();
  });

  it("放大后能「选这个」，发出的消息和截图方案卡同一格式", async () => {
    const sendText = vi.fn(async () => true);
    render(<ChatActionsContext.Provider value={{ sendText }}><ChoiceMessage item={choiceCall} result={choiceResult} followUps={[]} sessionId={2} /></ChatActionsContext.Provider>);
    fireEvent.click(screen.getByLabelText(zh.chat.choicesZoom(2)));
    expect(document.querySelector(".chat-view.is-zoomed iframe")).not.toBeNull();
    fireEvent.click(screen.getByText(zh.chat.choicesPickThis(2)));
    expect(document.querySelector(".chat-view.is-zoomed")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByText(zh.chat.choicesSend(2))); });
    expect(sendText).toHaveBeenCalledWith(formatChoiceAnswer({ kind: "picked", index: 1, note: null }, { question: "导出按钮用哪种？", labels: ["实心", "胶囊"] }, "zh"));
  });
});
