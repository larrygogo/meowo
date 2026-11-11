/// 对话窗三栏（停靠侧栏 / 对话区 / 右侧面板）的宽度分配，一处算清。
///
/// 为什么集中：此前侧栏和面板各管各的上限，都只拿 .chat-body 宽减 320/360，没扣对方。
/// 1440 宽侧栏常驻时把面板拖到底，对话区只剩 80px（调研 c4）；侧栏在大屏拖到 560 再换
/// 1000 宽窗口，宽度不回收，主列只剩 440、面板直接没了（调研 s5）。宽度还写进了
/// localStorage，下次开窗还是坏的。现在渲染时按实测 .chat-body 宽重新钳制：存储值照旧保留
/// （窗口拉回宽时恢复用户拖的宽度），但画出来的宽度永远给对话区留 CHAT_MIN_WIDTH。
import { useEffect, useLayoutEffect, useState, type RefObject } from "react";

/** 对话区最窄：照 Kimi Code，320 还能放下审批卡按钮和输入框工具行。 */
export const CHAT_MIN_WIDTH = 320;
export const SIDEBAR_MIN_WIDTH = 200;
/** 侧栏默认宽与绝对上限（乘界面缩放）。240：2026-10 实拍反馈 208 偏窄、会话名读不全。
 *  上限 440：Kimi / VS Code 的侧栏都有上限，再宽只是吃对话区。 */
export const SIDEBAR_BASE_WIDTH = 240;
export const SIDEBAR_BASE_MAX = 440;
export const PANEL_MIN_WIDTH = 320;
/** 面板默认宽封顶：没封顶时 2560 宽下默认 1140px，提交框和「提交全部」拉成一千多 px 的横条，
 *  文件名和增删数隔着半个屏幕（调研 c6）。560 约等于 Kimi 面板的舒适宽。 */
export const PANEL_DEFAULT_MAX = 560;

/** 侧栏默认宽随界面缩放（关卡 A-20261111-ui-spec 第 1 条：推翻「列宽不随密度缩放」）。 */
export const sidebarDefaultWidth = (ui: number) => Math.round(SIDEBAR_BASE_WIDTH * ui);
export const sidebarMaxWidth = (ui: number) => Math.round(SIDEBAR_BASE_MAX * ui);
/** 下限同样乘缩放：150% 下拖到 200，三段页签只剩省略号（脚本 t26 实测），等于 1.0 时的 133。 */
export const sidebarMinWidth = (ui: number) => Math.round(SIDEBAR_MIN_WIDTH * ui);
/** 窄窗门槛：窗口宽 ≤ 它时停靠侧栏收成抽屉。原先固定 880 = 240 侧栏 + 640（对话区与面板下限之和），
 *  侧栏默认宽随缩放变大后没跟上：150% 缩放、900 宽开面板时侧栏 300 + 对话 320 只给面板剩 261，
 *  低于面板自己的下限（做空审查第二轮实测）。改成随侧栏默认宽一起抬，100% 时仍是 880。 */
export const sidebarNarrowBreakpoint = (ui: number) => sidebarDefaultWidth(ui) + CHAT_MIN_WIDTH + PANEL_MIN_WIDTH;
/** 面板默认宽：窗口宽减侧栏后的一半，夹在 [PANEL_MIN_WIDTH, PANEL_DEFAULT_MAX]。 */
export const panelDefaultWidth = (windowWidth: number) =>
  Math.min(PANEL_DEFAULT_MAX, Math.max(PANEL_MIN_WIDTH, Math.round((windowWidth - 280) / 2)));

/**
 * 按 .chat-body 实测宽钳制两侧宽度。sidebar / panel 传 null 表示该栏不在（收起、抽屉形态、
 * 面板最大化）。窗口变窄时面板先让（让到 PANEL_MIN_WIDTH），再让侧栏；最小窗口 620 开着
 * 面板时面板让到 300，对话区仍保 320。body 为 0 表示还没量到，原样返回。
 */
export function fitPanes(input: { body: number; sidebar: number | null; panel: number | null; ui?: number }): { sidebar: number | null; panel: number | null } {
  const { body, ui = 1 } = input;
  if (body <= 0) return { sidebar: input.sidebar, panel: input.panel };
  let sidebar = input.sidebar;
  if (sidebar != null) {
    const room = body - CHAT_MIN_WIDTH - (input.panel != null ? PANEL_MIN_WIDTH : 0);
    sidebar = Math.max(sidebarMinWidth(ui), Math.min(sidebar, sidebarMaxWidth(ui), room));
  }
  let panel = input.panel;
  if (panel != null) {
    panel = Math.max(0, Math.min(Math.max(panel, PANEL_MIN_WIDTH), body - (sidebar ?? 0) - CHAT_MIN_WIDTH));
  }
  return { sidebar, panel };
}

/** 读 :root 上的 --cc-ui（界面缩放），设置里改了会实时跟上（appearance.ts 改的是 :root 的内联样式）。 */
export function readUiScale(): number {
  if (typeof document === "undefined") return 1;
  const raw = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--cc-ui"));
  return Number.isFinite(raw) && raw > 0 ? raw : 1;
}

export function useUiScale(): number {
  const [ui, setUi] = useState(readUiScale);
  useEffect(() => {
    if (typeof MutationObserver !== "function") return;
    const observer = new MutationObserver(() => setUi(readUiScale()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
    return () => observer.disconnect();
  }, []);
  return ui;
}

/** 元素的边框盒宽度，跟 ResizeObserver 走。0 = 没量到（jsdom）。 */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(Math.round(el.getBoundingClientRect().width));
    if (typeof ResizeObserver !== "function") return;
    const observer = new ResizeObserver(() => setWidth(Math.round(el.getBoundingClientRect().width)));
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}

/** 拖拽 / 键盘调宽时，另一栏在 .chat-body 里实际占的宽（不在就是 0）。 */
export function siblingWidth(body: Element | null, selector: string): number {
  const el = body?.querySelector<HTMLElement>(`:scope > ${selector}`);
  return el ? el.getBoundingClientRect().width : 0;
}
