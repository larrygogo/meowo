// 手机页上的名单：和桌面主窗口同一张名单、同一种行、同一种行内决定单（规格 .lead/20261111-roster）。
// 黑轨收成屏幕左缘窄窄一条，眼睛照样站在轮到你的那一行旁边。点「进入现场」就是原来的手机对话页。
import { useCallback, useMemo, useRef, useState } from "react";
import { useT } from "../../i18n";
import { useSidebarGitBadges } from "../chat/SidebarGitBadge";
import { Eyes } from "../island/islandParts";
import { GAZE_ROW, RosterItem } from "./RosterShell";
import { useAttention, useEyesY, useRosterClock, useRosterLease, useRosterRows } from "./useRoster";

export function PhoneRoster({ onEnter, onPrefetch }: { onEnter: (id: number) => void; onPrefetch?: (id: number) => void }) {
  const t = useT();
  const { sessions, rows } = useRosterRows();
  // 手机停在名单上时也持有名单租约：任何会话的审批都留着等人答，和桌面谁先答谁生效。
  useRosterLease();
  const [expanded, setExpanded] = useState<number | null>(null);
  const attention = useAttention(rows, expanded);
  // 引用要稳：行是按内容记忆的，每次渲染换一个新函数等于没记。
  const toggle = useCallback((id: number) => setExpanded((prev) => (prev === id ? null : id)), []);
  const clock = useRosterClock();
  const rootRef = useRef<HTMLDivElement>(null);
  const eyesY = useEyesY(rootRef, attention.id == null ? null : `.roster-item[data-session-id="${attention.id}"] .roster-row`);
  const gitBadges = useSidebarGitBadges(useMemo(() => rows.map((row) => row.cwd), [rows]));
  return (
    <div ref={rootRef} className="roster-phone">
      <aside className="roster-rail" aria-label={t.roster.rail}>
        <span className="roster-eyes" style={{ top: eyesY }} data-at={attention.id ?? "home"}>
          <Eyes tone={attention.tone} small gaze={attention.id != null ? GAZE_ROW : null} />
        </span>
      </aside>
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
