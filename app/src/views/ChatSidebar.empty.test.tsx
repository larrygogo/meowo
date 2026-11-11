import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// UI 调研 shell #16：新建页已经打开时，侧栏空态还放一个「新建会话」，同屏 3 个新建入口；
// 一个 CLI 都没装时空态也不指路。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));

import { ChatSidebar } from "./ChatSidebar";

const withAgents = (agents: unknown) =>
  invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "list_agents" ? agents : undefined));

describe("侧栏空态", () => {
  afterEach(cleanup);
  beforeEach(() => { invoke.mockReset(); localStorage.clear(); });

  it("新建页已打开时空态只写文字，不再多一个新建按钮", async () => {
    withAgents([{ id: "claude", display_name: "Claude Code", installed: true }]);
    render(<ChatSidebar activeId={0} newSessionActive approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByText("暂无会话");
    // 只剩头部那一个
    expect(screen.getAllByRole("button", { name: "新建会话" })).toHaveLength(1);
  });

  it("没装任何 CLI 时空态给去安装，直达设置账号页", async () => {
    withAgents([{ id: "claude", display_name: "Claude Code", installed: false }]);
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByText("暂无会话");
    fireEvent.click(await screen.findByRole("button", { name: "去安装" }));
    expect(localStorage.getItem("meowo-settings-section")).toBe("account");
    expect(invoke).toHaveBeenCalledWith("open_settings");
    expect(screen.getAllByRole("button", { name: "新建会话" })).toHaveLength(1);
  });
});
