// 对话窗口示意组件：官网功能页「对话窗口」板块的纯展示 mock（非运行态）。
// 逐一对齐产品对话窗的真实结构：左侧会话栏、标题栏（实时同步点 + 视图分段 + 关闭）、
// 用户气泡、工具活动卡、待办卡（完成划线/进行高亮）、横向审批卡、圆角 24 输入区
// （附件钮 + 模型/工作模式/权限模式胶囊 + 上下文环 + 圆形浅色发送钮）。
// 内部固定 660px 设计宽，外层按容器宽度整体缩放（同 DemoFrame 的做法），任意栏宽都完整可见。
// aria-hidden、全 span、无动画。
"use client";

import React, { useEffect, useRef } from "react";
import type { Lang } from "@/lib/i18n";

const DW = 720; // 设计宽度：侧栏 208（同产品）+ 主列内容（三枚模式胶囊 + 上下文环）所需宽度

const C = {
  bg: "#232325", // color-mix(98% #212123 + 2% white)
  sidebarBg: "#2a2a2c",
  bgRaise: "#28282a", // compose: color-mix(96% bg + 4% white)
  border: "rgba(255,255,255,0.09)",
  hairline: "rgba(255,255,255,0.07)",
  text: "#eef1ef",
  dim: "#a7afab",
  faint: "#8b918c",
  accent: "#2dd4a7",
  okVivid: "#4ec9a5",
  warn: "#e0a23c",
  err: "#e0584c",
};

const COPY = {
  zh: {
    sidebar: "会话",
    sessions: [
      { title: "重构吸边状态机", repo: "meowo", icon: "✳", iconColor: "#d97757", active: true },
      { title: "接入账号用量面板", repo: "autopilot", icon: "K", iconColor: "#5b8db8", active: false },
      { title: "升级 tauri 到 2.3", repo: "cc-relay", icon: "G", iconColor: "#6fae6a", active: false },
    ],
    title: "重构吸边状态机",
    cwd: "C:/Users/dev/workspace/meowo",
    live: "实时同步",
    segChat: "对话",
    segTerm: "终端",
    userMsg: "把状态机拆成 3 个纯函数，补上吸附边界的单测",
    toolRow: "执行了 12 次工具调用",
    toolKinds: "Read · Edit · Bash · Tests",
    todoTitle: "待办",
    todoCount: "2/4",
    todos: [
      { text: "拆解状态机", state: "done" },
      { text: "更新吸附逻辑", state: "done" },
      { text: "补吸附边界单测", state: "doing" },
      { text: "跑全量测试", state: "pending" },
    ],
    approval: "Agent 请求运行命令",
    approvalTool: "工具",
    allow: "允许",
    deny: "拒绝",
    placeholder: "直接与 Agent 对话（Enter 发送，Shift+Enter 换行）",
    model: "k3",
    modeWork: "工作模式: 计划",
    modePerm: "权限模式: 自动",
    contextPct: "15",
    contextText: "157K/1.0M",
    enterHint: "Enter ⏎",
    settings: "打开设置",
  },
  en: {
    sidebar: "Sessions",
    sessions: [
      { title: "Refactor edge-snap FSM", repo: "meowo", icon: "✳", iconColor: "#d97757", active: true },
      { title: "Wire up the usage panel", repo: "autopilot", icon: "K", iconColor: "#5b8db8", active: false },
      { title: "Bump tauri to 2.3", repo: "cc-relay", icon: "G", iconColor: "#6fae6a", active: false },
    ],
    title: "Refactor edge-snap state machine",
    cwd: "/home/dev/workspace/meowo",
    live: "Live",
    segChat: "Chat",
    segTerm: "Terminal",
    userMsg: "Split the state machine into 3 pure functions and add edge-snap boundary tests",
    toolRow: "12 tool calls",
    toolKinds: "Read · Edit · Bash · Tests",
    todoTitle: "Todos",
    todoCount: "2/4",
    todos: [
      { text: "Break down the state machine", state: "done" },
      { text: "Update edge-snap logic", state: "done" },
      { text: "Add edge-snap tests", state: "doing" },
      { text: "Run the full test suite", state: "pending" },
    ],
    approval: "Command needs approval",
    approvalTool: "Tool",
    allow: "Allow",
    deny: "Deny",
    placeholder: "Message the agent (Enter to send, Shift+Enter for a newline)",
    model: "k3",
    modeWork: "Mode: Plan",
    modePerm: "Permissions: Auto",
    contextPct: "15",
    contextText: "157K/1.0M",
    enterHint: "Enter ⏎",
    settings: "Settings",
  },
} as const;

type TodoState = "done" | "doing" | "pending";
const TODO_MARK: Record<TodoState, string> = { done: "✓", doing: "●", pending: "○" };

export default function ChatWindowMock({ lang = "zh" }: { lang?: Lang }) {
  const t = COPY[lang];
  const wrap = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const w = wrap.current;
    const el = inner.current;
    if (!w || !el) return;
    const apply = () => {
      const scale = w.clientWidth / DW;
      el.style.transform = `scale(${scale})`;
      w.style.height = `${el.scrollHeight * scale}px`;
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(w);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const s: Record<string, React.CSSProperties> = {
    window: {
      width: DW,
      background: C.bg,
      border: `1px solid ${C.border}`,
      borderRadius: 18,
      color: C.text,
      overflow: "hidden",
      display: "flex",
      fontFamily:
        '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
      boxShadow: "0 20px 50px rgba(0,0,0,0.45)",
      transformOrigin: "top left",
    },
    // 左侧会话栏
    sidebar: {
      width: 208,
      flex: "0 0 auto",
      display: "flex",
      flexDirection: "column",
      borderRight: `1px solid ${C.hairline}`,
      background: C.sidebarBg,
    },
    sidebarHead: {
      height: 64,
      flex: "0 0 auto",
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      padding: "0 8px 0 14px",
    },
    sidebarTitle: { fontSize: 10.5, fontWeight: 600, letterSpacing: "0.06em", color: C.dim },
    sidebarBtns: { display: "flex", gap: 2, color: C.dim, fontSize: 13 },
    sidebarBtn: { width: 22, height: 22, display: "grid", placeItems: "center" },
    sidebarList: { display: "flex", flexDirection: "column", gap: 3, padding: 8, paddingTop: 0 },
    sideItem: {
      display: "flex",
      alignItems: "flex-start",
      gap: 9,
      padding: "8px 10px",
      borderRadius: 9,
      color: C.dim,
    },
    sideItemOn: { color: C.text, background: "rgba(255,255,255,0.07)" },
    sideSettings: { marginTop: "auto", display: "flex", alignItems: "center", gap: 9, padding: "10px 18px", color: C.dim, fontSize: 12, borderTop: `1px solid ${C.hairline}` },
    sideIcon: {
      flex: "0 0 auto",
      width: 15,
      height: 15,
      marginTop: 1,
      display: "grid",
      placeItems: "center",
      borderRadius: 4,
      fontSize: 9,
      fontWeight: 700,
      color: "#fff",
    },
    sideName: { fontSize: 12, fontWeight: 500, lineHeight: 1.35, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    sideRepo: { fontSize: 10, color: C.faint, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    // 主列
    main: { flex: 1, minWidth: 0, display: "flex", flexDirection: "column" },
    topbar: {
      height: 64,
      flex: "0 0 auto",
      display: "flex",
      alignItems: "center",
      gap: 12,
      padding: "0 14px 0 18px",
      borderBottom: `1px solid ${C.hairline}`,
    },
    heading: { flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 1 },
    title: { fontSize: 15, fontWeight: 600, letterSpacing: "-0.015em", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    cwd: { fontSize: 9, color: C.faint, opacity: 0.72, fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace', overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
    live: { flex: "0 0 auto", display: "inline-flex", alignItems: "center", gap: 5, fontSize: 9, color: C.faint, whiteSpace: "nowrap" },
    liveDot: { width: 6, height: 6, borderRadius: "50%", background: "#55b982" },
    tabs: { flex: "0 0 auto", display: "flex", padding: 3, borderRadius: 10, background: "rgba(255,255,255,0.05)" },
    tab: { fontSize: 10, lineHeight: 1, padding: "5px 10px", borderRadius: 7, color: C.faint },
    tabOn: { color: C.text, background: "#2e2e30", boxShadow: "0 1px 4px rgba(0,0,0,0.08)" },
    close: { flex: "0 0 auto", color: C.dim, fontSize: 13, padding: 4 },
    stream: { display: "flex", flexDirection: "column", gap: 10, padding: "16px 20px 12px" },
    userBubble: {
      alignSelf: "flex-end",
      maxWidth: "82%",
      background: "rgba(255,255,255,0.07)",
      borderRadius: "18px 18px 5px 18px",
      padding: "11px 15px",
      fontSize: 13,
      lineHeight: 1.58,
    },
    activityCard: { border: `1px solid ${C.hairline}`, borderRadius: 14, background: "rgba(255,255,255,0.03)" },
    summaryRow: { minHeight: 40, display: "flex", alignItems: "center", gap: 9, padding: "0 12px 0 14px", fontSize: 11, color: C.dim },
    summaryMain: { flex: "0 0 auto", fontWeight: 500 },
    summaryKinds: { minWidth: 0, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: C.faint },
    chevron: { flex: "0 0 auto", color: C.faint, fontSize: 11 },
    todoHead: { fontWeight: 500, color: C.text },
    todoCount: { color: C.faint, fontVariantNumeric: "tabular-nums" },
    todoCurrent: { minWidth: 0, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: C.dim },
    todoList: { display: "flex", flexDirection: "column", gap: 5, padding: "8px 14px 12px", borderTop: `1px solid ${C.hairline}` },
    todoRow: { display: "flex", alignItems: "baseline", gap: 9, fontSize: 12, lineHeight: 1.45 },
    approvalCard: {
      display: "flex",
      alignItems: "flex-start",
      gap: 14,
      padding: 12,
      border: `1px solid color-mix(in srgb, ${C.warn} 52%, ${C.border})`,
      borderRadius: 14,
      background: `color-mix(in srgb, ${C.warn} 10%, ${C.bg})`,
    },
    approvalCopy: { flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 3 },
    approvalTitle: { fontSize: 12, fontWeight: 600, whiteSpace: "nowrap" },
    approvalTool: { fontSize: 10, color: C.faint },
    approvalCmd: {
      marginTop: 4,
      padding: "7px 9px",
      borderRadius: 7,
      background: "rgba(255,255,255,0.05)",
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
      fontSize: 11,
      color: C.dim,
    },
    approvalActions: { flex: "0 0 auto", display: "flex", gap: 7 },
    approvalBtn: {
      height: 32,
      padding: "0 12px",
      borderRadius: 8,
      fontSize: 11.5,
      border: `1px solid ${C.border}`,
      color: C.text,
      background: C.bg,
      display: "inline-flex",
      alignItems: "center",
      whiteSpace: "nowrap",
    },
    allowBtn: { border: `1px solid color-mix(in srgb, ${C.okVivid} 55%, ${C.border})`, background: `color-mix(in srgb, ${C.okVivid} 14%, ${C.bg})` },
    denyBtn: { border: `1px solid color-mix(in srgb, ${C.err} 45%, ${C.border})` },
    composeWrap: { padding: "0 20px 14px", marginTop: "auto" },
    compose: {
      minHeight: 132,
      display: "flex",
      flexDirection: "column",
      justifyContent: "space-between",
      gap: 8,
      padding: "14px 16px 12px",
      border: `1px solid ${C.hairline}`,
      borderRadius: 24,
      background: C.bgRaise,
      boxShadow: "0 14px 38px rgba(0,0,0,0.10), 0 2px 8px rgba(0,0,0,0.05)",
    },
    textarea: { padding: "2px 6px", fontSize: 13, lineHeight: 1.6, color: C.faint },
    composeRow: { display: "flex", alignItems: "center", gap: 7 },
    attachBtn: { width: 34, height: 34, display: "grid", placeItems: "center", borderRadius: "50%", color: C.dim, fontSize: 15, lineHeight: 1 },
    pill: {
      display: "inline-flex",
      alignItems: "center",
      gap: 4,
      height: 24,
      padding: "0 9px",
      border: `1px solid ${C.border}`,
      borderRadius: 999,
      color: C.dim,
      fontSize: 10,
      whiteSpace: "nowrap",
    },
    context: { display: "inline-flex", alignItems: "center", gap: 5, color: C.faint, fontSize: 9.5, fontVariantNumeric: "tabular-nums" },
    contextRing: {
      width: 18,
      height: 18,
      borderRadius: "50%",
      display: "grid",
      placeItems: "center",
      fontSize: 7,
      color: C.dim,
      background: `conic-gradient(${C.accent} 0deg 54deg, rgba(255,255,255,0.12) 54deg 360deg)`,
    },
    contextRingInner: { width: 12, height: 12, borderRadius: "50%", background: C.bgRaise, display: "grid", placeItems: "center" },
    spacer: { flex: 1 },
    enterHint: { color: C.faint, fontSize: 9.5 },
    sendBtn: {
      width: 38,
      height: 38,
      display: "grid",
      placeItems: "center",
      borderRadius: "50%",
      background: C.text,
      color: C.bg,
      fontSize: 14,
      fontWeight: 600,
    },
  };

  return (
    <div ref={wrap} style={{ width: "100%", maxWidth: DW, margin: "0 auto", overflow: "hidden" }} aria-hidden="true">
      <div ref={inner} style={s.window}>
      {/* 左：会话栏 */}
      <div style={s.sidebar}>
        <div style={s.sidebarHead}>
          <span style={s.sidebarTitle}>{t.sidebar}</span>
          <span style={s.sidebarBtns}>
            <span style={s.sidebarBtn}>+</span>
            <span style={s.sidebarBtn}>‹</span>
          </span>
        </div>
        <div style={s.sidebarList}>
          {t.sessions.map((item) => (
            <div key={item.title} style={{ ...s.sideItem, ...(item.active ? s.sideItemOn : {}) }}>
              <span style={{ ...s.sideIcon, background: item.iconColor }}>{item.icon}</span>
              <span style={{ minWidth: 0 }}>
                <span style={{ ...s.sideName, display: "block" }}>{item.title}</span>
                <span style={{ ...s.sideRepo, display: "block" }}>{item.repo}</span>
              </span>
            </div>
          ))}
        </div>
        <div style={s.sideSettings}>
          <span style={{ fontSize: 13 }}>⚙</span>
          {t.settings}
        </div>
      </div>

      {/* 右：主列 */}
      <div style={s.main}>
        <div style={s.topbar}>
          <div style={s.heading}>
            <span style={s.title}>{t.title}</span>
            <span style={s.cwd}>{t.cwd}</span>
          </div>
          <span style={s.live}>
            <span style={s.liveDot} />
            {t.live}
          </span>
          <span style={s.tabs}>
            <span style={{ ...s.tab, ...s.tabOn }}>{t.segChat}</span>
            <span style={s.tab}>{t.segTerm}</span>
          </span>
          <span style={s.close}>×</span>
        </div>

        <div style={s.stream}>
          <div style={s.userBubble}>{t.userMsg}</div>

          <div style={s.activityCard}>
            <div style={s.summaryRow}>
              <span style={s.summaryMain}>{t.toolRow}</span>
              <span style={s.summaryKinds}>{t.toolKinds}</span>
              <span style={s.chevron}>›</span>
            </div>
          </div>

          <div style={s.activityCard}>
            <div style={s.summaryRow}>
              <span style={s.todoHead}>{t.todoTitle}</span>
              <span style={s.todoCount}>{t.todoCount}</span>
              <span style={s.todoCurrent}>{t.todos[2].text}</span>
              <span style={s.chevron}>›</span>
            </div>
            <div style={s.todoList}>
              {t.todos.map((todo) => (
                <div key={todo.text} style={s.todoRow}>
                  <span
                    style={{
                      width: 12,
                      flex: "0 0 auto",
                      textAlign: "center",
                      fontSize: 10,
                      color: todo.state === "doing" ? C.accent : todo.state === "done" ? `color-mix(in srgb, ${C.accent} 75%, ${C.text})` : C.faint,
                    }}
                  >
                    {TODO_MARK[todo.state as TodoState]}
                  </span>
                  <span
                    style={{
                      color: todo.state === "done" ? C.faint : todo.state === "doing" ? C.text : C.dim,
                      fontWeight: todo.state === "doing" ? 500 : 400,
                      textDecoration: todo.state === "done" ? "line-through" : "none",
                    }}
                  >
                    {todo.text}
                  </span>
                </div>
              ))}
            </div>
          </div>

          <div style={s.approvalCard}>
            <div style={s.approvalCopy}>
              <span style={s.approvalTitle}>{t.approval}</span>
              <span style={s.approvalTool}>
                {t.approvalTool} <code style={{ color: C.text, fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace' }}>Bash</code>
              </span>
              <span style={s.approvalCmd}>cargo test</span>
            </div>
            <div style={s.approvalActions}>
              <span style={{ ...s.approvalBtn, ...s.allowBtn }}>{t.allow}</span>
              <span style={{ ...s.approvalBtn, ...s.denyBtn }}>{t.deny}</span>
            </div>
          </div>
        </div>

        <div style={s.composeWrap}>
          <div style={s.compose}>
            <div style={s.textarea}>{t.placeholder}</div>
            <div style={s.composeRow}>
              <span style={s.attachBtn}>+</span>
              <span style={s.pill}>
                {t.model}
                <span style={{ fontSize: 7 }}>▾</span>
              </span>
              <span style={s.pill}>
                {t.modeWork}
                <span style={{ fontSize: 7 }}>▾</span>
              </span>
              <span style={s.pill}>
                {t.modePerm}
                <span style={{ fontSize: 7 }}>▾</span>
              </span>
              <span style={s.context}>
                <span style={s.contextRing}>
                  <span style={s.contextRingInner}>{t.contextPct}</span>
                </span>
                {t.contextText}
              </span>
              <span style={s.spacer} />
              <span style={s.enterHint}>{t.enterHint}</span>
              <span style={s.sendBtn}>↑</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
  );
}
