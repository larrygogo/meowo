/// 用鼠标点击作答终端里的横排选项（opencode 的权限框），不依赖键盘焦点。
///
/// 为什么：这种选项行的焦点只用底色表示，屏幕文字里看不出。此前按「焦点在第一项」算方向键，用户在
/// 终端里动过方向键之后回对话页点「允许一次」，按键会落到别的项上，可能是 Reject 或 Allow always
/// （第二轮审查 2026-11-15）。选项本身可以点（程序文件里每个选项挂着 onMouseUp，点哪个就选哪个）：
/// 1.19.5 真机把焦点挪到 Reject 之后点 Allow once，CLI 只放行了这一次（tests/probe_tui_script.rs 的
/// click 步骤）。所以对话页按屏幕上那个选项的位置发一次点击。
///
/// 位置每次现读：从后端仿真屏幕的网格里找到带按键说明的那一行和这个选项，找不到就不发任何东西。

import { managedTerminalFrame, writeManagedTerminal } from "../../api";

/// 东亚宽字符占两格（与 terminalAttention 的列估算同一张表）。
const WIDE_CHAR = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦]/;
const cells = (text: string) => Array.from(text).reduce((sum, char) => sum + (WIDE_CHAR.test(char) ? 2 : 1), 0);
/// 选项行的标志：同一行右侧的按键说明。
const OPTION_ROW = /⇆\s*select/i;
/// 按下与抬起之间的间隔（真机探针用的就是这个量级）。
const CLICK_GAP_MS = 80;

/// 这个选项在屏幕网格里的位置（从 0 起的行、列，落在选项文字的第二个字上）。找不到返回 null。
/// 选项要是一个完整的格子：前后各有两个以上空格（或行首的边框、行尾），`Allow` 不会命中 `Allow once`。
export function pointerTarget(lines: readonly string[], label: string): { row: number; col: number } | null {
  const wanted = label.trim();
  if (!wanted) return null;
  for (let row = lines.length - 1; row >= 0; row -= 1) {
    const line = lines[row];
    if (!OPTION_ROW.test(line)) continue;
    let from = 0;
    for (;;) {
      const at = line.indexOf(wanted, from);
      if (at < 0) break;
      const before = line.slice(0, at);
      const after = line.slice(at + wanted.length);
      const startsCell = /(?:^|\s{2,}|[┃│|]\s*)$/.test(before);
      const endsCell = /^(?:\s{2,}|\s*$)/.test(after);
      if (startsCell && endsCell) return { row, col: cells(before) + Math.min(1, Math.max(0, cells(wanted) - 1)) };
      from = at + 1;
    }
    // 最靠下的那条选项行里没有这个选项：不往上找旧画面里的。
    return null;
  }
  return null;
}

/// SGR 鼠标序列：左键按下、抬起（行列从 1 起）。
export function clickSequence(target: { row: number; col: number }): [string, string] {
  const at = `${target.col + 1};${target.row + 1}`;
  return [`\x1b[<0;${at}M`, `\x1b[<0;${at}m`];
}

type Deps = {
  frame: (sessionId: number) => Promise<{ lines: string[] } | null>;
  write: (sessionId: number, data: string) => Promise<void>;
  sleep: (ms: number) => Promise<void>;
};
const realDeps: Deps = {
  frame: managedTerminalFrame,
  write: writeManagedTerminal,
  sleep: (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)),
};

/// 点一下终端里的这个选项。返回 false = 屏幕上没找到它，什么都没发。
export async function clickOptionInTerminal(sessionId: number, label: string, deps: Deps = realDeps): Promise<boolean> {
  const frame = await deps.frame(sessionId);
  const target = frame ? pointerTarget(frame.lines, label) : null;
  if (!target) return false;
  const [press, release] = clickSequence(target);
  await deps.write(sessionId, press);
  await deps.sleep(CLICK_GAP_MS);
  await deps.write(sessionId, release);
  return true;
}
