import { describe, it, expect, vi, beforeEach } from "vitest";

describe("platform", () => {
  beforeEach(() => vi.resetModules());

  it("isMac true 当 host_os 返回 macos", async () => {
    vi.doMock("@tauri-apps/api/core", () => ({
      invoke: vi.fn().mockResolvedValue("macos"),
    }));
    const { detectHostOs, isMac } = await import("./platform");
    await detectHostOs();
    expect(isMac()).toBe(true);
  });

  it("isMac false 当 host_os 返回 windows", async () => {
    vi.doMock("@tauri-apps/api/core", () => ({
      invoke: vi.fn().mockResolvedValue("windows"),
    }));
    const { detectHostOs, isMac } = await import("./platform");
    await detectHostOs();
    expect(isMac()).toBe(false);
  });
});
