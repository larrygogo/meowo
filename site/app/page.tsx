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
    title: "窗口开了一堆",
    body: "Claude Code 在一个标签页，Codex 在另一个，Kimi 又是新开的窗口。想知道各自跑到哪了，只能挨个点过去看。",
  },
  {
    title: "它停下来问你，你没看见",
    body: "会话卡在「要应用这 3 处修改吗」，或者工具调用失败退出了。终端压在最底下，可能十几分钟后才发现。",
  },
  {
    title: "回不去现场",
    body: "想接着改，先得想起来那个会话开在哪。断开的更麻烦，还要自己 cd 回项目目录再 resume。",
  },
];

const SCENES = [
  {
    label: "看状态",
    title: "会话都在这一列里",
    body: "运行中是橙色转圈，等你回复的是黄点，断开的是虚线环。Claude Code 的会话还会显示 Context 用了多少，快到上限一眼看得出来。",
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
    label: "待交互",
    title: "它停下来的时候，你会知道",
    body: "需要确认，或者卡在报错上，会话就会进「待交互」这个 tab，等得越久排得越靠前。系统通知默认开着，点一下直接跳到那个终端。",
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
    label: "回终端",
    title: "点一下就回到现场",
    body: "还连着的会话，直接切到它所在的那个标签页。已经断开的，会在原来的项目目录新开一个终端，跑 claude --resume 接上之前的对话。",
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
    label: "整理",
    title: "顺手收拾一下",
    body: "重要的加个星标就置顶了。名字不好认可以直接改，改完 resume 列表里也是新名字。想记一句「先确认 API key」就挂张便签，只存在本地。暂时不看的收进归档，随时能翻出来。",
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
            哪个会话
            <br />
            正在等你回复？
          </h1>
          <p className="lead lead-light">
            Meowo 是个常驻桌面角落的小窗口，把 Claude Code、Codex、Kimi 的会话摆在一起：
            <br className="hide-sm" />
            谁在跑、谁卡住了、谁在等你确认。点一下，回到它所在的终端。
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
              先看看功能 <ArrowRightIcon />
            </Link>
          </div>
          <ProductShowcase className="hero-showcase" />
        </div>
      </section>

      {/* 痛点 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">为什么做这个</span>
            <h2 className="h1">同时开三个会话之后</h2>
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
            <span className="eyebrow">怎么用</span>
            <h2 className="h1">一个窗口，四件事</h2>
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
            <span className="eyebrow">其它</span>
            <h2 className="h1">还有这些</h2>
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
