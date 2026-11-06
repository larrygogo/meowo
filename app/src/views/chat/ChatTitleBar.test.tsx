import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
import { ChatTitleBar } from "./ChatTitleBar";

afterEach(cleanup);

const props = {
  leading: null, title: <span>标题</span>, archive: null, end: null, cwd: "C:\\repo", gitSummary: null,
  changesOpen: false, panelOpen: false, terminalOn: false,
  onToggleChanges: () => {}, onTogglePanel: () => {}, onToggleTerminal: () => {}, onError: () => {},
};

describe("标题栏的面板开关", () => {
  it("面板收起时 agent 在操作浏览器：开关上亮点，提示说明原因", () => {
    const { rerender } = render(<ChatTitleBar {...props} />);
    expect(document.querySelector(".chat-panel-agent-dot")).toBeNull();
    rerender(<ChatTitleBar {...props} panelAgentBusy />);
    expect(document.querySelector(".chat-panel-agent-dot")).not.toBeNull();
    expect(screen.getByRole("button", { name: /Agent 正在操作浏览器/ })).toBeTruthy();
  });
});
