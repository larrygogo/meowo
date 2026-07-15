"use client";

// Hero 演示 iframe 包装：iframe 内部固定 880×600（demo 的设计尺寸，光标定位依赖它），
// 外层按容器宽度整体缩放，保证任意屏宽都完整可见、比例不变、光标定位准确。

import { useEffect, useRef } from "react";
import type { Lang } from "@/lib/i18n";

const DW = 880;

export default function DemoFrame({ lang }: { lang: Lang }) {
  const wrap = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);

  useEffect(() => {
    const w = wrap.current;
    const f = frame.current;
    if (!w || !f) return;
    const apply = () => {
      f.style.transform = `scale(${w.clientWidth / DW})`;
    };
    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(w);
    return () => ro.disconnect();
  }, []);

  return (
    <div className="hero-demo-wrap" ref={wrap}>
      <iframe
        ref={frame}
        className="hero-demo-frame"
        src={`/demo/embed-demo.html?lang=${lang}`}
        title="Meowo demo"
        loading="lazy"
        scrolling="no"
        tabIndex={-1}
        aria-hidden="true"
      />
    </div>
  );
}
