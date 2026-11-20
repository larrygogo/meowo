import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockConvertFileSrc } from "@tauri-apps/api/mocks";

import { ImageRef } from "./Message";
import { allowImagePreview } from "../../api";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  allowImagePreview: vi.fn(async () => false),
}));
import { FILE_TOKEN_REFRESHED_EVENT, IMAGE_LOAD_FAILED_EVENT } from "../../remoteMode";

// jsdom 下没有 Tauri internals,convertFileSrc 会直接抛,装 mock 版顶上。
beforeEach(() => mockConvertFileSrc("windows"));
afterEach(() => {
  cleanup();
  clearMocks();
});

/// 远程端桌面重启后 /file 降级凭据换代,旧凭据读图全 401。此前 ImageRef 一次失败即
/// 定格成文件名徽章,手机不刷新就永远看不到图。钉住「失败上报 → 凭据换代后重试」。
describe("ImageRef 失败重试", () => {
  it("失败时上报并回退徽章,凭据换代后重新挂回缩略图", async () => {
    const failed = vi.fn();
    window.addEventListener(IMAGE_LOAD_FAILED_EVENT, failed);
    try {
      const { container } = render(<ImageRef path="C:\tmp\meowo-paste\1\image.png" />);
      fireEvent.error(container.querySelector("img")!);
      // 桌面上先请宿主放行这张图（这里放行不了），然后才回退。
      await act(async () => {});
      expect(failed).toHaveBeenCalledTimes(1);
      expect(container.querySelector(".chat-image-chip")).toBeTruthy();
      expect(container.querySelector("img")).toBeNull();

      act(() => {
        window.dispatchEvent(new CustomEvent(FILE_TOKEN_REFRESHED_EVENT));
      });
      expect(container.querySelector("img")).toBeTruthy();
      expect(container.querySelector(".chat-image-chip")).toBeNull();
    } finally {
      window.removeEventListener(IMAGE_LOAD_FAILED_EVENT, failed);
    }
  });

  it("未失败的缩略图不响应换代事件(不白白重载)", () => {
    const { container } = render(<ImageRef path="C:\tmp\meowo-paste\1\image.png" />);
    const img = container.querySelector("img");
    act(() => {
      window.dispatchEvent(new CustomEvent(FILE_TOKEN_REFRESHED_EVENT));
    });
    expect(container.querySelector("img")).toBe(img);
  });
});

/// 2026-11-16 用户反馈「拖拽进来的图片看不到预览」：asset 白名单只有粘贴临时目录和 CLI 图片缓存，
/// 从桌面拖进来、在文件对话框里选的图读取被拒。第一次失败时请宿主只放行这一个文件，再换地址重试一次。
describe("ImageRef：白名单外的图先请宿主放行再重试", () => {
  it("放行成功：换地址重试，缩略图出来，不回退文件名", async () => {
    vi.mocked(allowImagePreview).mockResolvedValueOnce(true);
    const path = "C:/Users/me/Desktop/shot.png";
    const { container } = render(<ImageRef path={path} />);
    const first = container.querySelector("img")!.getAttribute("src");
    fireEvent.error(container.querySelector("img")!);
    await act(async () => {});
    expect(allowImagePreview).toHaveBeenCalledWith(path);
    const img = container.querySelector("img")!;
    expect(img.getAttribute("src")).not.toBe(first);
    expect(img.getAttribute("src")).toContain("preview=1");
    expect(container.querySelector(".chat-image-chip")).toBeNull();
  });

  it("放行后重试还是失败：才回退文件名，不再反复请求", async () => {
    vi.mocked(allowImagePreview).mockClear();
    vi.mocked(allowImagePreview).mockResolvedValueOnce(true);
    const { container } = render(<ImageRef path="C:\Users\me\Desktop\broken.png" />);
    fireEvent.error(container.querySelector("img")!);
    await act(async () => {});
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector(".chat-image-chip")).toBeTruthy();
    expect(allowImagePreview).toHaveBeenCalledTimes(1);
  });
});

/// 做空第二轮：用户消息里的图片引用行同样过守卫——网络共享路径不进 asset 协议、
/// 也不请宿主放行（allow_image_preview 会对路径做文件系统操作）。
describe("ImageRef 拒网络共享路径", () => {
  it("//host 与 \\\\host 直接回退徽章，不发请求", () => {
    vi.mocked(allowImagePreview).mockClear();
    for (const path of ["//attacker/s/x.png", "\\\\attacker\\s\\x.png"]) {
      const { container, unmount } = render(<ImageRef path={path} />);
      expect(container.querySelector("img")).toBeNull();
      expect(container.querySelector(".chat-image-chip")).toBeTruthy();
      unmount();
    }
    expect(allowImagePreview).not.toHaveBeenCalled();
  });
});
