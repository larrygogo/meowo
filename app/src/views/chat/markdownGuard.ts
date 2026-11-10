// 交给 react-markdown 之前的廉价预检：病态正文直接按原文显示，不去解析。
//
// 实测（2026-11，react-markdown 10 + remark-gfm 4，jsdom）：
// - 3000 层 `>` 嵌套引用：mdast→hast→React 逐层递归，栈溢出抛错，整窗白屏；
// - 20000 层成对嵌套的 `[…]`：链接标签反复回溯，主线程同步卡 7 秒；同样的 `[` 后面再跟
//   `](u)` 凑成链接则是 34 秒。只开不闭的 20000 个 `[` 反而只要 0.1 秒。
// 正常文档两样都到不了：引用/列表嵌套写到 5 层就罕见，链接标签里套 `[` 更是一两层。阈值留足
// 余量，几万字的正常报告照常排版（ChatMarkdown.test 有这个用例）。
//
// 只扫一遍、不分配，开销与 text.length 成正比；流式消息每个 delta 都会重算，必须便宜。

/** 超过这个长度不解析：整段 markdown 解析是同步的，几十万字一次就是秒级卡顿。 */
export const MARKDOWN_MAX_CHARS = 200_000;
/** 行首容器（`>` 引用、`-`/`*`/`+`/`1.` 列表）的最大嵌套层数。 */
export const MARKDOWN_MAX_CONTAINER_DEPTH = 32;
/** 同一段落里未闭合 `[` 的最大深度（链接标签不跨空行，空行处清零）。 */
export const MARKDOWN_MAX_BRACKET_DEPTH = 32;
// 下面三项来自做空审查实测（2026-11-10，同一套 react-markdown + remark-gfm）：
// - `"*a "×4000 + "b" + " a*"×4000`（24KB）卡 4 秒后栈溢出；`*`×20000、`**`×15000、`~~`×15000
//   各自超过 60 秒——强调分隔符的配对是回溯式的，段落里分隔符一多就爆。
// - 440 层缩进嵌套列表 1.6 秒（行首没有标记、靠缩进嵌套，containerDepth 数不到）。
// - 20000 个 `[a](b)` 0.9 秒。
// 正常文档：一段里写几十处加粗已经很多；缩进超过 128 列只出现在代码里（代码块已跳过）；
// 一条消息上千个链接也罕见。
/** 同一段落里强调分隔符（`*`、`~`、词边界的 `_`）连续段的最大个数。 */
export const MARKDOWN_MAX_EMPHASIS_RUNS = 256;
/** 单个强调分隔符连续段的最大长度（分隔线 `***` 远到不了）。 */
export const MARKDOWN_MAX_EMPHASIS_RUN_LENGTH = 64;
/** 行首缩进的最大列数（tab 记 4 列）。 */
export const MARKDOWN_MAX_INDENT = 128;
/** 整条消息里 `](` 的最大个数。 */
export const MARKDOWN_MAX_LINKS = 2000;

const SPACE = 32;
const TAB = 9;
const NEWLINE = 10;

function isBlank(code: number) {
  return code === SPACE || code === TAB;
}

/** 从 start 起数这一行开头叠了几层容器标记；返回层数（行内其余内容不看）。 */
function containerDepth(text: string, start: number): number {
  let depth = 0;
  let i = start;
  const end = text.length;
  while (i < end) {
    while (i < end && isBlank(text.charCodeAt(i))) i += 1;
    const code = text.charCodeAt(i);
    if (code === 62 /* > */) {
      depth += 1;
      i += 1;
      continue;
    }
    if ((code === 45 /* - */ || code === 42 /* * */ || code === 43 /* + */) && isBlank(text.charCodeAt(i + 1))) {
      depth += 1;
      i += 2;
      continue;
    }
    // 有序列表：1–9 位数字 + `.` 或 `)` + 空白。
    let j = i;
    while (j < end && j - i < 9 && text.charCodeAt(j) >= 48 && text.charCodeAt(j) <= 57) j += 1;
    const mark = text.charCodeAt(j);
    if (j > i && (mark === 46 /* . */ || mark === 41 /* ) */) && isBlank(text.charCodeAt(j + 1))) {
      depth += 1;
      i = j + 2;
      continue;
    }
    break;
  }
  return depth;
}

function isWordChar(code: number) {
  return (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122) || code > 127;
}

/** 行首缩进列数（tab 记 4 列）。 */
function indentColumns(text: string, start: number): number {
  let cols = 0;
  for (let i = start; i < text.length; i += 1) {
    const code = text.charCodeAt(i);
    if (code === SPACE) cols += 1;
    else if (code === TAB) cols += 4;
    else break;
  }
  return cols;
}

/** 这一行是不是围栏代码块的开/关行（行首最多 3 个空格后接 ``` 或 ~~~）。返回围栏字符或 0。 */
function fenceChar(text: string, start: number): number {
  let i = start;
  while (i < text.length && i - start < 3 && text.charCodeAt(i) === SPACE) i += 1;
  const code = text.charCodeAt(i);
  if (code !== 96 /* ` */ && code !== 126 /* ~ */) return 0;
  return text.charCodeAt(i + 1) === code && text.charCodeAt(i + 2) === code ? code : 0;
}

/** 这段正文交给 react-markdown 会不会栈溢出或长时间卡死。 */
export function markdownTooComplex(text: string): boolean {
  if (text.length > MARKDOWN_MAX_CHARS) return true;
  let bracket = 0;
  let emphasisRuns = 0;
  let links = 0;
  let lineStart = 0;
  let lineHasContent = false;
  // 围栏代码块里的内容 remark 不做行内解析，`**kwargs`、深缩进都不贵，整段跳过不计。
  let fence = 0;
  for (let i = 0; i <= text.length; i += 1) {
    const code = i < text.length ? text.charCodeAt(i) : NEWLINE;
    if (i === lineStart) {
      const f = fenceChar(text, i);
      if (fence) {
        if (f === fence) fence = 0;
        // 代码块内整行跳到行尾。
        const nl = text.indexOf("\n", i);
        if (nl < 0) break;
        i = nl;
        lineStart = nl + 1;
        lineHasContent = false;
        continue;
      }
      if (f) {
        fence = f;
        bracket = 0;
        emphasisRuns = 0;
      }
      const depth = containerDepth(text, i);
      if (depth > MARKDOWN_MAX_CONTAINER_DEPTH) return true;
      const indent = indentColumns(text, i);
      if (indent > MARKDOWN_MAX_INDENT) return true;
      // 列表项、引用、表格行、标题各自是独立的行内上下文，强调不跨过去：开新块就清零。
      // 不清的话，几百条「- **名称**：说明」连写的正常报告会被误判。
      const first = text.charCodeAt(i + indent);
      if (depth > 0 || first === 124 /* | */ || first === 35 /* # */) emphasisRuns = 0;
    }
    if (code === NEWLINE) {
      if (!lineHasContent) {
        // 空行：段落结束，链接标签和强调都不会跨过去
        bracket = 0;
        emphasisRuns = 0;
      }
      lineStart = i + 1;
      lineHasContent = false;
      continue;
    }
    if (!isBlank(code) && code !== 13 /* CR，CRLF 的空行也算空行 */) lineHasContent = true;
    if (code === 91 /* [ */) {
      bracket += 1;
      if (bracket > MARKDOWN_MAX_BRACKET_DEPTH) return true;
    } else if (code === 93 /* ] */) {
      if (bracket > 0) bracket -= 1;
      if (text.charCodeAt(i + 1) === 40 /* ( */ && ++links > MARKDOWN_MAX_LINKS) return true;
    } else if (code === 42 /* * */ || code === 126 /* ~ */ || code === 95 /* _ */) {
      // 一个连续段只算一次；段长单独设限。
      let j = i;
      while (j < text.length && text.charCodeAt(j) === code) j += 1;
      const runLength = j - i;
      // 词内的 `_`（snake_case）不能开合强调，不算。
      const intraword = code === 95 && isWordChar(text.charCodeAt(i - 1)) && isWordChar(text.charCodeAt(j));
      if (!intraword) {
        if (runLength > MARKDOWN_MAX_EMPHASIS_RUN_LENGTH) return true;
        if (++emphasisRuns > MARKDOWN_MAX_EMPHASIS_RUNS) return true;
      }
      i = j - 1;
    }
  }
  return false;
}
