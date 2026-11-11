// 源码扫描：程序化吸底一律走 stickToBottom（UI 优化 c1）。`.chat-scroll` 是 scroll-behavior:smooth，
// 裸写 scrollTop = scrollHeight 会启动一段平滑动画，动画途中的 scroll 事件把 followRef 判成脱底，
// 长会话打开就停在半截（s3/s4 实测 1440×800 停在 437/1395）。U1-6 统一过一次，审批卡留白观察者与
// 尺寸观察者两处漏网，这里钉住，别再长出第三处。
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const FILES = ["../ChatWindow.tsx", "./useStickToBottom.ts"];
// 只抓「钉到底」这一种写法；loadEarlier 的保位（prevTop + 高度差）已自带瞬移，不在此列。
const BARE = /\.scrollTop\s*=\s*[\w.]*\.scrollHeight\s*;/;

describe("程序化吸底不许裸写 scrollTop = scrollHeight", () => {
  for (const file of FILES) {
    it(file, () => {
      const source = readFileSync(resolve(__dirname, file), "utf8");
      // stickToBottom 自己是唯一合法的写法：剔掉它的函数体再扫。
      const stripped = source.replace(/export function stickToBottom\([^)]*\)\s*\{[\s\S]*?\n\}/, "");
      const hits = stripped.split("\n").map((line, index) => ({ line: line.trim(), no: index + 1 }))
        .filter(({ line }) => !line.startsWith("//") && BARE.test(line));
      expect(hits).toEqual([]);
    });
  }
});
