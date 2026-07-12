import type { Metadata } from "next";
import FeatureGrid from "@/components/FeatureGrid";
import Reveal from "@/components/Reveal";
import CtaBand from "@/components/CtaBand";
import { CheckIcon } from "@/components/icons";
import { StickerWindow, CollapsedStrip } from "@/components/screenshots";

export const metadata: Metadata = {
  title: "功能 · Meowo",
  description:
    "Meowo 的功能：会话看板、待交互与通知、终端跳转、会话管理、Windows 吸边缩略、macOS 菜单栏面板、用量读数。",
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
          <h1 className="h1">功能</h1>
          <p className="lead">
            Meowo 收集 Claude Code、Codex、Kimi 的会话事件，在一个桌面小窗里显示它们的状态。
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
            <h2 className="h1">功能列表</h2>
          </div>
          <FeatureGrid />
        </div>
      </section>

      {/* 深入 1：看板 & 提醒 */}
      <section className="section section-sunken">
        <div className="container">
          <div className="split">
            <div className="split-text">
              <span className="eyebrow">看板与通知</span>
              <h2 className="h2">等待输入的会话排在最前</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                会话按状态分成几个 tab。「待交互」里的会话按等待时长排序，等得最久的在最上面。需要回复或出错时，可以弹一条系统通知。
              </p>
              <ul className="checklist">
                <Check>四个 tab：全部 / 待交互 / 运行中 / 已归档，各自带数量</Check>
                <Check>「待交互」内部按等待时长排序，等得最久的排最前</Check>
                <Check>系统通知会去重，同一件事只弹一次；点击后切到该会话</Check>
                <Check>卡片上直接显示最近一条 AI 输出，不用展开</Check>
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
              <span className="eyebrow">终端跳转与会话管理</span>
              <h2 className="h2">点击卡片，切到终端</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                连接中的会话切到它所在的标签页；已断开的会话，在原项目目录新开终端恢复对话。星标、便签、改名、归档在右键菜单或 ⋮ 按钮里。
              </p>
              <ul className="checklist">
                <Check>
                  Windows 上切到 Windows Terminal 的对应标签页，macOS 上聚焦 Terminal 或 iTerm2
                </Check>
                <Check>
                  会话已断开时，在原项目目录新开终端并执行{" "}
                  <code className="inline">claude --resume</code>
                </Check>
                <Check>加星、写便签、改名、归档：右键卡片，或点卡片右上角的 ⋮</Check>
                <Check>
                  改名与 <code className="inline">/rename</code> 等效，resume 列表里显示的也是新名字
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
              <span className="eyebrow">窗口行为</span>
              <h2 className="h2">吸边、置顶与搜索</h2>
              <p className="lead" style={{ fontSize: 17, marginTop: 14 }}>
                Windows 上把窗口拖到屏幕边缘，它会缩成一根细条，鼠标悬停时展开。窗口可以置顶。底栏有搜索框和配额读数。
              </p>
              <ul className="checklist">
                <Check>拖到屏幕左边、右边或顶边松手，窗口缩成一根状态条</Check>
                <Check>点 pin 之后，窗口保持在最上层</Check>
                <Check>底栏的放大镜按标题或仓库名过滤会话</Check>
                <Check>底栏显示 5 小时 / 7 天配额的使用比例，越接近上限颜色越偏红</Check>
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
            <span className="eyebrow">平台</span>
            <h2 className="h1">Windows 和 macOS 的形态不同</h2>
          </div>
          <div className="grid grid-2">
            <Reveal>
              <div className="fcard">
                <h3 style={{ fontSize: 19 }}>Windows · 桌面贴纸</h3>
                <ul className="checklist" style={{ marginTop: 16 }}>
                  <Check>拖到屏幕左 / 右 / 顶边会缩成一根条，鼠标悬停展开</Check>
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
