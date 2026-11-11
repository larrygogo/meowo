import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// dock 里的卡（Lead 关卡）按 --chat-overlay-room 限高会自激闪烁：room = 消息区高度，dock 卡越高
// 消息区越矮。2026-11-11 leader 录屏，最小复现卡高 104↔483px 来回跳（scratchpad flicker/repro）。
const read = (p: string) => readFileSync(resolve(__dirname, p), "utf8");
const rule = (css: string, selector: string) => {
  const at = css.indexOf(`${selector} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};

describe("dock 里的卡不按浮层剩余高度限高", () => {
  it("桌面：.chat-dock > .chat-approval 有固定上限，且不读 --chat-overlay-room", () => {
    const body = rule(read("../../styles.css"), ".chat-dock > .chat-approval");
    expect(body).toContain("max-height");
    expect(body).not.toContain("--chat-overlay-room");
  });
  it("手机：body.remote-ui 下同样覆盖", () => {
    const body = rule(read("../../mobile/mobile.css"), "body.remote-ui .chat-dock > .chat-approval");
    expect(body).toContain("max-height");
    expect(body).not.toContain("--chat-overlay-room");
  });
  it("浮层卡在场时，dock 里的关卡整张让出，不分屏幕高矮（否则审批卡被挤到 30px、压到标题栏上）", () => {
    const css = read("../../styles.css");
    expect(css).toContain(".chat-main:has(> .chat-approval-overlay.is-active) > .chat-dock > .lead-gate-card { display: none; }");
    // 曾只在 max-height:640 断点让位，1280×720、1366×768 照样挤坏（做空第二轮）：不许再套断点。
    const at = css.indexOf(".lead-gate-card { display: none; }");
    // 规则所在行之前最近的一段（到上一个规则结束为止）里不能有 @media。
    const before = css.slice(Math.max(0, css.lastIndexOf("}", at - 2)), at);
    expect(before).not.toContain("@media");
  });
});
