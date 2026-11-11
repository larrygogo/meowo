import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// UI 调研 shell #15：「从列表中移除」立即生效，误点了没有立即找回的办法（标记完成有撤销）。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { WorkspaceDirectory } from "./WorkspaceDirectory";

const OPTIONS = [{ value: "C:/w/meowo", label: "meowo" }, { value: "C:/w/old", label: "old" }];
const removeOld = async () => {
  fireEvent.click(screen.getAllByRole("button", { name: "选项" })[1]);
  fireEvent.click(await screen.findByRole("menuitem", { name: "从列表中移除" }));
};

describe("工作区移除可撤销", () => {
  beforeEach(() => { invoke.mockReset(); invoke.mockResolvedValue(undefined); });
  afterEach(() => { cleanup(); vi.useRealTimers(); });

  it("移除后给 5 秒撤销，点撤销放回并通知后端恢复", async () => {
    const onHidden = vi.fn();
    render(<WorkspaceDirectory options={OPTIONS} onFilter={() => {}} onHidden={onHidden} />);
    await removeOld();
    expect(screen.queryByText("C:/w/old")).toBeNull();
    expect(invoke).toHaveBeenCalledWith("hide_workspace", { cwd: "C:/w/old" });
    fireEvent.click(screen.getByRole("button", { name: "撤销" }));
    expect(screen.getByText("C:/w/old")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("hide_workspace", { cwd: "C:/w/old", restore: true });
    expect(screen.queryByRole("button", { name: "撤销" })).toBeNull();
  });

  it("5 秒后撤销条自己消失", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    render(<WorkspaceDirectory options={OPTIONS} onFilter={() => {}} />);
    await removeOld();
    expect(screen.getByRole("button", { name: "撤销" })).toBeTruthy();
    act(() => { vi.advanceTimersByTime(5_000); });
    expect(screen.queryByRole("button", { name: "撤销" })).toBeNull();
    expect(screen.queryByText("C:/w/old")).toBeNull();
  });
});
