/// 对话窗左侧会话栏的版式：停靠时的收起与宽度（持久化）、窄窗时的浮窗抽屉、分栏柄拖拽。
///
/// 为什么单独成 hook：这几条规则互相咬合（窄窗不写收起偏好、拉宽自动收抽屉、抽屉首开后
/// 常驻、抽屉开着要占一层 Esc），此前和会话、审批、发送的状态混在 ChatWindow 同一段里。
/// 侧栏「开关」本身留在调用方：远程窄屏上它的意思是「回会话列表」，那是导航，不是版式。

import { useEffect, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { pushEscLayer } from "../../escLayers";
import { NEW_SESSION_EVENT } from "../../remoteMode";
import { CHAT_MIN_WIDTH, measuredGutter, SIDEBAR_MIN_WIDTH, sidebarDefaultWidth, sidebarMaxWidth, sidebarMinWidth, sidebarNarrowBreakpoint, siblingWidth, useUiScale } from "./paneLayout";

const SIDEBAR_COLLAPSED_KEY = "meowo-chat-sidebar-collapsed";
/** 停靠侧栏宽度的持久化键。上下限与默认宽见 paneLayout：默认 240 乘界面缩放（关卡
 *  A-20261111-ui-spec），拖拽上限扣掉右侧面板再给对话区留 320，另有绝对上限。 */
const SIDEBAR_WIDTH_KEY = "meowo-chat-sidebar-width";
/** 窄窗门限：低于它停靠侧栏放不下，自动收起、开关改开浮窗。随界面缩放（见 paneLayout 的 sidebarNarrowBreakpoint），100% 时 880。 */
const narrowQuery = (ui: number) => `(max-width: ${sidebarNarrowBreakpoint(ui)}px)`;

export function useSidebarLayout() {
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1");
  // 停靠侧栏宽随会话持久化（localStorage）；收起只藏不改宽，再展开原宽回来。
  // null = 用户没拖过：宽度跟着界面缩放走（设置里调缩放时实时变），拖过就照用户的。
  const ui = useUiScale();
  const [draggedWidth, setSidebarWidth] = useState<number | null>(() => {
    const stored = Number(localStorage.getItem(SIDEBAR_WIDTH_KEY));
    return Number.isFinite(stored) && stored >= SIDEBAR_MIN_WIDTH ? stored : null;
  });
  const minWidth = sidebarMinWidth(ui);
  const sidebarWidth = Math.max(minWidth, draggedWidth ?? sidebarDefaultWidth(ui));
  /** 拖拽 / 键盘调宽的上限：绝对上限与「.chat-body 减右侧面板减 320」取小（调研 s5/c4）。 */
  const maxIn = (body: Element | null) =>
    body
      ? Math.max(minWidth, Math.min(sidebarMaxWidth(ui), body.getBoundingClientRect().width - siblingWidth(body, ".chat-diff-panel") - measuredGutter(body) - CHAT_MIN_WIDTH))
      : sidebarMaxWidth(ui);
  // 窄窗（100% 缩放时 <880px，随缩放抬高）自动收起停靠侧栏：两列塞不下。此时开关切换的是**浮窗侧栏**（overlay，
  // 盖在主列上、不挤压布局），且不写收起偏好——窗口拉宽后回到用户原本的停靠选择。
  // jsdom 没有 matchMedia，恒按宽窗走（测试语义不变）。
  const [narrow, setNarrow] = useState(() => typeof window.matchMedia === "function" && window.matchMedia(narrowQuery(ui)).matches);
  const [overlaySidebar, setOverlaySidebar] = useState(false);
  // 抽屉首开后保持挂载（S-2 同款常驻）：每次打开都重挂会让 ChatSidebar 的列表状态
  // 清零、远程 650ms 轮询空窗期整栏「加载中」闪屏（移动端实拍反馈）。隐藏用 hidden
  // 属性而非卸载，列表数据与滚动位置都在；拉宽回桌面形态时随 narrow 翻转整体卸载。
  const [drawerMounted, setDrawerMounted] = useState(false);
  useEffect(() => {
    if (narrow && overlaySidebar) setDrawerMounted(true);
  }, [narrow, overlaySidebar]);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(narrowQuery(ui));
    const onChange = () => {
      setNarrow(mq.matches);
      // 拉宽后浮窗失去意义，收掉并回到停靠形态。
      if (!mq.matches) setOverlaySidebar(false);
    };
    // 设置里调了界面缩放：门槛变了，按新门槛立刻判一次。
    onChange();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [ui]);
  // 窄窗浮窗侧栏（抽屉）开着期间注册 Esc 层收抽屉：此前只有 scrim 点外关，
  // 键盘用户没有出口。走 escLayers 统一栈，窗口级「Esc=拒绝审批」自动让位。
  useEffect(() => {
    if (!narrow || !overlaySidebar) return;
    const popLayer = pushEscLayer();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      setOverlaySidebar(false);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      popLayer();
      window.removeEventListener("keydown", onKey);
    };
  }, [narrow, overlaySidebar]);
  // 远程:侧栏点「新建会话」派发页内导航事件(RemoteApp 据此叠加渲染 sheet)。窄屏抽屉
  // 层级(z 61)在 sheet 之上,不收就盖住 sheet——收到事件即收抽屉。桌面此事件永不派发。
  useEffect(() => {
    const close = () => setOverlaySidebar(false);
    window.addEventListener(NEW_SESSION_EVENT, close);
    return () => window.removeEventListener(NEW_SESSION_EVENT, close);
  }, []);

  const sidebarVisible = narrow ? overlaySidebar : !sidebarCollapsed;
  /// 停靠形态的收起/展开（写偏好）。窄窗下切的是抽屉，不写偏好。
  const toggleDocked = () => {
    if (narrow) { setOverlaySidebar((prev) => !prev); return; }
    setSidebarCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, next ? "1" : "0");
      return next;
    });
  };

  // 侧栏分栏柄：与 startDiffResize 同套手法（pointer capture + 按帧合流 + 松手持久化），
  // 方向相反——柄在侧栏右缘，向右拖变宽。起点读 aside 实测宽而非 state：与 diff 同款
  // 的「state 被压过时从 state 起算会跳变」在这里对应窄窗回宽等口径。
  const startSidebarResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const handle = event.currentTarget;
    const body = handle.closest(".chat-body");
    if (!body) return;
    event.preventDefault();
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const sidebar = handle.previousElementSibling as HTMLElement | null;
    const startWidth = sidebar ? sidebar.getBoundingClientRect().width : sidebarWidth;
    const maxWidth = maxIn(body);
    let lastWidth = startWidth;
    body.classList.add("is-sidebar-resizing");
    // pointermove 每次都 setState 会以指针事件频率重渲整栏（几百行会话列表），
    // 按帧合流，一帧至多提交一次宽度（与 diff 分栏同一课）。
    let raf = 0;
    const onMove = (move: PointerEvent) => {
      lastWidth = Math.min(Math.max(startWidth + (move.clientX - startX), minWidth), maxWidth);
      if (!raf) {
        raf = requestAnimationFrame(() => {
          raf = 0;
          setSidebarWidth(lastWidth);
        });
      }
    };
    const onUp = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      setSidebarWidth(lastWidth);
      body.classList.remove("is-sidebar-resizing");
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      handle.removeEventListener("pointercancel", onUp);
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(Math.round(lastWidth)));
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
    handle.addEventListener("pointercancel", onUp);
  };

  // 分栏柄的键盘调宽（←/→ 每次 16px，上限同拖拽）：分栏柄是可聚焦的 separator，
  // 只能拖不能键调就把键盘用户挡在外面。
  const nudgeSidebarWidth = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const maxWidth = maxIn(event.currentTarget.closest(".chat-body"));
    const next = Math.min(Math.max(sidebarWidth + (event.key === "ArrowRight" ? 16 : -16), minWidth), maxWidth);
    setSidebarWidth(next);
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(Math.round(next)));
  };

  // 双击分栏柄恢复默认宽（跟回界面缩放）：拖坏了不必再一点点拖回来。
  const resetSidebarWidth = () => {
    localStorage.removeItem(SIDEBAR_WIDTH_KEY);
    setSidebarWidth(null);
  };

  return {
    sidebarCollapsed, sidebarWidth, resetSidebarWidth, uiScale: ui, narrow, overlaySidebar, setOverlaySidebar, drawerMounted, sidebarVisible,
    toggleDocked, startSidebarResize, nudgeSidebarWidth, sidebarMinWidth: minWidth,
  };
}
