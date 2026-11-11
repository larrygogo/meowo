import { useState, type MutableRefObject } from "react";
import { ChatSidebar } from "../views/ChatSidebar";
import { openNewSessionWindow } from "../api";
import { useT } from "../i18n";
import { RemoteSettingsScreen } from "./RemoteSettings";
import { RemoteTabBar, type RemoteTab } from "./RemoteTabBar";
import { PhoneRoster } from "../views/roster/PhoneRoster";

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
  // 「会话」这一屏默认是名单（和桌面主窗口同一张）。搜索、已完成、工作区这些找会话的入口留在
  // 原来的会话列表里，从标题右边切过去：名单只管此刻在跑的这队 agent。
  const [all, setAll] = useState(false);
  return (
    <section className={"remote-session-list" + (settings ? " is-settings" : "")} aria-label={settings ? t.remote.settingsLabel : t.chat.sidebarTitle}>
      <header className="remote-session-list-head">
        <div>
          <h1>{settings ? t.remote.settingsLabel : t.remote.sessionListTitle}</h1>
          {!settings && <p>{t.remote.sessionListSubtitle}</p>}
        </div>
        {!settings && (
          <button type="button" className="remote-session-list-switch" onClick={() => setAll((v) => !v)}>
            {all ? t.roster.back : t.roster.allSessions}
          </button>
        )}
      </header>
      {!all && !settings && <PhoneRoster onEnter={onSelect} onPrefetch={onPrefetch} />}
      <div className="remote-session-list-pane" hidden={settings || !all}>
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
