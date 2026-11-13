import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { ENTRIES } from "./errors";

/// sentinel 与后端文案的对账测试:errors.ts 文件头警告「改 Rust 文案必须同步改这里」,
/// 反方向同样会漂移——后端删了文案、这边的条目就成了永远命中不了的死条目(实際发生过:
/// 「后台会话没有回执」)。每条匹配串必须能在 Rust 源码或前端 i18n 文案(zh.ts,如
/// terminalStartExited 这类前端自产的同前缀消息)里找到字面出处。

const here = dirname(fileURLToPath(import.meta.url));

function collectRs(dir: string, out: string[]) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "target" || entry.name === "node_modules") continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) collectRs(path, out);
    else if (entry.name.endsWith(".rs")) out.push(path);
  }
}

describe("errors.ts sentinel 对账", () => {
  it("每条匹配串都在 Rust 源码或 zh.ts 里有字面出处", () => {
    const files: string[] = [];
    collectRs(join(here, "../../src-tauri/src"), files);
    collectRs(join(here, "../../src-tauri/crates"), files);
    const corpus =
      files.map((file) => readFileSync(file, "utf8")).join("\n") +
      readFileSync(join(here, "zh.ts"), "utf8") +
      // 远程桥的 `remote/<code>` sentinel 由前端传输层自产（7M-7），出处在这里。
      readFileSync(join(here, "../mobile/transport.ts"), "utf8") +
      // 原生composer保护在写入前由前端判定，仍复用结构化reason与双语映射。
      readFileSync(join(here, "../views/ChatWindow.tsx"), "utf8") +
      // 发送回执的前端失败也有独立来源，不能只扫描对话窗入口。
      readFileSync(join(here, "../views/chat/drafts.ts"), "utf8");
    const missing = ENTRIES.filter((entry) => !corpus.includes(entry.m)).map((entry) => entry.m);
    expect(missing).toEqual([]);
  });
});
