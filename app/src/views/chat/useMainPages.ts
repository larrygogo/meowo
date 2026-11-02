/// 对话窗主区的两张「页」（从 ChatWindow.tsx 拆出，行为不变）：新建会话页、会话管理页。
/// 它们盖在对话主区上面（对话主区不卸载只藏起来），选任何会话都让位回到对话。
///
/// 入口：窗内窗外的「新建会话」经后端发 chat-new-session；设置页「已完成会话」经后端发
/// chat-open-admin（未开窗时分别带 ?newSession=1 / ?admin=done 开窗）；侧栏菜单直接调 openAdmin。
import { useCallback, useEffect, useRef, useState } from "react";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { SELECT_SESSION_EVENT } from "../../remoteMode";

type NewSessionPrefill = { cwd?: string | null; provider?: string | null };
type AdminStatus = "all" | "open" | "done";

/** 后端带 ?newSession=1 开窗（贴纸等窗外入口的「新建会话」）时首屏直接是新建会话页。 */
function initialNewSession(): (NewSessionPrefill & { nonce: number }) | null {
  const query = new URLSearchParams(window.location.search);
  if (query.get("newSession") !== "1") return null;
  return { cwd: query.get("cwd"), provider: query.get("provider"), nonce: 0 };
}

const initialAdminDone = () => new URLSearchParams(window.location.search).get("admin") === "done";

export function useMainPages({ resetTo, closeOverlay }: {
  /** 切到某条会话（ChatWindow 的 resetTo）。 */
  resetTo: (id: number) => void;
  /** 收起窄窗的浮层侧栏（从浮层里选会话 / 开页面时）。 */
  closeOverlay: () => void;
}) {
  // 会话管理页（对照 Kimi Code）：从侧栏菜单打开，在侧栏或管理页里选任何会话都回到对话。
  // 设置页「已完成会话」入口经后端开窗带 ?admin=done：首屏就是会话管理页、筛在已完成。
  const [adminOpen, setAdminOpen] = useState(initialAdminDone);
  const [adminStatus, setAdminStatus] = useState<AdminStatus>(() => (initialAdminDone() ? "done" : "all"));
  // 管理页打开时预选的工作区：新建页「最近会话 · 查看更多」带进来；侧栏菜单进来的不预选。
  const [adminWorkspaces, setAdminWorkspaces] = useState<string[]>([]);
  // 新建会话页（对照 Kimi：不弹窗，主区就地换成新建页）。nonce 让再点一次「新建会话」
  // 换一份干净的表单（带上新的预填）。
  const [newSessionOpen, setNewSessionOpen] = useState(initialNewSession);
  // 新建页开着时拖进窗口的文件夹（交给新建页设为工作区）。拖拽订阅只挂一次，经 ref 读开合。
  const [newSessionDrop, setNewSessionDrop] = useState<{ path: string; nonce: number } | null>(null);
  const newSessionOpenRef = useRef(false);
  newSessionOpenRef.current = newSessionOpen != null;

  const showAdmin = useCallback((status: AdminStatus, workspaces: string[]) => {
    setNewSessionOpen(null);
    setAdminWorkspaces(workspaces);
    setAdminStatus(status);
    setAdminOpen(true);
  }, []);
  /** 被带去某个会话（点卡片、通知、快速切换…）时两张页都让位。 */
  const leavePages = useCallback(() => {
    setNewSessionOpen(null);
    setAdminOpen(false);
  }, []);

  const openAdmin = useCallback(() => showAdmin("all", []), [showAdmin]);
  const openAdminFromOverlay = useCallback(() => {
    closeOverlay();
    setNewSessionOpen(null);
    setAdminOpen(true);
  }, [closeOverlay]);
  /** 新建页「最近会话 · 查看更多」：预选那个工作区。 */
  const openAdminFor = useCallback((dir: string) => showAdmin("all", [dir]), [showAdmin]);
  const closeAdmin = useCallback(() => setAdminOpen(false), []);
  // 窗口已开时，设置页的入口经后端发 chat-open-admin 到这里。
  useTauriEvent<string>("chat-open-admin", () => {
    closeOverlay();
    showAdmin("done", []);
  });

  const selectSession = useCallback((id: number) => {
    leavePages();
    resetTo(id);
  }, [leavePages, resetTo]);
  const selectFromOverlay = useCallback((id: number) => {
    closeOverlay();
    leavePages();
    resetTo(id);
  }, [closeOverlay, leavePages, resetTo]);

  // 窗内窗外的「新建会话」都经后端发 chat-new-session 到这里（侧栏按钮、Ctrl+N、贴纸…）。
  useTauriEvent<NewSessionPrefill>("chat-new-session", (event) => {
    closeOverlay();
    setAdminOpen(false);
    setNewSessionDrop(null);
    setNewSessionOpen({ cwd: event.payload?.cwd, provider: event.payload?.provider, nonce: Date.now() });
  });
  /** 新建页开着时拖进窗口的路径（是不是文件夹由新建页去核实）。 */
  const dropFolder = useCallback((path: string) => setNewSessionDrop({ path, nonce: Date.now() }), []);

  // 远程新建会话启动成功:RemoteApp 把临时负 id 经页内事件送回来(桥不 reveal,这是移动端
  // 唯一的导航通路)。选中走侧栏点选同一条 resetTo 通道;负 id 由 ChatWindow 的 binding 轮询
  // (managed_terminal_binding 在远程白名单内)在认领后原地重绑成真 id。桌面此事件永不派发。
  useEffect(() => {
    const select = (e: Event) => {
      const id = (e as CustomEvent<unknown>).detail;
      if (typeof id === "number") selectFromOverlay(id);
    };
    window.addEventListener(SELECT_SESSION_EVENT, select);
    return () => window.removeEventListener(SELECT_SESSION_EVENT, select);
  }, [selectFromOverlay]);

  return {
    adminOpen, adminStatus, adminWorkspaces, newSessionOpen, newSessionDrop, newSessionOpenRef,
    openAdmin, openAdminFromOverlay, openAdminFor, closeAdmin, leavePages, selectSession, selectFromOverlay, dropFolder,
  };
}
