import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 二期 #6：系统通知按种类开关，设置页复用灵动岛那组勾选（种类与文案同一套）。
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  leadProviderInstallations: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { GeneralSection } from "./About";
import { zh } from "../i18n/zh";

beforeEach(() => {
  api.leadWorkflowInstalled.mockResolvedValue(false);
  api.leadProviderInstallations.mockResolvedValue([]);
  api.setSettings.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

const kindSwitch = (kind: string) =>
  screen.getByRole("switch", { name: `${zh.settings.notifyKinds} · ${zh.settings.islandAlertKinds[kind]}` });

describe("设置页：系统通知按种类开关", () => {
  it("老设置文件没有 notify_kinds：四类都显示为开，回合结束也开", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true });
    render(<GeneralSection />);
    await waitFor(() => expect(kindSwitch("waiting")).toBeTruthy());
    for (const kind of ["waiting", "approval", "question", "error"]) {
      expect(kindSwitch(kind).getAttribute("aria-checked")).toBe("true");
    }
  });

  it("关掉「会话跑完」只写 notify_kinds，不动灵动岛的提醒开关", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: true, island_alerts: ["approval"] });
    render(<GeneralSection />);
    await waitFor(() => expect(kindSwitch("waiting")).toBeTruthy());
    fireEvent.click(kindSwitch("waiting"));
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    const saved = api.setSettings.mock.calls.at(-1)![0] as { notify_kinds: string[]; island_alerts: string[] };
    expect(saved.notify_kinds).toEqual(["approval", "question", "error"]);
    expect(saved.island_alerts).toEqual(["approval"]);
  });

  it("总开关关着时不显示种类勾选", async () => {
    api.getSettings.mockResolvedValue({ notifications_enabled: false });
    render(<GeneralSection />);
    await screen.findByRole("switch", { name: zh.settings.notify });
    expect(screen.queryByRole("switch", { name: new RegExp(zh.settings.notifyKinds) })).toBeNull();
  });
});
