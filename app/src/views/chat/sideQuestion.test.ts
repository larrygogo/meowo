import { describe, expect, it } from "vitest";
import { mergeAnswerWindow, readSideAnswer, sideAnswerWindow, sideQuestionText } from "./sideQuestion";
import type { SideQuestion } from "../../generated/contracts/SideQuestion";

const spec: SideQuestion = {
  command: "/btw",
  footer_marker: "esc to close",
  ready_marker: "c to copy",
  scroll_input: "\x1b[B",
  close_input: "\x1b",
};
const READY = "    ↑/↓ to scroll · c to copy · f to fork · Esc to close";

/// 覆盖层屏幕（claude 2.1.314 实拍形态）：历史区 + `▔` 分隔 + 命令行 + 答案窗口 + 提示行。
function overlay(answer: string[], footer = READY, question = ["    /btw 列出 1 到 60"]): string[] {
  return ["❯ 只回复一个字：行", "● 行", "▔".repeat(40), ...question, ...answer.map((line) => `      ${line}`), footer];
}

describe("sideQuestion", () => {
  it("只认 `<命令> <问题>`，问题折成一行", () => {
    expect(sideQuestionText("/btw 刚才改了哪些文件？", spec)).toBe("刚才改了哪些文件？");
    expect(sideQuestionText("/btw 第一行\n第二行", spec)).toBe("第一行 第二行");
    expect(sideQuestionText("/btw", spec)).toBeNull();
    expect(sideQuestionText("/btwhatever", spec)).toBeNull();
    expect(sideQuestionText("普通消息", spec)).toBeNull();
  });

  it("答案窗口：去掉覆盖层缩进、保留答案自己的缩进，问题折行不算答案", () => {
    const screen = overlay(["代码：", "  const a = 1;"], READY, ["    /btw 很长的问题第一段", "    续行第二段"]);
    expect(sideAnswerWindow(screen, spec)).toEqual({ lines: ["代码：", "  const a = 1;"], ready: true });
    // 作答中：提示行只有 Esc to close。
    expect(sideAnswerWindow(overlay(["· Answering…"], "    Esc to close"), spec)?.ready).toBe(false);
    // 普通屏幕（历史区里的 ❯ /btw 不算命令行）。
    expect(sideAnswerWindow(["❯ /btw 问题", "● 回答", "❯"], spec)).toBeNull();
  });

  it("滚动窗口按最长重叠拼接", () => {
    expect(mergeAnswerWindow(["1", "2", "3", "4"], ["4", "5", "6", "7"])).toEqual(["1", "2", "3", "4", "5", "6", "7"]);
    expect(mergeAnswerWindow(["1", "2", "3"], ["2", "3", "4"])).toEqual(["1", "2", "3", "4"]);
  });

  /// 实拍：答案 60 行、窗口 26 行、每按一次 ↓ 下移 3 行；读完按 Esc 收起覆盖层。
  it("等答完、逐窗滚动读全、最后收起覆盖层", async () => {
    const all = Array.from({ length: 60 }, (_, index) => String(index + 1));
    let top = 0;
    let open = true;
    let polls = 0;
    const writes: string[] = [];
    const answer = await readSideAnswer({
      screen: async () => {
        polls += 1;
        if (!open) return ["❯ 只回复一个字：行", "❯"];
        if (polls < 3) return overlay(["· Answering…"], "    Esc to close");
        return overlay(all.slice(top, top + 26));
      },
      write: async (data) => {
        writes.push(data);
        if (data === spec.scroll_input) top = Math.min(top + 3, all.length - 26);
        if (data === spec.close_input) open = false;
      },
      sleep: async () => {},
    }, spec);
    expect(answer).toBe(all.join("\n"));
    expect(writes[writes.length - 1]).toBe("\x1b");
    expect(open).toBe(false);
  });

  it("覆盖层出现后又消失（被别处关掉）返回 null", async () => {
    let polls = 0;
    const answer = await readSideAnswer({
      screen: async () => (polls++ < 2 ? overlay(["· Answering…"], "    Esc to close") : ["❯"]),
      write: async () => {},
      sleep: async () => {},
    }, spec);
    expect(answer).toBeNull();
  });
});
