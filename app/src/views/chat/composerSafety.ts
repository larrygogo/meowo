import type { ChatUi } from "../../api";

export type ComposerGuard = NonNullable<ChatUi["composer_guard"]>;
export type ComposerSafety = "empty" | "occupied" | "unknown";
export type ComposerCandidate = {
  state: ComposerSafety;
  inputRow?: number;
  inputCol?: number;
  isPlaceholder?: boolean;
};

// empty 只是文字候选：占位与用户同文草稿可能完全一样，必须由确认层保守裁定，不能用按键后没动来放行。
export function composerCandidate(screen: readonly string[] | null, guard: ComposerGuard | null | undefined): ComposerCandidate {
  if (!screen || !guard) return { state: "unknown" };
  try {
    const top = new RegExp(guard.top_pattern);
    const bottom = new RegExp(guard.bottom_pattern);
    const row = new RegExp(guard.row_pattern);
    let end = screen.length - 1;
    while (end >= 0 && !bottom.test(screen[end])) end -= 1;
    if (end < 0) return { state: "unknown" };
    let start = end;
    let body: string[];
    if (guard.prompt_footer) {
      // footer 与正文之间有空行。不能只取最后一个非空行，否则多行尾部会遮掉真正的 prompt。
      while (end > 0 && screen[end - 1].trim() === "") end -= 1;
      start = end - 1;
      while (start >= 0 && !top.test(screen[start])) {
        if (screen[start].trim() !== "" && !row.test(screen[start])) return { state: "unknown" };
        start -= 1;
      }
      if (start < 0) return { state: "unknown" };
      body = screen.slice(start, end);
    } else {
      while (start > 0 && row.test(screen[start - 1])) start -= 1;
      body = screen.slice(start, end);
      // OpenCode 顶线也是空内容行；从完整连续框体起点找，不能跳过正文去找最后一个空行。
      if (body.length > 0 && top.test(body[0])) { body.shift(); start += 1; }
      else if (start === 0 || !top.test(screen[start - 1])) return { state: "unknown" };
      if (guard.tail_rows > 0) {
        if (!guard.tail_pattern || body.length <= guard.tail_rows
          || !new RegExp(guard.tail_pattern).test(body[body.length - 1])) return { state: "unknown" };
        body.splice(body.length - guard.tail_rows);
      }
    }
    if (body.length === 0) return { state: "unknown" };
    const text: string[] = [];
    for (const line of body) {
      if (guard.prompt_footer && line.trim() === "") { text.push(""); continue; }
      const match = row.exec(line);
      if (!match || match[1] === undefined) return { state: "unknown" };
      text.push(match[1].trim());
    }
    if (!text[0].startsWith(guard.prompt_prefix)) return { state: "unknown" };
    const prefix = new RegExp(guard.input_start_pattern).exec(body[0]);
    if (!prefix || prefix[1] === undefined) return { state: "unknown" };
    text[0] = text[0].slice(guard.prompt_prefix.length).trimStart();
    const content = text.join("\n").trim();
    const candidate = new RegExp(guard.empty_pattern).test(content);
    return {
      state: candidate ? "empty" : "occupied",
      inputRow: start,
      // 插件只声明实拍单宽前缀；不能拿灰色占位的首字位置当输入起点。
      inputCol: prefix[1].length,
      isPlaceholder: candidate && content !== "",
    };
  } catch {
    return { state: "unknown" };
  }
}

// 兼容既有文字候选调用；empty 不代表最终可以提交。
export function composerSafety(screen: readonly string[] | null, guard: ComposerGuard | null | undefined): ComposerSafety {
  return composerCandidate(screen, guard).state;
}
