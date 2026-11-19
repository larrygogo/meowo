import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 设置 › 通知（设置页精简 2026-11-19）：四个事件只列一次，每行两列开关——系统通知列读写 notify_kinds，
// 灵动岛弹出列读写 island_alerts（改前分别在「通用 · 通知这些事」和「外观 · 自动弹出提醒」）。
// 灵动岛那一列只在 Windows / macOS 出现，而 CI 的 vitest 跑在 Linux（jsdom UA 带 "linux"）：
// 先把 UA 钉成 Windows 再导入组件（模块顶层按 UA 判平台）。
vi.hoisted(() => {
  Object.defineProperty(window.navigator, "userAgent", { value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) jsdom", configurable: true });
});
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  testBarkPush: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { NotifySection } from "./settings/NotifySection";
import { SETTINGS_DEFAULTS } from "./settings/state";
import { zh } from "../i18n/zh";

beforeEach(() => {
  api.setSettings.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

const KINDS = ["waiting", "approval", "question", "error"] as const;
const sysSwitch = (kind: string) =>
  screen.getByRole("switch", { name: `${zh.settings.notifyColSystem} · ${zh.settings.islandAlertKinds[kind]}` });
const islandSwitch = (kind: string) =>
  screen.getByRole("switch", { name: `${zh.settings.notifyColIsland} · ${zh.settings.islandAlertKinds[kind]}` });
const lastSaved = () => api.setSettings.mock.calls.at(-1)![0] as Record<string, unknown>;

describe("通知页：事件 × 渠道矩阵", () => {
  it("四个事件各一行、只出现一次，表头写两列的列名", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true });
    render(<NotifySection />);
    await waitFor(() => expect(sysSwitch("waiting")).toBeTruthy());
    const matrix = screen.getByTestId("notify-matrix");
    const head = matrix.querySelector(".notify-matrix-head")!;
    expect(Array.from(head.querySelectorAll(".notify-col")).map((el) => el.textContent)).toEqual([zh.settings.notifyColSystem, zh.settings.notifyColIsland]);
    const rows = Array.from(matrix.querySelectorAll(".notify-matrix-row"));
    expect(rows.map((r) => r.querySelector(".row-label")!.textContent)).toEqual(KINDS.map((k) => zh.settings.islandAlertKinds[k]));
    // 每行两个开关；整页同一个事件名只出现一次（改前通用、外观各列一遍）。
    for (const row of rows) expect(row.querySelectorAll("[role=switch]")).toHaveLength(2);
    for (const kind of KINDS) expect(screen.getAllByText(zh.settings.islandAlertKinds[kind])).toHaveLength(1);
  });

  it("老设置文件缺 notify_kinds / island_alerts：两列都按默认四类全开显示", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true });
    render(<NotifySection />);
    await waitFor(() => expect(sysSwitch("waiting")).toBeTruthy());
    for (const kind of KINDS) {
      expect(sysSwitch(kind).getAttribute("aria-checked")).toBe("true");
      expect(islandSwitch(kind).getAttribute("aria-checked")).toBe("true");
    }
  });

  it("系统通知列只写 notify_kinds，灵动岛列只写 island_alerts，互不带动", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true, notify_kinds: ["waiting", "error"], island_alerts: ["approval"] });
    render(<NotifySection />);
    await waitFor(() => expect(sysSwitch("waiting").getAttribute("aria-checked")).toBe("true"));
    expect(sysSwitch("approval").getAttribute("aria-checked")).toBe("false");
    expect(islandSwitch("approval").getAttribute("aria-checked")).toBe("true");

    fireEvent.click(sysSwitch("waiting"));
    await waitFor(() => expect(api.setSettings).toHaveBeenCalledTimes(1));
    expect(lastSaved().notify_kinds).toEqual(["error"]);
    expect(lastSaved().island_alerts).toEqual(["approval"]);

    fireEvent.click(islandSwitch("question"));
    await waitFor(() => expect(api.setSettings).toHaveBeenCalledTimes(2));
    expect(lastSaved().island_alerts).toEqual(["approval", "question"]);
  });

  it("桌面通知关着：系统通知列置灰但保留勾选，灵动岛列照常可改；提示音置灰而不是消失", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: false, notify_kinds: ["approval"], island_alerts: ["waiting"] });
    render(<NotifySection />);
    await waitFor(() => expect((sysSwitch("approval") as HTMLButtonElement).disabled).toBe(true));
    for (const kind of KINDS) expect((sysSwitch(kind) as HTMLButtonElement).disabled).toBe(true);
    expect(sysSwitch("approval").getAttribute("aria-checked")).toBe("true");
    expect(document.querySelector(".notify-matrix-head .notify-col")!.classList.contains("is-off")).toBe(true);
    expect((screen.getByRole("switch", { name: zh.settings.notifySound }) as HTMLButtonElement).disabled).toBe(true);

    expect((islandSwitch("error") as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(islandSwitch("error"));
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    expect(lastSaved().island_alerts).toEqual(["waiting", "error"]);
    expect(lastSaved().notify_kinds).toEqual(["approval"]);
  });

  it("桌面通知、提示音、任务栏提醒各自读写原字段", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true, notification_sound: true, attention_flash_enabled: true });
    render(<NotifySection />);
    const notify = await screen.findByRole("switch", { name: zh.settings.notify });
    await waitFor(() => expect(notify.getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(notify);
    await waitFor(() => expect(lastSaved().notifications_enabled).toBe(false));
    fireEvent.click(screen.getByRole("switch", { name: zh.settings.attentionFlash }));
    await waitFor(() => expect(lastSaved().attention_flash_enabled).toBe(false));
  });
});

// 手机推送(Bark)从「手机远程」搬来：远程访问关着也能配；测试用输入框里的草稿，不必先保存；失焦才落盘。
describe("通知页：手机推送", () => {
  it("Bark 推送:测试用草稿地址,失败给原因,失焦落盘 bark_url", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS });
    api.testBarkPush.mockRejectedValueOnce("push/invalid_url").mockResolvedValueOnce(undefined);
    render(<NotifySection />);
    const input = await screen.findByRole("textbox", { name: zh.remote.pushLabel });
    fireEvent.change(input, { target: { value: "https://api.day.app/KEY/" } });
    fireEvent.click(screen.getByRole("button", { name: zh.remote.pushTest }));
    expect(await screen.findByText("推送地址无效，请从 Bark App 里复制")).toBeTruthy();
    expect(api.testBarkPush).toHaveBeenCalledWith("https://api.day.app/KEY/");
    expect(api.setSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: zh.remote.pushTest }));
    expect(await screen.findByText(zh.remote.pushTestOk)).toBeTruthy();
    fireEvent.blur(input);
    await waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(expect.objectContaining({ bark_url: "https://api.day.app/KEY/" })));
  });

  it("填了地址后能暂停推送（bark_paused）", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS, bark_url: "https://api.day.app/KEY/", bark_paused: false });
    render(<NotifySection />);
    const toggle = await screen.findByRole("switch", { name: zh.remote.pushToggle });
    fireEvent.click(toggle);
    await waitFor(() => expect(lastSaved().bark_paused).toBe(true));
  });
});
