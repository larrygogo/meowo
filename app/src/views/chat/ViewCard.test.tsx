import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { ChatActionsContext, type ChatActions } from "./chatActions";
import { parseChoicesReceipt } from "./ChoiceCard";
import { isImageMessage } from "./imageTools";
import { parseViewReceipt, ViewMessage } from "./ViewCard";

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
    const { frame, value } = mount();
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/message", params: { role: "user", content: { type: "text", text: "  就用「弹一下」 " } } });
    expect(value.fillPrompt).toHaveBeenCalledWith("就用「弹一下」");
    expect(value.sendText).not.toHaveBeenCalled();
    post(frame.contentWindow, { jsonrpc: "2.0", method: "ui/message", params: { content: { type: "text", text: "长".repeat(5000) } } });
    expect((value.fillPrompt as ReturnType<typeof vi.fn>).mock.calls[1][0]).toHaveLength(2000);
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

  it("回执是失败时给原因，不出 iframe", () => {
    render(<ViewMessage item={call()} result={result("没给 title：写一个几个字的标题", true)} />);
    expect(document.querySelector("iframe")).toBeNull();
    expect(screen.getByText(/没给 title/)).toBeTruthy();
  });
});
