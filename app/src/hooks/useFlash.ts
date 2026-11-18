import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 即时成功反馈（「已复制」「已保存」「已导出」）停留多久：全应用只此一个数（U3，2026-11-18）。
 * 此前 1.5 秒（复制）、2 秒（设置已保存、诊断已导出）、2.5 秒（对话窗提示条）三种并存；
 * 统一取 2.5 秒——够把两三个字读完、又不至于盖住下一次操作（设计标准「成功提示 2.5 秒」）。
 * 带按钮或带原因的提示条不走这里，见 useLingerTimer（至少 8 秒、悬停暂停）。
 */
export const FEEDBACK_MS = 2500;

/**
 * 一次性的成功状态：flash() 置真，FEEDBACK_MS 后自动复位；再次 flash() 重新计时，
 * hide() 立即复位，卸载时清掉计时器（不在已卸载的组件上 setState）。
 */
export function useFlash(ms: number = FEEDBACK_MS) {
  const [shown, setShown] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const flash = useCallback(() => {
    setShown(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setShown(false), ms);
  }, [ms]);
  const hide = useCallback(() => {
    window.clearTimeout(timer.current);
    setShown(false);
  }, []);
  return { shown, flash, hide };
}
