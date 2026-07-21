/** 把 PTY snapshot 的 base64 增量追加为 UTF-8 文本；损坏的一帧不打断启动流程。 */
export function appendTerminalText(tail: string, data: string, decoder: TextDecoder): string {
  if (!data) return tail;
  try {
    const binary = atob(data);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
    return (tail + decoder.decode(bytes, { stream: true })).slice(-16_384);
  } catch {
    return tail;
  }
}

export type TerminalAttentionOption = {
  label: string;
  input: string;
  description?: string;
  selected?: boolean;
  focused?: boolean;
  position?: number;
  kind?: "choice" | "input" | "submit" | "chat";
};
export type TerminalAttention = { id: string; text: string; options?: TerminalAttentionOption[] };

export function visibleTerminalText(text: string): string {
  // backlog 可能保留已经处理过的旧提示。只看最后一次整屏清除之后的内容，避免重新挂载
  // ManagedTerminal 时把历史信任页再次报成当前状态。
  const clearAt = Math.max(text.lastIndexOf("\x1b[2J"), text.lastIndexOf("\x1b[3J"));
  const currentScreen = clearAt >= 0 ? text.slice(clearAt) : text;
  return currentScreen
    .replace(/\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/\r(?!\n)/g, "\n")
    .split("\n")
    .map((line) => line.trimEnd())
    .filter((line, index, lines) => line.trim() && line !== lines[index - 1])
    .slice(-80)
    .join("\n")
    .trim();
}

// 登录、凭据恢复等提示并不总是由 provider 暴露为稳定文案。这里仅收需要键盘确认的
// 高信号启动提示；命中后 GUI 会显示 CLI 的原文，而不是猜测并重写它的选项。
// 直接带 g 标志预编译：terminalAttention 跑在 150ms 节流扫描 / 80ms 启动轮询的热路径上，
// 每次调用重新 new RegExp 是纯浪费；matchAll 不会改写 lastIndex，共享实例是安全的。
const GENERIC_STARTUP_PROMPTS = [
  /restore[^\n]{0,100}(?:token|credential|authentication)/gi,
  /(?:token|credential|authentication)[^\n]{0,100}restore/gi,
  /oauth token has been revoked/gi,
  /no oauth token/gi,
  /run \/login to sign in/gi,
  /waiting for sign-in to complete/gi,
  /trust gateway/gi,
  /(?:oauth|token|credential|authentication|sign-in|sign in|login|gateway)[\s\S]{0,320}press (?:enter|esc|escape) to/gi,
  /press (?:enter|esc|escape) to[\s\S]{0,320}(?:oauth|token|credential|authentication|sign-in|sign in|login|gateway)/gi,
];

/// 导航提示：菜单在等键盘选择的信号。要求同时出现「方向键/导航」与「回车确认」两类线索，
/// 单独一个 ❯ 太常见（提示符、列表装饰都可能有），会把正文误报成菜单。中文本地化 CLI
/// 常把 enter/confirm 写作「回车/确认」，一并收。
const MENU_HINT = /(?:↑↓|↑\/↓|up\/down|arrow keys)[^\n]{0,80}(?:enter|select|confirm|回车|确认)|enter\s+(?:to\s+)?select/i;

/// 光标菜单：一句导航提示 + 一个 ❯ 标记当前项。返回选项块的行区间。
///
/// 边界靠**缩进**而不是空行：`visibleTerminalText` 与 xterm 抓屏都会丢掉空行，但保留行首
/// 缩进。同一个菜单的选项缩进一致，而小节标题（如 kimi 的 `Thinking (←→ to switch)`）
/// 缩进更浅——取「含 ❯ 且同缩进的连续行」正好圈出选项，不会把相邻小节吞进来。
function detectCursorMenu(
  visible: string,
): { index: number; lines: string[]; focused: number; title: string } | null {
  const lines = visible.split("\n");
  const hintLine = lines.findIndex((line) => MENU_HINT.test(line));
  if (hintLine < 0) return null;
  const focusedLine = lines.findIndex((line) => /^\s*❯\s+\S/.test(line));
  if (focusedLine < 0) return null;
  // 文本起始列：带 ❯ 的行要跨过标记本身，才能和其余项对齐比较。
  const textIndent = (line: string): number => {
    const match = /^(\s*)(❯\s+)?/.exec(line);
    return (match?.[1]?.length ?? 0) + (match?.[2]?.length ?? 0);
  };
  const target = textIndent(lines[focusedLine]);
  const sameBlock = (line: string) => line.trim().length > 0 && textIndent(line) === target;
  let start = focusedLine;
  while (start > 0 && sameBlock(lines[start - 1])) start -= 1;
  let end = focusedLine;
  while (end + 1 < lines.length && sameBlock(lines[end + 1])) end += 1;
  // 只有一项的「菜单」多半是误判（提示符、单条列表）。
  if (end - start < 1) return null;
  // 标题以**提示行**为锚取它上面一行，而不是「选项块之前最近的一行」——两者之间还夹着
  // provider 过滤行之类的东西，那种会把抬头显示成 "All Kimi Code"。
  const title = lines
    .slice(0, hintLine)
    .reverse()
    .find((line) => line.trim() && !/^[\s─━═|-]+$/.test(line));
  const index = lines.slice(0, start).join("\n").length;
  return {
    index,
    lines: lines.slice(start, end + 1),
    focused: focusedLine - start,
    title: title?.trim() ?? "",
  };
}

function promptSnippet(visible: string, index: number, contextBefore = 1): string {
  const lines = visible.split("\n");
  const matchedLine = visible.slice(0, index).split("\n").length - 1;
  // 命令审批需要多保留几行，才能显示命令和用途；普通启动选择只留一行上下文。
  return lines.slice(Math.max(0, matchedLine - contextBefore), matchedLine + 10).join("\n").trim();
}

/// 光标菜单选项块 → GUI 按钮。行首缩进只是菜单的对齐手段，不属于选项文字；
/// 菜单首尾循环，只能从 ❯ 光标做相对移动，不能靠「多按几次上键」归零。
function cursorMenuOptions(lines: string[], focused: number): TerminalAttentionOption[] {
  return lines.map((line, position) => {
    const label = line.replace(/^\s*(?:❯\s+)?/, "").trimEnd();
    const delta = position - focused;
    return {
      // 多列对齐用的大段空格在按钮上没意义；压成单空格，超长再截断。
      label: label.replace(/\s{2,}/g, "  ").slice(0, 80),
      input: delta === 0 ? "\r" : delta < 0
        ? "\x1b[A".repeat(-delta) + "\r"
        : "\x1b[B".repeat(delta) + "\r",
      focused: delta === 0,
      position,
      kind: "choice" as const,
    };
  });
}

/// 锚定光标菜单：best 已确认是阻塞提示，不再要求导航提示行（中文本地化 CLI 的长会话恢复
/// 等菜单经常没有 "enter to select" 这类线索），直接从提示锚点之后找 ❯ 光标所在的同缩进块。
/// 扩展时跳过以句读结尾的行——那是题干的说明句（如「完整恢复会消耗较多额度。」），
/// 与选项同缩进时容易被误吞进选项块。
function detectAnchoredCursorMenu(
  visible: string,
  fromIndex: number,
): { lines: string[]; focused: number } | null {
  const lines = visible.split("\n");
  const fromLine = visible.slice(0, fromIndex).split("\n").length - 1;
  const focusedLine = lines.findIndex((line, index) => index > fromLine && /^\s*❯\s+\S/.test(line));
  if (focusedLine < 0) return null;
  const textIndent = (line: string): number => {
    const match = /^(\s*)(❯\s+)?/.exec(line);
    return (match?.[1]?.length ?? 0) + (match?.[2]?.length ?? 0);
  };
  const target = textIndent(lines[focusedLine]);
  const optionLine = (line: string) =>
    line.trim().length > 0 && textIndent(line) === target && !/[。？！?!.：:]$/.test(line.trim());
  let start = focusedLine;
  while (start > 0 && optionLine(lines[start - 1])) start -= 1;
  let end = focusedLine;
  while (end + 1 < lines.length && optionLine(lines[end + 1])) end += 1;
  if (end - start < 1) return null;
  return { lines: lines.slice(start, end + 1), focused: focusedLine - start };
}

/** 返回当前启动阻塞提示的可见原文；null 表示没有需要 GUI 接管的交互。 */
/// `expectMenu`：刚发出一条会弹交互菜单的命令（如 `/model`）。只在这段窗口里认光标菜单——
/// 常开的话，agent 平时画的任何带 ❯ 的列表都会弹成卡片，噪声大于价值。
export function terminalAttention(
  text: string,
  markers: string[],
  interactivePrompt = false,
  expectMenu = false,
): TerminalAttention | null {
  if (!text) return null;
  const visible = visibleTerminalText(text);
  const lower = visible.toLowerCase();
  let best: { index: number; id: string } | null = null;
  const longSession = /this session is[^\n]{0,120}\bold and[^\n]{0,80}\btokens\b/i.exec(visible)
    ?? /resuming the full session will consume a substantial portion of your usage limits/i.exec(visible);
  if (longSession) best = { index: longSession.index, id: "claude:long-session-resume" };
  const commandApproval = /this command requires approval/i.exec(visible)
    ?? /do you want to proceed\?/i.exec(visible);
  if (commandApproval && (!best || commandApproval.index > best.index)) {
    best = { index: commandApproval.index, id: "claude:command-approval" };
  }
  for (const marker of markers) {
    const normalized = marker.toLowerCase();
    const index = lower.lastIndexOf(normalized);
    if (index >= 0 && (!best || index > best.index)) best = { index, id: `provider:${normalized}` };
  }
  for (const pattern of GENERIC_STARTUP_PROMPTS) {
    for (const match of lower.matchAll(pattern)) {
      const index = match.index ?? -1;
      if (index >= 0 && (!best || index > best.index)) {
        // id 按识别规则稳定，而不带整屏文字。TUI 重绘会改变原始流的重复片段，若把它们
        // 放进 id，同一提示就会被误报成几十个新提示，导致对话页闪烁。用户操作后的下一屏
        // 由一次性抓屏交付，因此连续两个同类选择器也不会丢。
        best = { index, id: `generic:${pattern.source}` };
      }
    }
  }
  if (!best && interactivePrompt) {
    const selectorHint = /enter to select[^\n]*/i.exec(visible);
    const numberedChoices = visible.match(/^\s*(?:[❯>]\s*)?\d+\.\s+/gm) ?? [];
    if (selectorHint && numberedChoices.length >= 2) {
      best = { index: selectorHint.index, id: "interactive:numbered-selector" };
    }
  }
  // 无编号的光标菜单（kimi 的 `/model`、provider 切换等）。与上面的编号选择器是两种形态：
  // 那种每项带 `1.`，这种只有一个 ❯ 光标 + 一句导航提示。实测形如：
  //   Select a model  (type to search)
  //   Tab toggle provider · ↑↓ navigate · Enter select · Esc cancel
  //     K2.7 Coding            Kimi Code
  //   ❯ K3                     Kimi Code ← current
  const cursorMenu = !best && expectMenu ? detectCursorMenu(visible) : null;
  if (cursorMenu) {
    return {
      id: "interactive:cursor-menu",
      text: cursorMenu.title,
      options: cursorMenuOptions(cursorMenu.lines, cursorMenu.focused),
    };
  }
  if (!best) return null;
  const snippet = best.id === "interactive:numbered-selector"
    ? visible
    : promptSnippet(visible, best.index, best.id === "claude:command-approval" ? 12 : 1);
  // 命令审批的选项已经转换成 GUI 按钮，详情区保留命令、用途和审批问题，只从第一个
  // 编号选项起裁掉。这样不会重复 Yes/No，也不会带上键位说明或 TUI 重绘尾部噪声。
  const snippetLines = snippet.split("\n");
  const firstOptionLine = snippetLines.findIndex((line) => /^\s*(?:[❯>]\s*)?\d+\.\s+/.test(line));
  const displayText = best.id === "claude:command-approval" && firstOptionLine >= 0
    ? snippetLines.slice(0, firstOptionLine).join("\n").trim()
    : snippet;
  const labels = snippet.split("\n").flatMap((line) => {
    const match = line.match(/^\s*([❯>]?)\s*(\d+)\.\s*(.+?)\s*$/);
    return match ? [{ index: Number(match[2]) - 1, label: match[3], focused: Boolean(match[1]) }] : [];
  });
  if (best.id === "interactive:numbered-selector") {
    const lines = snippet.split("\n");
    const occurrences: TerminalAttentionOption[] = [];
    let current: TerminalAttentionOption | null = null;
    for (const line of lines) {
      const numbered = line.match(/^\s*([❯>]?)\s*(\d+)\.\s*(?:\[([ x✓✔])\]\s*)?(.+?)\s*$/i);
      const checkbox = numbered?.[3];
      const numberedLabel = numbered?.[4]?.trim() ?? "";
      // 会话正文也常有普通的 `1. / 2. / 3.` 列表；只有复选框项和 Claude 明确提供的
      // “Chat about this” 才属于这个选择器，避免把正文里的审查结论复制成选项。
      if (numbered && (checkbox !== undefined || /chat about this/i.test(numberedLabel))) {
        const label = numbered[4].trim();
        current = {
          label,
          input: "",
          selected: Boolean(checkbox && !/\s/.test(checkbox)),
          focused: Boolean(numbered[1]),
          kind: /type something/i.test(label) ? "input" : /chat about this/i.test(label) ? "chat" : "choice",
        };
        occurrences.push(current);
        continue;
      }
      if (numbered) { current = null; continue; }
      const submit = line.match(/^\s*([❯>]?)\s*submit\s*$/i);
      if (submit) {
        current = {
          label: line.trim(),
          input: "",
          kind: "submit",
          focused: Boolean(submit[1]),
        };
        occurrences.push(current);
        continue;
      }
      if (/^[─━═\s]+$/.test(line) || /enter to select|↑\/↓|up\/down|esc to cancel|[?？]/i.test(line)) {
        current = null;
        continue;
      }
      if (current && line.trim()) {
        current.description = [current.description, line.trim()].filter(Boolean).join(" ");
      }
    }
    // 全屏 TUI 重绘可能把同一块内容多次留在 scrollback。按动作+标签合并，选中状态取
    // 最后一次重绘，描述取最短的完整版本（长版本通常混进了下一轮提示文字）。
    const unique = new Map<string, TerminalAttentionOption>();
    for (const occurrence of occurrences) {
      const key = `${occurrence.kind}:${occurrence.label.toLowerCase()}`;
      const existing = unique.get(key);
      if (!existing) {
        unique.set(key, { ...occurrence });
        continue;
      }
      existing.selected = occurrence.selected;
      existing.focused = occurrence.focused;
      if (occurrence.description && (!existing.description || occurrence.description.length < existing.description.length)) {
        existing.description = occurrence.description;
      }
    }
    const ordered = [...unique.values()];
    // 某些重绘只在状态行里写出 Submit，独立选择行被裁掉；Claude 的顺序固定为
    // checkbox choices → Submit → Chat about this，在 Chat 前补回它。
    if (!ordered.some((choice) => choice.kind === "submit")) {
      const chatAt = ordered.findIndex((choice) => choice.kind === "chat");
      ordered.splice(chatAt >= 0 ? chatAt : ordered.length, 0, { label: "Submit", input: "", kind: "submit", focused: false });
    }
    const focusedPosition = ordered.findIndex((choice) => choice.focused);
    const choices = ordered.map((choice, position) => {
      const delta = focusedPosition < 0 ? null : position - focusedPosition;
      return {
        ...choice,
        position,
        // 菜单会首尾循环，不能靠“多按几次上键”归零；必须从当前 ❯ 光标做相对移动。
        input: delta == null ? "" : delta < 0
          ? "\x1b[A".repeat(-delta) + "\r"
          : "\x1b[B".repeat(delta) + "\r",
      };
    });
    const questionLine = [...lines].reverse().find((line) => /[?？]/.test(line) && !/enter to select/i.test(line));
    const question = questionLine?.split(/→|->/).at(-1)?.trim() ?? "";
    return {
      id: best.id,
      text: question,
      options: choices,
    };
  }
  // trust、长会话恢复以及其他编号选择器共用同一套结构化按钮，不再退化成上一项/下一项。
  if (labels.length >= 2) {
    // TUI 重绘可能把同一菜单多次留在缓冲里；按编号合并，焦点状态取最后一次重绘。
    const byIndex = new Map<number, (typeof labels)[number]>();
    for (const entry of labels) byIndex.set(entry.index, entry);
    const merged = [...byIndex.values()].sort((a, b) => a.index - b.index);
    const focused = merged.filter((entry) => entry.focused).at(-1);
    return {
      id: best.id,
      text: displayText,
      options: merged.map(({ index, label }) => ({
        label,
        // 这些菜单和 numbered-selector 一样首尾循环，「先按 8 次上键归零」在 3 项菜单上
        // 会绕圈选错（点拒绝实际选中批准）。抓到 ❯ 光标就从它做相对移动；确实没有光标
        // 标记时才退回归零法——那类菜单没有更可靠的定位依据。
        input: focused
          ? (index < focused.index
            ? "\x1b[A".repeat(focused.index - index) + "\r"
            : "\x1b[B".repeat(index - focused.index) + "\r")
          : "\x1b[A".repeat(8) + "\x1b[B".repeat(Math.max(0, index)) + "\r",
      })),
    };
  }
  // 无编号的光标菜单（中文本地化 CLI 的长会话恢复等）：编号提取落空时从 ❯ 光标块提取选项，
  // 让 GUI 直接给出选项按钮，而不是把「上一项/下一项」甩给用户。
  const anchored = detectAnchoredCursorMenu(visible, best.index) ?? detectCursorMenu(visible);
  if (anchored) {
    return { id: best.id, text: displayText, options: cursorMenuOptions(anchored.lines, anchored.focused) };
  }
  return { id: best.id, text: displayText };
}

/** 兼容启动发送路径只关心是否阻塞的调用。 */
export function terminalNeedsAttention(text: string, markers: string[], interactivePrompt = false): boolean {
  return terminalAttention(text, markers, interactivePrompt) != null;
}
