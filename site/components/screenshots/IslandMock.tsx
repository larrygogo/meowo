import type { Lang } from "@/lib/i18n";

// 灵动岛示意（0.8.0 起两个平台的贴纸都换成了灵动岛）：屏幕顶部一块纯黑的岛，上面一行是收起时的
// 胶囊内容（额度环 · 眼睛 · 时间 · 会话头像），下面是点开后的面板。纯展示，不接数据。
// 配色照应用里的岛写死（--isl-*），不跟官网主题：岛要和屏幕顶边的黑融成一体。

type Row = { agent: string; color: string; title: string; meta: string; tone: "run" | "wait" | "pend" };

const COPY: Record<Lang, { panel: string; sessions: string; newSession: string; state: Record<Row["tone"], string>; rows: Row[]; aria: string }> = {
  zh: {
    panel: "Meowo",
    sessions: "会话",
    newSession: "新建会话",
    state: { run: "运行中", wait: "等待输入", pend: "待审批" },
    rows: [
      { agent: "C", color: "#d97757", title: "修支付回调的重复扣款", meta: "billing · 刚刚", tone: "pend" },
      { agent: "K", color: "#4f8cff", title: "重构登录页表单校验", meta: "web-app · 2 分钟前", tone: "run" },
      { agent: "X", color: "#a970ff", title: "补齐导出接口的单测", meta: "api · 5 分钟前", tone: "wait" },
    ],
    aria: "灵动岛示意：屏幕顶部的胶囊与点开后的会话面板",
  },
  en: {
    panel: "Meowo",
    sessions: "Sessions",
    newSession: "New session",
    state: { run: "Running", wait: "Needs input", pend: "Approval" },
    rows: [
      { agent: "C", color: "#d97757", title: "Fix duplicate charge in payment callback", meta: "billing · just now", tone: "pend" },
      { agent: "K", color: "#4f8cff", title: "Refactor login form validation", meta: "web-app · 2 min ago", tone: "run" },
      { agent: "X", color: "#a970ff", title: "Add tests for the export API", meta: "api · 5 min ago", tone: "wait" },
    ],
    aria: "Island mockup: the pill at the top of the screen and the session panel it opens",
  },
};

function Avatar({ row, size = 26 }: { row: Row; size?: number }) {
  const r = (size - 3) / 2;
  const c = 2 * Math.PI * r;
  return (
    <span className={`im-avatar is-${row.tone}`} style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="#2c2c2e" strokeWidth="2.5" />
        {row.tone !== "wait" && (
          <circle
            className="im-arc"
            cx={size / 2}
            cy={size / 2}
            r={r}
            fill="none"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeDasharray={row.tone === "run" ? `${c * 0.3} ${c}` : `${c} ${c}`}
          />
        )}
      </svg>
      <i style={{ color: row.color }}>{row.agent}</i>
      {row.tone === "wait" && <b className="im-dot" />}
    </span>
  );
}

export default function IslandMock({ lang }: { lang: Lang }) {
  const t = COPY[lang];
  return (
    <div className="im-screen" role="img" aria-label={t.aria}>
      <div className="im-island">
        <div className="im-pill">
          <span className="im-ring">
            <svg width="26" height="26" viewBox="0 0 26 26" aria-hidden="true">
              <circle cx="13" cy="13" r="11" fill="none" stroke="#3a3a3c" strokeWidth="2.5" />
              <circle cx="13" cy="13" r="11" fill="none" stroke="#30d158" strokeWidth="2.5" strokeLinecap="round" strokeDasharray="20 69" transform="rotate(-90 13 13)" />
            </svg>
            <em>29</em>
          </span>
          <span className="im-eyes" aria-hidden="true"><i /><i /></span>
          <span className="im-clock">14:08</span>
          <span className="im-agents">
            {t.rows.map((row) => <Avatar key={row.title} row={row} />)}
          </span>
        </div>
        <div className="im-panel">
          <div className="im-head">
            <b>{t.panel} <span>{t.sessions}</span></b>
            <span className="im-chip">+ {t.newSession}</span>
          </div>
          {t.rows.map((row) => (
            <div className="im-row" key={row.title}>
              <Avatar row={row} size={30} />
              <span className="im-text">
                <span className="im-title">{row.title}</span>
                <span className="im-meta">{row.meta}</span>
              </span>
              <span className={`im-state is-${row.tone}`}>{t.state[row.tone]}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
