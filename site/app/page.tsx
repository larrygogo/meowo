import Link from "next/link";
import { getLatestRelease } from "@/lib/release";
import { ArrowRightIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ProductShowcase from "@/components/ProductShowcase";
import { StickerWindow } from "@/components/screenshots";

const PROBLEMS = [
  {
    title: "会话散在多个终端窗口",
    body: "Claude Code、Codex、Kimi 各跑各的。想知道某个会话到哪一步了，得逐个窗口切过去看。",
  },
  {
    title: "等待确认的会话容易被忽略",
    body: "会话在等一个确认，或者工具调用失败停住了。终端被别的窗口压着，几分钟后才发现。",
  },
  {
    title: "找不到会话在哪个终端",
    body: "想接着聊某个会话，得先回忆它开在哪个窗口、哪个标签页。",
  },
];

const SCENES = [
  {
    label: "看板",
    title: "所有会话的状态在同一个列表里",
    body: "每张卡片显示会话标题、项目名、连接状态和最近一条 AI 输出。Claude Code 的会话另外显示 Context 已用百分比。不需要在 Windows Terminal 和 iTerm2 之间切换。",
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
    title: "等待输入的会话排在最前",
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
    label: "终端",
    title: "点击卡片，切到会话所在的终端",
    body: "连接中的会话会切到它所在的标签页。已经断开的会话，Meowo 在原项目目录新开一个终端，执行 claude --resume 恢复对话。",
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
    title: "对单个会话的操作",
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
            Claude Code、Codex、Kimi
            <br />
            的会话状态，常驻桌面
          </h1>
          <p className="lead lead-light">
            Meowo 是一个桌面小窗。它读取各个 CLI 上报的会话事件，
            <br className="hide-sm" />
            显示每个会话正在运行、在等你回复，还是已经断开。
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

      {/* 痛点 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">背景</span>
            <h2 className="h1">它针对的三个问题</h2>
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
            <h2 className="h1">它做什么</h2>
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
            <span className="eyebrow">功能列表</span>
            <h2 className="h1">其余功能</h2>
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
