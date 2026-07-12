"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";

type RevealProps = {
  children: ReactNode;
  className?: string;
};

/** 滚动进入视口时淡入上移。IntersectionObserver 不可用时直接显示，绝不永久隐藏内容。 */
export default function Reveal({ children, className = "" }: RevealProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(false);

  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") {
      setShown(true);
      return;
    }
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (e.isIntersecting) {
            setShown(true);
            io.unobserve(e.target);
          }
        });
      },
      { threshold: 0.12 }
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <div ref={ref} className={`reveal${shown ? " in" : ""} ${className}`.trim()}>
      {children}
    </div>
  );
}
