import { describe, it, expect, vi, afterEach, beforeEach, type MockInstance } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../remoteMode", async (orig) => ({ ...(await orig<typeof import("../remoteMode")>()), remoteUi: () => true }));
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { ChatMarkdown } from "./ChatMarkdown";
import { setPreviewSession } from "./chat/phonePreview";

// 手机远程页：对话里的本机链接经电脑上的预览代理打开（phonePreview.ts），打不开就地说原因。
describe("手机上点对话里的本机链接", () => {
  let open: MockInstance<typeof window.open>;
  beforeEach(() => {
    setPreviewSession(7);
    open = vi.spyOn(window, "open").mockImplementation(() => null);
  });
  afterEach(() => {
    cleanup();
    invoke.mockReset();
    invoke.mockImplementation(() => Promise.resolve(null));
    open.mockRestore();
    setPreviewSession(null);
  });

  it("正文链接：走预览，打不开时在链接后面写出原因", async () => {
    invoke.mockImplementation(() => Promise.reject(new Error("preview/not_listening")));
    const { container } = render(<ChatMarkdown text={"打开 [http://localhost:5173](http://localhost:5173) 看看"} />);
    fireEvent.click(container.querySelector("a")!);
    expect(invoke).toHaveBeenCalledWith("preview_open", { sessionId: 7, url: "http://localhost:5173" });
    await waitFor(() => expect(container.querySelector(".chat-md-link-error [role=status]")?.textContent).toContain("预览还没启动"));
  });

  it("行内代码里的本机网址同样走预览", () => {
    const { container } = render(<ChatMarkdown text={"跑在 `http://127.0.0.1:3000/app` 上"} />);
    fireEvent.click(container.querySelector("code.chat-md-link-code")!);
    expect(invoke).toHaveBeenCalledWith("preview_open", { sessionId: 7, url: "http://127.0.0.1:3000/app" });
  });

  it("外部网址照旧交给浏览器，不碰预览", () => {
    const { container } = render(<ChatMarkdown text={"[PR](https://github.com/x/y/pull/1)"} />);
    fireEvent.click(container.querySelector("a")!);
    expect(invoke).not.toHaveBeenCalledWith("preview_open", expect.anything());
    expect(invoke).toHaveBeenCalledWith("open_link", expect.objectContaining({ url: "https://github.com/x/y/pull/1" }));
  });
});
