import { useState, type MutableRefObject } from "react";
import { ChatSidebar } from "../views/ChatSidebar";
import { Dropdown } from "../views/menu";
import { useT } from "../i18n";
import { remoteThemeMode, setRemoteThemeMode } from "../appearance";
import type { ThemeMode } from "../api";
import logoUrl from "../../src-tauri/icons/128x128@2x.png";

export function RemoteSessionList({ activeId, approvalAwaitingIds, visibleOrderRef, searchInputRef, onSelect, onPrefetch }: {
  activeId: number;
  approvalAwaitingIds: ReadonlySet<number>;
  visibleOrderRef?: MutableRefObject<number[]>;
  searchInputRef?: MutableRefObject<HTMLInputElement | null>;
  onSelect: (id: number) => void;
  onPrefetch?: (id: number) => void;
}) {
  const t = useT();
  // 明暗模式只存在这台手机上（见 appearance.ts 的 remoteThemeMode），入口放在列表页头：
  // 手机页没有设置页，列表又是每次打开的落点。
  const [theme, setTheme] = useState<ThemeMode>(remoteThemeMode);
  return (
    <section className="remote-session-list" aria-label={t.chat.sidebarTitle}>
      <header className="remote-session-list-head">
        <img src={logoUrl} width="34" height="34" alt="" />
        <div>
          <h1>{t.remote.sessionListTitle}</h1>
          <p>{t.remote.sessionListSubtitle}</p>
        </div>
        <div className="remote-theme" aria-label={t.remote.themeLabel}>
          <Dropdown<ThemeMode>
            value={theme}
            options={[
              { value: "system", label: t.settings.themeSystem },
              { value: "light", label: t.settings.themeLight },
              { value: "dark", label: t.settings.themeDark },
            ]}
            onChange={(mode) => {
              setTheme(mode);
              setRemoteThemeMode(mode);
            }}
          />
        </div>
      </header>
      <ChatSidebar
        activeId={activeId}
        approvalAwaitingIds={approvalAwaitingIds}
        visibleOrderRef={visibleOrderRef}
        searchInputRef={searchInputRef}
        onSelect={onSelect}
        onPrefetch={onPrefetch}
        onCollapse={() => {}}
      />
    </section>
  );
}
