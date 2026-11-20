import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { exportChatImage, resolveChatImage, revealTerminalPath } from "../../api";
import { ChatMarkdown } from "../ChatMarkdown";
import { ChatCwdContext } from "./chatCwd";
import { resetChatImageCache, sentImageArgs, sentImageReceipt, SentImageMessage } from "./ChatImage";
import { FileMarkdown } from "./FileMarkdown";
import { ChatSessionContext, resetImageHostGate } from "./remoteImageGate";
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
  resetImageHostGate();
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

  it("发图给了网址（现在一律拒）：说清只能发本地图片、要先下载到本地，不说成「位置不允许读取」", () => {
    render(<SentImageMessage item={send("s4", { source: "https://example.com/banner.png" })} result={result("s4", "[Image error: forbidden] send_image 只接受本地图片文件，不下载网址。", true)} />);
    const card = document.querySelector(".chat-sent-failed")?.textContent ?? "";
    expect(card).toContain("只能发本地图片");
    expect(card).toContain("banner.png");
    expect(card).not.toContain("这个位置不允许读取");
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

  it("回执里指向网络共享的引用行不进 asset 协议（不发起 SMB），显示成读不到", () => {
    for (const forged of ["//attacker/s/x.png", "\\\\attacker\\s\\y.png", "file://attacker/s/z.png"]) {
      cleanup();
      render(<ToolActivity item={{ type: "tool_use", id: "f", timestamp: null, name: "Bash", kind: "run", summary: "curl" }} result={result("f", `page
[Image: source: ${forged}]`)} />);
      act(() => FakeObserver.all.forEach((o) => o.fire()));
      expect(document.querySelector("img[src*='attacker']"), forged).toBeNull();
      expect([...document.querySelectorAll("img")].every((img) => !img.getAttribute("src")?.includes("attacker"))).toBe(true);
    }
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
    expect(resolveChatImage).toHaveBeenCalledWith(`C:\\dev\\icon.${ext}`, "C:\\dev", null);
    // SVG 不内联进 DOM，也不直接给 <img>：显示后端栅格化的 `<原件>.png`（做空第四轮）。
    const shown = ext === "svg" ? `icon.${ext}.png` : `icon.${ext}`;
    expect(document.querySelector(".chat-tool-media img")?.getAttribute("src")).toBe(`asset://local/C:\\Users\\me\\.meowo\\images\\local\\${shown}`);
    expect(document.querySelector(".chat-tool-media svg:not(.icon)")).toBeNull();
  });

  it("同一路径写三次、回执各带当时副本的引用行：三行显示三张不同的图，不按路径现解析", async () => {
    const write = (id: string): Extract<ChatItem, { type: "tool_use" }> =>
      ({ type: "tool_use", id, timestamp: null, name: "Write", kind: "write", summary: "logo.png", detail: { kind: "write", path: "C:\\dev\\logo.png", content: "", lines: 0, truncated: false } });
    const copy = (hash: string) => `C:\\Users\\me\\.meowo\\images\\tool\\${hash}.png`;
    const items: ChatItem[] = [
      { type: "user_text", id: "u", timestamp: null, text: "改三版图标" },
      write("w1"), result("w1", `File written\n[Image: source: ${copy("aaa1")}]`),
      write("w2"), result("w2", `File written\n[Image: source: ${copy("bbb2")}]`),
      write("w3"), result("w3", `File written\n[Image: source: ${copy("ccc3")}]`),
      { type: "assistant_text", id: "a", timestamp: null, text: "好了" },
    ];
    render(<ChatCwdContext.Provider value={CWD}><Transcript sessionId={1} items={items} /></ChatCwdContext.Provider>);
    // 过程块收着时缩略图露在摘要行下；展开后每行各一张。两处都应是三张不同的副本。
    const folded = [...document.querySelectorAll(".chat-tool-media img")].map((img) => img.getAttribute("src"));
    expect(folded).toEqual(["aaa1", "bbb2", "ccc3"].map((hash) => `asset://local/${copy(hash)}`));
    fireEvent.click(document.querySelector(".turn-fold-head")!);
    await act(async () => {});
    const rows = [...document.querySelectorAll("details.chat-tool")].map((row) => row.nextElementSibling?.querySelector("img")?.getAttribute("src"));
    expect(new Set(rows).size).toBe(3);
    expect(rows).toEqual(["aaa1", "bbb2", "ccc3"].map((hash) => `asset://local/${copy(hash)}`));
    expect(resolveChatImage).not.toHaveBeenCalled();
  });

  it("旧数据（回执没有引用行）才按 detail.path 现解析", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\Users\\me\\.meowo\\images\\local\\logo.png" });
    render(<ChatCwdContext.Provider value={CWD}><ToolActivity
      item={{ type: "tool_use", id: "w0", timestamp: null, name: "Write", kind: "write", summary: "logo.png", detail: { kind: "write", path: "C:\\dev\\logo.png", content: "", lines: 0, truncated: false } }}
      result={result("w0", "File written")} /></ChatCwdContext.Provider>);
    await act(async () => {});
    expect(resolveChatImage).toHaveBeenCalledWith("C:\\dev\\logo.png", "C:\\dev", null);
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
    expect(resolveChatImage).toHaveBeenCalledWith("docs/arch.png", "C:\\dev", null);
    expect(document.querySelector(".chat-md-image img")?.getAttribute("src")).toBe("asset://local/C:\\Users\\me\\.meowo\\images\\local\\arch.png");
  });

  it("网络图片点了才走后端下载，页面不直连第三方", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: "C:\\Users\\me\\.meowo\\images\\web\\ab12.png" });
    render(<ChatMarkdown text="![](https://example.com/a.png)" />);
    await act(async () => {});
    expect(resolveChatImage).not.toHaveBeenCalled();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /example\.com/ })); });
    expect(resolveChatImage).toHaveBeenCalledWith("https://example.com/a.png", null, null);
    expect(document.querySelector(".chat-md-image img")?.getAttribute("src")).toBe("asset://local/C:\\Users\\me\\.meowo\\images\\web\\ab12.png");
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

describe("网络图片点了才加载", () => {
  // String.raw：普通字符串里的 `\U`、`\m` 会被吃成字母，路径变成 `C:Usersme…`（不是绝对路径，白名单不认）。
  const WEB = String.raw`C:\Users\me\.meowo\images\web\ab12.png`;
  const md = (text: string, session: number | null = 7) => (
    <ChatSessionContext.Provider value={session}><div className="chat-md"><ChatMarkdown text={text} /></div></ChatSessionContext.Provider>
  );

  it("进了视口也不下载，只显示域名和「点击加载图片」", async () => {
    vi.stubGlobal("IntersectionObserver", FakeObserver);
    vi.mocked(resolveChatImage).mockResolvedValue({ path: WEB });
    render(md("![图表](https://cdn.example.com/chart.png)"));
    await act(async () => { FakeObserver.all.forEach((observer) => observer.fire()); });
    expect(resolveChatImage).not.toHaveBeenCalled();
    const gate = document.querySelector("button.chat-image-chip.is-gated")!;
    expect(gate.textContent).toContain("cdn.example.com");
    expect(gate.textContent).toContain("点击加载图片");
    expect(document.querySelector("img")).toBeNull();
  });

  it("点击后才调后端，带上会话 id", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: WEB });
    render(md("![](https://cdn.example.com/chart.png)"));
    await act(async () => { fireEvent.click(document.querySelector("button.is-gated")!); });
    expect(resolveChatImage).toHaveBeenCalledWith("https://cdn.example.com/chart.png", null, 7);
    expect(document.querySelector(".chat-md-image img")?.getAttribute("src")).toBe(`asset://local/${WEB}`);
  });

  it("同一会话同域名点过一次：屏幕上的和之后出现的图都自动加载；不同域名仍要点", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: WEB });
    render(md("![](https://cdn.example.com/a.png)\n\n![](https://cdn.example.com/b.png)\n\n![](https://other.org/c.png)"));
    expect(document.querySelectorAll("button.is-gated")).toHaveLength(3);
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: /cdn\.example\.com/ })[0]); });
    expect(resolveChatImage).toHaveBeenCalledWith("https://cdn.example.com/a.png", null, 7);
    expect(resolveChatImage).toHaveBeenCalledWith("https://cdn.example.com/b.png", null, 7);
    expect(resolveChatImage).not.toHaveBeenCalledWith("https://other.org/c.png", null, 7);
    const left = document.querySelectorAll("button.is-gated");
    expect(left).toHaveLength(1);
    expect(left[0].textContent).toContain("other.org");
    // 之后才出现的同域名图（下一条回复）直接加载。
    cleanup();
    render(md("![](https://cdn.example.com/d.png)"));
    await act(async () => {});
    expect(document.querySelector("button.is-gated")).toBeNull();
    expect(resolveChatImage).toHaveBeenCalledWith("https://cdn.example.com/d.png", null, 7);
  });

  it("放行只算当前会话：换个会话同域名还要点", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: WEB });
    render(md("![](https://cdn.example.com/a.png)", 7));
    await act(async () => { fireEvent.click(document.querySelector("button.is-gated")!); });
    cleanup();
    render(md("![](https://cdn.example.com/e.png)", 8));
    await act(async () => {});
    expect(document.querySelector("button.is-gated")?.textContent).toContain("cdn.example.com");
    expect(resolveChatImage).not.toHaveBeenCalledWith("https://cdn.example.com/e.png", null, 8);
  });

  it("本地路径不受影响，照常自动显示", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: String.raw`C:\i\a.png` });
    render(md("![](C:\a.png)"));
    await act(async () => {});
    expect(document.querySelector("button.is-gated")).toBeNull();
    expect(resolveChatImage).toHaveBeenCalledWith("C:\a.png", null, 7);
  });

  it("README 预览里的网络图同样点了才下", async () => {
    vi.mocked(resolveChatImage).mockResolvedValue({ path: WEB });
    const local = vi.fn(async () => "data:image/png;base64,AAAA");
    render(<ChatSessionContext.Provider value={7}><FileMarkdown text={"![badge](https://img.shields.io/b.svg)\n\n![logo](docs/logo.png)"} resolveImage={local} /></ChatSessionContext.Provider>);
    await act(async () => {});
    expect(resolveChatImage).not.toHaveBeenCalled();
    expect(local).toHaveBeenCalledWith("docs/logo.png");
    const gate = document.querySelector("button.is-gated")!;
    expect(gate.textContent).toContain("img.shields.io");
    await act(async () => { fireEvent.click(gate); });
    expect(resolveChatImage).toHaveBeenCalledWith("https://img.shields.io/b.svg", null, null);
    // 路径里有反斜杠，不能拼进 CSS 选择器（会被当转义），逐个比 src。
    expect([...document.querySelectorAll("img")].some((img) => img.getAttribute("src") === `asset://local/${WEB}`)).toBe(true);
  });
});
