/// CLI 终端界面里输入框上方的运行中状态行（claude：`✻ Composing… (41s · ↓ 427 tokens)`）。
/// 对话页运行条照搬它（2026-10 用户要求「内容改成终端这样的」）。
///
/// 匹配正则由插件经 `ChatUi.status_line_pattern` 下发（捕获组 1 = 动词、2 = 括号统计），
/// 前端不认 agent 身份。从屏幕底部往上找第一条：状态行贴着输入框，正文里偶然长得像的
/// 行在它上面。认不出返回 null，运行条退回本地计时——宁可少报，不可把正文误当状态。

export type CliStatus = { verb: string; detail: string | null };

let cached: { source: string; re: RegExp | null } | null = null;
function compile(source: string): RegExp | null {
  if (cached?.source === source) return cached.re;
  let re: RegExp | null = null;
  try {
    re = new RegExp(source);
  } catch {
    // 后端声明了一个 JS 不认的正则：当作没声明，别让运行条整个崩掉。
  }
  cached = { source, re };
  return re;
}

export function parseCliStatus(screen: string, pattern: string): CliStatus | null {
  const re = compile(pattern);
  if (!re) return null;
  const lines = screen.split("\n");
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const match = re.exec(lines[index]);
    if (!match?.[1]) continue;
    // 「esc to interrupt」是给终端键盘用户的操作提示，对话页有自己的停止键，不搬过来。
    const detail = match[2]
      ?.split(" · ")
      .map((part) => part.trim())
      .filter((part) => part && !/esc to interrupt/i.test(part))
      .join(" · ");
    return { verb: match[1], detail: detail || null };
  }
  return null;
}

/// 本地计时的展示：与终端同一副写法（41s / 2m 5s / 1h 3m）。
export function fmtElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${total % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
