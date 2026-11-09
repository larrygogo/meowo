// 官网吉祥物：应用里真实的灵动岛眼睛（app/src/demo/mascot.tsx 打包成 <meowo-eyes>，
// 由两个根 layout 用 <script type="module" src="/mascot/meowo-eyes.js"> 加载）。
// 脚本到之前元素是空的，占位尺寸先按 size 撑开，避免加载完跳一下。
import type { CSSProperties } from "react";

export type EyesTone = "running" | "pending" | "waiting" | "error" | "idle";
export type EyesCue = "star" | "surprised" | "blush" | "wink";

export default function Mascot({
  tone = "running",
  cue,
  quota,
  busy = false,
  size = 120,
  className,
}: {
  tone?: EyesTone;
  cue?: EyesCue;
  /** 额度已用百分比：≥85 发愁，100 转蚊香圈。 */
  quota?: number;
  /** 同时在跑的会话多：专注脸。 */
  busy?: boolean;
  size?: number;
  className?: string;
}) {
  // 眼睛的天然比例：宽 38、高 16（见 mascot.tsx 的 BASE_WIDTH / BASE_HEIGHT）。
  const style: CSSProperties = { display: "inline-block", width: size, height: (size * 16) / 38 };
  return (
    <meowo-eyes
      class={className}
      tone={tone}
      cue={cue}
      quota={quota == null ? undefined : String(quota)}
      busy={busy ? "" : undefined}
      size={String(size)}
      style={style}
      aria-hidden="true"
    />
  );
}
