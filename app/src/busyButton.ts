/**
 * 「操作进行中」按钮的统一写法：aria-disabled + 在点击处理里拦截，不用 disabled。
 *
 * 为什么：真 disabled 会让当前聚焦的按钮立刻失焦，焦点掉到 body，键盘用户下一次 Tab
 * 从页首重来，读屏也念不到「正在进行」（第三轮设计准则「焦点」；做空第一轮 #4，2026-11-12
 * 列出标题栏「标记完成」「结束会话」、门卡「恢复会话」「接管」等十几处同病）。
 * aria-disabled 不改变可聚焦性，读屏会念「不可用」；但浏览器照样派发 click，
 * 所以处理函数必须自己拦——这个 helper 就是把「标记」和「拦截」绑在一起，免得只改一半。
 *
 * 用法：<button {...busyProps(busy, onClick)}>…</button>
 * 表单校验型的禁用（如未填内容不可提交）跟进行中无关，仍可用 disabled，两者可以同时写。
 * 外观：样式表里该按钮基类的 :disabled 规则要同时覆盖 [aria-disabled="true"]，长相才一致。
 * 防回归：busyButton.test.tsx 扫源码，拦新增的 disabled={…busy|sending|pending…}。
 */
export function busyProps<E extends { preventDefault(): void }>(busy: boolean, onClick?: (event: E) => void) {
  return {
    "aria-disabled": busy || undefined,
    onClick: (event: E) => {
      // 进行中的点击直接吞掉；preventDefault 让 type=submit 的按钮也不触发表单提交。
      if (busy) {
        event.preventDefault();
        return;
      }
      onClick?.(event);
    },
  } as const;
}
