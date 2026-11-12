import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

// 实拍 settingswin-search / settingswin-noresults（620×460）：搜索时右侧是各分区平铺的结果或
// 「没有匹配的设置项」，左栏却还亮着进搜索前的「通用」（x 8–160，y 76–108），和右侧内容对不上。
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  availableTerminals: vi.fn(),
  listAgents: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(() => Promise.resolve("0.0.0")) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: () => Promise.resolve(), show: () => Promise.resolve(), setFocus: () => Promise.resolve() }) }));
vi.mock("../useUpdate", () => ({ useUpdate: () => ({ status: "idle", version: null }) }));
vi.mock("../useShowWhenReady", () => ({ useShowWhenReady: () => {} }));
// 搜索态会挂载全部分区；联网与多窗口相关的三个分区和本条无关，换成空壳。
vi.mock("./settings/AccountSection", () => ({ AccountSection: () => null }));
vi.mock("./settings/ExtensionsSection", () => ({ ExtensionsSection: () => null }));
vi.mock("./settings/NetworkSection", () => ({ NetworkSection: () => null }));
vi.mock("./settings/RemoteAccessCard", () => ({ RemoteAccessCard: () => null }));

import { About } from "./About";
import { zh } from "../i18n/zh";

beforeEach(() => {
  localStorage.clear();
  api.getSettings.mockResolvedValue({});
  api.setSettings.mockResolvedValue(undefined);
  api.leadWorkflowInstalled.mockResolvedValue(false);
  api.availableTerminals.mockResolvedValue([]);
  api.listAgents.mockResolvedValue([]);
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

const onItems = () => Array.from(document.querySelectorAll(".side-nav .nav-item.on")).map((el) => el.textContent);
const currentItems = () => document.querySelectorAll('.side-nav .nav-item[aria-current="page"]').length;

describe("设置窗：搜索时左栏不高亮分区", () => {
  it("平时高亮当前分区；输入搜索词后一项都不高亮；清词后恢复", async () => {
    render(<About />);
    const input = await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    expect(onItems()).toEqual([zh.settings.nav.general]);
    expect(currentItems()).toBe(1);

    fireEvent.change(input, { target: { value: "zzz-没有这个设置" } });
    expect(onItems()).toEqual([]);
    expect(currentItems()).toBe(0);

    fireEvent.change(input, { target: { value: "" } });
    expect(onItems()).toEqual([zh.settings.nav.general]);
    expect(currentItems()).toBe(1);
  });
});
