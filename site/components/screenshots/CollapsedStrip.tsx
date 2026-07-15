// 吸边缩略条示意组件——收起后的「电子红绿灯」：一列彩色圆点，一眼看清各会话状态。

import React from "react";

type Dot = { color: string; pulse?: boolean };

// 红 / 黄 / 绿，像一枚竖排的电子红绿灯：报错、待交互、运行中。
const DOTS: Dot[] = [
  { color: "#e0584c" },
  { color: "#e0a23c" },
  { color: "#4ec9a5", pulse: true },
];

type Props = {
  edge?: "left" | "right";
  className?: string;
  style?: React.CSSProperties;
};

export default function CollapsedStrip({ edge = "right", className = "", style }: Props) {
  const isRight = edge === "right";
  return (
    <div
      className={`cstrip-mock ${className}`}
      style={{
        width: 30,
        height: 150,
        background: "rgba(33,33,35,0.96)",
        border: "1px solid rgba(255,255,255,0.12)",
        borderRadius: isRight ? "12px 0 0 12px" : "0 12px 12px 0",
        borderRightColor: isRight ? "rgba(255,255,255,0.2)" : undefined,
        borderLeftColor: isRight ? undefined : "rgba(255,255,255,0.2)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        boxShadow: "0 12px 32px rgba(0,0,0,0.32)",
        ...style,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 9, padding: "4px 0" }}>
        {DOTS.map((d, i) => (
          <span
            key={i}
            style={{
              width: 11,
              height: 11,
              borderRadius: "50%",
              background: d.color,
              boxShadow: `0 0 0 3px ${d.color}26`,
              animation: d.pulse ? "cstrip-pulse 1.4s ease-in-out infinite" : undefined,
            }}
          />
        ))}
      </div>
    </div>
  );
}
