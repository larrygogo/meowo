import Reveal from "./Reveal";
import { StickerWindow, CollapsedStrip } from "./screenshots";
import type { CardData } from "./screenshots/StickerWindow";
import { type Lang } from "@/lib/i18n";

type Props = {
  lang?: Lang;
  className?: string;
};

const CARDS: Record<Lang, CardData[]> = {
  zh: [
    { title: "重构吸边状态机", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。", time: "刚刚", model: "claude-opus-4" },
    { title: "接入账号用量面板", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "要应用这 3 处修改吗？(y/n)", time: "刚刚" },
    { title: "升级 tauri 到 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "已更新 Cargo.toml，等你确认几处 breaking change。", time: "12 分钟前" },
    { title: "修复 statusline 兼容性", repo: "clawmo-ios", provider: "claude", state: "stopped", aiText: "兼容性修好并已合并，收工。", time: "3 小时前" },
  ],
  en: [
    { title: "Refactor edge-snap state machine", repo: "meowo", provider: "claude", state: "running", pct: 62, aiText: "Split the state machine into 3 pure functions; adding boundary tests.", time: "just now", model: "claude-opus-4" },
    { title: "Wire up the usage panel", repo: "autopilot", provider: "codex", state: "waiting", pct: 43, aiText: "Apply these 3 changes? (y/n)", time: "just now" },
    { title: "Bump tauri to 2.3", repo: "cc-relay", provider: "kimi", state: "idle", aiText: "Updated Cargo.toml; a few breaking changes to confirm.", time: "12 min ago" },
    { title: "Fix statusline compatibility", repo: "clawmo-ios", provider: "claude", state: "stopped", aiText: "Compatibility fixed and merged. Done.", time: "3 hr ago" },
  ],
};

/** 深色「桌面」窗口壳里放真实贴纸组件（可多语言）+ 吸在右边缘的红绿灯，替代原静态截图。 */
export default function ProductShowcase({ lang = "zh", className = "" }: Props) {
  return (
    <div className={`showcase ${className}`.trim()}>
      <Reveal>
        <div className="window">
          <div className="window-bar">
            <span className="tl r" />
            <span className="tl y" />
            <span className="tl g" />
          </div>
          <div className="window-body hero-desktop">
            <div className="hero-desktop-glow" />
            <StickerWindow lang={lang} activeTab="all" cards={CARDS[lang]} />
            <CollapsedStrip edge="right" className="hero-desktop-strip" />
          </div>
        </div>
      </Reveal>
    </div>
  );
}
