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
  it("矮屏浮层卡在场时，dock 里的关卡整张让出（否则审批卡被挤到 30px）", () => {
    const css = read("../../styles.css");
    const at = css.indexOf("@media (max-height: 640px)");
    expect(at).toBeGreaterThan(-1);
    const block = css.slice(at, at + 300);
    expect(block).toContain(".chat-approval-overlay.is-active");
    expect(block).toContain(".lead-gate-card { display: none; }");
  });
});
