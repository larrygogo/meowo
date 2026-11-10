// 关对话窗前的确认：面板终端里还有程序在跑时先问一句，取消就不关。
//
// 为什么要拦：窗口真正销毁时后端（window.rs 的 Destroyed 分支）kill_all 面板里所有终端，连同
// 底下的子进程（dev server、测试、构建）。关单个页签早有确认（usePanel 的 close），关整扇窗却
// 一声不吭全杀掉。
//
// 三条关窗路径：
// - 标题栏关闭按钮：先确认，同意了才调 close()——取消时窗口根本没收到关闭请求。
// - 系统原生关闭（Alt+F4、任务栏、macOS 红绿灯与 Cmd+W 菜单项）：前端拿不到按键，只能在
//   onCloseRequested 里拦。注册了 close-requested 的 JS 监听后，tauri 会把关闭拦下交给前端
//   （tauri-2.8 manager/window.rs：has_js_listener → prevent_close），本该由前端调 destroy()
//   收尾，但本应用能力表没开 core:window:allow-destroy。所以确认后先摘掉这个监听再 close()：
//   没有 JS 监听时走默认关闭，也就不会再进这个处理函数——不会死循环。
// - 只在面板里确实开过终端时才挂监听：没有终端时原生关闭完全不经过前端，行为与改动前一致，
//   JS 卡死时窗口也照样关得掉。
//
// 后端配合：window.rs 的关窗收尾（清桌面审批租约、清「正在看」、交还挂起审批）只在 Destroyed
// 跑、不在 CloseRequested 跑，所以这里取消关闭不会让审批落回终端。取消后仍广播
// WINDOW_CLOSE_CANCELLED_EVENT 让终端视图与审批通道重新登记，作兜底（重复登记无害）。
import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { shellTerminalBusy } from "../../api";
import { appConfirm } from "../../confirm";
import type { Dict } from "../../i18n/zh";
import { remoteUi } from "../../remoteMode";

export const WINDOW_CLOSE_CANCELLED_EVENT = "meowo:window-close-cancelled";

// ── 本窗开着的面板终端（编号 → 所属会话）──
// usePanel 只拿得到正在看的会话的终端清单；关窗要问的是全部会话的，所以在这里统一记账。
const liveShells = new Map<number, number>();
const listeners = new Set<() => void>();
function changed() {
  for (const listener of listeners) listener();
}
export function trackShell(id: number, owner: number) {
  if (liveShells.get(id) === owner) return;
  liveShells.set(id, owner);
  changed();
}
export function untrackShell(id: number) {
  if (liveShells.delete(id)) changed();
}
export function untrackShellOwner(owner: number) {
  let hit = false;
  for (const [id, o] of liveShells) if (o === owner) hit = liveShells.delete(id) || hit;
  if (hit) changed();
}
export function moveShellOwner(from: number, to: number) {
  let hit = false;
  for (const [id, o] of liveShells) if (o === from) { liveShells.set(id, to); hit = true; }
  if (hit) changed();
}
export function trackedShellIds(): number[] {
  return [...liveShells.keys()];
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
const hasTrackedShells = () => liveShells.size > 0;

/** 这些终端里有程序在跑就弹确认；返回 true = 可以关。关页签与关窗共用同一句话。 */
export async function confirmCloseShells(ids: readonly number[], t: Dict): Promise<boolean> {
  if (ids.length === 0) return true;
  const busy = await shellTerminalBusy([...ids]).catch(() => []);
  if (busy.length === 0) return true;
  const lines = busy.map((item) => `${item.title} · ${item.processes.map((p) => `${p.name || "?"} (PID ${p.pid})`).join(", ")}`);
  return appConfirm(`${t.chat.shellBusyMessage}\n\n${lines.join("\n")}`, {
    title: t.chat.shellBusyTitle,
    danger: true,
    confirmLabel: t.chat.shellBusyConfirm,
  });
}

/** 返回给标题栏关闭按钮用的 requestClose；同时接管系统原生关闭（见文件头）。 */
export function useGuardedWindowClose(t: Dict): () => void {
  const tRef = useRef(t);
  tRef.current = t;
  const hasShells = useSyncExternalStore(subscribe, hasTrackedShells, hasTrackedShells);
  const unlistenRef = useRef<(() => void | Promise<void>) | null>(null);
  const inFlight = useRef(false);
  // 摘掉监听后 close() 却失败了（罕见）：重新挂上，别让之后的原生关闭失去确认。
  const [epoch, setEpoch] = useState(0);

  const guard = useCallback(async (native: boolean) => {
    if (inFlight.current) return; // 确认框开着时再点关闭，不叠第二个
    inFlight.current = true;
    try {
      const ok = await confirmCloseShells(trackedShellIds(), tRef.current);
      if (!ok) {
        if (native) window.dispatchEvent(new CustomEvent(WINDOW_CLOSE_CANCELLED_EVENT));
        return;
      }
      const unlisten = unlistenRef.current;
      unlistenRef.current = null;
      try { await unlisten?.(); } catch { /* 摘不掉也照样关：最坏是 close 又被拦下一次 */ }
      try {
        await getCurrentWindow().close();
      } catch {
        if (unlisten) setEpoch((n) => n + 1);
      }
    } finally {
      inFlight.current = false;
    }
  }, []);

  useEffect(() => {
    if (remoteUi() || !hasShells) return;
    let disposed = false;
    let win: ReturnType<typeof getCurrentWindow>;
    try { win = getCurrentWindow(); } catch { return; }
    if (typeof win.onCloseRequested !== "function") return;
    win.onCloseRequested((event) => {
      // 必须同步 preventDefault：处理函数返回时没拦，tauri 会去调 destroy()（本应用没这个权限）。
      event.preventDefault();
      void guard(true);
    }).then((unlisten) => {
      if (disposed) void Promise.resolve(unlisten()).catch(() => {});
      else unlistenRef.current = unlisten;
    }).catch(() => {});
    return () => {
      disposed = true;
      const unlisten = unlistenRef.current;
      unlistenRef.current = null;
      if (unlisten) void Promise.resolve(unlisten()).catch(() => {});
    };
  }, [hasShells, epoch, guard]);

  return useCallback(() => void guard(false), [guard]);
}

// 测试用：模块级账本在用例之间清零。
export function resetTrackedShellsForTest() {
  if (liveShells.size === 0) return;
  liveShells.clear();
  changed();
}
