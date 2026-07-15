// 终端多标签示意：展示「不同会话 = 不同终端 tab」，点卡片精准跳到对应那一个。

import React from "react";
import { AgentLogo, type ProviderId } from "../SupportedAgents";
import type { Lang } from "@/lib/i18n";

type Tab = { repo: string; provider: ProviderId; active?: boolean };

const TABS: Tab[] = [
  { repo: "meowo", provider: "claude" },
  { repo: "autopilot", provider: "codex", active: true },
  { repo: "cc-relay", provider: "kimi" },
];

const LINES = {
  zh: { prompt: "codex：接入账号用量面板", ask: "要应用这 3 处修改吗？(y/n)" },
  en: { prompt: "codex: wire up the usage panel", ask: "Apply these 3 changes? (y/n)" },
};

export default function TerminalMock({ style, lang = "zh" }: { style?: React.CSSProperties; lang?: Lang }) {
  const t = LINES[lang];
  return (
    <div className="term" style={style}>
      <div className="term-tabbar">
        {TABS.map((tab) => (
          <span key={tab.repo} className={`term-tab${tab.active ? " active" : ""}`}>
            <AgentLogo id={tab.provider} size={13} tile={false} />
            <span>{tab.repo}</span>
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
          <span className="term-arrow">›</span> {t.prompt}
        </div>
        <div className="term-line term-ask">
          <span className="term-bullet">●</span> {t.ask}
          <span className="term-cursor" />
        </div>
      </div>
    </div>
  );
}
