import type { MutableRefObject } from "react";
import { ChatSidebar } from "../views/ChatSidebar";
import { useT } from "../i18n";
import { RemoteSettings } from "./RemoteSettings";
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
  return (
    <section className="remote-session-list" aria-label={t.chat.sidebarTitle}>
      <header className="remote-session-list-head">
        <img src={logoUrl} width="34" height="34" alt="" />
        <div>
          <h1>{t.remote.sessionListTitle}</h1>
          <p>{t.remote.sessionListSubtitle}</p>
        </div>
        <RemoteSettings />
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
