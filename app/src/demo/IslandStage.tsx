// 官网灵动岛演示（0.8.0 起两个平台的贴纸都换成了灵动岛；用户嫌手画的示意「太假」）。
// 直接用应用里真实的 Island / IslandPanel 组件和真实 agent 图标，数据是假的。舞台是一块带
// 刘海的 Mac 屏幕：岛包住刘海，循环播放「收起 → 弹出审批提醒 → 收起 → 点开面板」。
// 尺寸固定 880×600（与贴纸演示同一设计尺寸，官网 DemoFrame 按容器宽度整体缩放）。
import { useEffect, useMemo, useState } from "react";
import { Island, ISLAND_ALERT_SIZE, ISLAND_EAR } from "../views/Island";
import { IslandPanel } from "../views/island/IslandPanel";
import type { IslandAlertEvent } from "../generated/contracts/IslandAlertEvent";
import { makeSession, type Item } from "./data";
import type { DemoLang } from "./strings";

/// 刘海宽高（逻辑点），取 14 寸 MacBook Pro 的比例。
const NOTCH = { width: 184, height: 32 };
const PANEL_WIDTH = 600 + ISLAND_EAR * 2;

type Phase = "compact" | "alert" | "expanded";
/// 每一段停多久（毫秒），播完从头再来。
const SCRIPT: [Phase, number][] = [
  ["compact", 3200],
  ["alert", 3600],
  ["compact", 1400],
  ["expanded", 5200],
];

const COPY = {
  zh: {
    menu: ["Meowo", "文件", "编辑", "显示"],
    clock: "11月5日 周四 14:08",
    sessions: [
      { title: "重构登录页表单校验", project: "web-app", provider: "claude" as const },
      { title: "修支付回调的重复扣款", project: "billing", provider: "kimi" as const },
      { title: "补齐导出接口的单测", project: "api", provider: "codex" as const },
    ],
    alertDetail: null,
  },
  en: {
    menu: ["Meowo", "File", "Edit", "View"],
    clock: "Thu Nov 5  2:08 PM",
    sessions: [
      { title: "Refactor login form validation", project: "web-app", provider: "claude" as const },
      { title: "Fix duplicate charge in payment callback", project: "billing", provider: "kimi" as const },
      { title: "Add tests for the export API", project: "api", provider: "codex" as const },
    ],
    alertDetail: null,
  },
};

function buildSessions(lang: DemoLang): Item[] {
  const [run, pend, wait] = COPY[lang].sessions;
  const running = makeSession({ ...run, status: "running", todoDone: 2, todoTotal: 5, agoMin: 0 });
  const pending = makeSession({ ...pend, status: "running", agoMin: 1 });
  pending.pending_review = "approval";
  const waiting = makeSession({ ...wait, status: "waiting", agoMin: 4 });
  (waiting as Item & { unread?: boolean }).unread = true;
  return [running, pending, waiting];
}

export function IslandStage({ lang = "zh" }: { lang?: DemoLang }) {
  const copy = COPY[lang];
  const sessions = useMemo(() => buildSessions(lang), [lang]);
  const [step, setStep] = useState(0);
  useEffect(() => {
    const id = window.setTimeout(() => setStep((s) => (s + 1) % SCRIPT.length), SCRIPT[step][1]);
    return () => window.clearTimeout(id);
  }, [step]);
  const phase = SCRIPT[step][0];
  const pending = sessions[1];
  const alert: IslandAlertEvent | null = phase === "alert"
    ? { sessionId: pending.session.id, kind: "approval", title: pending.task_title ?? "", project: pending.project_name ?? "", detail: copy.alertDetail }
    : null;
  const noop = () => {};
  const vars = { "--isl-safe-top": `${NOTCH.height}px` } as React.CSSProperties;

  return (
    <div className="isd-desktop" style={vars}>
      <div className="isd-wallpaper" />
      <div className="isd-menubar">
        <span className="isd-menu">
          {copy.menu.map((m, i) => <span key={m} className={i === 0 ? "is-app" : undefined}>{m}</span>)}
        </span>
        <span className="isd-status">
          <i className="isd-icon is-wifi" />
          <i className="isd-icon is-battery" />
          <span>{copy.clock}</span>
        </span>
      </div>
      <div className="isd-window">
        <div className="isd-window-bar"><i /><i /><i /></div>
        <div className="isd-window-body">
          <span /><span /><span className="is-short" /><span /><span className="is-short" />
        </div>
      </div>
      <div className={`isd-island is-${phase}`}>
        {phase === "expanded" ? (
          <div className="island-shell is-entering" style={{ width: PANEL_WIDTH, "--island-pill-w": "360px", "--island-pill-h": `${NOTCH.height}px` } as React.CSSProperties}>
            <IslandPanel
              sessions={sessions}
              usage={{}}
              quotaProviders={[]}
              onOpenSession={noop}
              onMeasure={noop}
              onPointerDownDrag={noop}
              onNewSession={noop}
              onOpenChat={noop}
              onOpenSettings={noop}
            />
          </div>
        ) : (
          <div style={phase === "alert" ? { width: ISLAND_ALERT_SIZE.width, height: ISLAND_ALERT_SIZE.height + NOTCH.height } : undefined}>
            <Island
              sessions={sessions}
              compactStyle="counts"
              alert={alert}
              usages={[{ provider: "claude", pct: 29 }]}
              onMeasure={noop}
              onActivate={noop}
              onOpenSession={noop}
              onPointerDownDrag={noop}
              notch={NOTCH}
            />
          </div>
        )}
      </div>
    </div>
  );
}
