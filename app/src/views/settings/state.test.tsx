// useSettingsState 的首读缓存：getSettings() 的 Promise 缓存在 loadRef 里供首帧 patch 等待。
// 首读失败时被拒的 Promise 若留在缓存里，之后第一次 patch 的 await reload() 必打到这个已拒
// 缓存——patch 丢失，还把真正的保存错误盖成误导性的首读错误。失败必须清缓存、下次重拉。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
}));
vi.mock("../../api", () => api);

import { useSettingsState, SETTINGS_DEFAULTS } from "./state";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.setSettings.mockResolvedValue(undefined);
});
afterEach(() => cleanup());

describe("useSettingsState", () => {
  it("首读失败后缓存被清空：随后的 patch 重新拉取并正常保存，不报误导性错误", async () => {
    api.getSettings
      .mockRejectedValueOnce(new Error("ipc down"))
      .mockResolvedValue(SETTINGS_DEFAULTS);

    const hook = renderHook(() => useSettingsState());
    await act(async () => {}); // 挂载时的 reload 落定（失败；修复后缓存已清）
    expect(api.getSettings).toHaveBeenCalledTimes(1);

    let result: string | null | undefined;
    await act(async () => {
      result = await hook.result.current[1]({ theme: "light" });
    });

    // patch 不丢：重新拉取成功 → 基于真实设置合并 → 落盘成功返回 null。
    expect(result).toBeNull();
    expect(api.getSettings).toHaveBeenCalledTimes(2); // 重新拉取，而非复用已拒缓存
    expect(api.setSettings).toHaveBeenCalledWith({ ...SETTINGS_DEFAULTS, theme: "light" });
  });

  it("首读成功后 patch 基于缓存合并，不重复拉取", async () => {
    api.getSettings.mockResolvedValue(SETTINGS_DEFAULTS);

    const hook = renderHook(() => useSettingsState());
    await act(async () => {});

    let result: string | null | undefined;
    await act(async () => {
      result = await hook.result.current[1]({ opacity: 80 });
    });

    expect(result).toBeNull();
    expect(api.getSettings).toHaveBeenCalledTimes(1); // 成功缓存仍然复用
    expect(api.setSettings).toHaveBeenCalledWith({ ...SETTINGS_DEFAULTS, opacity: 80 });
  });
});
