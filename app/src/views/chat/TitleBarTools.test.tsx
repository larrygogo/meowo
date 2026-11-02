import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

const api = vi.hoisted(() => ({ listFolderEditors: vi.fn(), openFolderIn: vi.fn(), openProjectDir: vi.fn() }));
vi.mock("../../api", async (orig) => ({ ...(await orig<typeof import("../../api")>()), ...api }));

import { GitStatusChip, OpenInEditor } from "./TitleBarTools";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.openFolderIn.mockResolvedValue(undefined);
  api.openProjectDir.mockResolvedValue(undefined);
  localStorage.clear();
});
afterEach(() => cleanup());

describe("标题栏：用编辑器打开（对照 Kimi 的分体按钮）", () => {
  it("主钮用第一个探测到的编辑器；▾ 里换一个会记住并直接打开", async () => {
    api.listFolderEditors.mockResolvedValue([
      { id: "vscode", name: "VS Code", icon: null },
      { id: "cursor", name: "Cursor", icon: null },
    ]);
    const { unmount } = render(<OpenInEditor cwd="C:/repo" onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "用 VS Code 打开" }));
    expect(api.openFolderIn).toHaveBeenCalledWith("C:/repo", "vscode");

    fireEvent.click(screen.getByRole("button", { name: "选择打开方式" }));
    // 编辑器之后恒有系统文件管理器一项。
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["VS Code", "Cursor", "文件资源管理器"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Cursor" }));
    expect(api.openFolderIn).toHaveBeenLastCalledWith("C:/repo", "cursor");
    expect(screen.getByRole("button", { name: "用 Cursor 打开" })).toBeTruthy();

    // 换个会话（重新挂载）仍是上次选的。
    unmount();
    render(<OpenInEditor cwd="C:/other" onError={vi.fn()} />);
    expect(await screen.findByRole("button", { name: "用 Cursor 打开" })).toBeTruthy();
  });

  it("文件管理器走打开目录；编辑器没打开时把原因交出去", async () => {
    api.listFolderEditors.mockResolvedValue([{ id: "vscode", name: "VS Code", icon: null }]);
    api.openFolderIn.mockRejectedValue("editor/not_found");
    const onError = vi.fn();
    render(<OpenInEditor cwd="C:/repo" onError={onError} />);
    fireEvent.click(await screen.findByRole("button", { name: "用 VS Code 打开" }));
    await waitFor(() => expect(onError).toHaveBeenCalledWith("没找到这个编辑器，可能已卸载"));
    fireEvent.click(screen.getByRole("button", { name: "选择打开方式" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "文件资源管理器" }));
    expect(api.openProjectDir).toHaveBeenCalledWith("C:/repo");
  });
});

describe("标题栏：分支与改动胶囊", () => {
  const file = (additions: number | null, deletions: number | null) => ({ path: "a.ts", status: "M", additions, deletions });
  it("分支、领先远程、增删行合计；提示里说全", () => {
    render(<GitStatusChip
      summary={{ gitAvailable: true, isRepo: true, branch: "fix/x", ahead: 30, behind: 0, files: [file(20, 4), file(5, 5)] }}
      active={false}
      onToggle={vi.fn()}
    />);
    const chip = screen.getByRole("button", { name: "查看改动" });
    expect(chip.textContent).toBe("fix/x↑30+25-9");
    expect(chip.getAttribute("data-tip")).toBe("fix/x · 领先远程 30 个提交 · 2 个文件有未提交的改动");
  });
  it("没有行数（二进制等）只报文件数，没改动就只有分支", () => {
    const { rerender } = render(<GitStatusChip
      summary={{ gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files: [file(null, null)] }}
      active={false}
      onToggle={vi.fn()}
    />);
    expect(screen.getByRole("button", { name: "查看改动" }).textContent).toBe("main1");
    rerender(<GitStatusChip
      summary={{ gitAvailable: true, isRepo: true, branch: "main", ahead: 0, behind: 0, files: [] }}
      active
      onToggle={vi.fn()}
    />);
    expect(screen.getByRole("button", { name: "查看改动" }).textContent).toBe("main");
    expect(screen.getByRole("button", { name: "查看改动" }).getAttribute("aria-pressed")).toBe("true");
  });
});
