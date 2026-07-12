// 吸边缩略条示意组件

import React from "react";

type Props = {
  edge?: "left" | "right";
  className?: string;
  style?: React.CSSProperties;
};

const DOTS = ["running", "waiting", "on"] as const;

export default function CollapsedStrip({ edge = "right", className = "", style }: Props) {
  const isRight = edge === "right";
  return (
    <div
      className={`cstrip-mock ${className}`}
      style={{
        width: 32,
        height: 160,
        background: "rgba(33,33,35,0.95)",
        border: "1px solid rgba(255,255,255,0.12)",
        borderRadius: isRight ? "10px 0 0 10px" : "0 10px 10px 0",
        borderRightColor: isRight ? "rgba(255,255,255,0.2)" : undefined,
        borderLeftColor: isRight ? undefined : "rgba(255,255,255,0.2)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        boxShadow: "none",
        ...style,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 7, padding: "4px 0" }}>
        {DOTS.map((s, i) => (
          <span
            key={i}
            style={{
              width: 10,
              height: 10,
              borderRadius: "50%",
              background:
                s === "running" ? "#4ec9a5" : s === "waiting" ? "#e0a23c" : "#4ec9a5",
              boxShadow: "0 0 0 0.5px rgba(0,0,0,0.25)",
              animation: s === "running" ? "cstrip-pulse 1.2s ease-in-out infinite" : undefined,
            }}
          />
        ))}
      </div>
    </div>
  );
}
