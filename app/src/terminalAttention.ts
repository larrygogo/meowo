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
const GENERIC_STARTUP_PROMPTS = [
  /restore[^\n]{0,100}(?:token|credential|authentication)/i,
  /(?:token|credential|authentication)[^\n]{0,100}restore/i,
  /oauth token has been revoked/i,
  /no oauth token/i,
  /run \/login to sign in/i,
  /waiting for sign-in to complete/i,
  /trust gateway/i,
  /(?:oauth|token|credential|authentication|sign-in|sign in|login|gateway)[\s\S]{0,320}press (?:enter|esc|escape) to/i,
  /press (?:enter|esc|escape) to[\s\S]{0,320}(?:oauth|token|credential|authentication|sign-in|sign in|login|gateway)/i,
];

function promptSnippet(visible: string, index: number, contextBefore = 1): string {
  const lines = visible.split("\n");
  const matchedLine = visible.slice(0, index).split("\n").length - 1;
  // 命令审批需要多保留几行，才能显示命令和用途；普通启动选择只留一行上下文。
  return lines.slice(Math.max(0, matchedLine - contextBefore), matchedLine + 10).join("\n").trim();
}

/** 返回当前启动阻塞提示的可见原文；null 表示没有需要 GUI 接管的交互。 */
export function terminalAttention(text: string, markers: string[], interactivePrompt = false): TerminalAttention | null {
  if (!text) return null;
  const visible = visibleTerminalText(text);
  const lower = visible.toLocaleLowerCase();
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
    const normalized = marker.toLocaleLowerCase();
    const index = lower.lastIndexOf(normalized);
    if (index >= 0 && (!best || index > best.index)) best = { index, id: `provider:${normalized}` };
  }
  for (const pattern of GENERIC_STARTUP_PROMPTS) {
    const flags = pattern.flags.includes("g") ? pattern.flags : `${pattern.flags}g`;
    for (const match of lower.matchAll(new RegExp(pattern.source, flags))) {
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
  if (!best) return null;
  const snippet = best.id === "interactive:numbered-selector"
    ? visible
    : promptSnippet(visible, best.index, best.id === "claude:command-approval" ? 5 : 1);
  // 命令审批的选项已经转换成 GUI 按钮，详情区保留命令、用途和审批问题，只从第一个
  // 编号选项起裁掉。这样不会重复 Yes/No，也不会带上键位说明或 TUI 重绘尾部噪声。
  const snippetLines = snippet.split("\n");
  const firstOptionLine = snippetLines.findIndex((line) => /^\s*(?:[❯>]\s*)?\d+\.\s+/.test(line));
  const displayText = best.id === "claude:command-approval" && firstOptionLine >= 0
    ? snippetLines.slice(0, firstOptionLine).join("\n").trim()
    : snippet;
  const labels = snippet.split("\n").flatMap((line) => {
    const match = line.match(/^\s*(?:[❯>]\s*)?(\d+)\.\s*(.+?)\s*$/);
    return match ? [{ index: Number(match[1]) - 1, label: match[2] }] : [];
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
      const key = `${occurrence.kind}:${occurrence.label.toLocaleLowerCase()}`;
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
    return {
      id: best.id,
      text: displayText,
      options: labels.map(({ index, label }) => ({
        label,
        // 先回到第一项再下移，避免依赖当前光标停在哪一项。
        input: "\x1b[A".repeat(8) + "\x1b[B".repeat(Math.max(0, index)) + "\r",
      })),
    };
  }
  return { id: best.id, text: displayText };
}

/** 兼容启动发送路径只关心是否阻塞的调用。 */
export function terminalNeedsAttention(text: string, markers: string[], interactivePrompt = false): boolean {
  return terminalAttention(text, markers, interactivePrompt) != null;
}
