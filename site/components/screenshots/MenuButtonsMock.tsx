// 「终端菜单按钮化」示意组件：CLI 在终端弹出的交互菜单（这里是长会话恢复），
// 在对话窗口里被识别并渲染成可直接点击的按钮。纯展示 mock：aria-hidden、无 button、无动画。
// 视觉对齐产品的终端 attention 卡（琥珀色系），风格与 ChatWindowMock 一致。

import React from "react";
import type { Lang } from "@/lib/i18n";

const C = {
  bg: "#212123",
  border: "rgba(255,255,255,0.08)",
  text: "#eef1ef",
  dim: "#9aa39e",
  faint: "#8b918c",
  accent: "#2dd4a7",
  amber: "#e0a23c",
};

const COPY = {
  zh: {
    terminalTitle: "重构吸边状态机 · meowo",
    segChat: "对话",
    segTerm: "终端",
    cardTitle: "如何恢复这个长会话？",
    cardSub: "完整恢复会消耗较多额度，建议从摘要恢复。",
    options: ["从摘要恢复（推荐）", "恢复完整会话", "取消"],
    hint: "点按钮 = 把对应按键发回终端",
  },
  en: {
    terminalTitle: "Refactor edge-snap state machine · meowo",
    segChat: "Chat",
    segTerm: "Terminal",
    cardTitle: "How should this long session resume?",
    cardSub: "A full resume uses substantially more quota; resuming from the summary is recommended.",
    options: ["Resume from summary (recommended)", "Resume full session", "Cancel"],
    hint: "A click sends the matching keys back to the terminal",
  },
} as const;

export default function MenuButtonsMock({ lang = "zh" }: { lang?: Lang }) {
  const t = COPY[lang];
  const s: Record<string, React.CSSProperties> = {
    window: {
      width: "min(460px, 100%)",
      margin: "0 auto",
      background: C.bg,
      border: `1px solid ${C.border}`,
      borderRadius: 18,
      color: C.text,
      overflow: "hidden",
      fontFamily:
        '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
      boxShadow: "0 20px 50px rgba(0,0,0,0.45)",
    },
    topbar: {
      display: "flex",
      alignItems: "center",
      gap: 10,
      padding: "10px 14px",
      borderBottom: `1px solid ${C.border}`,
    },
    sessionTitle: {
      flex: 1,
      minWidth: 0,
      fontSize: 12.5,
      fontWeight: 600,
      overflow: "hidden",
      textOverflow: "ellipsis",
      whiteSpace: "nowrap",
    },
    seg: {
      flex: "none",
      display: "inline-flex",
      padding: 3,
      gap: 2,
      borderRadius: 10,
      background: "rgba(255,255,255,0.05)",
    },
    segItem: { fontSize: 10, lineHeight: 1, padding: "5px 10px", borderRadius: 7, color: C.dim },
    segOn: { background: "#2e2e30", color: C.text, boxShadow: "0 1px 4px rgba(0,0,0,0.08)" },
    body: { padding: 14 },
    card: {
      border: `1px solid color-mix(in srgb, ${C.amber} 52%, ${C.border})`,
      borderRadius: 14,
      background: `color-mix(in srgb, ${C.amber} 10%, ${C.bg})`,
      padding: 14,
    },
    cardTitle: { fontSize: 13, fontWeight: 600, color: C.amber },
    cardSub: { marginTop: 4, fontSize: 11.5, lineHeight: 1.5, color: C.dim },
    options: { display: "flex", flexDirection: "column", gap: 7, marginTop: 12 },
    option: {
      display: "block",
      padding: "8px 12px",
      borderRadius: 9,
      border: `1px solid ${C.border}`,
      background: "rgba(255,255,255,0.04)",
      fontSize: 12,
      color: C.text,
      textAlign: "left",
    },
    optionPrimary: {
      border: `1px solid color-mix(in srgb, ${C.accent} 55%, ${C.border})`,
      background: `color-mix(in srgb, ${C.accent} 14%, ${C.bg})`,
      color: C.accent,
      fontWeight: 600,
    },
    hint: {
      marginTop: 10,
      display: "flex",
      alignItems: "center",
      gap: 6,
      fontSize: 10.5,
      color: C.faint,
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    },
  };

  return (
    <div style={s.window} aria-hidden="true">
      <div style={s.topbar}>
        <span style={s.sessionTitle}>{t.terminalTitle}</span>
        <span style={s.seg}>
          <span style={{ ...s.segItem, ...s.segOn }}>{t.segChat}</span>
          <span style={s.segItem}>{t.segTerm}</span>
        </span>
      </div>
      <div style={s.body}>
        <div style={s.card}>
          <div style={s.cardTitle}>{t.cardTitle}</div>
          <div style={s.cardSub}>{t.cardSub}</div>
          <div style={s.options}>
            {t.options.map((label, i) => (
              <span key={label} style={{ ...s.option, ...(i === 0 ? s.optionPrimary : {}) }}>
                {label}
              </span>
            ))}
          </div>
        </div>
        <div style={s.hint}>
          <span>›</span>
          {t.hint}
        </div>
      </div>
    </div>
  );
}
