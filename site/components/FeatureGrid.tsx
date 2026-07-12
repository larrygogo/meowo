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
    title: "实时会话看板",
    body: (
      <>
        每张卡片显示项目名、会话标题、最近一条 AI 正文和连接状态。Claude Code 会话还会显示{" "}
        <code className="inline">Context</code> 已用百分比。
      </>
    ),
  },
  {
    icon: <BellIcon />,
    title: "待交互提醒",
    body: "会话需要你回复或出错时弹一条系统通知；「待交互」按等待时间排序，先处理等得最久的。",
  },
  {
    icon: <TerminalIcon />,
    title: "点击直达终端",
    body: (
      <>
        点连接中的会话跳到对应终端标签页；点已断开的会话，在原目录新开终端并执行{" "}
        <code className="inline">--resume</code> 续聊。
      </>
    ),
  },
  {
    icon: <CardsIcon />,
    title: "卡片管理",
    body: (
      <>
        给会话加星置顶、写本地便签、直接改名（和 <code className="inline">/rename</code>{" "}
        同步）、归档收起。操作入口在右键菜单或 ⋮ 按钮里。
      </>
    ),
  },
  {
    icon: <MagnetIcon />,
    title: "吸边与菜单栏",
    body: "Windows 拖到屏幕边缘缩成缩略条，悬停展开；macOS 为菜单栏面板，图标实时显示运行中与待交互计数。",
  },
  {
    icon: <ChartIcon />,
    title: "账号与用量",
    body: "底栏常显 5 小时 / 7 天配额利用率；设置页可查看账号、分模型用量与重置时间。",
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
