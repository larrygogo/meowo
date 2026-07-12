import Link from "next/link";
import { REPO, RELEASE_LATEST } from "@/lib/site";
import { ArrowRightIcon, DownloadIcon } from "@/components/icons";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ProductShowcase from "@/components/ProductShowcase";
import { StickerWindow } from "@/components/screenshots";

const PROBLEMS = [
  {
    title: "终端开了一堆",
    body: "Claude Code、Codex、Kimi 各跑各的，想看进度得一个个窗口切。",
  },
  {
    title: "AI 卡住你没发现",
    body: "会话在等你回复、或者工具调用失败，终端被压在后面，半天才注意到。",
  },
  {
    title: "回到现场很麻烦",
    body: "想继续某个会话，得先找到它所在的终端标签页，再重新聚焦。",
  },
];

const SCENES = [
  {
    label: "状态一览",
    title: "几个终端的会话，一个窗口看完",
    body: "运行中、待交互、已断开，用颜色和 Context 百分比区分。不用在 Windows Terminal / iTerm2 之间来回切。",
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
    label: "提醒",
    title: "等你回复时，别让它晾着",
    body: "会话卡住或需要你确认时，会进到「待交互」tab，排最前面。也可以弹系统通知，点一下直接跳到对应终端。",
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
    label: "跳转",
    title: "点卡片，回到会话所在终端",
    body: "连接中的会话直接跳到对应标签页；已经断开的，会在原项目目录新开终端并执行 claude --resume 恢复对话。",
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
    label: "管理",
    title: "星标、便签、改名、归档",
    body: "右键卡片或点右上角 ⋮ 按钮：给重要会话加星置顶，写一条本地备忘，直接改名，或者把暂时不用的会话收起来。",
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

export default function Home() {
  return (
    <main>
      {/* Hero */}
      <section className="hero hero-dark">
        <div className="container">
          <span className="pill pill-dark">
            <span className="dot" />
            免费 · 开源 · Windows 与 macOS
          </span>
          <h1 className="h-display">
            别再在终端里
            <br />
            找你的 AI 会话了
          </h1>
          <p className="lead lead-light">
            Meowo 是一个桌面小窗口，实时显示 Claude Code、Codex、Kimi 的会话状态。
            <br className="hide-sm" />
            不用切终端，也能看到谁在跑、谁在等你。
          </p>
          <div className="hero-cta">
            <a
              className="btn btn-light btn-lg"
              href={RELEASE_LATEST}
              target="_blank"
              rel="noopener noreferrer"
            >
              <DownloadIcon />
              下载最新版
            </a>
            <Link className="btn btn-ghost-light btn-lg" href="/features">
              看它怎么工作 <ArrowRightIcon />
            </Link>
          </div>
          <ProductShowcase className="hero-showcase" />
        </div>
      </section>

      {/* 痛点 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">这些场景熟悉吗</span>
            <h2 className="h1">会话多了，难免手忙脚乱</h2>
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
            <span className="eyebrow">能做什么</span>
            <h2 className="h1">从看状态到回现场</h2>
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
            <span className="eyebrow">更多功能</span>
            <h2 className="h1">看板之外还有这些</h2>
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
