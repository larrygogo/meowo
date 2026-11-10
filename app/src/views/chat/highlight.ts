/**
 * 面板内代码高亮（文件预览 + diff 视图 + 对话里的代码块）。
 * 刻意不做 highlightAuto 自动探测——短行/片段上误判率高、颜色跳变比不高亮更糟。
 *
 * 渲染按行、高亮按篇：全文先整体交给 hljs（多行字符串/块注释/模板串的跨行状态
 * 得以保留），再把结果 HTML 按换行拆回逐行——断行处补闭合、下一行重开同名
 * span，每行 HTML 各自平衡，可独立塞进行容器（diff 行/带行号的代码行）。
 *
 * hljs 本体在 highlightEngine.ts，第一次真要上色时才动态加载（它占对话窗主 chunk 约三成，
 * 构建实测）。加载完成前 highlightCode / highlightLines 返回转义后的纯文本；调用方用
 * useHighlighter() 订阅，引擎到了就重渲染上色。纯文本与上色后字形、字号、行高完全一样，
 * 只是颜色变化，不会有布局跳动。
 */
import { useEffect, useSyncExternalStore } from "react";

type Engine = typeof import("./highlightEngine");

/** 扩展名（小写、不含点）→ 已注册的 hljs 语言名。 */
const EXT_TO_LANG: Record<string, string> = {
  ts: "typescript", tsx: "typescript", mts: "typescript", cts: "typescript",
  js: "javascript", jsx: "javascript", mjs: "javascript", cjs: "javascript",
  json: "json",
  sh: "bash", bash: "bash", zsh: "bash",
  py: "python",
  rs: "rust",
  go: "go",
  java: "java",
  c: "c", h: "c",
  cpp: "cpp", hpp: "cpp", cc: "cpp", cxx: "cpp", hh: "cpp",
  cs: "csharp",
  css: "css", scss: "css", less: "css",
  html: "xml", htm: "xml", xml: "xml", vue: "xml", svg: "xml",
  yml: "yaml", yaml: "yaml",
  md: "markdown", markdown: "markdown",
  toml: "ini", ini: "ini", cfg: "ini",
  sql: "sql",
  swift: "swift",
  kt: "kotlin", kts: "kotlin",
  rb: "ruby",
  php: "php",
  diff: "diff", patch: "diff",
  txt: "plaintext",
};

/** 已注册语言自带的别名（hljs 语言定义里的 aliases）。手抄一份是为了不加载引擎也能判断围栏标注认不认识；
 *  highlight.test.ts 加载引擎后逐项对账，语言表改了没同步会红。 */
export const LANGUAGE_ALIASES: Record<string, readonly string[]> = {
  bash: ["sh", "zsh"], c: ["h"], cpp: ["cc", "c++", "h++", "hpp", "hh", "hxx", "cxx"], csharp: ["cs", "c#"], css: [],
  diff: ["patch"], dockerfile: ["docker"], go: ["golang"], ini: ["toml"], java: ["jsp"], javascript: ["js", "jsx", "mjs", "cjs"],
  json: ["jsonc", "json5"], kotlin: ["kt", "kts", "ktm", "ktx"], markdown: ["md", "mkdown", "mkd"], php: [], plaintext: ["text", "txt"],
  python: ["py", "gyp", "ipython"], ruby: ["rb", "gemspec", "podspec", "thor", "irb"], rust: ["rs"], sql: [], swift: [],
  typescript: ["ts", "tsx", "mts", "cts"], xml: ["html", "xhtml", "rss", "atom", "xjb", "xsd", "xsl", "plist", "wsf", "svg"], yaml: ["yml"],
};
const ALIAS_TO_LANG = new Map<string, string>(
  Object.entries(LANGUAGE_ALIASES).flatMap(([name, aliases]) => [[name, name] as const, ...aliases.map((alias) => [alias, name] as const)]),
);

/** markdown 代码围栏的语言标注 → 已注册的 hljs 语言名。围栏里用户/模型写的常是
 *  扩展名式别名（```ts、```py、```sh），先认扩展名别名表、再认 hljs 的语言别名；都不认识
 *  返回 null（按纯文本渲染，不做自动探测——文件头注释里说过为什么）。一律返回规范名，
 *  免得高亮缓存键分裂成 ts/typescript 两份。 */
export function languageForFence(tag: string): string | null {
  const lower = tag.trim().toLowerCase();
  if (!lower) return null;
  return EXT_TO_LANG[lower] ?? ALIAS_TO_LANG.get(lower) ?? null;
}

/** 路径 → 高亮语言；不认识/无扩展名/点文件（.gitignore 等）返回 null（按纯文本渲染）。 */
export function languageForPath(path: string): string | null {
  const base = path.split("/").pop()?.toLowerCase() ?? "";
  if (base === "dockerfile" || base.startsWith("dockerfile.")) return "dockerfile";
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return null;
  return EXT_TO_LANG[base.slice(dot + 1)] ?? null;
}

/** HTML 转义：纯文本兜底与 hljs 失败降级共用。 */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ── 引擎按需加载 ──
let engine: Engine | null = null;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();

/** 加载 hljs（幂等）。失败时清掉记录，下一次需要时再试；期间一直按纯文本显示。 */
export function loadHighlighter(): Promise<void> {
  if (engine) return Promise.resolve();
  loading ??= import("./highlightEngine").then(
    (mod) => {
      engine = mod;
      for (const listener of listeners) listener();
    },
    (error) => {
      loading = null;
      console.error("加载代码高亮失败", error);
    },
  );
  return loading;
}

export function highlighterReady(): boolean {
  return engine !== null;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/** 订阅高亮引擎是否就绪；needed 为真（有语言要上色）时顺手触发加载。返回值放进 useMemo
 *  依赖里，引擎到了才会重算出带颜色的 HTML。 */
export function useHighlighter(needed = true): boolean {
  const ready = useSyncExternalStore(subscribe, highlighterReady, highlighterReady);
  useEffect(() => {
    if (needed && !ready) void loadHighlighter();
  }, [needed, ready]);
  return ready;
}

/** 高亮一段代码，返回 hljs HTML；语言为空/未注册/引擎未就绪/高亮抛错时回退为转义纯文本。 */
export function highlightCode(code: string, lang: string | null): string {
  return engine ? engine.highlightCode(code, lang) : escapeHtml(code);
}

/** 逐行高亮（行数与输入一一对应）；引擎未就绪时逐行转义。 */
export function highlightLines(lines: string[], lang: string | null): string[] {
  return engine && lang ? engine.highlightLines(lines, lang) : lines.map(escapeHtml);
}
