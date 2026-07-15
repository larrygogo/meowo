import Link from "next/link";
import { getLatestRelease } from "@/lib/release";
import { ArrowRightIcon } from "@/components/icons";
import DownloadButton from "@/components/DownloadButton";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ProductShowcase from "@/components/ProductShowcase";
import SupportedAgents from "@/components/SupportedAgents";
import ThemeShowcase from "@/components/ThemeShowcase";
import { StickerWindow, CollapsedStrip, TerminalMock } from "@/components/screenshots";

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

const RUNNING_CARDS = [
  {
    title: "重构吸边状态机",
    repo: "meowo",
    provider: "claude" as const,
    state: "running" as const,
    pct: 62,
    aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。",
    time: "刚刚",
    model: "claude-opus-4",
  },
  {
    title: "接入账号用量面板",
    repo: "autopilot",
    provider: "codex" as const,
    state: "waiting" as const,
    pct: 43,
    aiText: "要应用这 3 处修改吗？(y/n)",
    time: "刚刚",
  },
  {
    title: "升级 tauri 到 2.3",
    repo: "cc-relay",
    provider: "kimi" as const,
    state: "idle" as const,
    aiText: "已更新 Cargo.toml，等你确认几处 breaking change。",
    time: "12 分钟前",
  },
];

export default async function Home() {
  const release = await getLatestRelease();

  return (
    <main>
      {/* Hero */}
      <section className="hero-dark">
        <div className="container">
          <span className="pill pill-dark">
            <span className="dot" />
            开源 · MIT · Windows 与 macOS
            {release ? ` · ${release.tag}` : ""}
          </span>
          <h1 className="h-display">
            多开 AI 编程
            <br />
            <span className="grad">一切尽在计划之中</span>
          </h1>
          <p className="lead lead-light">
            本地优先的 AI 编程代理工作台。展开是桌面贴纸，收起是电子红绿灯，点一下直达对应终端。
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
          <p className="hero-note">无需预装 AI CLI · 应用内一键安装、登录与接入</p>
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
            <span className="eyebrow">一个工作台</span>
            <h2 className="h1">接住每一个会话</h2>
            <p className="lead">从桌面一角的状态，到点一下就回到终端，常用操作全部就位。</p>
          </div>

          <div className="scenes">
            {/* a. 双形态：展开贴纸 / 收起红绿灯 */}
            <Reveal>
              <div className="scene">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark forms-stage">
                    <StickerWindow activeTab="all" cards={RUNNING_CARDS.slice(0, 2)} />
                    <CollapsedStrip edge="right" className="forms-edge-strip" />
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">窗口形态</span>
                  <h3 className="h2">展开是桌面贴纸，收起是电子红绿灯</h3>
                  <p className="lead">
                    需要看细节时，它是钉在桌面一角的贴纸，卡片、状态、用量一览无余。拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯——红、黄、绿三色，一眼看清哪个会话报错、哪个在等你、哪个还在跑。鼠标悬停立刻展开。
                </p>
                </div>
              </div>
            </Reveal>

            {/* b. 点击直达终端 tab */}
            <Reveal>
              <div className="scene scene-rev">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark">
                    <div className="route-scene">
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
                      <div className="route-arrow">
                        <ArrowRightIcon style={{ transform: "rotate(90deg)" }} />
                      </div>
                      <TerminalMock />
                    </div>
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">点击直达</span>
                  <h3 className="h2">点一下会话，跳到它所在的终端</h3>
                  <p className="lead">
                    每个会话跑在各自的终端里——不同项目、不同 AI 工具。点卡片，Meowo 直接把你带到它所在的那个终端标签页，不用在一堆窗口里翻找。
                  </p>
                  <ul className="checklist">
                    <li>
                      <span className="ck">
                        <ArrowRightIcon />
                      </span>
                      <span>Windows 切到 Windows Terminal 的对应 tab，macOS 聚焦 Terminal / iTerm2</span>
                    </li>
                    <li>
                      <span className="ck">
                        <ArrowRightIcon />
                      </span>
                      <span>开启系统通知后，点通知同样一步直达该会话</span>
                    </li>
                    <li>
                      <span className="ck">
                        <ArrowRightIcon />
                      </span>
                      <span>已断开的会话，自动回到原目录并按对应工具续接</span>
                    </li>
                  </ul>
                </div>
              </div>
            </Reveal>

            {/* c. 用量与上下文监控 */}
            <Reveal>
              <div className="scene">
                <div className="scene-shot">
                  <div className="scene-stage stage-dark">
                    <StickerWindow activeTab="all" cards={RUNNING_CARDS} />
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">尽在掌握</span>
                  <h3 className="h2">配额与上下文，都在计划之中</h3>
                  <p className="lead">
                    底栏实时显示 5 小时 / 7 天配额的使用比例，越接近上限颜色越偏红；每张卡片显示会话的上下文已用百分比。快到限额、上下文快满，你都提前知道——不用焦虑，也不会被突然中断打个措手不及。
                  </p>
                </div>
              </div>
            </Reveal>

            {/* f. 会话菜单集成 */}
            <Reveal>
              <div className="scene scene-rev">
                <div className="scene-shot">
                  <div className="scene-stage" style={{ paddingBottom: 64 }}>
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
                  </div>
                </div>
                <div className="scene-text">
                  <span className="eyebrow">会话菜单</span>
                  <h3 className="h2">常用操作，全集成进一个菜单</h3>
                  <p className="lead">
                    右键卡片，或点右上角的 ⋮：一键新建会话、打开项目目录、加星置顶、写一条只存在本地的便签、改名、归档。想做的都在这——不用导出切换，也不用回终端敲命令。
                  </p>
                </div>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      {/* d + e. 账号与网络 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">账号与网络</span>
            <h2 className="h1">多账号一键切，网络自己说了算</h2>
            <p className="lead">官方账号、API 中转、按工具设置代理，全部在设置里点选，不碰配置文件。</p>
          </div>
          <div className="accounts">
            <Reveal>
              <div className="acct-card">
                <h3>官方多账号，一键切换</h3>
                <p>同一个工具保存多个官方账号，各自独立登录与会话历史，互不影响。点一下切换，配额、登录状态立刻跟着走。</p>
                <div className="acct-rows">
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#d97757" }}>工</span>
                    <span className="aname">Claude · 工作</span>
                    <span className="abadge on">使用中</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#5b8db8" }}>个</span>
                    <span className="aname">Claude · 个人</span>
                    <span className="abadge off">切换</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#6fae6a" }}>C</span>
                    <span className="aname">Codex · 默认账号</span>
                    <span className="abadge off">切换</span>
                  </div>
                </div>
              </div>
            </Reveal>
            <Reveal>
              <div className="acct-card">
                <h3>API 中转 + 按工具代理</h3>
                <p>没有官方账号也能用：按模型接入 API 中转，配置期间仍走官方账号。每个工具还能单独走直连、跟随系统或自定义代理。</p>
                <div className="acct-rows">
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#7a5bb8" }}>↳</span>
                    <span className="aname">Opus · API 中转</span>
                    <span className="abadge relay">中转</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#0f9e78" }}>P</span>
                    <span className="aname">代理 · SOCKS5</span>
                    <span className="abadge on">自定义</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#8a938e" }}>≡</span>
                    <span className="aname">其余工具</span>
                    <span className="abadge off">跟随系统</span>
                  </div>
                </div>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      {/* h. 多风格多配色 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">你的桌面，你说了算</span>
            <h2 className="h1">多种风格与配色，随手切换</h2>
            <p className="lead">下面这块就是活的，点点看它怎么变。</p>
          </div>
          <Reveal>
            <ThemeShowcase />
          </Reveal>
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
          <div style={{ textAlign: "center", marginTop: 44 }}>
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
