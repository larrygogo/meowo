// 高保真贴纸示意组件：用于官网展示真实界面样式（非运行态，纯展示）
// 颜色、布局、字号尽量贴近 app/src/styles.css 中的实际样式

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
  className?: string;
  style?: React.CSSProperties;
};

const TAB_LABELS = {
  all: "全部",
  waiting: "待交互",
  running: "运行中",
  archived: "已归档",
};

const TAB_COUNTS = {
  all: 4,
  waiting: 1,
  running: 2,
  archived: 0,
};

function ProviderIcon({ provider }: { provider: CardData["provider"] }) {
  // 与真实 Sticker 一致：Claude 在卡片/配额栏显示裸 logomark，不带设置页使用的橙色方块底座。
  return <AgentLogo id={provider} size={14} tile={false} />;
}

function StatusIndicator({ state, pct }: { state: CardState; pct?: number }) {
  if (state === "running") {
    return (
      <div style={styles.ind}>
        <div style={{ ...styles.ring, borderColor: "rgba(78,201,165,0.22)", background: "rgba(78,201,165,0.22)" }}>
          <div style={{ ...styles.sweep, background: "conic-gradient(from 0deg, rgba(78,201,165,0) 0deg, #4ec9a5 110deg, rgba(78,201,165,0) 110deg 360deg)" }} />
          <div style={styles.mask} />
          <div style={{ ...styles.core, background: "#4ec9a5", color: "#06281f" }}>{pct}%</div>
        </div>
      </div>
    );
  }
  if (state === "waiting") {
    return (
      <div style={styles.ind}>
        <div style={{ ...styles.ring, borderColor: "rgba(224,162,60,0.22)", background: "rgba(224,162,60,0.22)" }}>
          <div style={{ ...styles.sweep, background: "#e0a23c", animation: "none" }} />
          <div style={styles.mask} />
          <div style={{ ...styles.core, background: "#e0a23c", color: "#2a1d02" }}>{pct}%</div>
        </div>
      </div>
    );
  }
  if (state === "error") {
    return (
      <div style={styles.ind}>
        <div style={{ width: 9, height: 9, borderRadius: "50%", background: "#e0584c", boxShadow: "0 0 0 4px rgba(224,88,76,0.25)" }} />
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
      <div style={{ width: 9, height: 9, borderRadius: "50%", background: "#4ec9a5" }} />
    </div>
  );
}

export default function StickerWindow({
  activeTab = "all",
  cards,
  showMenu,
  showNote,
  className = "",
  style,
}: Props) {
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
            <span key={k} style={{ ...styles.stab, color: activeTab === k ? "#3fdcac" : "#a7afab" }}>
              {TAB_LABELS[k]}
              {k !== "all" && k !== "archived" && (
                <span style={{ ...styles.stabN, color: activeTab === k ? "#3fdcac" : "#7a817c" }}>
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
            {c.starred && (
              <div style={styles.starCorner} />
            )}
            <div style={styles.top}>
              <StatusIndicator state={c.state} pct={c.pct} />
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
                <span style={{ ...styles.tag, color: "#3fdcac", background: "rgba(45,212,167,0.16)" }}>AI</span>
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
            <span style={{ ...styles.utab, opacity: 1, background: "rgba(255,255,255,0.08)" }}>
              <ProviderIcon provider="claude" />
            </span>
            <span style={styles.utab}>
              <ProviderIcon provider="codex" />
            </span>
          </div>
          <div style={styles.urow}>
            <span style={styles.ulabel}>5 小时配额</span>
            <span style={styles.utrack}><i style={{ ...styles.ufill, width: "62%", background: "#e0a23c" }} /></span>
            <span style={styles.uval}>62%</span>
          </div>
          <div style={styles.urow}>
            <span style={styles.ulabel}>7 天配额</span>
            <span style={styles.utrack}><i style={{ ...styles.ufill, width: "38%", background: "#4ec9a5" }} /></span>
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

const styles: Record<string, React.CSSProperties> = {
  window: {
    width: 360,
    background: "rgba(33,33,35,0.95)",
    border: "1px solid rgba(255,255,255,0.09)",
    borderRadius: 16,
    color: "#eef1ef",
    padding: "4px 8px 0",
    display: "flex",
    flexDirection: "column",
    overflow: "hidden",
    fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
    boxShadow: "0 20px 50px rgba(0,0,0,0.45)",
    position: "relative",
    // 演示窗口自成层叠上下文：内部菜单可以盖住卡片，但不能越过官网 sticky Header。
    zIndex: 0,
    isolation: "isolate",
  },
  drag: {
    height: 14,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  },
  tabs: {
    display: "flex",
    gap: 6,
    padding: "2px 0 6px",
  },
  tabseg: {
    position: "relative",
    display: "flex",
    flex: 1,
    padding: 3,
    borderRadius: 10,
    background: "rgba(255,255,255,0.05)",
    boxShadow: "none",
  },
  tabSlider: {
    position: "absolute",
    top: 3,
    bottom: 3,
    left: 3,
    width: "calc((100% - 6px) / 4)",
    borderRadius: 7,
    background: "rgba(255,255,255,0.09)",
    boxShadow: "none",
    transition: "transform 0.24s cubic-bezier(0.34,1.2,0.5,1)",
    zIndex: 0,
  },
  stab: {
    position: "relative",
    zIndex: 1,
    flex: 1,
    fontSize: 11,
    padding: "3px 2px",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    gap: 3,
    whiteSpace: "nowrap",
  },
  stabN: {
    fontSize: 9.5,
  },
  scroll: {
    flex: 1,
    overflow: "hidden",
    padding: "4px 0 18px",
    display: "flex",
    flexDirection: "column",
    gap: 6,
  },
  card: {
    padding: "10px 11px",
    background: "rgba(255,255,255,0.05)",
    border: "1px solid rgba(255,255,255,0.09)",
    borderRadius: 16,
    boxShadow: "none",
  },
  starCorner: {
    position: "absolute",
    top: 3,
    right: 3,
    width: 13,
    height: 13,
    background: "#e0a23c",
    borderTopRightRadius: 13,
    borderBottomLeftRadius: 13,
    pointerEvents: "none",
  },
  top: {
    display: "flex",
    alignItems: "center",
    gap: 9,
  },
  ind: {
    position: "relative",
    flex: "none",
    width: 36,
    height: 36,
    borderRadius: 14,
    background: "rgba(0,0,0,0.22)",
    border: "1px solid rgba(255,255,255,0.09)",
    boxShadow: "none",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  },
  ring: {
    position: "absolute",
    inset: 0,
    borderRadius: 13,
    overflow: "hidden",
  },
  sweep: {
    position: "absolute",
    inset: "-50%",
    willChange: "transform",
    animation: "run-spin 1.5s linear infinite",
  },
  mask: {
    position: "absolute",
    inset: 2,
    borderRadius: 11,
    background: "#000",
  },
  core: {
    position: "absolute",
    top: "50%",
    left: "50%",
    width: 24,
    height: 24,
    transform: "translate(-50%, -50%)",
    borderRadius: "50%",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontSize: 9,
    fontWeight: 700,
  },
  line1: {
    display: "flex",
    alignItems: "center",
    gap: 7,
    minHeight: 22,
  },
  title: {
    flex: 1,
    minWidth: 0,
    fontSize: 12.5,
    fontWeight: 600,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  time: {
    flex: "none",
    fontSize: 10,
    color: "#7a817c",
  },
  menuBtn: {
    flex: "none",
    width: 20,
    height: 20,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 6,
    color: "#7a817c",
  },
  line2: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    marginTop: 3,
  },
  repo: {
    fontSize: 10.5,
    color: "#a7afab",
    minWidth: 0,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  model: {
    flex: "none",
    marginLeft: "auto",
    fontSize: 10,
    lineHeight: 1.5,
    padding: "0 6px",
    borderRadius: 4,
    background: "rgba(255,255,255,0.09)",
    color: "#a7afab",
  },
  subrow: {
    display: "flex",
    alignItems: "center",
    gap: 6,
    marginTop: 7,
  },
  tag: {
    flex: "none",
    minWidth: 20,
    textAlign: "center",
    padding: "1px 5px",
    borderRadius: 5,
    fontSize: 9,
    fontWeight: 700,
    color: "#7a817c",
    background: "rgba(255,255,255,0.09)",
  },
  sub: {
    flex: 1,
    minWidth: 0,
    fontSize: 10.5,
    color: "#7a817c",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  note: {
    display: "flex",
    alignItems: "flex-start",
    gap: 6,
    marginTop: 7,
    padding: "5px 8px",
    fontSize: 10.5,
    lineHeight: 1.5,
    color: "#eef1ef",
    background: "rgba(224,162,60,0.13)",
    border: "1px solid rgba(224,162,60,0.3)",
    borderRadius: 9,
  },
  bar: {
    flex: "none",
    display: "flex",
    alignItems: "center",
    gap: 11,
    margin: "2px 0 6px",
    padding: "7px 10px 7px 6px",
    borderRadius: 14,
    background: "rgba(255,255,255,0.05)",
    border: "1px solid rgba(255,255,255,0.09)",
    boxShadow: "none",
  },
  uscreen: {
    flex: "none",
    display: "flex",
    flexDirection: "column",
    justifyContent: "center",
    gap: 4,
    padding: "6px 10px",
    borderRadius: 10,
    background: "rgba(255,255,255,0.05)",
    boxShadow: "none",
  },
  utabs: {
    display: "flex",
    gap: 3,
    paddingBottom: 3,
  },
  utab: {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: "2px 4px",
    borderRadius: 4,
    opacity: 0.42,
    background: "transparent",
    border: "none",
  },
  urow: {
    display: "flex",
    alignItems: "center",
    gap: 7,
    fontSize: 9.5,
    lineHeight: 1,
  },
  ulabel: {
    flex: "none",
    minWidth: 58,
    fontWeight: 600,
    color: "rgba(255,255,255,0.4)",
  },
  utrack: {
    flex: 1,
    minWidth: 46,
    height: 5,
    borderRadius: 3,
    overflow: "hidden",
    background: "rgba(255,255,255,0.09)",
  },
  ufill: {
    display: "block",
    height: "100%",
    borderRadius: 3,
  },
  uval: {
    flex: "none",
    minWidth: 30,
    textAlign: "right",
    fontWeight: 700,
    fontFamily: 'ui-monospace, Consolas, monospace',
    color: "rgba(255,255,255,0.4)",
  },
  barActions: {
    marginLeft: "auto",
    display: "flex",
    alignItems: "center",
    gap: 6,
  },
  stkAct: {
    flex: "none",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 6,
    borderRadius: 10,
    color: "#7a817c",
    background: "rgba(255,255,255,0.12)",
    boxShadow: "none",
  },
  ctxMenu: {
    position: "absolute",
    right: 12,
    top: 78,
    minWidth: 132,
    display: "flex",
    flexDirection: "column",
    gap: 1,
    padding: 4,
    background: "#2e2e2c",
    border: "1px solid rgba(255,255,255,0.09)",
    borderRadius: 6,
    boxShadow: "0 8px 20px rgba(0,0,0,0.3)",
    zIndex: 10,
  },
  ctxItem: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "6px 10px",
    border: "none",
    background: "transparent",
    borderRadius: 4,
    color: "#eef1ef",
    fontSize: 11.5,
    cursor: "pointer",
    textAlign: "left",
  },
  ctxSep: {
    height: 1,
    margin: "3px 6px",
    background: "rgba(255,255,255,0.09)",
  },
};
