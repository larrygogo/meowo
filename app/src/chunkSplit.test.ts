import { describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, resolve } from "node:path";

const SRC = dirname(fileURLToPath(import.meta.url));

/**
 * 拆 chunk 的守卫：沿**静态** import 从入口走一遍，懒加载的大块不许被重新拖进来。
 *
 * 这类回潮没有失败信号：不报错、功能也照常，只是 vite 构建多一条「is dynamically imported
 * by … but also statically imported by …」警告，体积悄悄涨回去（2026-11 实测：一个
 * `import { localUrlsIn } from "./chat/BrowserPane"` 就把 GitDiffView 连同 rehype-raw
 * 拖进对话窗主 chunk，hljs 24 种语言也在里面，合计约 350KB）。
 *
 * 只认 `import … from` / `export … from` / `import "…"`；`import type`、`export type` 编译后
 * 消失，不算。动态 `import()` 不算——它就是拆分点。
 */
const STATIC_IMPORT = /(?:^|\n)\s*(import|export)\s+(type\s+)?(?:[^;'"]*?\s+from\s+)?["']([^"']+)["']/g;

function resolveLocal(from: string, spec: string): string | null {
  if (!spec.startsWith(".")) return null;
  const base = resolve(dirname(from), spec);
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, join(base, "index.ts"), join(base, "index.tsx")]) {
    if (/\.(ts|tsx)$/.test(candidate) && existsSync(candidate)) return candidate;
  }
  return null;
}

/** 从 entry 出发静态可达的本地模块与 npm 包名。 */
function staticGraph(entry: string): { files: Set<string>; packages: Set<string> } {
  const files = new Set<string>();
  const packages = new Set<string>();
  const queue = [entry];
  while (queue.length > 0) {
    const file = queue.pop()!;
    if (files.has(file)) continue;
    files.add(file);
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(STATIC_IMPORT)) {
      if (match[2]) continue;
      const spec = match[3];
      const local = resolveLocal(file, spec);
      if (local) queue.push(local);
      else if (!spec.startsWith(".")) packages.add(spec);
    }
  }
  return { files, packages };
}

const rel = (files: Set<string>) => [...files].map((file) => relative(SRC, file).replace(/\\/g, "/"));

describe("拆 chunk 守卫", () => {
  it("对话窗主 chunk 不静态带改动面板与 hljs", () => {
    const { files, packages } = staticGraph(join(SRC, "views/ChatWindow.tsx"));
    const reached = rel(files);
    // 先确认确实走到了：图走空了下面的「不包含」就全成了空话。
    expect(reached).toContain("views/ChatMarkdown.tsx");
    expect(reached).toContain("views/chat/highlight.ts");
    expect(reached).not.toContain("views/chat/GitDiffView.tsx");
    expect(reached).not.toContain("views/chat/FileMarkdown.tsx");
    expect(reached).not.toContain("views/chat/highlightEngine.ts");
    expect([...packages].filter((name) => name.startsWith("highlight.js") || name === "rehype-raw")).toEqual([]);
  });

  it("各窗口入口不静态带非基准语言的字典", () => {
    for (const entry of ["main.tsx", "mobile/main.tsx"]) {
      const reached = rel(staticGraph(join(SRC, entry)).files);
      expect(reached, entry).toContain("i18n/zh.ts");
      expect(reached, entry).not.toContain("i18n/en.ts");
    }
  });
});
