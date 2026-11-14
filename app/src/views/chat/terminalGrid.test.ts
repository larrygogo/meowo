import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { FALLBACK_GRID, launchGrid } from "./terminalGrid";

describe("新起终端的初始尺寸", () => {
  it("取当前终端的网格", () => {
    expect(launchGrid(() => ({ cols: 143, rows: 41 }))).toEqual({ cols: 143, rows: 41 });
  });
  it("终端没挂载或网格无效时用占位尺寸", () => {
    expect(launchGrid(null)).toEqual(FALLBACK_GRID);
    expect(launchGrid(() => null)).toEqual(FALLBACK_GRID);
    expect(launchGrid(() => ({ cols: 0, rows: 0 }))).toEqual(FALLBACK_GRID);
  });
  // 走查输入 B12：分叉曾写死 100×30。对话窗里起终端的尺寸只从 launchGrid 来，不再出现写死的数字。
  it("对话窗不写死终端尺寸", () => {
    const source = readFileSync(resolve(__dirname, "../ChatWindow.tsx"), "utf8");
    expect(source).not.toMatch(/cols: 100|, 100, 30\)/);
    expect(source.match(/launchGrid\(terminalGridRef\.current\)/g)?.length).toBe(3);
  });
});
