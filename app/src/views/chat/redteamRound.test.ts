import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// 界面打磨第二轮做空审查（2026-11-12）查出的三类回归，各钉一条规则守护。真实效果靠截图工具量
// （elementFromPoint 命中审计、Lead 面板六种窗口尺寸、新建页勾 worktree 后输入框位置），这里只防规则被删。
const read = (p: string) => readFileSync(resolve(__dirname, p), "utf8");
const rule = (css: string, selector: string) => {
  const at = css.indexOf(`${selector} {`);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};
const styles = read("../../styles.css");
const mobile = read("../../mobile/mobile.css");

describe("手机上外扩的命中面不被 overflow 裁掉", () => {
  it("会话列表页签：桌面规则的 overflow: hidden 在手机上关掉（改前命中只剩 28～30）", () => {
    expect(mobile).toContain("body.remote-ui .chat-sidebar-tabs button { overflow: visible; }");
  });
  it("设置屏分段：.seg-btn 的 overflow: hidden 在手机设置屏关掉（改前命中 28）", () => {
    expect(mobile).toContain(":where(body.remote-ui) .remote-settings-screen .seg-btn { overflow: visible; }");
  });
  it("有卡片时停靠栏胶囊：这一排上下垫出外扩的余量，收起时归零", () => {
    expect(mobile).toContain("--dock-hit-room: clamp(0px,");
    expect(mobile).toContain("padding-top: var(--dock-hit-room);");
    expect(mobile).toContain("margin-top: calc(-1 * var(--dock-hit-room));");
  });
  it("权限模式、模型两颗带边框的按钮外扩 7px（从边框里面量，6 只到 42）", () => {
    expect(mobile).toContain("body.remote-ui .chat-compose-actions > .chat-model > .chat-model-button::after { top: -7px; bottom: -7px; }");
  });
});

describe("有待拍板卡时打开 Lead 面板的让位顺序", () => {
  it("挤占只在主列不矮时生效；矮窗面板回到浮层", () => {
    expect(rule(styles, ".chat-main:not([data-short]) > .chat-dock:has(> .lead-gate-card) > .dock-work-anchor")).toContain("height: auto");
    expect(styles).not.toMatch(/^\.chat-dock:has\(> \.lead-gate-card\) > \.dock-work-anchor \{/m);
  });
  it("面板先让、最少留标题行；对话区留 3～4 行，不再被压到 18px 的半行", () => {
    expect(rule(styles, ".chat-main:not([data-short]) > .chat-dock:has(> .lead-gate-card) > .dock-work-anchor:has(> .dock-work-panel)")).toContain("flex-shrink: 8");
    expect(styles).toContain("> .chat-scroll { min-height: clamp(0px, calc(100vh - 480px), 88px); }");
    expect(styles).toContain(".chat-main[data-short]:has(> .chat-dock > .lead-gate-card) > .chat-scroll { visibility: hidden; }");
  });
});

describe("矮窗新建页勾上 worktree 后输入框不出窗口", () => {
  it("窗口不到 500 高时收掉大图标、收紧标题间距（1000×428 改前输入框 346–478）", () => {
    const at = styles.indexOf("@media (max-height: 500px) {");
    expect(at).toBeGreaterThan(0);
    const block = styles.slice(at, styles.indexOf("\n}", at));
    expect(block).toContain(".nsp-hero-mark { display: none; }");
  });
});
