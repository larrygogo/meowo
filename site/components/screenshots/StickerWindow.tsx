// 高保真贴纸示意组件：用于官网展示真实界面样式（非运行态，纯展示）
// 颜色、材质、阴影尽量逐项对齐 app/src/styles.css 的实际样式。
// 支持三个外观维度，与产品一致：
//   theme  深/浅主题
//   bgRgb  贴纸底色（7 种配色）
//   flat   风格：立体（默认，凹槽/凸起/雕刻质感）↔ 扁平（抹平所有立体，纯色面 + 1px 描边）
// 关键还原：状态徽标块 .stk-ind 是一块「黑色小屏幕」，深浅主题下都保持黑底。

import React from "react";
import { AgentLogo, type ProviderId } from "../SupportedAgents";

type CardState = "running" | "waiting" | "error" | "idle" | "stopped";

type CardData = {
  title: string;
  repo: string;
  provider: ProviderId;
  state: CardState;
  pct?: number;
  aiText?: string;
  userText?: string;
  note?: string;
  time?: string;
  model?: string;
  starred?: boolean;
};

type Props = {
  activeTab?: "all" | "waiting" | "running" | "archived";
  cards: CardData[];
  showMenu?: boolean;
  showNote?: boolean;
  theme?: "dark" | "light";
  bgRgb?: string;
  flat?: boolean;
  className?: string;
  style?: React.CSSProperties;
};

const TAB_LABELS = { all: "全部", waiting: "待交互", running: "运行中", archived: "已归档" };
const TAB_COUNTS = { all: 4, waiting: 1, running: 2, archived: 0 };

type Tokens = ReturnType<typeof tokens>;

function tokens(theme: "dark" | "light", flat: boolean) {
  const dark = theme !== "light";
  const cardElev = dark
    ? "0 1px 2px rgba(0,0,0,0.3), 0 6px 16px -3px rgba(0,0,0,0.5), inset 0 1px 0 rgba(255,255,255,0.11), inset 0 -1px 0 rgba(0,0,0,0.22)"
    : "0 1px 2px rgba(0,0,0,0.09), 0 6px 16px -3px rgba(0,0,0,0.16), inset 0 1px 0 rgba(255,255,255,0.9), inset 0 -1px 0 rgba(0,0,0,0.05)";
  return {
    dark,
    flat,
    text: dark ? "#eef1ef" : "#23282a",
    dim: dark ? "#a7afab" : "#565f5a",
    faint: dark ? "#7a817c" : "#6f7873",
    accentText: dark ? "#3fdcac" : "#0b7c5c",
    surface: dark ? "rgba(255,255,255,0.05)" : "rgba(0,0,0,0.035)",
    surfaceHover: dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.065)",
    border: dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.1)",

    // 卡片/底栏立体投影
    cardElev: flat ? "none" : cardElev,

    // 状态徽标块「黑色小屏幕」——两主题都黑
    indBg: flat ? (dark ? "rgba(0,0,0,0.22)" : "rgba(0,0,0,0.08)") : "linear-gradient(180deg, #080808 0%, #2a2a2a 100%)",
    indBorder: flat ? (dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.1)") : dark ? "rgba(0,0,0,0.55)" : "rgba(0,0,0,0.3)",
    indShadow: flat
      ? "none"
      : dark
        ? "0 -2px 4px rgba(0,0,0,0.55), 0 2px 2px -0.5px rgba(255,255,255,0.14), inset 0 3px 5px rgba(0,0,0,0.95)"
        : "0 -2px 4px rgba(0,0,0,0.3), 0 2.5px 2px -0.5px rgba(255,255,255,1), inset 0 3px 5px rgba(0,0,0,0.6)",

    // tab 分段槽（凹）+ 滑块（凸）
    segBg: flat ? (dark ? "rgba(255,255,255,0.05)" : "rgba(0,0,0,0.035)") : dark ? "rgba(0,0,0,0.24)" : "rgba(0,0,0,0.06)",
    segShadow: flat ? "none" : dark ? "inset 0 1px 2px rgba(0,0,0,0.3)" : "inset 0 1px 2px rgba(0,0,0,0.08)",
    sliderBg: flat ? (dark ? "rgba(255,255,255,0.09)" : "#f2f2f0") : dark ? "rgba(255,255,255,0.12)" : "#fff",
    sliderShadow: flat
      ? "none"
      : dark
        ? "0 1px 2px rgba(0,0,0,0.35), 0 2px 6px -2px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.12)"
        : "0 1px 2px rgba(0,0,0,0.12), 0 2px 6px -2px rgba(0,0,0,0.16)",
    engrave: flat ? "none" : dark ? "0 1px 1px rgba(0,0,0,0.5)" : "0 1px 0 rgba(255,255,255,0.9)",

    // 用量凹槽读数屏 + 轨道
    uscreenBg: flat ? (dark ? "rgba(255,255,255,0.05)" : "rgba(0,0,0,0.035)") : dark ? "rgba(0,0,0,0.3)" : "rgba(0,0,0,0.09)",
    uscreenShadow: flat
      ? "none"
      : dark
        ? "inset 0 2px 5px rgba(0,0,0,0.55), inset 0 -1px 0 rgba(255,255,255,0.06)"
        : "inset 0 2px 4px rgba(0,0,0,0.17), inset 0 -1px 0 rgba(255,255,255,0.7)",
    trackBg: flat ? (dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.1)") : dark ? "rgba(0,0,0,0.3)" : "rgba(0,0,0,0.1)",
    trackShadow: flat ? "none" : dark ? "inset 0 1px 2px rgba(0,0,0,0.4)" : "inset 0 1px 2px rgba(0,0,0,0.12)",
    fillShadow: flat ? "none" : "inset 0 1px 0 rgba(255,255,255,0.35)",
    ulabel: dark ? "rgba(255,255,255,0.4)" : "rgba(0,0,0,0.46)",

    // 底栏图标按钮（凸）
    actBg: flat ? (dark ? "rgba(255,255,255,0.05)" : "rgba(0,0,0,0.035)") : dark ? "rgba(255,255,255,0.12)" : "#fff",
    actShadow: flat
      ? "none"
      : dark
        ? "0 1px 2px rgba(0,0,0,0.35), 0 2px 6px -2px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.12)"
        : "0 1px 2px rgba(0,0,0,0.12), 0 2px 6px -2px rgba(0,0,0,0.16)",
    utabOn: dark ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.08)",

    // 徽章 / 文本装饰
    tagBg: dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.06)",
    modelBg: dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.06)",
    aiBg: dark ? "rgba(45,212,167,0.16)" : "rgba(18,166,127,0.15)",

    // 状态色（鲜亮，随主题）；soft/clear 由同一份 rgb 派生，避免字符串耦合。
    ok: dark ? "#4ec9a5" : "#17ab77",
    okSoft: `rgba(${dark ? "78,201,165" : "23,171,119"}, 0.22)`,
    okClear: `rgba(${dark ? "78,201,165" : "23,171,119"}, 0)`,
    warn: dark ? "#e0a23c" : "#d99a1a",
    warnSoft: `rgba(${dark ? "224,162,60" : "217,154,26"}, 0.22)`,
    err: dark ? "#e0584c" : "#db453a",

    windowShadow: dark ? "0 20px 50px rgba(0,0,0,0.45)" : "0 18px 44px rgba(20,24,22,0.16)",
  };
}

function ProviderIcon({ provider }: { provider: CardData["provider"] }) {
  return <AgentLogo id={provider} size={14} tile={false} />;
}

function StatusIndicator({ state, pct, styles, t }: { state: CardState; pct?: number; styles: Styles; t: Tokens }) {
  if (state === "running") {
    return (
      <div style={styles.ind}>
        <div style={{ ...styles.ring, borderColor: t.okSoft, background: t.okSoft }}>
          <div style={{ ...styles.sweep, background: `conic-gradient(from 0deg, ${t.okClear} 0deg, ${t.ok} 110deg, ${t.okClear} 110deg 360deg)` }} />
          <div style={styles.mask} />
          <div style={{ ...styles.core, background: t.ok, color: "#06281f" }}>{pct}%</div>
        </div>
      </div>
    );
  }
  if (state === "waiting") {
    return (
      <div style={styles.ind}>
        <div style={{ ...styles.ring, borderColor: t.warnSoft, background: t.warnSoft }}>
          <div style={{ ...styles.sweep, background: t.warn, animation: "none" }} />
          <div style={styles.mask} />
          <div style={{ ...styles.core, background: t.warn, color: "#2a1d02" }}>{pct}%</div>
        </div>
      </div>
    );
  }
  if (state === "error") {
    return (
      <div style={styles.ind}>
        <div style={{ width: 9, height: 9, borderRadius: "50%", background: t.err, boxShadow: `0 0 0 4px ${t.err}40` }} />
      </div>
    );
  }
  if (state === "stopped") {
    return (
      <div style={styles.ind}>
        <div style={{ width: 16, height: 16, borderRadius: "50%", border: "1.5px dashed #7a817c" }} />
      </div>
    );
  }
  return (
    <div style={styles.ind}>
      <div style={{ width: 9, height: 9, borderRadius: "50%", background: t.ok }} />
    </div>
  );
}

export default function StickerWindow({
  activeTab = "all",
  cards,
  showMenu,
  showNote,
  theme = "dark",
  bgRgb,
  flat = true,
  className = "",
  style,
}: Props) {
  const t = tokens(theme, flat);
  const styles = makeStyles(t, bgRgb);
  return (
    <div className={`stk-win ${className}`} style={{ ...styles.window, ...style }}>
      <div style={styles.drag} />
      <div style={styles.tabs}>
        <div style={styles.tabseg}>
          <div
            style={{
              ...styles.tabSlider,
              transform: `translateX(${["all", "waiting", "running", "archived"].indexOf(activeTab) * 100}%)`,
            }}
          />
          {(["all", "waiting", "running", "archived"] as const).map((k) => (
            <span key={k} style={{ ...styles.stab, color: activeTab === k ? t.accentText : t.dim }}>
              {TAB_LABELS[k]}
              {k !== "all" && k !== "archived" && (
                <span style={{ ...styles.stabN, color: activeTab === k ? t.accentText : t.faint }}>
                  {TAB_COUNTS[k]}
                </span>
              )}
            </span>
          ))}
        </div>
      </div>
      <div style={styles.scroll}>
        {cards.map((c, i) => (
          <div key={i} style={{ ...styles.card, position: "relative" }}>
            {c.starred && <div style={styles.starCorner} />}
            <div style={styles.top}>
              <StatusIndicator state={c.state} pct={c.pct} styles={styles} t={t} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={styles.line1}>
                  <span style={styles.title}>{c.title}</span>
                  <span style={styles.time}>{c.time ?? "刚刚"}</span>
                  <span style={styles.menuBtn}>
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <circle cx="12" cy="12" r="1" />
                      <circle cx="5" cy="12" r="1" />
                      <circle cx="19" cy="12" r="1" />
                    </svg>
                  </span>
                </div>
                <div style={styles.line2}>
                  <ProviderIcon provider={c.provider} />
                  <span style={styles.repo}>{c.repo}</span>
                  {c.model && <span style={styles.model}>{c.model}</span>}
                </div>
              </div>
            </div>
            {showNote && c.note && (
              <div style={styles.note}>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#e0a23c" strokeWidth="2">
                  <path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" />
                  <path d="M15 21v-5a1 1 0 0 1 1-1h5" />
                </svg>
                <span>{c.note}</span>
              </div>
            )}
            {c.aiText && (
              <div style={styles.subrow}>
                <span style={{ ...styles.tag, color: t.accentText, background: t.aiBg }}>AI</span>
                <span style={styles.sub}>{c.aiText}</span>
              </div>
            )}
            {c.userText && (
              <div style={styles.subrow}>
                <span style={styles.tag}>你</span>
                <span style={styles.sub}>{c.userText}</span>
              </div>
            )}
          </div>
        ))}
      </div>
      {showMenu && (
        <div style={styles.ctxMenu}>
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.79 21.55a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.554 10.34a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" /></svg>
            星标置顶
          </button>
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M16 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11l5-5V5a2 2 0 0 0-2-2z" /><path d="M15 21v-5a1 1 0 0 1 1-1h5" /></svg>
            添加便签
          </button>
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 20h9" /><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" /></svg>
            重命名
          </button>
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect width="20" height="5" x="2" y="3" rx="1" /><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8" /><path d="M10 12h4" /></svg>
            归档
          </button>
          <div style={styles.ctxSep} />
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 5v14M5 12h14" /></svg>
            新建会话
          </button>
          <div style={styles.ctxSep} />
          <button style={styles.ctxItem}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" /></svg>
            打开项目目录
          </button>
        </div>
      )}
      <div style={styles.bar}>
        <div style={styles.uscreen}>
          <div style={styles.utabs}>
            <span style={{ ...styles.utab, opacity: 1, background: t.utabOn }}>
              <ProviderIcon provider="claude" />
            </span>
            <span style={styles.utab}>
              <ProviderIcon provider="codex" />
            </span>
          </div>
          <div style={styles.urow}>
            <span style={styles.ulabel}>5 小时配额</span>
            <span style={styles.utrack}><i style={{ ...styles.ufill, width: "62%", background: t.warn }} /></span>
            <span style={styles.uval}>62%</span>
          </div>
          <div style={styles.urow}>
            <span style={styles.ulabel}>7 天配额</span>
            <span style={styles.utrack}><i style={{ ...styles.ufill, width: "38%", background: t.ok }} /></span>
            <span style={styles.uval}>38%</span>
          </div>
        </div>
        <div style={styles.barActions}>
          <span style={styles.stkAct}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 5v14M5 12h14" /></svg>
          </span>
          <span style={styles.stkAct}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8" /><line x1="21" y1="21" x2="16.65" y2="16.65" /></svg>
          </span>
          <span style={styles.stkAct}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </span>
          <span style={styles.stkAct}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 17v5" /><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" /></svg>
          </span>
        </div>
      </div>
    </div>
  );
}

type Styles = Record<string, React.CSSProperties>;

function makeStyles(t: Tokens, bgRgb?: string): Styles {
  const rgb = bgRgb ?? (t.dark ? "33, 33, 35" : "247, 247, 249");
  return {
    window: {
      width: 360,
      maxWidth: "100%",
      background: `rgba(${rgb}, ${t.dark ? 0.96 : 1})`,
      border: `1px solid ${t.border}`,
      borderRadius: 16,
      color: t.text,
      padding: "4px 8px 0",
      display: "flex",
      flexDirection: "column",
      overflow: "hidden",
      fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
      boxShadow: t.windowShadow,
      position: "relative",
      zIndex: 0,
      isolation: "isolate",
    },
    drag: { height: 14, display: "flex", alignItems: "center", justifyContent: "center" },
    tabs: { display: "flex", gap: 6, padding: "2px 0 6px" },
    tabseg: { position: "relative", display: "flex", flex: 1, padding: 3, borderRadius: 10, background: t.segBg, boxShadow: t.segShadow },
    tabSlider: {
      position: "absolute",
      top: 3,
      bottom: 3,
      left: 3,
      width: "calc((100% - 6px) / 4)",
      borderRadius: 7,
      background: t.sliderBg,
      boxShadow: t.sliderShadow,
      transition: "transform 0.24s cubic-bezier(0.34,1.2,0.5,1)",
      zIndex: 0,
    },
    stab: { position: "relative", zIndex: 1, flex: 1, fontSize: 11, padding: "3px 2px", display: "flex", alignItems: "center", justifyContent: "center", gap: 3, whiteSpace: "nowrap", textShadow: t.engrave },
    stabN: { fontSize: 9.5 },
    scroll: { flex: 1, overflow: "hidden", padding: "4px 0 18px", display: "flex", flexDirection: "column", gap: 6 },
    card: { padding: "10px 11px", background: t.surface, border: `1px solid ${t.border}`, borderRadius: 16, boxShadow: t.cardElev },
    starCorner: { position: "absolute", top: 3, right: 3, width: 13, height: 13, background: "#e0a23c", borderTopRightRadius: 13, borderBottomLeftRadius: 13, pointerEvents: "none" },
    top: { display: "flex", alignItems: "center", gap: 9 },
    ind: {
      position: "relative",
      flex: "none",
      width: 36,
      height: 36,
      borderRadius: 14,
      background: t.indBg,
      border: `1px solid ${t.indBorder}`,
      boxShadow: t.indShadow,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
    },
    ring: { position: "absolute", inset: 0, borderRadius: 13, overflow: "hidden" },
    sweep: { position: "absolute", inset: "-50%", willChange: "transform", animation: "run-spin 1.5s linear infinite" },
    mask: { position: "absolute", inset: 2, borderRadius: 11, background: "#000" },
    core: { position: "absolute", top: "50%", left: "50%", width: 24, height: 24, transform: "translate(-50%, -50%)", borderRadius: "50%", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 9, fontWeight: 700 },
    line1: { display: "flex", alignItems: "center", gap: 7, minHeight: 22 },
    title: { flex: 1, minWidth: 0, fontSize: 12.5, fontWeight: 600, color: t.text, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    time: { flex: "none", fontSize: 10, color: t.faint },
    menuBtn: { flex: "none", width: 20, height: 20, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 6, color: t.faint },
    line2: { display: "flex", alignItems: "center", gap: 8, marginTop: 3 },
    repo: { fontSize: 10.5, color: t.dim, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    model: { flex: "none", marginLeft: "auto", fontSize: 10, lineHeight: 1.5, padding: "0 6px", borderRadius: 4, background: t.modelBg, color: t.dim },
    subrow: { display: "flex", alignItems: "center", gap: 6, marginTop: 7 },
    tag: { flex: "none", minWidth: 20, textAlign: "center", padding: "1px 5px", borderRadius: 5, fontSize: 9, fontWeight: 700, color: t.faint, background: t.tagBg },
    sub: { flex: 1, minWidth: 0, fontSize: 10.5, color: t.faint, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    note: { display: "flex", alignItems: "flex-start", gap: 6, marginTop: 7, padding: "5px 8px", fontSize: 10.5, lineHeight: 1.5, color: t.text, background: "rgba(224,162,60,0.13)", border: "1px solid rgba(224,162,60,0.3)", borderRadius: 9 },
    bar: { flex: "none", display: "flex", alignItems: "center", gap: 10, margin: "2px 0 6px", padding: "7px 9px 7px 6px", borderRadius: 14, background: t.surface, border: `1px solid ${t.border}`, boxShadow: t.cardElev },
    uscreen: { flex: "0 1 auto", minWidth: 0, display: "flex", flexDirection: "column", justifyContent: "center", gap: 4, padding: "6px 10px", borderRadius: 10, background: t.uscreenBg, boxShadow: t.uscreenShadow },
    utabs: { display: "flex", gap: 3, paddingBottom: 3 },
    utab: { display: "flex", alignItems: "center", justifyContent: "center", padding: "2px 4px", borderRadius: 4, opacity: 0.42, background: "transparent", border: "none" },
    urow: { display: "flex", alignItems: "center", gap: 7, fontSize: 9.5, lineHeight: 1 },
    ulabel: { flex: "none", minWidth: 54, fontWeight: 600, color: t.ulabel, textShadow: t.engrave },
    utrack: { flex: 1, minWidth: 36, height: 5, borderRadius: 3, overflow: "hidden", background: t.trackBg, boxShadow: t.trackShadow },
    ufill: { display: "block", height: "100%", borderRadius: 3, boxShadow: t.fillShadow },
    uval: { flex: "none", minWidth: 28, textAlign: "right", fontWeight: 700, fontFamily: "ui-monospace, Consolas, monospace", color: t.ulabel, textShadow: t.engrave },
    barActions: { flex: "none", marginLeft: "auto", display: "flex", alignItems: "center", gap: 5 },
    stkAct: { flex: "none", display: "inline-flex", alignItems: "center", justifyContent: "center", padding: 6, borderRadius: 10, color: t.faint, background: t.actBg, boxShadow: t.actShadow },
    ctxMenu: { position: "absolute", right: 12, top: 78, minWidth: 132, display: "flex", flexDirection: "column", gap: 1, padding: 4, background: t.dark ? "#2e2e2c" : "#ffffff", border: `1px solid ${t.dark ? "rgba(255,255,255,0.09)" : "rgba(0,0,0,0.1)"}`, borderRadius: 8, boxShadow: t.dark ? "0 8px 20px rgba(0,0,0,0.3)" : "0 8px 24px rgba(20,24,22,0.16)", zIndex: 10 },
    ctxItem: { display: "flex", alignItems: "center", gap: 8, padding: "6px 10px", border: "none", background: "transparent", borderRadius: 5, color: t.text, fontSize: 11.5, cursor: "pointer", textAlign: "left" },
    ctxSep: { height: 1, margin: "3px 6px", background: t.border },
  };
}
