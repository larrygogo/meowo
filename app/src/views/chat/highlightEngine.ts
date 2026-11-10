/**
 * highlight.js 本体与 24 种语言（约 170KB minified）。只经 highlight.ts 的 loadHighlighter
 * 动态加载：此前它被 ToolActivity / ToolPreview / terminalText / ChatMarkdown 静态引用，
 * 占对话窗主 chunk 约三成（vite 构建产物实测），而首屏多半一个代码块都还没出现。
 * 渲染按行、高亮按篇的说明见 highlight.ts 文件头。
 */
import hljs from "highlight.js/lib/core";
import { escapeHtml } from "./highlight";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import diff from "highlight.js/lib/languages/diff";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import markdown from "highlight.js/lib/languages/markdown";
import php from "highlight.js/lib/languages/php";
import plaintext from "highlight.js/lib/languages/plaintext";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";

const LANGUAGES = {
  bash, c, cpp, csharp, css, diff, dockerfile, go, ini, java, javascript, json,
  kotlin, markdown, php, plaintext, python, ruby, rust, sql, swift, typescript, xml, yaml,
} as const;
for (const [name, language] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, language);
}

/** 加载后用来校验 highlight.ts 里手抄的别名表没漂移（highlight.test.ts）。 */
export function registeredAliases(): Record<string, string[]> {
  return Object.fromEntries(Object.keys(LANGUAGES).map((name) => [name, [...(hljs.getLanguage(name)?.aliases ?? [])]]));
}

/** 高亮一段代码，返回 hljs HTML；语言为空/未注册/高亮抛错时回退为转义纯文本。 */
export function highlightCode(code: string, lang: string | null): string {
  if (!lang || !hljs.getLanguage(lang)) return escapeHtml(code);
  try {
    // ignoreIllegals：diff 侧会把不连续的 hunk 拼成一篇，语法不完整是常态，
    // 不能因一处解析失败整篇退成纯文本。
    return hljs.highlight(code, { language: lang, ignoreIllegals: true }).value;
  } catch {
    return escapeHtml(code);
  }
}

/**
 * 把 hljs 整篇输出按换行拆回逐行 HTML。hljs 的 value 只含三种片段：
 * `<span class="…">`、`</span>`、转义后的文本（class 值不含 '>'，按 '>' 截取安全）。
 * 维护未闭合 span 栈：遇换行先按栈深补闭合，下一行以同栈重开——跨行 token
 * （多行注释/字符串）的颜色因此延续，且每行自平衡。
 */
function splitHighlightedHtml(html: string): string[] {
  const lines: string[] = [];
  const open: string[] = [];
  let current = "";
  let index = 0;
  while (index < html.length) {
    const char = html[index];
    if (char === "\n") {
      lines.push(current + "</span>".repeat(open.length));
      current = open.join("");
      index += 1;
    } else if (char === "<") {
      const close = html.indexOf(">", index);
      if (close === -1) {
        // 残缺标签按理不会出现（hljs 输出总是成对完整）；防御性当文本吞掉。
        current += html.slice(index);
        break;
      }
      const tag = html.slice(index, close + 1);
      if (tag.startsWith("</")) open.pop();
      else open.push(tag);
      current += tag;
      index = close + 1;
    } else {
      let next = index;
      while (next < html.length && html[next] !== "<" && html[next] !== "\n") next += 1;
      current += html.slice(index, next);
      index = next;
    }
  }
  lines.push(current + "</span>".repeat(open.length));
  return lines;
}

/// 超过这个总量不高亮、只转义：hljs 同步跑在渲染路径上，后端给面板的文件/diff 上限是
/// 200KB，整篇送 hljs 会把主线程卡到接近秒级。阈值比对话区（ChatMarkdown 20K）宽——
/// 这里每个文件只算一次（useMemo），不像流式消息每个 delta 都重来。
const HIGHLIGHT_MAX_TOTAL_CHARS = 100_000;

/**
 * 逐行高亮：行数组拼成整篇送 hljs（跨行状态保留），拆回后行数与输入一一对应。
 * 拆分结果行数对不上（拆分假设被打破）时宁可整体退纯文本，也不冒错位渲染的险。
 */
export function highlightLines(lines: string[], lang: string | null): string[] {
  if (!lang || !hljs.getLanguage(lang)) return lines.map(escapeHtml);
  let total = 0;
  for (const line of lines) total += line.length + 1;
  if (total > HIGHLIGHT_MAX_TOTAL_CHARS) return lines.map(escapeHtml);
  try {
    const html = hljs.highlight(lines.join("\n"), { language: lang, ignoreIllegals: true }).value;
    const split = splitHighlightedHtml(html);
    return split.length === lines.length ? split : lines.map(escapeHtml);
  } catch {
    return lines.map(escapeHtml);
  }
}
