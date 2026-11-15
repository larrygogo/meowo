import { describe, expect, it } from "vitest";

import { TYPED_READY_TIMEOUT_MS, typeAnswerIntoTerminal, typedAnswerReady, typedAnswerText } from "./terminalTypedAnswer";

const FLOW = { openInput: "4", readyPattern: String.raw`^\s*type feedback · ↵ submit\.?\s*$`, submitInput: "\r" };

/// 假终端：记下写入，屏幕由用例给。时钟只靠 sleep 推进，测试不真等。
function fakeTerminal(screen: (elapsed: number, writes: string[]) => string[] | null) {
  const writes: string[] = [];
  let clock = 0;
  return {
    writes,
    elapsed: () => clock,
    deps: {
      write: async (_session: number, data: string) => { writes.push(data); },
      screen: async () => screen(clock, writes),
      sleep: async (ms: number) => { clock += ms; },
      now: () => clock,
    },
  };
}

describe("现场打字的作答：认出等输入的状态才打字", () => {
  it("进入键写出去之后屏幕出现就绪标志：再写正文，最后写提交键", async () => {
    const terminal = fakeTerminal(() => ["  ▶ 4. Reject with feedback", "  Type feedback · ↵ submit."]);
    expect(await typeAnswerIntoTerminal(7, FLOW, "改用 pnpm", terminal.deps)).toBe("sent");
    expect(terminal.writes).toEqual(["4", "改用 pnpm", "\r"]);
  });

  it("正文与提交键分两次写，中间留出间隔（合成一次会被当成粘贴，回车落进正文）", async () => {
    const stamps: [string, number][] = [];
    const terminal = fakeTerminal(() => ["  Type feedback · ↵ submit."]);
    const write = terminal.deps.write;
    terminal.deps.write = async (session, data) => { stamps.push([data, terminal.elapsed()]); await write(session, data); };
    await typeAnswerIntoTerminal(7, FLOW, "x", terminal.deps);
    expect(stamps[2][1] - stamps[1][1]).toBeGreaterThanOrEqual(200);
  });

  it("就绪标志迟迟不出现：到时限为止只写过进入键，正文不写", async () => {
    const terminal = fakeTerminal(() => ["  ▶ 1. Approve once", "  ↑/↓ select · 1/2/3/4 choose · ↵ confirm"]);
    expect(await typeAnswerIntoTerminal(7, FLOW, "第 1 步先别跑", terminal.deps)).toBe("not_ready");
    expect(terminal.writes).toEqual(["4"]);
    expect(terminal.elapsed()).toBeGreaterThanOrEqual(TYPED_READY_TIMEOUT_MS);
  });

  it("重绘晚了几拍：等到就绪再写", async () => {
    const terminal = fakeTerminal((elapsed) => (elapsed < 900 ? ["  ↑/↓ select · 1/2/3/4 choose · ↵ confirm"] : ["  Type feedback · ↵ submit."]));
    expect(await typeAnswerIntoTerminal(7, FLOW, "x", terminal.deps)).toBe("sent");
    expect(terminal.writes).toEqual(["4", "x", "\r"]);
  });

  it("读不到屏幕（会话没有屏幕仿真）：当作没就绪，不打字", async () => {
    const terminal = fakeTerminal(() => null);
    expect(await typeAnswerIntoTerminal(7, FLOW, "x", terminal.deps)).toBe("not_ready");
    expect(terminal.writes).toEqual(["4"]);
  });

  it("正文是空的：一个键都不按", async () => {
    const terminal = fakeTerminal(() => ["  Type feedback · ↵ submit."]);
    expect(await typeAnswerIntoTerminal(7, FLOW, "  \n ", terminal.deps)).toBe("not_ready");
    expect(terminal.writes).toEqual([]);
  });

  it("多行正文压成一行：换行在这些输入框里就是提交", () => {
    expect(typedAnswerText("第一行\n第二行\r\n  第三行  ")).toBe("第一行 第二行 第三行");
    expect(typedAnswerText("  单行  ")).toBe("单行");
  });

  it("就绪标志逐行、不分大小写；后端给了坏正则当作认不出", () => {
    expect(typedAnswerReady(["x", "  TYPE FEEDBACK · ↵ SUBMIT."], FLOW)).toBe(true);
    // 正文里引用了这句（不是整行）不算。
    expect(typedAnswerReady(["● 面板底部会显示 Type feedback · ↵ submit. 这样的提示"], FLOW)).toBe(false);
    expect(typedAnswerReady(["anything"], { readyPattern: "(" })).toBe(false);
  });
});
