import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// 做空审查 2026-11-11：620 宽开右侧面板时，页签名可视宽只有 14～21px，模型按钮只剩「O…」、权限模式
// 只剩「权…」；150% 下子任务头的 general-purpose 胶囊被硬裁。窄到放不下名字时一律只留图标，
// 名字进悬停提示与无障碍名，不靠省略号硬挤。
const read = (p: string) => readFileSync(resolve(__dirname, p), "utf8");

// 判定函数 tabsCompact 的单测在 SidePanel.test.tsx（那边有 xterm / webview 的替身）。
describe("右侧面板页签：放不下名字时只留图标", () => {
  it("收起后名字仍在无障碍名与悬停提示里", () => {
    const src = read("./SidePanel.tsx");
    expect(src).toContain("aria-label={name}");
    expect(src).toContain("data-tip={compact ? name : undefined}");
    const css = read("../../styles.css");
    expect(css).toContain(".side-panel-tabs.is-compact .side-panel-tab-name { display: none; }");
  });
});

describe("主列窄时输入框的模型、权限按钮只留图标", () => {
  const css = read("../../styles.css");
  const win = read("../ChatWindow.tsx");
  it("data-narrow 下收掉名字、换上图标", () => {
    expect(css).toContain(".chat-main[data-narrow] .chat-compose .chat-model-label { display: none; }");
    expect(css).toContain(".chat-main[data-narrow] .chat-compose .chat-model-narrow-icon { display: inline-flex; }");
  });
  it("判据只看主列自身的宽高与界面缩放（不看卡片与 dock，免得和卡高互相牵动）", () => {
    expect(win).toContain('host.toggleAttribute("data-narrow", mainWidth > 0 && mainWidth < NARROW_MAIN_WIDTH * ui)');
    expect(win).toContain('host.toggleAttribute("data-short", mainHeight > 0 && mainHeight < SHORT_MAIN_HEIGHT * ui)');
  });
  it("只留图标时当前值在悬停提示里：模型名、权限模式的当前档", () => {
    expect(win).toContain("`${t.chat.switchModel}: ${history.model}`");
    expect(win).toContain("`${options.length > 0 ? t.chat.switchMode : t.chat.cycleMode}: ${value}`");
  });
  it("子任务头的类型胶囊能省略，主列偏窄时整颗收掉", () => {
    expect(css).toContain(".chat-main[data-slim] .chat-subagent-type { display: none; }");
    // 取规则本体（行首的那条），不是上面 data-slim 那条选择器的尾巴。
    const at = css.search(/^\.chat-subagent-type \{/m);
    const body = css.slice(at, css.indexOf("}", at));
    expect(body).toContain("text-overflow: ellipsis");
    expect(body).toContain("flex: 0 1 auto");
  });
});
