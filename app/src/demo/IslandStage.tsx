// 官网灵动岛演示（0.8.0 起贴纸都换成了灵动岛）。用户先嫌手画的示意「太假」，又建议模拟
// Windows 桌面：这里是一块 Windows 桌面（壁纸 + 底部任务栏 + 一个正在跑 agent 的终端窗口），
// 屏幕顶边是应用里真实的 Island / IslandPanel 组件（真实 agent 图标，数据是假的）。
// 一只假鼠标按剧本循环操作：收起 → 弹出审批提醒 → 点开面板 → 点桌面收起。
// 尺寸固定 880×600（与贴纸演示同一设计尺寸，官网 DemoFrame 按容器宽度整体缩放）。
import { useEffect, useMemo, useRef, useState } from "react";
import { Island, ISLAND_ALERT_SIZE, ISLAND_EAR, ISLAND_PILL_HEIGHT } from "../views/Island";
import { IslandPanel } from "../views/island/IslandPanel";
import type { IslandAlertEvent } from "../generated/contracts/IslandAlertEvent";
import { makeSession, type Item } from "./data";
import type { DemoLang } from "./strings";
import logoUrl from "../../src-tauri/icons/128x128@2x.png";

const PANEL_WIDTH = 600 + ISLAND_EAR * 2;
type Phase = "compact" | "alert" | "expanded";

const COPY = {
  zh: {
    tab: "claude — web-app",
    sessions: [
      { title: "重构登录页表单校验", project: "web-app", provider: "claude" as const },
      { title: "修支付回调的重复扣款", project: "billing", provider: "kimi" as const },
      { title: "补齐导出接口的单测", project: "api", provider: "codex" as const },
    ],
    terminal: [
      { k: "prompt", t: "PS C:\\dev\\web-app> claude" },
      { k: "user", t: "> 重构登录页表单校验，把规则抽到 schema 里" },
      { k: "ai", t: "● 我先看一下现有的表单组件和校验逻辑。" },
      { k: "tool", t: "● Read(src/components/LoginForm.tsx)" },
      { k: "dim", t: "  ⎿  Read 182 lines" },
      { k: "tool", t: "● Update(src/components/LoginForm.tsx)" },
      { k: "dim", t: "  ⎿  Updated with 24 additions and 9 removals" },
      { k: "tool", t: "● Bash(npm test -- login)" },
      { k: "ok", t: "  ⎿  ✓ 12 passed" },
      { k: "spin", t: "✻ 正在把校验规则抽到 schema… (38s · ↓ 2.1k tokens)" },
    ],
  },
  en: {
    tab: "claude — web-app",
    sessions: [
      { title: "Refactor login form validation", project: "web-app", provider: "claude" as const },
      { title: "Fix duplicate charge in payment callback", project: "billing", provider: "kimi" as const },
      { title: "Add tests for the export API", project: "api", provider: "codex" as const },
    ],
    terminal: [
      { k: "prompt", t: "PS C:\\dev\\web-app> claude" },
      { k: "user", t: "> Refactor the login form validation into a schema" },
      { k: "ai", t: "● Let me look at the current form component and its validation." },
      { k: "tool", t: "● Read(src/components/LoginForm.tsx)" },
      { k: "dim", t: "  ⎿  Read 182 lines" },
      { k: "tool", t: "● Update(src/components/LoginForm.tsx)" },
      { k: "dim", t: "  ⎿  Updated with 24 additions and 9 removals" },
      { k: "tool", t: "● Bash(npm test -- login)" },
      { k: "ok", t: "  ⎿  ✓ 12 passed" },
      { k: "spin", t: "✻ Moving the rules into a schema… (38s · ↓ 2.1k tokens)" },
    ],
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

const sleep = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));
const ease = (x: number) => (x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2);

/** 系统托盘的时间：照真实时钟走，和岛上的时间一致。 */
function useClock(lang: DemoLang) {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 15_000);
    return () => window.clearInterval(id);
  }, []);
  const locale = lang === "zh" ? "zh-CN" : "en-US";
  return {
    time: now.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hour12: false }),
    date: now.toLocaleDateString(locale, { year: "numeric", month: "numeric", day: "numeric" }),
  };
}

export function IslandStage({ lang = "zh" }: { lang?: DemoLang }) {
  const copy = COPY[lang];
  const sessions = useMemo(() => buildSessions(lang), [lang]);
  const [phase, setPhase] = useState<Phase>("compact");
  const cursorRef = useRef<HTMLDivElement>(null);
  const pos = useRef({ x: 560, y: 430 });
  const clock = useClock(lang);

  // 剧本：一只假鼠标循环操作。卸载即停。
  useEffect(() => {
    let alive = true;
    const place = () => {
      const el = cursorRef.current;
      if (el) el.style.transform = `translate(${pos.current.x}px, ${pos.current.y}px)`;
    };
    const move = async (x: number, y: number, ms: number) => {
      const from = { ...pos.current };
      const start = performance.now();
      while (alive) {
        const k = Math.min(1, (performance.now() - start) / ms);
        pos.current = { x: from.x + (x - from.x) * ease(k), y: from.y + (y - from.y) * ease(k) };
        place();
        if (k >= 1) return;
        // 用定时器而不是 requestAnimationFrame：iframe 不在视口里时 rAF 会被浏览器停掉，剧本就卡住了。
        await sleep(16);
      }
    };
    const click = () => {
      const d = document.createElement("div");
      d.className = "demo-ripple";
      d.style.left = `${pos.current.x}px`;
      d.style.top = `${pos.current.y}px`;
      document.body.appendChild(d);
      window.setTimeout(() => d.remove(), 600);
    };
    (async () => {
      place();
      while (alive) {
        setPhase("compact");
        await sleep(2200);
        if (!alive) return;
        setPhase("alert"); // kimi 要你批准
        await sleep(3400);
        if (!alive) return;
        setPhase("compact");
        await sleep(900);
        await move(470, 24, 1100); // 移到岛上
        await sleep(250);
        click();
        setPhase("expanded");
        await sleep(900);
        await move(600, 112, 700); // 在面板上看一眼
        await sleep(2600);
        await move(640, 360, 900); // 点岛外面的桌面收起
        await sleep(200);
        click();
        setPhase("compact");
        await sleep(1200);
        await move(560, 430, 900);
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const pending = sessions[1];
  const alert: IslandAlertEvent | null = phase === "alert"
    ? { sessionId: pending.session.id, kind: "approval", title: pending.task_title ?? "", project: pending.project_name ?? "", detail: null }
    : null;
  const noop = () => {};

  return (
    <div className="isw-desktop">
      <div className="isw-wallpaper" />

      <div className="isw-term">
        <div className="isw-term-bar">
          <span className="isw-term-tab">
            <span className="isw-term-glyph">&gt;_</span>
            {copy.tab}
          </span>
          <span className="isw-term-ctl"><i>─</i><i>☐</i><i>✕</i></span>
        </div>
        <div className="isw-term-body">
          {copy.terminal.map((line, i) => <div key={i} className={`is-${line.k}`}>{line.t}</div>)}
        </div>
      </div>

      <div className={`isw-island is-${phase}`}>
        {phase === "expanded" ? (
          <div className="island-shell is-entering" style={{ width: PANEL_WIDTH, "--island-pill-w": "380px", "--island-pill-h": `${ISLAND_PILL_HEIGHT}px` } as React.CSSProperties}>
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
          // 提醒按外层高度撑满（.isl-notch.is-alert 是 height:100%），外层必须给死尺寸。
          <div style={phase === "alert" ? { width: ISLAND_ALERT_SIZE.width, height: ISLAND_ALERT_SIZE.height } : { height: ISLAND_PILL_HEIGHT }}>
            <Island
              sessions={sessions}
              compactStyle="counts"
              alert={alert}
              usages={[{ provider: "claude", pct: 29 }]}
              onMeasure={noop}
              onActivate={noop}
              onOpenSession={noop}
              onPointerDownDrag={noop}
            />
          </div>
        )}
      </div>

      <div className="isw-taskbar">
        <div className="isw-apps">
          <span className="isw-app is-search" />
          <span className="isw-app is-folder" />
          <span className="isw-app is-browser" />
          <span className="isw-app is-term is-active"><b>&gt;_</b></span>
          <span className="isw-app is-meowo is-active"><img src={logoUrl} alt="" /></span>
        </div>
        <div className="isw-tray">
          <span className="isw-tray-icons"><i className="is-net" /><i className="is-vol" /></span>
          <span className="isw-tray-clock"><b>{clock.time}</b><span>{clock.date}</span></span>
        </div>
      </div>

      <div className="isw-cursor" ref={cursorRef}>
        <svg width="20" height="22" viewBox="0 0 20 22">
          <path d="M2 1 L2 17 L6.5 13.5 L9.5 20 L12.5 18.7 L9.6 12.3 L15.5 11.8 Z" fill="#fff" stroke="#1b1917" strokeWidth="1.4" />
        </svg>
      </div>
    </div>
  );
}
