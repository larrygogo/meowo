import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  getAccounts: vi.fn(),
  setKeepAwake: vi.fn(),
  setPushPaused: vi.fn(),
}));
vi.mock("../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../api")>()), ...api }));

import { RemoteSettings } from "./RemoteSettings";
import { REMOTE_PREFS_EVENT, remoteTurnFolding } from "../remotePrefs";

/** 手机设置面板：手机本地的两项不碰电脑；电脑的两项各走只改一个字段的接口。 */
describe("RemoteSettings", () => {
  beforeEach(() => {
    localStorage.clear();
    api.getSettings.mockResolvedValue({ keep_awake_while_running: true, bark_url: "", bark_paused: false, chat_turn_folding: false });
    api.getAccounts.mockResolvedValue([]);
    api.setKeepAwake.mockResolvedValue(undefined);
    api.setPushPaused.mockResolvedValue(undefined);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  const openPanel = async () => {
    render(<RemoteSettings />);
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("dialog", { name: "设置" });
    await waitFor(() => expect(screen.getByRole("switch", { name: "运行时防止休眠" })).not.toHaveProperty("disabled", true));
  };

  it("防止休眠走 set_keep_awake,写失败退回原值并提示", async () => {
    await openPanel();
    const sw = screen.getByRole("switch", { name: "运行时防止休眠" });
    expect(sw.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(sw);
    expect(api.setKeepAwake).toHaveBeenCalledWith(false);
    expect(sw.getAttribute("aria-checked")).toBe("false");

    api.setKeepAwake.mockRejectedValueOnce("boom");
    fireEvent.click(sw);
    await screen.findByRole("alert");
    expect(sw.getAttribute("aria-checked")).toBe("false");
  });

  it("没填推送地址不显示推送开关;填了能暂停", async () => {
    await openPanel();
    expect(screen.queryByRole("switch", { name: "手机推送" })).toBeNull();
    cleanup();
    api.getSettings.mockResolvedValue({ keep_awake_while_running: true, bark_url: "https://api.day.app/KEY/", bark_paused: false });
    await openPanel();
    fireEvent.click(await screen.findByRole("switch", { name: "手机推送" }));
    expect(api.setPushPaused).toHaveBeenCalledWith(true);
  });

  it("消息自动折叠只存在手机上,不调电脑接口", async () => {
    await openPanel();
    const changed = vi.fn();
    window.addEventListener(REMOTE_PREFS_EVENT, changed);
    try {
      const sw = screen.getByRole("switch", { name: "消息自动折叠" });
      expect(sw.getAttribute("aria-checked")).toBe("false");
      act(() => { fireEvent.click(sw); });
      expect(sw.getAttribute("aria-checked")).toBe("true");
      expect(remoteTurnFolding()).toBe(true);
      expect(changed).toHaveBeenCalledTimes(1);
      expect(api.setKeepAwake).not.toHaveBeenCalled();
      expect(api.setPushPaused).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(REMOTE_PREFS_EVENT, changed);
    }
  });

  it("Esc 关闭面板", async () => {
    await openPanel();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
