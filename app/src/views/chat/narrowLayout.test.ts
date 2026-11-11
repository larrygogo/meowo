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

// leader 2026-11-11 定的统一原则：手机/窄宽下的小控件（分段控件、页签组、图标切换、行内图标钮）
// 视觉高度与桌面同档，44px 命中面靠不可见的 ::after 外扩补，不靠把控件本身撑高。
describe("手机上的小控件不被触屏规则撑高", () => {
  const mobile = read("../../mobile/mobile.css");
  const css = read("../../styles.css");
  it("后台任务面板头：标题行右端是关闭钮，分段筛选另起一行并平分", () => {
    expect(mobile).toContain("body.remote-ui .panel-tasks .dock-work-actions { display: contents; }");
    expect(mobile).toContain("body.remote-ui .panel-tasks .dock-work-close { order: 1; }");
    expect(mobile).toMatch(/body\.remote-ui \.panel-tasks \.chat-task-filter \{ order: 2; flex: 1 1 100%;/);
    // 桌面面板不到 480 宽同样排法（容器查询挂在面板锚点上）。
    expect(css).toMatch(/@container dock-anchor \(max-width: 527px\) \{[^@]*\.panel-tasks \.dock-work-close \{ order: 1; \}/);
  });
  it("分段筛选的段高 28（凹槽 32），命中面靠 ::after 上下外扩", () => {
    expect(mobile).toMatch(/body\.remote-ui \.chat-task-filter > button \{[^}]*min-height: 28px/);
    expect(mobile).toContain("body.remote-ui .chat-task-filter > button::after { inset: -8px 0; }");
    // 曾经的写法：直接把每一段撑到 44px，凹槽成了面板里最粗的一条。
    expect(mobile).not.toMatch(/\.chat-task-filter > button \{[^}]*min-height: 44px/);
  });
  it("更改面板：行内图标钮与列表/树切换不再撑到 36px", () => {
    expect(mobile).not.toMatch(/\.scm-icon-btn[^{]*\{[^}]*min-height: 36px/);
    expect(mobile).not.toMatch(/\.chat-diff-layout button \{[^}]*min-height: 36px/);
    expect(mobile).toContain("body.remote-ui .scm-icon-btn::after { inset: -10px 0; }");
  });
  it("更改面板的行右侧按列对齐：动作格定宽、统计列定最小宽并右对齐，组头计数坐进同一尾列", () => {
    expect(css).toMatch(/^\.scm \{ --scm-slot: 22px; --scm-slot-gap: 0px; --scm-stat-w: \d+px; \}/m);
    expect(css).toMatch(/^\.scm-letters \{[^}]*min-width: var\(--scm-stat-w\)[^}]*justify-content: flex-end/m);
    expect(css).toMatch(/^\.scm-icon-btn \{[^}]*width: var\(--scm-slot, 24px\)/m);
    expect(read("./SourceControl.tsx")).toContain('<span className="scm-letters"><span className="scm-count">');
  });
  it("搜索框与列表/树切换等高：切换组跟着搜索框拉伸", () => {
    expect(css).toMatch(/^\.scm-tools \{[^}]*align-items: stretch/m);
  });
});
