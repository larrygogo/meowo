// 源码里不许出现裸控制字符（\t \r \n 以外）：ChatWindow.tsx 注释里曾混进一个裸 0x02，
// 多个测试/演示数据里 ESC 直接以原始字节写在字符串里，编辑器里看不见、diff 里显示成乱码，
// 复制粘贴还会丢（做空审查 UI 优化 fix1）。要用就写 \x1b / \x02 这类转义。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { expect, it } from "vitest";

const ROOT = join(__dirname);
const files: string[] = [];
const walk = (dir: string) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) { if (name !== "generated") walk(path); }
    else if (/\.(tsx?|css)$/.test(name)) files.push(path);
  }
};
walk(ROOT);

it("app/src 下 ts/tsx/css 不含裸控制字符", () => {
  // eslint-disable-next-line no-control-regex
  const bad = /[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/;
  const hits = files.filter((file) => bad.test(readFileSync(file, "utf8"))).map((file) => file.slice(ROOT.length + 1));
  expect(files.length).toBeGreaterThan(100);
  expect(hits).toEqual([]);
});
