import type { ReactNode } from "react";
import Reveal from "./Reveal";
import {
  BoardIcon,
  BellIcon,
  TerminalIcon,
  CardsIcon,
  MagnetIcon,
  ChartIcon,
  PlugIcon,
  NetworkIcon,
  ShieldIcon,
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
        每张卡片显示项目名、会话标题、最近一条 AI 输出和连接状态。支持读取 Context 的 AI Agent
        还会显示已用百分比。
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
    title: "一键启动与续接",
    body: "选择项目目录和 AI 工具即可新建会话。点击卡片切回对应终端；会话已断开时，自动回到原目录续接，无需记命令或会话 ID。",
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
  {
    icon: <PlugIcon />,
    title: "一键安装、登录与接入",
    body: "无需先在终端配置环境：直接在 Meowo 里安装 AI CLI、发起登录，并自动接入所需 hooks。检测到连接缺失时，也能一键修复。",
  },
  {
    icon: <NetworkIcon />,
    title: "按 AI 工具设置代理",
    body: "设置全局默认代理，也可以为每个 AI 工具单独选择直连、跟随系统或自定义代理。",
  },
  {
    icon: <ShieldIcon />,
    title: "本地优先",
    body: "会话与设置保存在本机。Meowo 通过本地数据库汇总状态，不把会话内容上传到自己的服务器。",
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
