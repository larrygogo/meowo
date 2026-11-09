"use client";

// 演示 iframe 包装：iframe 内部是固定的设计尺寸（灵动岛 880×600，对话窗 640×780，窄于 880 时侧栏自动收起，字更大；
// 右侧面板 1180×760，对话旁开着改动页签；手机页 390×780，手机远程页的真实布局），
// 外层按容器宽度整体缩放，保证任意屏宽都完整可见、比例不变、光标定位准确。

import { useEffect, useRef } from "react";
import type { Lang } from "@/lib/i18n";

/// 各段演示的设计尺寸。
const SIZE = { island: [880, 600], demo: [880, 600], chat: [640, 780], panel: [1180, 760], mobile: [390, 780] } as const;

/// page：嵌哪段演示。island = 灵动岛（首页与功能页顶部）；chat = 对话窗口（真实 ChatWindow +
/// 假会话）；panel = 对话窗 + 右侧改动面板；mobile = 手机远程页；demo = 早期演示（官网已不引用）。
/// scene：交给演示页的场景参数（目前只有灵动岛的 "alert"：定格在审批提醒，首页时间线用）。
export default function DemoFrame({ lang, page = "island", scene }: { lang: Lang; page?: keyof typeof SIZE; scene?: string }) {
  const [dw, dh] = SIZE[page];
  const wrap = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    const w = wrap.current;
    const f = frame.current;
    if (!w || !f) return;
    const apply = () => {
      f.style.transform = `scale(${w.clientWidth / dw})`;
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(w);
    return () => ro.disconnect();
  }, [dw]);

  return (
    <div className="hero-demo-wrap" ref={wrap} style={{ maxWidth: dw, aspectRatio: `${dw} / ${dh}` }}>
      <iframe
        ref={frame}
        className="hero-demo-frame"
        style={{ width: dw, height: dh }}
        src={`/demo/embed-${page}.html?lang=${lang}${scene ? `&scene=${scene}` : ""}`}
        title="Meowo demo"
        loading="lazy"
        scrolling="no"
        tabIndex={-1}
        aria-hidden="true"
      />
    </div>
  );
}
