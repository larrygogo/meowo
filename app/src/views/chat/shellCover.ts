// 主窗口的名单盖在对话窗上时（views/roster/RosterShell），对话窗没有卸载：它挂在 window 上的那些
// 键盘监听还活着。不拦的话，名单上按数字键 1 会批准被盖住的那张审批卡、按两下 Esc 会拒掉它
// （做空审查 2026-11-12 H1：人在看不到命令的情况下做了不可撤销的决定）。
//
// 做法分两层：
// - window 冒泡段的监听（对话窗绝大多数快捷键，含审批的 1 / Esc）：这里在 document 冒泡段把事件
//   截住，它们收不到。React 的事件委托在根容器上，比 document 靠里，名单自己的按键处理不受影响。
// - window 捕获段的监听（面板、灯箱的 Esc）：截不住，由它们各自在开头查 isShellCovered()。
//
// `inert` 只挡焦点和点击，挡不住 window 级监听，所以不能只靠它。
let covered = false;

/** 名单是否正盖着对话窗。 */
export function isShellCovered(): boolean {
  return covered;
}

export function setShellCovered(next: boolean): void {
  covered = next;
}

if (typeof document !== "undefined") {
  const gate = (event: Event) => {
    if (covered) event.stopPropagation();
  };
  // 模块加载时就挂上，早于任何组件的 effect。三种键盘事件都拦：有的快捷键听 keyup。
  document.addEventListener("keydown", gate);
  document.addEventListener("keyup", gate);
  document.addEventListener("keypress", gate);
}
