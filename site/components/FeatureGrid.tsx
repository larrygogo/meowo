import type { ReactNode } from "react";
import Reveal from "./Reveal";
import {
  BoardIcon,
  BellIcon,
  TerminalIcon,
  CardsIcon,
  MagnetIcon,
  ChartIcon,
} from "./icons";

type Feature = {
  icon: ReactNode;
  title: string;
  body: ReactNode;
};

const FEATURES: Feature[] = [
  {
    icon: <BoardIcon />,
    title: "会话卡片",
    body: (
      <>
        项目、标题、AI 最近说的那句话、连着还是断了，都在卡片上。Claude Code 的会话多一个{" "}
        <code className="inline">Context</code> 百分比。
      </>
    ),
  },
  {
    icon: <BellIcon />,
    title: "待交互提醒",
    body: "它要你回复，或者报错停住了，就弹一条系统通知。同一件事只弹一次，不会连环骚扰。",
  },
  {
    icon: <TerminalIcon />,
    title: "跳回终端",
    body: (
      <>
        连着的会话直接切到那个标签页。断开的，在原目录新开终端{" "}
        <code className="inline">--resume</code> 接着聊。
      </>
    ),
  },
  {
    icon: <CardsIcon />,
    title: "星标、便签、改名、归档",
    body: (
      <>
        都在右键菜单或者 ⋮ 里。改名走的是和 <code className="inline">/rename</code>{" "}
        一样的记录，所以 resume 列表里显示的也是新名字。
      </>
    ),
  },
  {
    icon: <MagnetIcon />,
    title: "吸边和菜单栏",
    body: "Windows 上拖到屏幕边缘就缩成一根细条，鼠标碰一下才展开。macOS 上它是个菜单栏面板，不占 Dock。",
  },
  {
    icon: <ChartIcon />,
    title: "配额还剩多少",
    body: "底栏常年显示 5 小时和 7 天的用量比例。想看得细一点，设置页里有分模型的用量和重置时间。",
  },
];

export default function FeatureGrid() {
  return (
    <div className="grid grid-3">
      {FEATURES.map((f) => (
        <Reveal key={f.title}>
          <div className="fcard">
            <div className="fi">{f.icon}</div>
            <h3>{f.title}</h3>
            <p>{f.body}</p>
          </div>
        </Reveal>
      ))}
    </div>
  );
}
