/// 托管终端与窗口快捷键的分界：哪些组合键从 xterm 放行给窗口（应用导航），哪些交给 PTY。
///
/// Ctrl+B 在 claude 交互模式里是「把正在跑的 bash / 子 agent 转到后台」，原先被拿去收侧栏，
/// 托管终端里 claude 的转后台就用不了（2026-11-11 调研 i4，关卡 A 定案：焦点在托管终端时
/// 交给 PTY 发 \x02，对话视图仍收侧栏）。macOS 收侧栏是 ⌘B，claude 的是 ⌃B，两者本不冲突，
/// 所以只让出「按着 Ctrl、没按 ⌘」的那一种：⌘B 照旧放行给窗口。
/// Ctrl+K/N/1/2 维持应用占用（SPEC：应用切换器优先），速查表写明它们原本的功能。

type Combo = Pick<KeyboardEvent, "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "code">;

const APP_NAV_CODES = ["KeyK", "KeyB", "KeyN", "Digit1", "Digit2"];

/// 托管终端里按下的键是否放行给窗口快捷键（xterm 不处理、不下发 PTY）。
export function isTerminalAppNavKey(event: Combo): boolean {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return false;
  if (!APP_NAV_CODES.includes(event.code)) return false;
  if (event.code === "KeyB" && event.ctrlKey && !event.metaKey) return false;
  return true;
}

/// 窗口级的 Ctrl/⌘+B 是否该收展侧栏：焦点在托管终端里的 Ctrl+B 归 PTY（xterm 已下发 \x02），
/// 窗口这边不能再收一次侧栏。
export function isSidebarToggleKey(event: Combo, active: Element | null): boolean {
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.code !== "KeyB") return false;
  if (event.ctrlKey && !event.metaKey && active?.closest(".managed-terminal")) return false;
  return true;
}
