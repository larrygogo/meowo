import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { clearMocks } from "@tauri-apps/api/mocks";
import { hasEscLayers } from "../escLayers";
import { installRemoteTransport } from "./transport";
import { remoteConfirm } from "./remoteConfirm";

const card = () => document.querySelector<HTMLElement>(".remote-confirm");
const buttons = () => [...document.querySelectorAll<HTMLButtonElement>(".remote-confirm .app-confirm-actions button")];

afterEach(() => {
  document.querySelectorAll(".remote-confirm-overlay").forEach((n) => n.remove());
  clearMocks();
  vi.restoreAllMocks();
});

describe("手机确认层 remoteConfirm", () => {
  it("应用内卡片：标题、正文、主按钮说清后果并按危险操作标注；默认焦点在取消", async () => {
    const answer = remoteConfirm({ title: "切换到此账号", message: "要重启 2 个会话吗？", danger: true, confirmLabel: "重启这些会话" });
    expect(card()?.getAttribute("role")).toBe("alertdialog");
    expect(card()?.textContent).toContain("切换到此账号");
    expect(card()?.textContent).toContain("要重启 2 个会话吗？");
    const [cancel, ok] = buttons();
    expect(cancel.textContent).toBe("取消");
    expect(ok.textContent).toBe("重启这些会话");
    expect(ok.className).toContain("is-danger");
    expect(document.activeElement).toBe(cancel);
    expect(hasEscLayers()).toBe(true);
    ok.click();
    await expect(answer).resolves.toBe(true);
    expect(card()).toBeNull();
    expect(hasEscLayers()).toBe(false);
  });

  it("取消、Esc、点卡外空白都算不同意，且退出 Esc 层", async () => {
    const a = remoteConfirm({ title: "t" });
    buttons()[0].click();
    await expect(a).resolves.toBe(false);

    const b = remoteConfirm({ title: "t" });
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await expect(b).resolves.toBe(false);

    const c = remoteConfirm({ title: "t" });
    document.querySelector<HTMLElement>(".remote-confirm-overlay")!.click();
    await expect(c).resolves.toBe(false);
    expect(hasEscLayers()).toBe(false);
    expect(card()).toBeNull();
  });

  it("没给主按钮文案时回退「确定」，非危险操作用主按钮样式", async () => {
    const answer = remoteConfirm({ title: "t", message: "m" });
    const ok = buttons()[1];
    expect(ok.textContent).toBe("确定");
    expect(ok.className).toBe("btn-primary");
    ok.click();
    await answer;
  });
});

describe("transport 的 confirm_dialog", () => {
  beforeEach(() => installRemoteTransport());

  it("走应用内确认层，不再调用 window.confirm", async () => {
    const native = vi.fn(() => true);
    vi.stubGlobal("confirm", native);
    const answer = invoke<boolean>("confirm_dialog", { title: "结束会话", message: "m", danger: true, confirmLabel: "结束会话" });
    await vi.waitFor(() => expect(card()).not.toBeNull());
    expect(buttons()[1].textContent).toBe("结束会话");
    buttons()[1].click();
    await expect(answer).resolves.toBe(true);
    expect(native).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
