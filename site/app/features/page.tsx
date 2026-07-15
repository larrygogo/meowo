import type { Metadata } from "next";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import ThemeShowcase from "@/components/ThemeShowcase";
import { CheckIcon } from "@/components/icons";
import { StickerWindow, CollapsedStrip } from "@/components/screenshots";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 的功能：展开贴纸 / 收起电子红绿灯、点击直达终端、配额与上下文监控、官方多账号一键切与 API 中转、按工具设置代理、一键安装登录 AI CLI、多风格多配色切换。",
};

function Check({ children }: { children: React.ReactNode }) {
  return (
    <li>
      <span className="ck">
        <CheckIcon />
      </span>
      <span>{children}</span>
    </li>
  );
}

export default function FeaturesPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">功能</span>
          <h1 className="h1">一个工作台，管理完整流程</h1>
          <p className="lead">
            从安装、登录 AI 编程代理，到查看状态、处理提醒、续接会话与切换账号，整个流程都在一个桌面工作台完成。
          </p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container" style={{ display: "flex", justifyContent: "center" }}>
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
        </div>
        <p className="showcase-cap">会话卡片、状态分类 tab、底栏用量读数</p>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">概览</span>
            <h2 className="h1">功能一览</h2>
          </div>
          <FeatureGrid />
        </div>
      </section>

      {/* 双形态：展开贴纸 / 收起红绿灯 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">窗口形态</span>
              <h2 className="h2">展开是桌面贴纸，收起是电子红绿灯</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                需要看细节时它是钉在桌面一角的贴纸；拖到屏幕边缘收起，就缩成一条竖排的电子红绿灯。红黄绿三色，一眼看清哪个会话报错、哪个在等你、哪个还在跑。
              </p>
              <ul className="checklist">
                <Check>拖到屏幕左 / 右 / 顶边松手，窗口缩成一条状态条，鼠标悬停展开</Check>
                <Check>红 = 报错、黄 = 待交互、绿 = 运行中，收起也不漏掉任何一个</Check>
                <Check>可以置顶；重启后沿用上次的窗口位置和吸附边</Check>
                <Check>macOS 上是菜单栏面板，图标显示运行中与待交互的会话数</Check>
              </ul>
            </div>
            <div className="scene-stage stage-dark forms-stage" style={{ minHeight: 340 }}>
              <StickerWindow
                activeTab="all"
                cards={[
                  {
                    title: "重构吸边状态机",
                    repo: "meowo",
                    provider: "claude",
                    state: "running",
                    pct: 62,
                    aiText: "把状态机拆成 3 个纯函数…",
                    time: "刚刚",
                  },
                  {
                    title: "接入账号用量面板",
                    repo: "autopilot",
                    provider: "codex",
                    state: "waiting",
                    pct: 43,
                    aiText: "要应用这 3 处修改吗？",
                    time: "刚刚",
                  },
                ]}
              />
              <CollapsedStrip edge="right" className="forms-edge-strip" />
            </div>
          </div>
        </div>
      </section>

      {/* 看板 & 提醒 & 点击直达 */}
      <section className="section">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">看板 · 通知 · 点击直达</span>
              <h2 className="h2">该你处理的会话，点一下就到</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                会话按状态分成几个 tab。「待交互」里的会话按等待时长排序，等得最久的在最上面。需要回复或出错时弹一条系统通知，点通知或点卡片，直接切到对应终端。
              </p>
              <ul className="checklist">
                <Check>四个 tab：全部 / 待交互 / 运行中 / 已归档，各自带数量</Check>
                <Check>「待交互」内部按等待时长排序，等得最久的排最前</Check>
                <Check>系统通知会去重，同一件事只弹一次；点击后切到该会话</Check>
                <Check>连接中切到 Windows Terminal / Terminal / iTerm2 的对应标签页</Check>
              </ul>
            </div>
            <div className="split-media" style={{ padding: 22 }}>
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
            </div>
          </div>
        </div>
      </section>

      {/* 安装、登录、启动、续接、会话菜单 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split rev">
            <div className="split-media" style={{ padding: 22, overflow: "visible" }}>
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
            <div className="split-text">
              <span className="eyebrow">安装 · 登录 · 启动 · 会话菜单</span>
              <h2 className="h2">从装好 Agent 到日常操作，都不用背命令</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                AI CLI 尚未安装时，可直接一键安装并发起登录；准备好后选项目目录和工具即可开始。新建会话、打开项目目录、加星、便签、改名、归档，全在右键菜单或 ⋮ 按钮里。
              </p>
              <ul className="checklist">
                <Check>一键安装 Claude Code、Codex、Kimi、Gemini CLI 或 OpenCode，并直接发起登录</Check>
                <Check>自动接入所需 hooks；检测到连接缺失时，一键修复</Check>
                <Check>选目录、选工具，点一下新建会话；断开的会话一键续接</Check>
                <Check>一键打开项目目录，改名与 <code className="inline">/rename</code> 同步</Check>
                <Check>加星置顶、写只存本地的便签、归档收起，都在同一个菜单</Check>
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* 账号：多账号 + 中转 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">账号</span>
            <h2 className="h1">官方多账号一键切，也支持 API 中转</h2>
            <p className="lead">
              同一个工具保存多个官方账号，随时切换；没有官方账号时按模型接入 API 中转。两种接入方式互斥，配置中转期间仍走官方账号。
            </p>
          </div>
          <div className="accounts">
            <Reveal>
              <div className="acct-card">
                <h3>官方多账号</h3>
                <p>每个账号有独立的登录凭据与会话历史，互不影响。切换后配额读数、登录状态立刻跟着走。</p>
                <div className="acct-rows">
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#d97757" }}>工</span>
                    <span className="aname">Claude · 工作</span>
                    <span className="abadge on">使用中</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#5b8db8" }}>个</span>
                    <span className="aname">Claude · 个人</span>
                    <span className="abadge off">切换到此账号</span>
                  </div>
                </div>
              </div>
            </Reveal>
            <Reveal>
              <div className="acct-card">
                <h3>API 中转</h3>
                <p>为模型填入中转地址、模型名与密钥即可启用；可从推荐项选择，也能填中转商提供的任意模型 ID。</p>
                <div className="acct-rows">
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#7a5bb8" }}>↳</span>
                    <span className="aname">Opus · 7 天</span>
                    <span className="abadge relay">中转</span>
                  </div>
                  <div className="acct-row">
                    <span className="avatar" style={{ background: "#0f9e78" }}>官</span>
                    <span className="aname">Sonnet · 官方账号</span>
                    <span className="abadge on">官方</span>
                  </div>
                </div>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      {/* 代理 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">网络与代理</span>
            <h2 className="h1">每个 AI 工具，走适合自己的网络路径</h2>
            <p className="lead">
              一份默认规则覆盖日常使用，需要时再按 AI 工具单独设置。Meowo 发起的用量查询、CLI 安装和新会话会复用对应配置。
            </p>
          </div>
          <div className="grid grid-3">
            <Reveal>
              <div className="fcard">
                <h3>全局默认</h3>
                <p>选择直连、跟随系统环境变量或自定义代理，未单独设置的 AI 工具自动跟随。</p>
              </div>
            </Reveal>
            <Reveal>
              <div className="fcard">
                <h3>按工具覆盖</h3>
                <p>不同 AI 工具可以使用不同代理，也可以让其中一部分保持直连，互不影响。</p>
              </div>
            </Reveal>
            <Reveal>
              <div className="fcard">
                <h3>常见格式直接填写</h3>
                <p>
                  支持 HTTP、SOCKS5 及带认证的代理，包括{" "}
                  <code className="inline">host:port:user:pass</code>。工具不支持某种协议时会明确提示。
                </p>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      {/* 主题与配色 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">外观</span>
            <h2 className="h1">多种风格与配色，随手切换</h2>
            <p className="lead">7 种贴纸配色、扁平与立体两种风格、深浅主题——点点下面这块试试。</p>
          </div>
          <Reveal>
            <ThemeShowcase />
          </Reveal>
          <p className="faint" style={{ textAlign: "center", marginTop: 22, fontSize: 13.5 }}>
            另外还能调整不透明度（配合系统毛玻璃透出桌面）与界面密度。
          </p>
        </div>
      </section>

      {/* 平台差异 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">平台</span>
            <h2 className="h1">Windows 和 macOS 的形态不同</h2>
          </div>
          <div className="grid grid-2">
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>Windows · 桌面贴纸</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>拖到屏幕左 / 右 / 顶边会缩成一条红绿灯，鼠标悬停展开</Check>
                  <Check>可以置顶；重启后沿用上次的窗口位置和吸附边</Check>
                  <Check>鼠标停在托盘图标上，能看到待交互和运行中的会话数</Check>
                </ul>
              </div>
            </Reveal>
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>macOS · 菜单栏面板</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>左键点图标弹出原生面板，失焦自动收起，不占 Dock</Check>
                  <Check>菜单栏图标上显示运行中和待交互的会话数</Check>
                  <Check>universal 包，已签名公证，双击打开</Check>
                </ul>
              </div>
            </Reveal>
          </div>
        </div>
      </section>

      <CtaBand />
    </main>
  );
}
