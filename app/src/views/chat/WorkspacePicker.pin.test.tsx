import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, within } from "@testing-library/react";
import { WorkspacePicker } from "./WorkspacePicker";
import { zh } from "../../i18n/zh";

afterEach(() => cleanup());

describe("新建会话页工作区下拉：固定常用工作区", () => {
  const dirs = ["C:/w/web", "C:/w/api", "C:/w/infra"];

  it("固定的单列「已固定」一节排最前（斜杠方向不同也认），其余在「最近的文件夹」", () => {
    render(<WorkspacePicker cwd="" dirs={dirs} pinned={["C:\\w\\api"]} onTogglePin={vi.fn()} onPick={vi.fn()} onPickFolder={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: zh.newSession.chooseWorkspace }));
    const menu = screen.getByRole("listbox");
    const text = menu.textContent ?? "";
    expect(text.indexOf(zh.chat.wsPinned)).toBeLessThan(text.indexOf("api"));
    expect(text.indexOf("api")).toBeLessThan(text.indexOf(zh.newSession.recentFolders));
    expect(text.indexOf(zh.newSession.recentFolders)).toBeLessThan(text.indexOf("web"));
    expect(within(menu).getByRole("button", { name: `${zh.chat.wsUnpin} api` }).getAttribute("aria-pressed")).toBe("true");
  });

  it("点图钉切固定，不选中目录、不关菜单", () => {
    const onTogglePin = vi.fn();
    const onPick = vi.fn();
    render(<WorkspacePicker cwd="" dirs={dirs} pinned={["C:/w/api"]} onTogglePin={onTogglePin} onPick={onPick} onPickFolder={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: zh.newSession.chooseWorkspace }));
    fireEvent.click(screen.getByRole("button", { name: `${zh.chat.wsPin} web` }));
    fireEvent.click(screen.getByRole("button", { name: `${zh.chat.wsUnpin} api` }));
    expect(onTogglePin.mock.calls).toEqual([["C:/w/web", true], ["C:/w/api", false]]);
    expect(onPick).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("不给 onTogglePin 就没有图钉", () => {
    render(<WorkspacePicker cwd="" dirs={dirs} onPick={vi.fn()} onPickFolder={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: zh.newSession.chooseWorkspace }));
    expect(screen.queryByRole("button", { name: new RegExp(`^${zh.chat.wsPin} `) })).toBeNull();
  });
});
