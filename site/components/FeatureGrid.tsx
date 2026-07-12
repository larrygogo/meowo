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
    title: "会话看板",
    body: (
      <>
        每张卡片显示项目名、会话标题、最近一条 AI 输出和连接状态。Claude Code 的会话另外显示{" "}
        <code className="inline">Context</code> 已用百分比。
      </>
    ),
  },
  {
    icon: <BellIcon />,
    title: "待交互与通知",
    body: "会话等待输入或报错时，进入「待交互」分类，按等待时长排序。可以开启系统通知，同一件事只弹一次。",
  },
  {
    icon: <TerminalIcon />,
    title: "终端跳转",
    body: (
      <>
        点连接中的会话，切到它所在的终端标签页。点已断开的会话，在原目录新开终端并执行{" "}
        <code className="inline">--resume</code> 续接对话。
      </>
    ),
  },
  {
    icon: <CardsIcon />,
    title: "会话管理",
    body: (
      <>
        加星置顶、写本地便签、改名（与 <code className="inline">/rename</code>{" "}
        同步）、归档收起。入口在右键菜单或 ⋮ 按钮。
      </>
    ),
  },
  {
    icon: <MagnetIcon />,
    title: "吸边与菜单栏",
    body: "Windows 上拖到屏幕边缘会缩成一根细条，鼠标悬停展开。macOS 上是菜单栏面板，图标显示运行中与待交互的会话数。",
  },
  {
    icon: <ChartIcon />,
    title: "用量读数",
    body: "底栏显示 5 小时 / 7 天配额的使用比例。设置页可以看到账号信息、分模型用量和重置时间。",
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
