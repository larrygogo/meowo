import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

// UI 调研 shell #4：引导「连接 AI CLI」只有文字，三条前面都打对勾，像已完成（mock 里一个 CLI 没装照样 3 个 ✓）。
// shell #3：欢迎页插图仍是已下线的贴纸卡片列表。
const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: () => Promise.resolve(), show: () => Promise.resolve(), setFocus: () => Promise.resolve() }) }));

import { Onboarding } from "./Onboarding";
import { zh } from "../i18n/zh";

const agent = (id: string, display_name: string, installed: boolean) => ({ id, display_name, installed });

beforeEach(() => {
  core.invoke.mockImplementation((cmd: string) => {
    if (cmd === "list_agents") return Promise.resolve([agent("claude", "Claude Code", true), agent("codex", "Codex", false)]);
    if (cmd === "get_settings") return Promise.resolve({});
    return Promise.resolve(null);
  });
});
afterEach(() => { cleanup(); core.invoke.mockReset(); localStorage.clear(); });

describe("引导", () => {
  it("欢迎页插图是对话窗，不再画贴纸卡片", () => {
    const { container } = render(<Onboarding />);
    expect(container.querySelector(".ob-hero .obm-chat")).not.toBeNull();
    expect(container.querySelector(".ob-hero .obm-card")).toBeNull();
  });

  it("连接 CLI 一步检测安装状态，未装给去安装直达账号页，说明不再打对勾", async () => {
    const { container } = render(<Onboarding />);
    fireEvent.click(screen.getByRole("button", { name: zh.onboarding.next }));
    expect(await screen.findByText("Claude Code")).toBeTruthy();
    const rows = Array.from(container.querySelectorAll(".ob-agent"));
    expect(rows.map((r) => r.textContent)).toEqual([`Claude Code${zh.onboarding.connect.installed}`, `Codex${zh.onboarding.connect.missing}`]);
    expect(container.querySelector(".ob-tick")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: zh.newSession.goInstall }));
    expect(localStorage.getItem("meowo-settings-section")).toBe("account");
    expect(core.invoke).toHaveBeenCalledWith("open_settings");
  });

  it("全都装好时不给去安装按钮", async () => {
    core.invoke.mockImplementation((cmd: string) => (cmd === "list_agents" ? Promise.resolve([agent("claude", "Claude Code", true)]) : Promise.resolve(null)));
    render(<Onboarding />);
    fireEvent.click(screen.getByRole("button", { name: zh.onboarding.next }));
    await screen.findByText("Claude Code");
    expect(screen.queryByRole("button", { name: zh.newSession.goInstall })).toBeNull();
  });
});
