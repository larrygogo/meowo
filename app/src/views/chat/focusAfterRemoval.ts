/// 列表项被「×」移除后，焦点该去哪儿。
///
/// 为什么需要：「×」随它那一项一起卸载，浏览器不会替焦点找下家，直接掉到 body——键盘用户得从
/// 窗口顶上重新 Tab（走查 C-10：p3-att-remove 移除附件后 activeElement=BODY）。对照 VS Code /
/// Linear 的列表：删掉一项后焦点落在原位置的下一项（列表往上收，原位置就是下一项），删的是最后
/// 一项就落上一项，列表空了交给 fallback（输入框）。
///
/// 只在「×」本身持有焦点时接管：鼠标点按钮在 WebKit 上不聚焦，那时焦点本来就不在这里，
/// 硬塞给输入框还会在手机上弹出键盘。
///
/// 用法：在 onClick 里、改状态**之前**调用（要先认出被点的按钮在列表里排第几）。
export function focusAfterRemoval(
  button: HTMLElement,
  { list, item, fallback }: {
    /** 列表容器的选择器（从按钮往上找）。 */
    list: string;
    /** 每一项里「×」的选择器（在容器里找，顺序即 DOM 序）。 */
    item: string;
    /** 列表空了（或容器随最后一项卸载）时的去处。 */
    fallback: () => HTMLElement | null | undefined;
  },
): void {
  if (button.ownerDocument.activeElement !== button) return;
  const container = button.closest<HTMLElement>(list);
  const before = container ? Array.from(container.querySelectorAll<HTMLElement>(item)) : [];
  const index = before.indexOf(button);
  // 等 React 提交这次删除再找：同步找到的还是旧 DOM。
  requestAnimationFrame(() => {
    const after = container?.isConnected ? Array.from(container.querySelectorAll<HTMLElement>(item)) : [];
    const target = after.length > 0 ? after[Math.min(Math.max(index, 0), after.length - 1)] : fallback();
    if (target?.isConnected) target.focus();
  });
}
