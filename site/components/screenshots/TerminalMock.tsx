// 终端多标签示意：展示「不同会话 = 不同终端 tab」，点卡片精准跳到对应那一个。

import React from "react";
import { AgentLogo, type ProviderId } from "../SupportedAgents";

type Tab = { repo: string; provider: ProviderId; active?: boolean };

const TABS: Tab[] = [
  { repo: "meowo", provider: "claude" },
  { repo: "autopilot", provider: "codex", active: true },
  { repo: "cc-relay", provider: "kimi" },
];

export default function TerminalMock({ style }: { style?: React.CSSProperties }) {
  return (
    <div className="term" style={style}>
      <div className="term-tabbar">
        {TABS.map((t) => (
          <span key={t.repo} className={`term-tab${t.active ? " active" : ""}`}>
            <AgentLogo id={t.provider} size={13} tile={false} />
            <span>{t.repo}</span>
          </span>
        ))}
        <span className="term-newtab">+</span>
      </div>
      <div className="term-screen">
        <div className="term-line">
          <span className="term-path">~/autopilot</span>
          <span className="term-branch"> git:(main)</span>
        </div>
        <div className="term-line">
          <span className="term-arrow">›</span> codex：接入账号用量面板
        </div>
        <div className="term-line term-ask">
          <span className="term-bullet">●</span> 要应用这 3 处修改吗？(y/n)
          <span className="term-cursor" />
        </div>
      </div>
    </div>
  );
}
