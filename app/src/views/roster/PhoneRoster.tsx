// 手机页上的名单：和桌面主窗口同一张名单、同一种行、同一种行内决定单（规格 .lead/20261111-roster）。
// 点「进入现场」就是原来的手机对话页。手机页上不放眼睛（同桌面，leader 2026-11-12）。
import { useCallback, useMemo, useState } from "react";
import { useT } from "../../i18n";
import { useSidebarGitBadges } from "../chat/SidebarGitBadge";
import { RosterItem } from "./RosterShell";
import { useRosterClock, useRosterLease, useRosterRows } from "./useRoster";

export function PhoneRoster({ onEnter, onPrefetch }: { onEnter: (id: number) => void; onPrefetch?: (id: number) => void }) {
  const t = useT();
  const { sessions, rows } = useRosterRows();
  // 手机停在名单上时也持有名单租约：任何会话的审批都留着等人答，和桌面谁先答谁生效。
  useRosterLease();
  const [expanded, setExpanded] = useState<number | null>(null);
  // 引用要稳：行是按内容记忆的，每次渲染换一个新函数等于没记。
  const toggle = useCallback((id: number) => setExpanded((prev) => (prev === id ? null : id)), []);
  const clock = useRosterClock();
  const gitBadges = useSidebarGitBadges(useMemo(() => rows.map((row) => row.cwd), [rows]));
  return (
    <div className="roster-phone">
      <div className="roster-list" role="list">
        {sessions != null && rows.length === 0 && <p className="roster-empty">{t.empty.allTitle}</p>}
        {rows.map((item) => (
          <RosterItem
            key={item.session.id}
            item={item}
            open={expanded === item.session.id}
            clock={clock}
            gitBadges={gitBadges}
            onToggle={toggle}
            onEnter={onEnter}
            onPrefetch={onPrefetch}
          />
        ))}
      </div>
    </div>
  );
}
