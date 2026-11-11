import { useT } from "../i18n";

export type RemoteTab = "sessions" | "settings";

/// 手机列表页的浮动胶囊底栏：会话 / 新建 / 设置。只在竖屏列表页出现（RemoteSessionList 渲染），
/// 进了对话就让位给输入框。「会话」「设置」是同级的两屏，选中项用 aria-current 标出；
/// 「新建」是动作不是页面，走和侧栏「新建会话」同一个接口（远程桥转成页内浮层，见 RemoteApp）。
/// 只放文字不放图标（样张如此）：三项都是两个字，胶囊里再塞图标反而挤。
export function RemoteTabBar({ tab, onTab, onNew }: {
  tab: RemoteTab;
  onTab: (tab: RemoteTab) => void;
  onNew: () => void;
}) {
  const t = useT();
  return (
    <nav className="remote-tabbar" aria-label={t.remote.tabBarLabel}>
      <button
        type="button"
        className={"remote-tabbar-item" + (tab === "sessions" ? " is-active" : "")}
        aria-current={tab === "sessions" ? "page" : undefined}
        onClick={() => onTab("sessions")}
      >{t.remote.sessionListTitle}</button>
      <button type="button" className="remote-tabbar-item" onClick={onNew}>{t.remote.tabNew}</button>
      <button
        type="button"
        className={"remote-tabbar-item" + (tab === "settings" ? " is-active" : "")}
        aria-current={tab === "settings" ? "page" : undefined}
        onClick={() => onTab("settings")}
      >{t.remote.settingsLabel}</button>
    </nav>
  );
}
