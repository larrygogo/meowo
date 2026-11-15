import { describe, expect, it } from "vitest";

import fixture from "../../test/fixtures/approval-frames.json";
import { OPENCODE_ALWAYS_CONFIRM_SCREEN } from "../../test/requestSamples";
import { clickOptionInTerminal, clickSequence, pointerTarget } from "./terminalPointer";

/// opencode 1.19.5 的真机权限框（tests/probe_approval.rs 抓的整屏）。
const REAL = (fixture.frames as unknown as Record<string, { screen: string[] }>).opencode.screen;
const ROW = REAL.findIndex((line) => line.includes("Allow once"));

describe("终端里的横排选项：按屏幕位置点击，不靠焦点", () => {
  it("真机帧上三个选项的位置：列落在各自文字的第二个字上", () => {
    // 真机探针（probe_tui_script 的 click 步骤）点 Allow once 用的就是第 7 列：焦点在 Reject 上时点它，CLI 只放行了这一次。
    expect(pointerTarget(REAL, "Allow once")).toEqual({ row: ROW, col: 7 });
    expect(pointerTarget(REAL, "Allow always")).toEqual({ row: ROW, col: 20 });
    expect(pointerTarget(REAL, "Reject")).toEqual({ row: ROW, col: 35 });
  });

  it("要整格匹配：Allow 不会命中 Allow once，屏幕上别处的同名文字也不算", () => {
    expect(pointerTarget(REAL, "Allow")).toBeNull();
    expect(pointerTarget(["  ● 我会点 Allow once", ...REAL], "Allow once")).toEqual({ row: ROW + 1, col: 7 });
    expect(pointerTarget(["  ● 我会点 Allow once   然后继续"], "Allow once")).toBeNull();
  });

  it("只看最靠下的那条选项行：上面残留的旧框不算", () => {
    const stale = [...REAL, ...OPENCODE_ALWAYS_CONFIRM_SCREEN.split("\n")];
    expect(pointerTarget(stale, "Allow once")).toBeNull();
    expect(pointerTarget(stale, "Confirm")).not.toBeNull();
  });

  it("点击序列是 SGR 鼠标的左键按下与抬起，行列从 1 起", () => {
    expect(clickSequence({ row: 37, col: 7 })).toEqual(["\x1b[<0;8;38M", "\x1b[<0;8;38m"]);
  });

  it("现读屏幕、按下、抬起；屏幕上没有这个选项就什么都不写", async () => {
    const writes: string[] = [];
    const deps = (lines: string[] | null) => ({
      frame: async () => (lines ? { lines } : null),
      write: async (_session: number, data: string) => { writes.push(data); },
      sleep: async () => {},
    });
    expect(await clickOptionInTerminal(7, "Allow once", deps(REAL))).toBe(true);
    expect(writes).toEqual(clickSequence({ row: ROW, col: 7 }));
    writes.length = 0;
    expect(await clickOptionInTerminal(7, "Allow once", deps(["nothing here"]))).toBe(false);
    expect(await clickOptionInTerminal(7, "Allow once", deps(null))).toBe(false);
    expect(writes).toEqual([]);
  });
});
