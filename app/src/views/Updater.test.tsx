import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import { Updater } from "./Updater";
import { zh } from "../i18n/zh";

// mock 更新插件:各用例先设定 checkImpl 再渲染,模拟「有新版/最新/失败」与下载回调流。
let checkImpl: () => Promise<unknown>;
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: () => checkImpl(),
}));

afterEach(() => cleanup());

const mkUpdate = (over: Record<string, unknown> = {}) => ({
  version: "9.9.9",
  body: "- 修复了一个 bug\n- 新增了一个功能",
  downloadAndInstall: async () => {},
  ...over,
});

describe("Updater", () => {
  it("有新版本:显示新版本号、更新说明与「立即更新」按钮", async () => {
    checkImpl = async () => mkUpdate();
    render(<Updater />);
    expect(await screen.findByText(zh.updater.found("9.9.9"))).toBeTruthy();
    expect(screen.getByText(zh.updater.notes)).toBeTruthy();
    expect(screen.getByText(/修复了一个 bug/)).toBeTruthy();
    expect(screen.getByText(zh.updater.install)).toBeTruthy();
  });

  it("release notes 为空时不渲染更新内容区", async () => {
    checkImpl = async () => mkUpdate({ body: "  " });
    render(<Updater />);
    await screen.findByText(zh.updater.found("9.9.9"));
    expect(screen.queryByText(zh.updater.notes)).toBeNull();
  });

  it("已是最新:显示最新文案与「重新检查」", async () => {
    checkImpl = async () => null;
    render(<Updater />);
    expect(await screen.findByText(zh.updater.latest)).toBeTruthy();
    expect(screen.getByText(zh.updater.recheck)).toBeTruthy();
  });

  it("检查失败:显示错误文案,点「重试」重新检查并能查到新版", async () => {
    checkImpl = async () => {
      throw new Error("network");
    };
    render(<Updater />);
    expect(await screen.findByText(zh.updater.error)).toBeTruthy();
    checkImpl = async () => mkUpdate();
    fireEvent.click(screen.getByText(zh.updater.retry));
    expect(await screen.findByText(zh.updater.found("9.9.9"))).toBeTruthy();
  });

  it("点「立即更新」进入下载态,按回调计算百分比进度", async () => {
    checkImpl = async () =>
      mkUpdate({
        downloadAndInstall: async (cb?: (e: unknown) => void) => {
          cb?.({ event: "Started", data: { contentLength: 100 } });
          cb?.({ event: "Progress", data: { chunkLength: 50 } });
          await new Promise(() => {}); // 永不 resolve:停在下载态供断言
        },
      });
    const { container } = render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.install));
    await waitFor(() => expect(screen.getByText(zh.updater.downloadingPct(50))).toBeTruthy());
    const fill = container.querySelector(".up-prog-fill") as HTMLElement;
    expect(fill.style.width).toBe("50%");
    expect(screen.getByText(zh.updater.restartHint)).toBeTruthy();
  });

  it("总大小未知(无 Content-Length)时显示不带百分比的下载中与呼吸进度条", async () => {
    checkImpl = async () =>
      mkUpdate({
        downloadAndInstall: async (cb?: (e: unknown) => void) => {
          cb?.({ event: "Started", data: { contentLength: 0 } });
          await new Promise(() => {});
        },
      });
    const { container } = render(<Updater />);
    fireEvent.click(await screen.findByText(zh.updater.install));
    expect(await screen.findByText(zh.updater.downloading)).toBeTruthy();
    expect(container.querySelector(".up-prog-indet")).toBeTruthy();
  });
});
