import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// 2026-11-23 用户：「增加常用工作区允许被固定」。工作区菜单给「固定 / 取消固定」，固定的行标图钉。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { WorkspaceDirectory } from "./WorkspaceDirectory";
import { pinnedFirst } from "../sidebarDrag";
import { pathKey } from "../../paths";

// 后端已把固定的排在最前（session_query::with_pinned_first），这里按它给的顺序。
const OPTIONS = [{ value: "D:/tools/old", label: "old" }, { value: "C:/w/meowo", label: "meowo" }];
const openMenu = (index: number) => fireEvent.click(screen.getAllByRole("button", { name: "选项" })[index]);

describe("固定常用工作区", () => {
  beforeEach(() => { invoke.mockReset(); invoke.mockResolvedValue(undefined); });
  afterEach(cleanup);

  it("固定的行标图钉；斜杠方向、大小写不同也认得是同一个", () => {
    render(<WorkspaceDirectory options={OPTIONS} pinned={["D:\\Tools\\Old\\"]} onFilter={() => {}} />);
    const pins = screen.getAllByRole("img", { name: "已固定" });
    expect(pins).toHaveLength(1);
    expect(pins[0].closest(".chat-ws-dir")?.textContent).toContain("old");
  });

  it("没固定的菜单里是「固定」，点了写后端并让上层重拉清单；固定的是「取消固定」", async () => {
    const onHidden = vi.fn();
    render(<WorkspaceDirectory options={OPTIONS} pinned={["D:/tools/old"]} onFilter={() => {}} onHidden={onHidden} />);
    openMenu(1);
    fireEvent.click(await screen.findByRole("menuitem", { name: "固定" }));
    expect(invoke).toHaveBeenCalledWith("pin_workspace", { cwd: "C:/w/meowo", pinned: true });
    await vi.waitFor(() => expect(onHidden).toHaveBeenCalled());
    openMenu(0);
    fireEvent.click(await screen.findByRole("menuitem", { name: "取消固定" }));
    expect(invoke).toHaveBeenCalledWith("pin_workspace", { cwd: "D:/tools/old", pinned: false });
  });
});

describe("按工作区分组时固定的组排最前", () => {
  // 组键与侧栏分组同一种归一（groupByDir 用 pathKey）。
  const groups = ["C:/w/a", "C:/w/b", "C:/w/c"].map((cwd) => ({ key: pathKey(cwd) }));

  it("按固定先后排前，其余保持原来的相对顺序", () => {
    // 固定先后是 b 再 c（b 用反斜杠写，归一后同一个）；a 没固定，留在后面。
    expect(pinnedFirst(groups, ["C:\\w\\b\\", "C:/w/c"]).map((g) => g.key)).toEqual(["C:/w/b", "C:/w/c", "C:/w/a"].map(pathKey));
    expect(pinnedFirst(groups, [])).toEqual(groups);
  });
});
