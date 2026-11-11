// UI 优化 #53：Ctrl+K 命令面板补齐面板页签 6 条、切换模型、切换权限模式、打开 PR、用…打开，
// 有快捷键的在右侧标出（mac 显示 ⌘）。
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn(() => Promise.resolve([])));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { zh } from "../../i18n/zh";
import { buildPaletteCommands, OPEN_WITH_PREFIX, type PaletteContext } from "./paletteCommands";
import { QuickSwitcher } from "./WindowModals";

const full: PaletteContext = {
  remote: false,
  hasCwd: true,
  isRepo: true,
  panelVisible: true,
  canSwitchModel: true,
  canSwitchPermission: true,
  pullRequest: { number: 162, url: "https://github.com/o/r/pull/162" },
  openers: [{ id: "vscode", name: "VS Code" }, { id: "files", name: "文件资源管理器" }],
  mac: false,
};

describe("命令面板清单", () => {
  afterEach(cleanup);

  it("面板 6 条带各自快捷键，模型、权限、PR、打开方式都在", () => {
    const cmds = buildPaletteCommands(zh, full);
    const keyOf = (id: string) => cmds.find((cmd) => cmd.id === id)?.keys;
    expect(keyOf("panel-toggle")).toBe("Ctrl+Shift+B");
    expect(keyOf("panel-changes")).toBe("Ctrl+Shift+G");
    expect(keyOf("panel-files")).toBe("Ctrl+Shift+E");
    expect(keyOf("panel-browser")).toBe("Ctrl+T");
    expect(keyOf("panel-terminal")).toBe("Ctrl+`");
    expect(keyOf("panel-next-tab")).toBe("Ctrl+Tab");
    const labels = cmds.map((cmd) => cmd.label);
    expect(labels).toContain("切换模型");
    expect(labels).toContain("切换权限模式");
    expect(labels).toContain("打开 Pull Request #162");
    expect(labels).toContain("用 VS Code 打开");
    expect(cmds.some((cmd) => cmd.id === `${OPEN_WITH_PREFIX}files`)).toBe(true);
    // 速查表与设置仍殿后。
    expect(cmds.at(-1)?.id).toBe("open-settings");
  });

  it("mac 上键位显示 ⌘", () => {
    const cmds = buildPaletteCommands(zh, { ...full, mac: true });
    expect(cmds.find((cmd) => cmd.id === "panel-changes")?.keys).toBe("⌘+Shift+G");
    expect(cmds.find((cmd) => cmd.id === "new-session")?.keys).toBe("⌘+N");
  });

  it("做不了的不列：非仓库无改动页签、面板收着无切页签、无 PR、手机上无面板与打开方式", () => {
    const ids = (ctx: Partial<PaletteContext>) => buildPaletteCommands(zh, { ...full, ...ctx }).map((cmd) => cmd.id);
    expect(ids({ isRepo: false })).not.toContain("panel-changes");
    expect(ids({ panelVisible: false })).not.toContain("panel-next-tab");
    expect(ids({ pullRequest: null })).not.toContain("open-pr");
    expect(ids({ canSwitchModel: false, canSwitchPermission: false })).not.toContain("switch-model");
    const remote = ids({ remote: true });
    expect(remote.filter((id) => id.startsWith("panel-") || id.startsWith(OPEN_WITH_PREFIX))).toEqual([]);
  });

  it("命令面板里每条有快捷键的命令右侧渲染键帽", async () => {
    const cmds = buildPaletteCommands(zh, full);
    render(<QuickSwitcher activeId={1} commands={cmds} onPick={() => {}} onCommand={() => {}} onClose={() => {}} />);
    const row = await screen.findByRole("option", { name: /打开改动页签/ });
    expect(row.querySelector("kbd")?.textContent).toBe("Ctrl+Shift+G");
    expect(screen.getByRole("option", { name: /面板终端/ }).querySelector("kbd")?.textContent).toBe("Ctrl+`");
  });
});
