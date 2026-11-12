import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import { Updater } from "./Updater";
import { zh } from "../i18n/zh";

// 更新已迁到后端 IPC：分别 mock 检查、下载命令与 Tauri 进度事件。
const mocks = vi.hoisted(() => ({
  checkImpl: undefined as undefined | (() => Promise<unknown>),
  downloadImpl: async (): Promise<"downloading" | "ready"> => "ready",
  cancelImpl: vi.fn(async () => {}),
  installImpl: async () => {},
  listeners: {} as Record<string, ((e: { payload: unknown }) => void) | undefined>,
}));
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  checkUpdate: () => mocks.checkImpl?.(),
  downloadUpdate: () => mocks.downloadImpl(),
  cancelUpdateDownload: () => mocks.cancelImpl(),
  installDownloadedUpdate: () => mocks.installImpl(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, cb: (e: { payload: unknown }) => void) => {
    mocks.listeners[event] = cb;
    return () => { delete mocks.listeners[event]; };
  },
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

afterEach(() => {
  cleanup();
  mocks.checkImpl = undefined;
  mocks.downloadImpl = async () => "ready";
  mocks.cancelImpl.mockReset();
  mocks.installImpl = async () => {};
  mocks.listeners = {};
});

const mkUpdate = (over: Record<string, unknown> = {}) => ({
  version: "9.9.9",
  body: "- 修复了一个 bug\n- 新增了一个功能",
  downloadState: "available",
  ...over,
});

describe("Updater", () => {
  it("有新版本:显示新版本号、更新说明与下载按钮", async () => {
    mocks.checkImpl = async () => mkUpdate();
    render(<Updater />);
    expect(await screen.findByText(zh.updater.found("9.9.9"))).toBeTruthy();
    expect(screen.getByText(zh.updater.notes)).toBeTruthy();
    expect(screen.getByText(/修复了一个 bug/)).toBeTruthy();
    expect(screen.getByText(zh.updater.download)).toBeTruthy();
  });

  // 更新说明是 Markdown：此前按纯文本显示，「## 新功能」「- xxx」原样露在卡片里。
  it("更新说明按 Markdown 渲染：标题是标题、列表是列表，不露出 ## 和 - 源码", async () => {
    mocks.checkImpl = async () => mkUpdate({ body: "## 新功能\n\n- 输入框永远完整可见\n- 长会话打开更快\n\n## 修复\n\n- 放弃更改进回收站" });
    render(<Updater />);
    const body = await screen.findByTestId("up-notes-body");
    // ChatMarkdown 是懒加载的，取回来之前先按原文显示；等它渲染出来再断言。
    await waitFor(() => expect(body.querySelectorAll("h2").length).toBe(2));
    expect([...body.querySelectorAll("h2")].map((h) => h.textContent)).toEqual(["新功能", "修复"]);
    expect([...body.querySelectorAll("li")].map((li) => li.textContent?.trim())).toEqual(["输入框永远完整可见", "长会话打开更快", "放弃更改进回收站"]);
    expect(body.textContent).not.toContain("##");
    expect(body.textContent).not.toContain("- ");
  });

  // 「取消下载」和「下载更新」在同一个位置（三态按钮不跳）：双击「下载更新」的第二下不能把刚开始的下载取消掉。
  it("双击下载更新：第二下落在「取消下载」上也不取消；单独点一下才取消", async () => {
    mocks.checkImpl = async () => mkUpdate();
    mocks.downloadImpl = async () => await new Promise<"downloading" | "ready">(() => {});
    render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.download), { detail: 1 });
    const cancel = await screen.findByText(zh.updater.cancelDownload);
    fireEvent.click(cancel, { detail: 2 });
    await act(async () => {});
    expect(mocks.cancelImpl).not.toHaveBeenCalled();
    fireEvent.click(cancel, { detail: 1 });
    await waitFor(() => expect(mocks.cancelImpl).toHaveBeenCalledTimes(1));
  });

  it("下载中：取消按钮排在进度与提示之后，和「下载更新」同处最后一行", async () => {
    mocks.checkImpl = async () => mkUpdate();
    mocks.downloadImpl = async () => await new Promise<"downloading" | "ready">(() => {});
    const { container } = render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.download));
    await screen.findByText(zh.updater.cancelDownload);
    const dl = container.querySelector(".up-dl")!;
    expect(dl.lastElementChild?.textContent).toBe(zh.updater.cancelDownload);
  });

  it("release notes 为空时不渲染更新内容区", async () => {
    mocks.checkImpl = async () => mkUpdate({ body: "  " });
    render(<Updater />);
    await screen.findByText(zh.updater.found("9.9.9"));
    expect(screen.queryByText(zh.updater.notes)).toBeNull();
  });

  it("已是最新:显示最新文案与「重新检查」", async () => {
    mocks.checkImpl = async () => null;
    render(<Updater />);
    expect(await screen.findByText(zh.updater.latest)).toBeTruthy();
    expect(screen.getByText(zh.updater.recheck)).toBeTruthy();
  });

  it("检查失败:显示错误文案,点「重试」重新检查并能查到新版", async () => {
    mocks.checkImpl = async () => {
      throw new Error("network");
    };
    render(<Updater />);
    expect(await screen.findByText(zh.updater.error)).toBeTruthy();
    mocks.checkImpl = async () => mkUpdate();
    fireEvent.click(screen.getByText(zh.updater.retry));
    expect(await screen.findByText(zh.updater.found("9.9.9"))).toBeTruthy();
  });

  it("点下载进入下载态,按后端累计字节计算百分比进度", async () => {
    mocks.checkImpl = async () => mkUpdate();
    mocks.downloadImpl = async () => {
      mocks.listeners["update-download-progress"]?.({ payload: { downloaded: 50, contentLength: 100 } });
      return await new Promise<"downloading" | "ready">(() => {}); // 永不 resolve:停在下载态供断言
    };
    const { container } = render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.download));
    await waitFor(() => expect(screen.getByText(zh.updater.downloadingPct(50))).toBeTruthy());
    const fill = container.querySelector(".up-prog-fill") as HTMLElement;
    expect(fill.style.width).toBe("50%");
    expect(screen.getByText(zh.updater.restartHint)).toBeTruthy();
  });

  it("总大小未知(无 Content-Length)时显示不带百分比的下载中与呼吸进度条", async () => {
    mocks.checkImpl = async () => mkUpdate();
    mocks.downloadImpl = async () => {
      mocks.listeners["update-download-progress"]?.({ payload: { downloaded: 1, contentLength: null } });
      return await new Promise<"downloading" | "ready">(() => {});
    };
    const { container } = render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.download));
    expect(await screen.findByText(zh.updater.downloading)).toBeTruthy();
    expect(container.querySelector(".up-prog-indet")).toBeTruthy();
  });

  it("下载中可取消：调后端取消，收到取消事件后回到可重新下载状态（S-13）", async () => {
    mocks.checkImpl = async () => mkUpdate();
    mocks.downloadImpl = async () => await new Promise<"downloading" | "ready">(() => {}); // 停在下载态
    render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.download));
    const cancel = await screen.findByText(zh.updater.cancelDownload);
    fireEvent.click(cancel);
    await waitFor(() => expect(mocks.cancelImpl).toHaveBeenCalledTimes(1));
    // 后端广播取消事件 → 回到「发现新版本」，可重新下载
    act(() => mocks.listeners["update-download-cancelled"]?.({ payload: null }));
    expect(await screen.findByText(zh.updater.download)).toBeTruthy();
  });

  it("后台下载完成后显示重启更新按钮", async () => {
    let installed = false;
    mocks.installImpl = async () => { installed = true; };
    mocks.checkImpl = async () => mkUpdate({ downloadState: "ready" });
    render(<Updater />);
    expect(await screen.findByText(zh.updater.ready)).toBeTruthy();
    // 退出时会静默安装的提示：点「稍后」的用户由此知道更新不会丢
    expect(screen.getByText(zh.updater.readyHint)).toBeTruthy();
    fireEvent.click(screen.getByText(zh.updater.restart));
    await waitFor(() => expect(installed).toBe(true));
  });
});
