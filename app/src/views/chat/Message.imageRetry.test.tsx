import { act, cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockConvertFileSrc } from "@tauri-apps/api/mocks";

import { ImageRef } from "./Message";
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
  it("失败时上报并回退徽章,凭据换代后重新挂回缩略图", () => {
    const failed = vi.fn();
    window.addEventListener(IMAGE_LOAD_FAILED_EVENT, failed);
    try {
      const { container } = render(<ImageRef path="C:\tmp\meowo-paste\1\image.png" />);
      fireEvent.error(container.querySelector("img")!);
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
