// 侧问（对照 Kimi 的「侧边聊天」）：CLI 原生旁路提问（claude `/btw`）的答案只在屏幕覆盖层上、
// 不写 transcript，对话页只能从仿真屏幕把它读下来。规格与实拍见后端 chat_ui::SideQuestion。
import type { SideQuestion } from "../../generated/contracts/SideQuestion";

const leading = (line: string) => line.length - line.trimStart().length;

/// composer 里的 `<command> <问题>` → 问题（换行折成空格：终端里换行另有按键）；不是侧问返回 null。
export function sideQuestionText(text: string, spec: SideQuestion): string | null {
  const bare = text.trim();
  if (!bare.startsWith(`${spec.command} `) && !bare.startsWith(`${spec.command}\n`)) return null;
  const question = bare.slice(spec.command.length).replace(/\s+/g, " ").trim();
  return question || null;
}

/// 从整屏里认出覆盖层的答案窗口：最底的提示行 + 其上最近的 `<command> ` 行。认不出返回 null。
/// 问题过长会折行，续行与命令行同缩进；答案行比它多缩进两格，按这个分开并去掉答案的缩进
/// （只去覆盖层自己的那几格，答案里代码块的缩进原样保留）。
export function sideAnswerWindow(screen: string[] | null, spec: SideQuestion): { lines: string[]; ready: boolean } | null {
  if (!screen) return null;
  let footer = screen.length - 1;
  while (footer >= 0 && !screen[footer].trimEnd().toLowerCase().endsWith(spec.footer_marker)) footer -= 1;
  if (footer < 0) return null;
  let head = footer - 1;
  while (head >= 0 && !screen[head].trimStart().startsWith(`${spec.command} `)) head -= 1;
  if (head < 0) return null;
  const indent = leading(screen[head]);
  let start = head + 1;
  while (start < footer && screen[start].trim() && leading(screen[start]) === indent) start += 1;
  const lines = screen.slice(start, footer).map((line) => {
    const cut = Math.min(leading(line), indent + 2);
    return line.slice(cut).trimEnd();
  });
  return { lines, ready: screen[footer].toLowerCase().includes(spec.ready_marker) };
}

/// 滚动后的新窗口接到已读部分后面：取最长的「已读尾 = 新窗口头」重叠。新窗口至少前进一行
/// （与已读完全相同的窗口由调用方判为到底）。
export function mergeAnswerWindow(read: string[], next: string[]): string[] {
  for (let overlap = Math.min(read.length, next.length - 1); overlap > 0; overlap -= 1) {
    const tail = read.slice(read.length - overlap);
    if (tail.every((line, index) => line === next[index])) return [...read, ...next.slice(overlap)];
  }
  return [...read, ...next];
}

const same = (a: string[], b: string[]) => a.length === b.length && a.every((line, index) => line === b[index]);

/// 侧问已提交之后：等答完 → 逐窗滚动拼出全文 → 收起覆盖层。超时或覆盖层消失返回 null
/// （仍尽量收起，覆盖层留着会吞掉后续按键）。
export async function readSideAnswer(
  io: { screen: () => Promise<string[] | null>; write: (data: string) => Promise<void>; sleep?: (ms: number) => Promise<void> },
  spec: SideQuestion,
  timeoutMs = 120_000,
): Promise<string | null> {
  const sleep = io.sleep ?? ((ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms)));
  const close = async () => {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (!sideAnswerWindow(await io.screen(), spec)) return;
      await io.write(spec.close_input);
      await sleep(300);
    }
  };
  let window_: { lines: string[]; ready: boolean } | null = null;
  // 覆盖层出现前有一小段空窗，先等它出现再把「消失」当失败。
  let seen = false;
  for (let waited = 0; waited < timeoutMs; waited += 300) {
    window_ = sideAnswerWindow(await io.screen(), spec);
    if (window_?.ready) break;
    if (window_) seen = true;
    else if (seen) return null;
    await sleep(300);
  }
  if (!window_?.ready) {
    await close();
    return null;
  }
  let read = window_.lines;
  let last = window_.lines;
  // 每按一次下移几行；按完画面没变就再等一拍，仍不变才算到底（避免重绘慢于轮询被当成读完）。
  for (let step = 0; step < 400; step += 1) {
    await io.write(spec.scroll_input);
    await sleep(120);
    let next = sideAnswerWindow(await io.screen(), spec);
    if (next && same(next.lines, last)) {
      await sleep(250);
      next = sideAnswerWindow(await io.screen(), spec);
    }
    if (!next || same(next.lines, last)) break;
    read = mergeAnswerWindow(read, next.lines);
    last = next.lines;
  }
  await close();
  while (read.length > 0 && !read[0]) read = read.slice(1);
  while (read.length > 0 && !read[read.length - 1]) read = read.slice(0, -1);
  return read.join("\n");
}
