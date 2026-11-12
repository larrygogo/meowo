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

// 停靠面板（Lead/计划/进度/后台任务/子代理）曾挂在 dock 上沿、只按 min(300px, 50vh) 限高：dock 里有
// 关卡卡片时面板整块盖住标题栏并伸出窗口顶（做空实测 1200×760：.panel-lead 在 y=-94..96）。
describe("停靠面板不越过标题栏", () => {
  const css = read("../../styles.css");
  const body = rule(css, ".dock-work-panel");
  it("面板贴着胶囊行上方的锚点向上弹，不再挂 dock 上沿", () => {
    expect(body).toContain("bottom: 0");
    expect(body).not.toContain("bottom: 100%");
    expect(rule(css, ".chat-dock > .dock-work-anchor")).toContain("height: 0");
  });
  it("限高读 --dock-panel-room（锚点到标题栏下沿），超出面板内滚动", () => {
    expect(body).toMatch(/max-height:[^;]*--dock-panel-room/);
    expect(body).toContain("overflow: hidden");
    expect(rule(css, ".dock-work-body")).toContain("overflow-y: auto");
  });
  it("四种面板都由 DockWork 渲染进同一个锚点", () => {
    const src = read("./DockWork.tsx");
    expect(src).toContain('<div className="dock-work-anchor" ref={anchorRef}>{openPanel}</div>');
    // 面板不许再作为 dock 的直接子节点渲染（那样又回到挂 dock 上沿）。
    expect(src.match(/^ {6}\{panel === /gm)).toBeNull();
  });
});

// 有待拍板卡时面板从胶囊行向上弹，正好盖住卡片的下半张：后两个选项、意见框、「提交」都看不到
// （qa-lead-panel 1280×800：面板 y=436–626，卡片 159–618）。改成排进文档流、和卡片分高度。
describe("有待拍板卡时停靠面板挤占而不是覆盖", () => {
  const css = read("../../styles.css");
  const panel = rule(css, ".chat-main:not([data-short]) > .chat-dock:has(> .lead-gate-card) > .dock-work-anchor > .dock-work-panel");
  it("面板进文档流，限高不读 --dock-panel-room（进了流再读会自己影响自己）", () => {
    expect(panel).toContain("position: relative");
    expect(panel).toContain("bottom: auto");
    expect(panel).toContain("max-height");
    expect(panel).not.toContain("--dock-panel-room");
  });
  it("锚点和面板都能被压缩，输入框不会被挤出窗口", () => {
    const anchor = rule(css, ".chat-main:not([data-short]) > .chat-dock:has(> .lead-gate-card) > .dock-work-anchor");
    expect(anchor).toContain("height: auto");
    expect(anchor).toContain("min-height: 0");
    expect(anchor).toContain("flex-shrink: 1");
    expect(panel).toContain("min-height: 0");
  });
});

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
