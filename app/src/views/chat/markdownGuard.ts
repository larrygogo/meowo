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

/** 这段正文交给 react-markdown 会不会栈溢出或长时间卡死。 */
export function markdownTooComplex(text: string): boolean {
  if (text.length > MARKDOWN_MAX_CHARS) return true;
  let bracket = 0;
  let lineStart = 0;
  let lineHasContent = false;
  for (let i = 0; i <= text.length; i += 1) {
    const code = i < text.length ? text.charCodeAt(i) : NEWLINE;
    if (i === lineStart && containerDepth(text, i) > MARKDOWN_MAX_CONTAINER_DEPTH) return true;
    if (code === NEWLINE) {
      if (!lineHasContent) bracket = 0; // 空行：段落结束，链接标签不会跨过去
      lineStart = i + 1;
      lineHasContent = false;
      continue;
    }
    if (!isBlank(code) && code !== 13 /* CR，CRLF 的空行也算空行 */) lineHasContent = true;
    if (code === 91 /* [ */) {
      bracket += 1;
      if (bracket > MARKDOWN_MAX_BRACKET_DEPTH) return true;
    } else if (code === 93 /* ] */ && bracket > 0) {
      bracket -= 1;
    }
  }
  return false;
}
