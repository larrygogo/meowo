import { useState, type MutableRefObject } from "react";
import { ChatSidebar } from "../views/ChatSidebar";
import { openNewSessionWindow } from "../api";
import { useT } from "../i18n";
import { RemoteSettingsScreen } from "./RemoteSettings";
import { RemoteTabBar, type RemoteTab } from "./RemoteTabBar";

/// 手机竖屏的首页：大标题 + 一屏内容 + 浮动胶囊底栏。底栏在「会话」「设置」两屏间切换，
/// 「新建」打开新建会话浮层。切到设置时会话列表只是藏起来不卸载：搜索词、页签、滚动位置
/// 都留着，切回来不用重新拉一遍。
export function RemoteSessionList({ activeId, approvalAwaitingIds, visibleOrderRef, searchInputRef, onSelect, onPrefetch }: {
  activeId: number;
  approvalAwaitingIds: ReadonlySet<number>;
  visibleOrderRef?: MutableRefObject<number[]>;
  searchInputRef?: MutableRefObject<HTMLInputElement | null>;
  onSelect: (id: number) => void;
  onPrefetch?: (id: number) => void;
}) {
  const t = useT();
  const [tab, setTab] = useState<RemoteTab>("sessions");
  const settings = tab === "settings";
  return (
    <section className={"remote-session-list" + (settings ? " is-settings" : "")} aria-label={settings ? t.remote.settingsLabel : t.chat.sidebarTitle}>
      <header className="remote-session-list-head">
        <div>
          <h1>{settings ? t.remote.settingsLabel : t.remote.sessionListTitle}</h1>
          {!settings && <p>{t.remote.sessionListSubtitle}</p>}
        </div>
      </header>
      <div className="remote-session-list-pane" hidden={settings}>
        <ChatSidebar
          activeId={activeId}
          approvalAwaitingIds={approvalAwaitingIds}
          visibleOrderRef={visibleOrderRef}
          searchInputRef={searchInputRef}
          onSelect={onSelect}
          onPrefetch={onPrefetch}
          onCollapse={() => {}}
        />
      </div>
      {settings && <RemoteSettingsScreen />}
      <RemoteTabBar
        tab={tab}
        onTab={setTab}
        onNew={() => void openNewSessionWindow().catch(() => {})}
      />
    </section>
  );
}
