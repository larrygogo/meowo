/// 输入法守卫：这一下按键是不是归输入法（选候选词、确认拼音），而不是给界面的。
///
/// 只看 `isComposing` 不够：WebKit（macOS、iOS）上确认候选词的那一下回车，`isComposing` 已经是
/// false，只有 `keyCode` 还是 229。漏掉它，回车会被当成「发送」，半截拼音跟着消息发出去
/// （走查输入 A7）。贴纸的行内编辑、对话窗输入框、新建页输入框共用这一个判断，不各写各的。

type ImeKeyEvent = {
  isComposing?: boolean;
  keyCode?: number;
  nativeEvent?: { isComposing?: boolean; keyCode?: number };
};

/** 接受 React 合成事件或原生 KeyboardEvent。 */
export function imeComposing(event: ImeKeyEvent): boolean {
  const native = event.nativeEvent ?? event;
  return native.isComposing === true || event.keyCode === 229 || native.keyCode === 229;
}
