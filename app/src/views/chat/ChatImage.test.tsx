import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { exportChatImage, resolveChatImage, revealTerminalPath } from "../../api";
import { ChatMarkdown } from "../ChatMarkdown";
import { ChatCwdContext } from "./chatCwd";
import { resetChatImageCache, sentImageArgs, sentImageReceipt, SentImageMessage } from "./ChatImage";
import { ToolActivity } from "./ToolActivity";
import { TurnLiveContext } from "./toolState";
import { Transcript } from "./Transcript";

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (path: string) => `asset://local/${path}`,
}));
const saveDialog = vi.hoisted(() => vi.fn(async () => "D:\\Downloads\\login.png" as string | null));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: saveDialog, open: vi.fn() }));
vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  resolveChatImage: vi.fn(),
  exportChatImage: vi.fn(async () => undefined),
  revealTerminalPath: vi.fn(async () => undefined),
}));

const CWD = "C:\\dev";
const IMG = "C:\\Users\\me\\.meowo\\images\\sent\\login.png";
const send = (id: string, args: Record<string, string>): Extract<ChatItem, { type: "tool_use" }> =>
  ({ type: "tool_use", id, timestamp: null, name: "mcp__meowo__send_image", kind: "image", summary: JSON.stringify(args) });
const result = (id: string, text: string, isError = false): Extract<ChatItem, { type: "tool_result" }> =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });

/** 按需触发的 IntersectionObserver：测试里决定什么时候「进视口」。 */
class FakeObserver {
  static all: FakeObserver[] = [];
  constructor(private readonly callback: IntersectionObserverCallback) { FakeObserver.all.push(this); }
  observe() {}
  disconnect() {}
  fire() { this.callback([{ isIntersecting: true } as IntersectionObserverEntry], this as unknown as IntersectionObserver); }
}

beforeEach(() => {
  resetChatImageCache();
  vi.mocked(resolveChatImage).mockReset();
  vi.mocked(exportChatImage).mockClear();
  vi.mocked(revealTerminalPath).mockClear();
  saveDialog.mockClear();
});
afterEach(() => {
  cleanup();
  delete (globalThis as Record<string, unknown>).__MEOWO_REMOTE__;
  vi.unstubAllGlobals();
  FakeObserver.all = [];
});

describe("agent 发来的图片消息", () => {
  it("参数取说明、回执取图片引用行；失败回执按原因码给人话", () => {
    expect(sentImageArgs(send("s", { source: "a.png", caption: "看这里" }))).toEqual({ source: "a.png", caption: "看这里" });
    // 后端约定：说明在 detail（kind image）里，summary 不是参数 JSON 时也要取得到
    const withDetail = { ...send("s", {}), summary: "a.png", detail: { kind: "image", source: "a.png", caption: " 新图标 " } } as Parameters<typeof sentImageArgs>[0];
    expect(sentImageArgs(withDetail)).toEqual({ source: "a.png", caption: "新图标" });
    expect(sentImageReceipt(result("s", `[Image: source: ${IMG}]`))).toEqual({ images: [IMG], error: null });
    expect(sentImageReceipt(result("s", "[Image error: too_large] 32 MB", true)).error).toEqual({ code: "too_large", message: "32 MB" });
  });

  it("图直接可见、带说明，不是收起的工具行；点开灯箱有保存与在文件夹中显示", async () => {
    render(<SentImageMessage item={send("s1", { source: "C:\\dev\\login.png", caption: "登录页改好了" })} result={result("s1", `[Image: source: ${IMG}]`)} />);
    const img = document.querySelector(".chat-sent img")!;
    expect(img.getAttribute("src")).toBe(`asset://local/${IMG}`);
    expect(img.closest("details")).toBeNull();
    expect(screen.getByText("登录页改好了")).toBeTruthy();
    // 图读到之前按钮不可点（尺寸未知），读到后才能开灯箱。
    Object.defineProperty(img, "naturalWidth", { value: 1280 });
    Object.defineProperty(img, "naturalHeight", { value: 800 });
    fireEvent.load(img);
    fireEvent.click(document.querySelector(".chat-sent-open")!);
    const box = document.querySelector(".chat-lightbox")!;
    expect(box.querySelector(".chat-lightbox-caption")?.textContent).toBe("登录页改好了");
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "保存" }).at(-1)!); });
    expect(saveDialog).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: "login.png" }));
    expect(exportChatImage).toHaveBeenCalledWith(IMG, "D:\\Downloads\\login.png");
    fireEvent.click(screen.getAllByRole("button", { name: "在文件夹中显示" }).at(-1)!);
    expect(revealTerminalPath).toHaveBeenCalledWith("", IMG);
  });

  it("长图标「长图」、小图标按原尺寸摆", () => {
    render(<SentImageMessage item={send("s2", { source: "x" })} result={result("s2", `[Image: source: ${IMG}]`)} />);
    const img = document.querySelector(".chat-sent img")!;
    Object.defineProperty(img, "naturalWidth", { value: 390 });
    Object.defineProperty(img, "naturalHeight", { value: 2600 });
    fireEvent.load(img);
    expect(document.querySelector(".chat-sent-tile")?.getAttribute("data-shape")).toBe("long");
    expect(screen.getByText("长图")).toBeTruthy();
  });

  it.each([
    ["not_found", "文件不存在"],
    ["not_image", "这个文件不是图片"],
    ["too_large", "图片太大，超过了上限"],
    ["download_failed", "网络图片下载失败"],
  ])("失败（%s）说清原因，不出空白框", (code, text) => {
    render(<SentImageMessage item={send("s3", { source: "C:\\dev\\old.png" })} result={result("s3", `[Image error: ${code}] whatever`, true)} />);
    expect(document.querySelector(".chat-sent-failed")?.textContent).toContain(text);
    expect(document.querySelector(".chat-sent-failed")?.textContent).toContain("old.png");
    expect(document.querySelector(".chat-sent img")).toBeNull();
  });

  it("没回执：回合在跑是「正在发送」，回合停了是「已中断」", () => {
    const item = send("s4", { source: "a.png" });
    const { rerender } = render(<TurnLiveContext.Provider value="running"><SentImageMessage item={item} /></TurnLiveContext.Provider>);
    expect(document.querySelector(".chat-sent-pending")?.textContent).toContain("正在发送图片");
    rerender(<TurnLiveContext.Provider value="idle"><SentImageMessage item={item} /></TurnLiveContext.Provider>);
    expect(document.querySelector(".chat-sent-failed")?.textContent).toContain("已中断");
  });

  it("图读不出来（文件被删）回退成说明卡", () => {
    render(<SentImageMessage item={send("s5", { source: "a.png" })} result={result("s5", `[Image: source: ${IMG}]`)} />);
    fireEvent.error(document.querySelector(".chat-sent img")!);
    expect(document.querySelector(".chat-sent-failed")?.textContent).toContain("图片文件读不到了");
  });

  it("手机端保存走浏览器下载，不调宿主命令，也不给在文件夹中显示", async () => {
    (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
    const clicked = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(<SentImageMessage item={send("s6", { source: "a.png" })} result={result("s6", `[Image: source: ${IMG}]`)} />);
    const img = document.querySelector(".chat-sent img")!;
    Object.defineProperty(img, "naturalWidth", { value: 800 });
    Object.defineProperty(img, "naturalHeight", { value: 600 });
    fireEvent.load(img);
    fireEvent.click(document.querySelector(".chat-sent-open")!);
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "保存" }).at(-1)!); });
    expect(clicked).toHaveBeenCalled();
    expect(saveDialog).not.toHaveBeenCalled();
    expect(exportChatImage).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "在文件夹中显示" })).toBeNull();
    clicked.mockRestore();
  });

  it("时间线里发图不进过程块：同一回合别的工具收起时图照样露在外面", () => {
    const items: ChatItem[] = [
      { type: "user_text", id: "u", timestamp: null, text: "发我看看" },
      { type: "tool_use", id: "b1", timestamp: null, name: "Bash", kind: "run", summary: "ls" },
      result("b1", "ok"),
      { type: "tool_use", id: "b2", timestamp: null, name: "Bash", kind: "run", summary: "pwd" },
      result("b2", "ok"),
      send("s7", { source: "a.png", caption: "这张" }),
      result("s7", `[Image: source: ${IMG}]`),
      { type: "assistant_text", id: "a", timestamp: null, text: "好了" },
    ];
    render(<Transcript sessionId={1} items={items} />);
    const fold = document.querySelector(".turn-fold")!;
    expect(fold.classList.contains("is-open")).toBe(false);
    const sent = document.querySelector(".chat-sent")!;
    expect(fold.contains(sent)).toBe(false);
    expect(sent.querySelector("img")).toBeTruthy();
    // 过程块摘要不把发图算进「已运行」。
    expect(fold.textContent).toContain("已运行 2 条命令");
  });
});

describe("工具行的图片", () => {
  it("任何类别的回执里有图片引用行都露缩略图（不只其他类）", () => {
    render(<ToolActivity item={{ type: "tool_use", id: "b", timestamp: null, name: "Bash", kind: "run", summary: "node shot.js" }} result={result("b", `saved\n[Image: source: ${IMG}]`)} />);
    expect(document.querySelector(".chat-tool-media img")?.getAttribute("src")).toBe(`asset://local/${IMG}`);
    expect(document.body.textContent).not.toContain("[Image: source:");
  });

  it("读图片：行尾写「图片」，显示图不露路径", () => {
    render(<ToolActivity
      item={{ type: "tool_use", id: "r", timestamp: null, name: "Read", kind: "read", summary: "a.png", detail: { kind: "read", path: "C:\\dev\\a.png", offset: null, limit: null } }}
      result={result("r", `[Image: source: ${IMG}]`)} />);
    expect(document.querySelector(".chat-tool-count")?.textContent).toBe("图片");
    expect(document.querySelector(".chat-tool-media img")).toBeTruthy();
    expect(document.querySelector(".chat-tool-body")?.textContent ?? "").not.toContain(".meowo");
  });

  it.each(["png", "jpg", "gif", "webp", "svg"])("写出 .%s 文件：交给后端解析出持久副本后露缩略图", async (ext) => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: `C:\\Users\\me\\.meowo\\images\\local\\icon.${ext}` });
    render(
      <ChatCwdContext.Provider value={CWD}>
        <ToolActivity
          item={{ type: "tool_use", id: "w", timestamp: null, name: "Write", kind: "write", summary: `icon.${ext}`, detail: { kind: "write", path: `C:\\dev\\icon.${ext}`, content: "", lines: 0, truncated: false } }}
          result={result("w", "ok")} />
      </ChatCwdContext.Provider>,
    );
    await act(async () => {});
    expect(resolveChatImage).toHaveBeenCalledWith(`C:\\dev\\icon.${ext}`, "C:\\dev");
    // SVG 也是 <img>，不内联进 DOM（img 上下文不执行脚本）。
    expect(document.querySelector(".chat-tool-media img")?.getAttribute("src")).toBe(`asset://local/C:\\Users\\me\\.meowo\\images\\local\\icon.${ext}`);
    expect(document.querySelector(".chat-tool-media svg:not(.icon)")).toBeNull();
  });

  it("写 .ts 不露缩略图，失败的写入也不露", () => {
    render(<ToolActivity item={{ type: "tool_use", id: "w2", timestamp: null, name: "Write", kind: "write", summary: "a.ts", detail: { kind: "write", path: "C:\\dev\\a.ts", content: "x", lines: 1, truncated: false } }} result={result("w2", "ok")} />);
    expect(document.querySelector(".chat-tool-media")).toBeNull();
    expect(resolveChatImage).not.toHaveBeenCalled();
  });
});

describe("正文 Markdown 图片", () => {
  it("本地相对路径按会话目录交给后端，拿到持久路径后显示", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\Users\\me\\.meowo\\images\\local\\arch.png" });
    render(<ChatCwdContext.Provider value={CWD}><div className="chat-md"><ChatMarkdown text={"看图：![架构](docs/arch.png)"} /></div></ChatCwdContext.Provider>);
    expect(document.querySelector(".chat-md-image")?.getAttribute("data-state")).toBe("loading");
    await act(async () => {});
    expect(resolveChatImage).toHaveBeenCalledWith("docs/arch.png", "C:\\dev");
    expect(document.querySelector(".chat-md-image img")?.getAttribute("src")).toBe("asset://local/C:\\Users\\me\\.meowo\\images\\local\\arch.png");
  });

  it("网络图片也走后端，页面不直连第三方", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\Users\\me\\.meowo\\images\\web\\ab12.png" });
    render(<ChatMarkdown text="![](https://example.com/a.png)" />);
    await act(async () => {});
    expect(resolveChatImage).toHaveBeenCalledWith("https://example.com/a.png", null);
    expect(document.querySelector("img[src^='http']")).toBeNull();
  });

  it("解析失败回退成文件名徽章并写明原因", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ error: { code: "not_found", message: "" } });
    render(<ChatMarkdown text="![旧首页](C:\\old\\home.png)" />);
    await act(async () => {});
    const chip = document.querySelector(".chat-image-chip")!;
    expect(chip.textContent).toContain("旧首页");
    expect(chip.textContent).toContain("文件不存在");
  });

  it("懒加载：进视口之前不解析也不请求图片", async () => {
    vi.stubGlobal("IntersectionObserver", FakeObserver);
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\i\\a.png" });
    render(<ChatMarkdown text="![](C:\\a.png)" />);
    await act(async () => {});
    expect(resolveChatImage).not.toHaveBeenCalled();
    expect(document.querySelector(".chat-md-image img")).toBeNull();
    await act(async () => { FakeObserver.all.forEach((observer) => observer.fire()); });
    expect(resolveChatImage).toHaveBeenCalledTimes(1);
    expect(document.querySelector(".chat-md-image img")).toBeTruthy();
  });

  it("发来的图同样进视口才请求", () => {
    vi.stubGlobal("IntersectionObserver", FakeObserver);
    render(<SentImageMessage item={send("s8", { source: "a.png" })} result={result("s8", `[Image: source: ${IMG}]`)} />);
    expect(document.querySelector(".chat-sent img")).toBeNull();
    act(() => FakeObserver.all.forEach((observer) => observer.fire()));
    expect(document.querySelector(".chat-sent img")).toBeTruthy();
  });

  it("同一张图重复挂载只解析一次（缓存）", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\i\\a.png" });
    const { unmount } = render(<ChatMarkdown text="![](C:\\a.png)" />);
    await act(async () => {});
    unmount();
    render(<ChatMarkdown text="![](C:\\a.png)" />);
    await act(async () => {});
    expect(resolveChatImage).toHaveBeenCalledTimes(1);
  });
});
