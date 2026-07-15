"use client";

// 多风格 + 多配色实时切换的可交互展示。
// 直接复用真实贴纸组件 StickerWindow，配色与 app/src/appearance.ts 的 STICKER_COLORS 一致。

import { useState } from "react";
import { StickerWindow } from "./screenshots";

type Preset = { key: string; name: string; swatch: string; dark: string; light: string };

const COLORS: Preset[] = [
  { key: "neutral", name: "无色", swatch: "#9aa0a6", dark: "33, 33, 35", light: "247, 247, 249" },
  { key: "classic", name: "经典", swatch: "#d97757", dark: "38, 38, 36", light: "250, 249, 245" },
  { key: "slate", name: "石青", swatch: "#5b8db8", dark: "29, 37, 47", light: "239, 244, 250" },
  { key: "moss", name: "苔绿", swatch: "#6fae6a", dark: "30, 41, 33", light: "239, 248, 240" },
  { key: "plum", name: "暮紫", swatch: "#a87cc8", dark: "43, 34, 48", light: "248, 242, 251" },
  { key: "rose", name: "玫粉", swatch: "#d7748f", dark: "47, 34, 39", light: "251, 241, 244" },
  { key: "amber", name: "琥珀", swatch: "#d9a441", dark: "46, 40, 27", light: "251, 246, 232" },
];

type Theme = "dark" | "light";
type Style = "emboss" | "flat";

export default function ThemeShowcase() {
  const [color, setColor] = useState("classic");
  const [theme, setTheme] = useState<Theme>("dark");
  const [style, setStyle] = useState<Style>("flat");

  const preset = COLORS.find((c) => c.key === color) ?? COLORS[0];
  const rgb = theme === "dark" ? preset.dark : preset.light;

  return (
    <div className="theme-showcase">
      <div className="ts-controls">
        <div className="ts-group">
          <span className="ts-label">配色</span>
          <div className="ts-swatches">
            {COLORS.map((c) => (
              <button
                key={c.key}
                type="button"
                className={`ts-swatch ${color === c.key ? "on" : ""}`}
                style={{ background: c.swatch }}
                onClick={() => setColor(c.key)}
                aria-label={c.name}
                title={c.name}
              />
            ))}
          </div>
        </div>

        <div className="ts-group">
          <span className="ts-label">风格</span>
          <div className="ts-seg">
            <button type="button" className={style === "flat" ? "on" : ""} onClick={() => setStyle("flat")}>
              扁平
            </button>
            <button type="button" className={style === "emboss" ? "on" : ""} onClick={() => setStyle("emboss")}>
              立体
            </button>
          </div>
        </div>

        <div className="ts-group">
          <span className="ts-label">明暗</span>
          <div className="ts-seg">
            <button type="button" className={theme === "dark" ? "on" : ""} onClick={() => setTheme("dark")}>
              深色
            </button>
            <button type="button" className={theme === "light" ? "on" : ""} onClick={() => setTheme("light")}>
              浅色
            </button>
          </div>
        </div>

        <p className="ts-hint">7 种配色 · 扁平 / 立体 · 深 / 浅 · 还能调透明度与界面密度，随手换一套。</p>
      </div>

      <div className="ts-stage">
        <StickerWindow
          activeTab="all"
          theme={theme}
          bgRgb={rgb}
          flat={style === "flat"}
          cards={[
            {
              title: "重构吸边状态机",
              repo: "meowo",
              provider: "claude",
              state: "running",
              pct: 62,
              aiText: "把状态机拆成 3 个纯函数，正在补吸附边界单测。",
              time: "刚刚",
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
          ]}
        />
      </div>
    </div>
  );
}
