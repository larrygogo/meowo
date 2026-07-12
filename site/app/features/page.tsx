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
          <h1 className="h1">它都能干什么</h1>
          <p className="lead">
            一张小贴纸，把散在各个终端里的 Claude Code、Codex、Kimi 会话收到一起。
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
        <p className="showcase-cap">贴纸大概长这样</p>
      </section>

      <section className="section">
        <div className="container">
          <div className="section-head">
            <span className="eyebrow">概览</span>
            <h2 className="h1">大致是这些</h2>
          </div>
          <FeatureGrid />
        </div>
      </section>

      {/* 深入 1：看板 & 提醒 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">看板与提醒</span>
              <h2 className="h2">等你回复的，排最前面</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                会话按状态分了几个 tab。进到「待交互」里的，等得越久越靠前——被晾了 20 分钟的那个，不用你去翻。
              </p>
              <ul className="checklist">
                <Check>
                  全部 / 待交互 / 运行中 / 已归档四个 tab，每个都带计数
                </Check>
                <Check>
                  需要回复或者出错时弹一条系统通知，点通知直接切到那个会话
                </Check>
                <Check>
                  同一件事只通知一次，不会反复弹
                </Check>
                <Check>
                  卡片上带 AI 最近说的那句话，不用点开也知道进展
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
              <span className="eyebrow">跳转与整理</span>
              <h2 className="h2">点卡片，回到终端</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                不是打开一个新窗口给你看，而是把你送回会话原来待的地方。整理的事——星标、便签、改名、归档——在右键菜单和 ⋮ 里。
              </p>
              <ul className="checklist">
                <Check>
                  Windows 上精确切到 Windows Terminal 的那个标签，macOS 上聚焦 iTerm2 或 Terminal
                </Check>
                <Check>
                  断开的会话，在原项目目录新开终端跑{" "}
                  <code className="inline">claude --resume</code> 接上
                </Check>
                <Check>
                  改名写的是和 <code className="inline">/rename</code>{" "}
                  一样的记录，所以 resume 列表里也是新名字
                </Check>
                <Check>
                  便签只存在本地，跟会话内容没关系，纯粹是写给你自己看的
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
              <span className="eyebrow">不挡路</span>
              <h2 className="h2">大部分时候，它只是一根条</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                Windows 上把窗口拖到屏幕边缘松手，它就缩成一根细条，只剩几个状态色点。鼠标碰一下才展开，移开又收回去。
              </p>
              <ul className="checklist">
                <Check>左、右、顶三条边都能吸；拖离边缘就变回普通窗口</Check>
                <Check>pin 一下可以让它一直浮在最上层</Check>
                <Check>会话多了，底栏放大镜按标题或仓库名过滤</Check>
                <Check>底栏那两个数字是 5 小时和 7 天的配额用量，越满越红</Check>
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
            <h2 className="h1">两个系统上不太一样</h2>
          </div>
          <div className="grid grid-2">
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>Windows · 桌面贴纸</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>一个独立小窗口，可以吸到屏幕边上缩成细条</Check>
                  <Check>关掉再开，位置、尺寸、吸在哪条边都还在</Check>
                  <Check>鼠标悬在托盘图标上，能看到待交互和运行中的数量</Check>
                </ul>
              </div>
            </Reveal>
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>macOS · 菜单栏面板</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>没有浮窗，也不占 Dock。点菜单栏图标弹出面板，失焦自动收起</Check>
                  <Check>图标上直接带着运行中和待交互的数字</Check>
                  <Check>universal 包，签过名做过公证，双击就能开</Check>
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
