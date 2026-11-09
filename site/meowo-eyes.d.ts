// <meowo-eyes> 自定义元素的 JSX 类型（实现见 app/src/demo/mascot.tsx）。
import type { CSSProperties } from "react";

declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      "meowo-eyes": {
        class?: string;
        tone?: string;
        cue?: string;
        quota?: string;
        busy?: string;
        size?: string;
        style?: CSSProperties;
        "aria-hidden"?: "true";
      };
    }
  }
}
