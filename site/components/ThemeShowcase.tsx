"use client";

// 多风格 + 多配色实时切换的可交互展示。
// 直接复用真实贴纸组件 StickerWindow，配色与 app/src/appearance.ts 的 STICKER_COLORS 一致。

import { useState } from "react";
import { StickerWindow } from "./screenshots";
import { getDict, type Lang } from "@/lib/i18n";

type Preset = { key: string; swatch: string; dark: string; light: string };

const COLORS: Preset[] = [
  { key: "neutral", swatch: "#9aa0a6", dark: "33, 33, 35", light: "247, 247, 249" },
  { key: "classic", swatch: "#d97757", dark: "38, 38, 36", light: "250, 249, 245" },
  { key: "slate", swatch: "#5b8db8", dark: "29, 37, 47", light: "239, 244, 250" },
  { key: "moss", swatch: "#6fae6a", dark: "30, 41, 33", light: "239, 248, 240" },
  { key: "plum", swatch: "#a87cc8", dark: "43, 34, 48", light: "248, 242, 251" },
  { key: "rose", swatch: "#d7748f", dark: "47, 34, 39", light: "251, 241, 244" },
  { key: "amber", swatch: "#d9a441", dark: "46, 40, 27", light: "251, 246, 232" },
];

const DEMO = {
  zh: [
    { title: "重构吸边状态机", repo: "meowo", ai: "把状态机拆成 3 个纯函数，正在补吸附边界单测。" },
    { title: "接入账号用量面板", repo: "autopilot", ai: "要应用这 3 处修改吗？(y/n)" },
  ],
  en: [
    { title: "Refactor edge-snap state machine", repo: "meowo", ai: "Split the state machine into 3 pure functions; adding boundary tests." },
    { title: "Wire up the usage panel", repo: "autopilot", ai: "Apply these 3 changes? (y/n)" },
  ],
};

type Theme = "dark" | "light";
type Style = "emboss" | "flat";

export default function ThemeShowcase({ lang = "zh" }: { lang?: Lang }) {
  const [color, setColor] = useState("classic");
  const [theme, setTheme] = useState<Theme>("dark");
  const [style, setStyle] = useState<Style>("flat");

  const d = getDict(lang).theme;
  const preset = COLORS.find((c) => c.key === color) ?? COLORS[0];
  const rgb = theme === "dark" ? preset.dark : preset.light;
  const demo = DEMO[lang];

  return (
    <div className="theme-showcase">
      <div className="ts-controls">
        <div className="ts-group">
          <span className="ts-label">{d.color}</span>
          <div className="ts-swatches">
            {COLORS.map((c) => (
              <button
                key={c.key}
                type="button"
                className={`ts-swatch ${color === c.key ? "on" : ""}`}
                style={{ background: c.swatch }}
                onClick={() => setColor(c.key)}
                aria-label={d.swatches[c.key]}
                title={d.swatches[c.key]}
              />
            ))}
          </div>
        </div>

        <div className="ts-group">
          <span className="ts-label">{d.style}</span>
          <div className="ts-seg">
            <button type="button" aria-pressed={style === "flat"} className={style === "flat" ? "on" : ""} onClick={() => setStyle("flat")}>
              {d.flat}
            </button>
            <button type="button" aria-pressed={style === "emboss"} className={style === "emboss" ? "on" : ""} onClick={() => setStyle("emboss")}>
              {d.emboss}
            </button>
          </div>
        </div>

        <div className="ts-group">
          <span className="ts-label">{d.theme}</span>
          <div className="ts-seg">
            <button type="button" aria-pressed={theme === "dark"} className={theme === "dark" ? "on" : ""} onClick={() => setTheme("dark")}>
              {d.dark}
            </button>
            <button type="button" aria-pressed={theme === "light"} className={theme === "light" ? "on" : ""} onClick={() => setTheme("light")}>
              {d.light}
            </button>
          </div>
        </div>

        <p className="ts-hint">{d.hint}</p>
      </div>

      <div className="ts-stage">
        <StickerWindow
          activeTab="all"
          lang={lang}
          theme={theme}
          bgRgb={rgb}
          flat={style === "flat"}
          cards={[
            { title: demo[0].title, repo: demo[0].repo, provider: "claude", state: "running", pct: 62, aiText: demo[0].ai, time: getDict(lang).sticker.justNow },
            { title: demo[1].title, repo: demo[1].repo, provider: "codex", state: "waiting", pct: 43, aiText: demo[1].ai, time: getDict(lang).sticker.justNow },
          ]}
        />
      </div>
    </div>
  );
}
