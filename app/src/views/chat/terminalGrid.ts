/// 新起一个 PTY（恢复、接管、分叉）时的初始尺寸。
///
/// 一律取当前终端的网格：写死一个尺寸的话 CLI 先按占位宽度排版，终端挂上、fit 落地后再 resize，
/// 首屏整屏重排一遍。恢复和接管早就这样取，分叉曾写死 100×30（走查输入 B12）；三条路现在共用这一个
/// 函数，不再各写各的。

export type TerminalGrid = { cols: number; rows: number };

/// 终端还没挂载（读不到网格）时的占位尺寸。
export const FALLBACK_GRID: TerminalGrid = { cols: 100, rows: 30 };

export function launchGrid(read: (() => TerminalGrid | null) | null | undefined): TerminalGrid {
  const grid = read?.();
  return grid && grid.cols > 0 && grid.rows > 0 ? grid : FALLBACK_GRID;
}
