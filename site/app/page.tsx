import Link from "next/link";
import { getLatestRelease } from "@/lib/release";
import { ArrowRightIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ProductShowcase from "@/components/ProductShowcase";
import SupportedAgents from "@/components/SupportedAgents";
import { StickerWindow } from "@/components/screenshots";

const PROBLEMS = [
  {
    title: "会话散在多个终端窗口",
    body: "多个 AI 编程代理各跑各的。想知道某个会话到哪一步了，得逐个窗口切过去看。",
  },
  {
    title: "等待确认的会话容易被忽略",
    body: "会话在等一个确认，或者工具调用失败停住了。终端被别的窗口压着，几分钟后才发现。",
  },
  {
    title: "常用操作离不开命令行",
    body: "换项目、启动不同的 AI 工具、恢复旧会话，都要反复切目录、找会话 ID、输入不同命令。",
  },
];

const SCENES = [
  {
    label: "看板",
    title: "所有会话的状态在同一个列表里",
    body: "每张卡片显示会话标题、项目名、连接状态和最近一条 AI 输出。支持读取 Context 的 AI Agent 还会显示已用百分比，不必再逐个终端确认进度。",
    shot: (
      <StickerWindow
        activeTab="all"
        cards={[
          {
            title: "重构吸边状态机",
            repo: "meowo",
            provider: "claude",
            state: "running",
            pct: 62,
            aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。",
            time: "刚刚",
            model: "claude-opus-4",
          },
          {
            title: "接入账号用量面板",
            repo: "autopilot",
            provider: "codex",
            state: "waiting",
            pct: 43,
            aiText: "要应用这 3 处修改吗？(y/n)",
            time: "刚刚",
          },
          {
            title: "升级 tauri 到 2.3",
            repo: "cc-relay",
            provider: "kimi",
            state: "idle",
            aiText: "已更新 Cargo.toml，等你确认几处 breaking change。",
            time: "12 分钟前",
          },
        ]}
      />
    ),
  },
  {
    label: "通知",
    title: "该你处理的会话，自动排到前面",
    body: "会话需要确认，或者报错停住时，会进入「待交互」tab，按等待时长排序。可以开启系统通知，点击通知直接切到对应终端。",
    shot: (
      <StickerWindow
        activeTab="waiting"
        cards={[
          {
            title: "接入账号用量面板",
            repo: "autopilot",
            provider: "codex",
            state: "waiting",
            pct: 43,
            aiText: "要应用这 3 处修改吗？(y/n)",
            time: "刚刚",
          },
        ]}
      />
    ),
  },
  {
    label: "启动与续接",
    title: "新建、切换、续接，都不用敲命令",
    body: "选择项目目录和 AI 工具即可新建会话。点击已有卡片会切到对应终端；会话已断开时，Meowo 会回到原目录并按对应工具的方式续接。",
    shot: (
      <StickerWindow
        activeTab="all"
        cards={[
          {
            title: "修复 statusline 兼容性",
            repo: "clawmo-ios",
            provider: "claude",
            state: "stopped",
            aiText: "兼容性修好并已合并，收工。",
            time: "3 小时前",
          },
        ]}
      />
    ),
  },
  {
    label: "会话管理",
    title: "把重要会话整理好",
    body: "右键卡片，或点右上角的 ⋮ 按钮。可以给会话加星置顶，写一条只存在本地的便签，改名，或者把不再关注的会话归档收起。",
    shot: (
      <StickerWindow
        activeTab="all"
        cards={[
          {
            title: "重构吸边状态机",
            repo: "meowo",
            provider: "claude",
            state: "running",
            pct: 62,
            aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。",
            time: "刚刚",
            note: "记得先确认 API key",
            starred: true,
          },
        ]}
        showNote
        showMenu
      />
    ),
  },
];

export default async function Home() {
  const release = await getLatestRelease();

  return (
    <main>
      {/* Hero */}
      <section className="hero hero-dark">
        <div className="container">
          <span className="pill pill-dark">
            <span className="dot" />
            开源 · MIT · Windows 与 macOS
            {release ? ` · ${release.tag}` : ""}
          </span>
          <h1 className="h-display">
            AI 编程，多开不乱
            <br />
            常用操作，少敲命令
          </h1>
          <p className="lead lead-light">
            Meowo 是一款本地优先的 AI 编程代理桌面工作台，
            <br className="hide-sm" />
            从 AI CLI 的一键安装、登录，到会话状态与待办提醒，都在一个应用里完成。
          </p>
          <div className="hero-cta">
            <DownloadButton
              windows={release?.windows ?? null}
              macos={release?.macos ?? null}
              fallbackHref="/download"
              className="btn btn-light btn-lg"
            />
            <Link className="btn btn-ghost-light btn-lg" href="/features">
              查看功能 <ArrowRightIcon />
            </Link>
          </div>
          <ProductShowcase className="hero-showcase" />
        </div>
      </section>

      <SupportedAgents />

      {/* 痛点 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">为什么是 Meowo</span>
            <h2 className="h1">并行使用 AI 编程代理，本不该这么累</h2>
          </div>
          <div className="grid grid-3">
            {PROBLEMS.map((p) => (
              <Reveal key={p.title}>
                <div className="fcard">
                  <h3>{p.title}</h3>
                  <p>{p.body}</p>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 场景展示 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">概览</span>
            <h2 className="h1">一个工作台，接住每个会话</h2>
          </div>
          <div className="scenes">
            {SCENES.map((s, i) => (
              <Reveal key={s.title}>
                <div className={`scene ${i % 2 === 1 ? "scene-rev" : ""}`}>
                  <div className="scene-shot">{s.shot}</div>
                  <div className="scene-text">
                    <span className="eyebrow">{s.label}</span>
                    <h3 className="h2">{s.title}</h3>
                    <p className="lead">{s.body}</p>
                  </div>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>

      {/* 特性网格 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">完整工作流</span>
            <h2 className="h1">从会话到环境，一处管理</h2>
          </div>
          <FeatureGrid />
          <div style={{ textAlign: "center", marginTop: 40 }}>
            <Link className="btn btn-ghost" href="/features">
              查看全部功能 <ArrowRightIcon />
            </Link>
          </div>
        </div>
      </section>

      <CtaBand />
    </main>
  );
}
