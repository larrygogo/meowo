import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// UI 调研 shell #13：失败提示借用确认框，出现「取消 / 确定」两个按钮，点哪个效果都一样。
const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ label: "confirm-7", close: () => Promise.resolve() }) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("1.0.0") }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn(() => Promise.resolve("C:/x/diag.txt")) }));

import { ConfirmWindow } from "./ConfirmWindow";
import { AboutSection } from "./About";
import { zh } from "../i18n/zh";

afterEach(() => { cleanup(); core.invoke.mockReset(); });

describe("单按钮提示框", () => {
  it("alert 形态只有一个「知道了」，点它关掉", async () => {
    core.invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "confirm_dialog_payload" ? { title: "导出失败", message: "磁盘满", danger: false, confirm_label: null, alert: true } : null));
    render(<ConfirmWindow />);
    await screen.findByText("磁盘满");
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent)).toEqual([zh.dialog.gotIt]);
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    fireEvent.click(buttons[0]);
    expect(core.invoke).toHaveBeenCalledWith("confirm_dialog_result", { id: 7, ok: true });
  });

  it("确认形态照旧两个按钮", async () => {
    core.invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "confirm_dialog_payload" ? { title: "删除？", message: "确定删除", danger: true, confirm_label: "删除", alert: false } : null));
    render(<ConfirmWindow />);
    await screen.findByText("确定删除");
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([zh.dialog.cancel, "删除"]);
  });

  it("关于页打开更新窗失败、导出诊断失败都用单按钮提示", async () => {
    core.invoke.mockImplementation((cmd: string) =>
      cmd === "open_update_window" || cmd === "export_diag_log" ? Promise.reject("boom") : Promise.resolve(null));
    render(<AboutSection status="latest" newVersion={null} />);
    fireEvent.click(await screen.findByRole("button", { name: zh.about.checkUpdate }));
    fireEvent.click(screen.getByRole("button", { name: zh.about.diagLogExport }));
    await waitFor(() => expect(core.invoke.mock.calls.filter(([c]) => c === "confirm_dialog")).toHaveLength(2));
    for (const [, args] of core.invoke.mock.calls.filter(([c]) => c === "confirm_dialog")) {
      expect(args).toMatchObject({ alert: true });
    }
  });
});
