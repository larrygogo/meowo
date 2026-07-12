import type { Metadata } from "next";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import { CheckIcon } from "@/components/icons";
import { StickerWindow, CollapsedStrip } from "@/components/screenshots";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 能做什么：实时会话看板、待交互提醒、点击跳转终端、卡片管理、Windows 吸边缩略、macOS 菜单栏面板、账号用量。",
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
          <h1 className="h1">功能一览</h1>
          <p className="lead">
            用一张小贴纸，把 Claude Code、Codex、Kimi 的会话状态集中起来。
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
        <p className="showcase-cap">真实界面：会话卡片、状态分类 tab、底栏用量读数</p>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">全部特性</span>
            <h2 className="h1">六个小功能</h2>
          </div>
          <FeatureGrid />
        </div>
      </section>

      {/* 深入 1：看板 & 提醒 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">实时看板 & 提醒</span>
              <h2 className="h2">等你回复的，排最前面</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                按状态分 tab，「待交互」里的会话按等待时间排序。需要回复或出错时，会弹一条系统通知。
              </p>
              <ul className="checklist">
                <Check>
                  <b>状态分类 tab</b>：全部 / 待交互 / 运行中 / 已归档，各带计数
                </Check>
                <Check>
                  <b>等待最久优先</b>：「待交互」内按被晾时长排序
                </Check>
                <Check>
                  <b>去重系统通知</b>：需要回复或出错时弹一次，点击直接切到该会话
                </Check>
                <Check>
                  <b>最近一条 AI 正文</b>：不展开也知道它刚说了什么
                </Check>
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

      {/* 深入 2：直达终端 & 管理 */}
      <section className="section">
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
              <span className="eyebrow">直达终端 & 会话管理</span>
              <h2 className="h2">点卡片，回到终端</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                连接中的会话直接跳到对应标签页；断开的在原目录新开终端恢复对话。右键或点 ⋮ 按钮管理星标、便签、改名、归档。
              </p>
              <ul className="checklist">
                <Check>
                  <b>精确聚焦</b>：Windows 切到 Windows Terminal 对应标签，macOS 聚焦 iTerm2 / Terminal
                </Check>
                <Check>
                  <b>断线续聊</b>：在原项目目录新开终端并 <code className="inline">claude --resume</code>
                </Check>
                <Check>
                  <b>星标 / 便签 / 改名 / 归档</b>：右键卡片或点 ⋮ 按钮
                </Check>
                <Check>
                  <b>改名同步</b>：与 <code className="inline">/rename</code> 一致，resume 列表同步显示新名字
                </Check>
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* 吸边 & 用量 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">不挡工作流</span>
              <h2 className="h2">吸边、置顶、搜索、用量</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                Windows 上拖到屏幕边缘，贴纸会缩成一根细条，鼠标悬停时展开。可以 pin 置顶，也能按标题或仓库名搜索会话。底栏显示 5 小时 / 7 天配额利用率。
              </p>
              <ul className="checklist">
                <Check>
                  <b>吸边缩略</b>：拖到左 / 右 / 顶边，松手缩成状态条
                </Check>
                <Check>
                  <b>窗口置顶</b>：pin 后贴纸始终浮在最上层
                </Check>
                <Check>
                  <b>会话搜索</b>：底栏放大镜，按标题 / 仓库名即时过滤
                </Check>
                <Check>
                  <b>用量读数</b>：5 小时 / 7 天配额，越满越偏红
                </Check>
              </ul>
            </div>
            <div className="split-media" style={{ padding: "40px 0 40px 40px", display: "flex", justifyContent: "center", alignItems: "center", position: "relative", minHeight: 320 }}>
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
                style={{ width: 320, transform: "translateX(-24px)" }}
              />
              <CollapsedStrip
                edge="right"
                style={{
                  position: "absolute",
                  right: 0,
                  top: "50%",
                  transform: "translateY(-50%)",
                  zIndex: 1,
                }}
              />
            </div>
          </div>
        </div>
      </section>

      {/* 平台差异 */}
      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">平台差异</span>
            <h2 className="h1">Windows 和 macOS 各有形态</h2>
          </div>
          <div className="grid grid-2">
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>Windows · 桌面贴纸</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>吸边缩略：拖到左 / 右 / 顶边即缩成一根条，悬停展开</Check>
                  <Check>可 pin 置顶，重启沿用上次的窗口位置与吸附边</Check>
                  <Check>托盘悬停即见待交互 / 运行中会话数</Check>
                </ul>
              </div>
            </Reveal>
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>macOS · 菜单栏面板</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>左键弹出原生毛玻璃面板，失焦自动收起，不占 Dock</Check>
                  <Check>菜单栏图标实时显示运行中与待交互计数</Check>
                  <Check>universal 包，已签名公证，双击直接打开</Check>
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
