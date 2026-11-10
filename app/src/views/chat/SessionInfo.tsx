import { useRef, type ReactNode } from "react";
import type { BrowserPaneInfo, GitDiffSummaryDto, ShellTerminalInfo } from "../../api";
import { changedFileCount } from "./gitDiff";
import { useDismissable } from "../../hooks/useDismissable";
import { useT } from "../../i18n";
import type { PanelState, PanelTabType } from "./panelTabs";
import { PANEL_ICONS, PANEL_SHORTCUTS, shortcutLabel as mod } from "./panelIcons";
import { tabTitle, type PanelActions } from "./usePanel";

/** 「已打开标签页」最多列几个，多了显示「查看更多」（同 Kimi Code）。 */
const MAX_LISTED = 5;


/**
 * 会话信息栏：右侧面板收起时露出来的一列（照 Kimi Code 1.3.1 的 SessionInfoTabs）。
 * 改动数（点了开改动页签）、已经开着的页签、还能开的东西。分支和 PR 标题栏上已经有，这里不重复。
 *
 * 窗口够宽时停靠在对话右侧（docked），窄了从标题栏按钮弹出浮层（floating）。
 */
export function SessionInfo({ mode, state, actions, shells, panes, summary, onClose }: {
  mode: "docked" | "floating";
  state: PanelState;
  actions: PanelActions;
  shells: ReadonlyMap<number, ShellTerminalInfo>;
  panes: ReadonlyMap<number, BrowserPaneInfo>;
  summary: GitDiffSummaryDto | null;
  /** 浮层：点外、Esc、选了一项之后关掉。 */
  onClose?: () => void;
}) {
  const t = useT();
  const ref = useRef<HTMLElement | null>(null);
  // 点外关闭放在 click 阶段并拦下：点标题栏的开关按钮时只关不重开（pointerdown 关、click 又开）。
  useDismissable(ref, { open: mode === "floating", onClose: () => onClose?.(), outsideEvent: "click", interceptOutsideClick: true });
  const done = (run: () => void) => () => {
    run();
    onClose?.();
  };

  const files = summary?.isRepo ? summary.files : [];
  const additions = files.reduce((sum, file) => sum + (file.additions ?? 0), 0);
  const deletions = files.reduce((sum, file) => sum + (file.deletions ?? 0), 0);
  const listed = state.tabs.slice(0, MAX_LISTED);
  const more = state.tabs.length - listed.length;
  const openable: { type: PanelTabType; label: string; run: () => void }[] = [
    { type: "browser", label: t.chat.browserNew, run: () => actions.newBrowser() },
    { type: "term", label: t.chat.shellNew, run: actions.newTerminal },
    { type: "files", label: t.chat.filesTab, run: actions.openFiles },
  ];

  return (
    <aside
      ref={ref}
      className={"session-info is-" + mode}
      aria-label={t.chat.sessionInfo}
      role={mode === "floating" ? "dialog" : undefined}
    >
      {summary?.isRepo && (
        <Section title={t.chat.diffChangesTab}>
          <button type="button" className="session-info-row" onClick={done(actions.openDiff)}>
            {PANEL_ICONS.diff}
            <span className="session-info-label">
              {files.length === 0 ? t.chat.sessionInfoClean : t.chat.uncommitted(changedFileCount(files))}
            </span>
            {additions + deletions > 0 && (
              <span className="session-info-diff"><span className="tb-git-add">+{additions}</span> <span className="tb-git-del">−{deletions}</span></span>
            )}
          </button>
        </Section>
      )}
      {listed.length > 0 && (
        <Section title={t.chat.sessionInfoOpenTabs}>
          {listed.map((tab) => (
            <button key={tab.key} type="button" className="session-info-row" onClick={done(() => actions.activate(tab.key))}>
              {PANEL_ICONS[tab.type]}
              <span className="session-info-label">{tabTitle(tab, shells, panes, t)}</span>
            </button>
          ))}
          {more > 0 && (
            <button type="button" className="session-info-row is-more" onClick={done(actions.show)}>
              <span className="session-info-label">{t.chat.sessionInfoMore(more)}</span>
            </button>
          )}
        </Section>
      )}
      <Section title={t.chat.panelLaunch}>
        {openable.map((item) => (
          <button key={item.type} type="button" className="session-info-row" onClick={done(item.run)}>
            {PANEL_ICONS[item.type]}
            <span className="session-info-label">{item.label}</span>
            <kbd>{mod(PANEL_SHORTCUTS[item.type])}</kbd>
          </button>
        ))}
      </Section>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="session-info-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}
