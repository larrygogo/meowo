import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { GitDiffSummaryDto, ShellTerminalInfo } from "../../api";
import { GitDiffView, type DiffPanelTab } from "./GitDiffView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
// xterm 在 jsdom 里画不出来；这里只关心面板怎么排页签、怎么切显隐。
vi.mock("./ShellTerminal", () => ({
  ShellTerminal: ({ id, visible }: { id: number; visible: boolean }) => (
    <div className="shell-term" data-testid={`shell-${id}`} data-visible={String(visible)}>
      <textarea aria-label={`shell-input-${id}`} />
    </div>
  ),
}));

const summary: GitDiffSummaryDto = { gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files: [] };
const shells: ShellTerminalInfo[] = [
  { id: 3, owner: 1, title: "pwsh", exited: false },
  { id: 7, owner: 1, title: "pwsh", exited: true },
];

afterEach(cleanup);

function renderPanel(activeTab: DiffPanelTab) {
  vi.mocked(invoke).mockResolvedValue([]);
  const handlers = { onCollapse: vi.fn(), onTabChange: vi.fn(), onNewShell: vi.fn(), onCloseShell: vi.fn() };
  render(
    <GitDiffView
      cwd={"C:\\repo"}
      summary={summary}
      width={420}
      collapsed={false}
      maximized={false}
      onToggleMaximize={() => {}}
      comments={[]}
      onAddComment={() => {}}
      onRemoveComment={() => {}}
      touchedPaths={[]}
      activeTab={activeTab}
      shells={shells}
      {...handlers}
    />,
  );
  return handlers;
}

describe("面板里的终端页签", () => {
  it("每个终端一个页签，只显示选中的那个；其余照样挂着", () => {
    renderPanel("shell:7");
    expect(screen.getByTestId("shell-3").dataset.visible).toBe("false");
    expect(screen.getByTestId("shell-7").dataset.visible).toBe("true");
    const tabs = screen.getAllByRole("button", { name: /pwsh/ });
    expect(tabs).toHaveLength(2);
    expect(tabs[1].className).toContain("is-active");
    expect(tabs[1].className).toContain("is-exited");
    // 看终端时「文件」「更改」都不算选中。
    expect(screen.getByRole("button", { name: "文件" }).className).not.toContain("is-active");
  });

  it("点页签切换、点叉关闭、点加号新建", () => {
    const { onTabChange, onCloseShell, onNewShell } = renderPanel("changes");
    const [first] = screen.getAllByRole("button", { name: /pwsh/ });
    fireEvent.click(first);
    expect(onTabChange).toHaveBeenCalledWith("shell:3");
    fireEvent.click(screen.getAllByLabelText("关闭终端")[1]);
    expect(onCloseShell).toHaveBeenCalledWith(7);
    // 点叉不应顺带切到那个页签。
    expect(onTabChange).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "新建终端" }));
    expect(onNewShell).toHaveBeenCalledTimes(1);
  });

  it("焦点在终端里按 Esc 交给 shell，不收面板", () => {
    const { onCollapse } = renderPanel("shell:3");
    fireEvent.keyDown(screen.getByLabelText("shell-input-3"), { key: "Escape" });
    expect(onCollapse).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onCollapse).toHaveBeenCalledTimes(1);
  });
});
