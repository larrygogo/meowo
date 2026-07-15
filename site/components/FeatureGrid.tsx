import type { ReactNode } from "react";
import Reveal from "./Reveal";
import {
  BoardIcon,
  BellIcon,
  TerminalIcon,
  CardsIcon,
  TrafficIcon,
  ChartIcon,
  PlugIcon,
  UsersIcon,
  NetworkIcon,
  PaletteIcon,
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
        还会显示上下文已用百分比。
      </>
    ),
  },
  {
    icon: <BellIcon />,
    title: "待交互与通知",
    body: "会话等待输入或报错时进入「待交互」，按等待时长排序。开启系统通知后，点通知直接跳到对应会话，同一件事只弹一次。",
  },
  {
    icon: <TerminalIcon />,
    title: "点击直达终端 tab",
    body: "点卡片，直接切到该会话所在的终端标签页。会话已断开时，自动回到原目录并按对应工具的方式续接——无需记命令或会话 ID。",
  },
  {
    icon: <CardsIcon />,
    title: "会话菜单一站集成",
    body: (
      <>
        右键或点 ⋮：一键新建会话、打开项目目录、加星置顶、写本地便签、改名（与{" "}
        <code className="inline">/rename</code> 同步）、归档。常用操作都在这，不用导出切换、也不用敲命令。
      </>
    ),
  },
  {
    icon: <TrafficIcon />,
    title: "展开贴纸，收起红绿灯",
    body: "展开时是钉在桌面一角的贴纸；拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯——红黄绿三色一眼看清各会话状态。",
  },
  {
    icon: <ChartIcon />,
    title: "用量与上下文监控",
    body: "底栏显示 5 小时 / 7 天配额使用比例，越接近上限越偏红；卡片显示会话上下文用量。不焦虑，一切都在计划之中。",
  },
  {
    icon: <PlugIcon />,
    title: "一键安装、登录与接入",
    body: "无需先在终端配置环境：直接在 Meowo 里安装 AI CLI、发起登录，并自动接入所需 hooks。检测到连接缺失时，也能一键修复。",
  },
  {
    icon: <UsersIcon />,
    title: "多账号 + API 中转",
    body: "同一个工具保存多个官方账号，一键切换，各自独立登录与会话历史；也支持按模型接入 API 中转，配置期间仍走官方账号。",
  },
  {
    icon: <NetworkIcon />,
    title: "按 AI 工具设置代理",
    body: "设置全局默认代理，也可以为每个 AI 工具单独选择直连、跟随系统或自定义代理，支持 HTTP / SOCKS5 及带认证的地址。",
  },
  {
    icon: <PaletteIcon />,
    title: "多风格 · 多配色",
    body: "7 种贴纸配色、扁平与立体两种风格、深浅主题随系统或手动切换，还能调不透明度与界面密度——挑一套顺眼的摆在桌面上。",
  },
  {
    icon: <ShieldIcon />,
    title: "本地优先",
    body: "会话与设置保存在本机。Meowo 通过本地 SQLite 汇总状态，不把会话内容上传到自己的服务器。",
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
