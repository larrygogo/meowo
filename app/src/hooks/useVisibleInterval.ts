import { useEffect, useRef } from "react";

/**
 * 只在页面可见时跑的定时器：每 ms 一次（挂载时不跑，首拉由调用方自己决定）；页面隐藏
 * （窗口最小化、手机锁屏或切到后台）时跳过，重新可见的那一刻立即补一次，不等下一拍。
 *
 * 对话窗的兜底轮询原先不看 document.hidden：窗口收起后照样每 2s（手机端 650ms）拉一整份
 * transcript，手机锁屏时就是白耗电和流量；同文件里会话启动探测的轮询早就这么做了。
 * fn 走 ref，换引用不重建定时器；ms 变了才重建。
 */
export function useVisibleInterval(fn: () => void, ms: number) {
  const fnRef = useRef(fn);
  fnRef.current = fn;
  useEffect(() => {
    const tick = () => {
      if (!document.hidden) fnRef.current();
    };
    const timer = window.setInterval(tick, ms);
    const onVisible = () => {
      if (!document.hidden) fnRef.current();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [ms]);
}
