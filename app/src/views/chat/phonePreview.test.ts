import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
let remote = true;
vi.mock("../../remoteMode", () => ({ remoteUi: () => remote }));

import { openPhonePreview, previewUrl, setPreviewSession } from "./phonePreview";

type FakeWin = { closed: boolean; opener: unknown; document: Document; location: { replace: ReturnType<typeof vi.fn> }; close: ReturnType<typeof vi.fn> };

function fakeWindow(): FakeWin {
  const doc = document.implementation.createHTMLDocument("");
  const win: FakeWin = { closed: false, opener: window, document: doc, location: { replace: vi.fn() }, close: vi.fn(() => { win.closed = true; }) };
  return win;
}

describe("手机预览：点本机链接经电脑上的代理打开", () => {
  let open: MockInstance<typeof window.open>;
  let win: FakeWin;

  beforeEach(() => {
    remote = true;
    invoke.mockReset();
    win = fakeWindow();
    open = vi.spyOn(window, "open").mockImplementation(() => win as unknown as Window);
    setPreviewSession(42);
  });
  afterEach(() => {
    open.mockRestore();
    setPreviewSession(null);
  });

  it("点的同一拍就开新页（iOS 只认手势里同步的 window.open），拿到地址再导过去", async () => {
    let resolve!: (v: unknown) => void;
    invoke.mockReturnValue(new Promise((r) => { resolve = r; }));
    const pending = openPhonePreview("http://localhost:5173/app?x=1", "正在打开预览…");
    expect(pending).not.toBeNull();
    // 后端还没回话，新页已经开了，写着「正在打开」，并且切断了 opener。
    expect(open).toHaveBeenCalledTimes(1);
    expect(win.document.body.textContent).toBe("正在打开预览…");
    expect(win.opener).toBeNull();
    expect(invoke).toHaveBeenCalledWith("preview_open", { sessionId: 42, url: "http://localhost:5173/app?x=1" });
    resolve({ port: 5173, path: "/__meowo/preview?c=abc" });
    await pending;
    expect(win.location.replace).toHaveBeenCalledWith(`${window.location.protocol}//${window.location.hostname}:5173/__meowo/preview?c=abc`);
  });

  it("打不开：关掉空页，把后端的原因交给调用方就地显示", async () => {
    invoke.mockRejectedValue(new Error("preview/not_listening"));
    const pending = openPhonePreview("http://127.0.0.1:3000/", "…");
    await expect(pending).rejects.toThrow("preview/not_listening");
    expect(win.close).toHaveBeenCalled();
    expect(win.location.replace).not.toHaveBeenCalled();
  });

  it("不归它管的照旧走原来的打开方式：桌面端、外部网址、没有会话", () => {
    remote = false;
    expect(openPhonePreview("http://localhost:5173/", "…")).toBeNull();
    remote = true;
    expect(openPhonePreview("https://github.com/", "…")).toBeNull();
    setPreviewSession(null);
    expect(openPhonePreview("http://localhost:5173/", "…")).toBeNull();
    expect(open).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("预览地址用手机此刻访问 Meowo 的主机名（局域网 IP、Tailscale、IPv6 都原样）", () => {
    const ticket = { port: 5173, path: "/__meowo/preview?c=x" };
    expect(previewUrl(ticket, { protocol: "http:", hostname: "192.168.5.108" })).toBe("http://192.168.5.108:5173/__meowo/preview?c=x");
    expect(previewUrl(ticket, { protocol: "http:", hostname: "[fd7a::1]" })).toBe("http://[fd7a::1]:5173/__meowo/preview?c=x");
  });
});
