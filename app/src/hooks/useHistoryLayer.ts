import { useCallback, useEffect, useRef } from "react";

/**
 * 手机上的浮层接上浏览器历史（走查 M-2，2026-11-12）：iOS 左缘右滑、安卓返回键原先不关浮层——
 * 「更改」页一滑直接退回会话列表，新建浮层（列表页没压过历史）一滑就离开了 Meowo 页面，
 * 填了一半的提示词一起没了。全仓原先只有对话窗「列表 → 会话」压了一条（ChatWindow 的 meowoChat）。
 *
 * 规则：open 期间压一条带 meowoLayer 记号的历史；返回手势弹掉它时调 onBack 关浮层；
 * 用户自己点 × 关掉时把这条记录退掉（history.back），不留「白退一步」。记录在原 state 上叠加，
 * 保留 meowoChat，对话窗自己的 popstate 判定（state.meowoChat 为真就留在对话）不受影响。
 * 只在 enabled（手机远程页）时生效，桌面窗口没有返回手势，不碰历史。
 */
let layerSeq = 0;

type LayerState = { meowoLayer?: string; meowoChat?: boolean } & Record<string, unknown>;
const readState = (): LayerState | null => {
  try {
    return (window.history.state as LayerState | null) ?? null;
  } catch {
    return null;
  }
};

export function useHistoryLayer(open: boolean, onBack: () => void, enabled = true) {
  const tokenRef = useRef<string | null>(null);
  const priorRef = useRef<LayerState | null>(null);
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;

  useEffect(() => {
    if (!open || !enabled) return;
    const token = `layer-${++layerSeq}`;
    const prior = readState();
    try {
      window.history.pushState({ ...(prior ?? {}), meowoLayer: token }, "");
    } catch {
      return; // 沙箱化环境拿不到历史：退化为只有 × 能关
    }
    tokenRef.current = token;
    priorRef.current = prior;
    const onPop = (event: PopStateEvent) => {
      if ((event.state as LayerState | null)?.meowoLayer === token) return;
      if (tokenRef.current !== token) return;
      tokenRef.current = null;
      onBackRef.current();
    };
    window.addEventListener("popstate", onPop);
    return () => {
      window.removeEventListener("popstate", onPop);
      if (tokenRef.current !== token) return;
      // 不是返回手势关的（点了 ×、遮罩等）：把自己那条退掉。
      tokenRef.current = null;
      if (readState()?.meowoLayer === token) window.history.back();
    };
  }, [open, enabled]);

  /**
   * 浮层关闭的同时要进入对话（新建浮层启动成功）：不退历史，而是把浮层那条原地换成对话记录，
   * 对话窗看到 meowoChat 就不再压第二条。浮层之前已经在对话里（state 已有 meowoChat）时直接退掉即可。
   */
  const handOffToChat = useCallback(() => {
    const token = tokenRef.current;
    if (!token) return;
    tokenRef.current = null;
    if (readState()?.meowoLayer !== token) return;
    const prior = priorRef.current ?? {};
    if (prior.meowoChat === true) {
      window.history.back();
      return;
    }
    try {
      window.history.replaceState({ ...prior, meowoChat: true }, "");
    } catch {
      /* 拿不到历史时什么都不做 */
    }
  }, []);

  return { handOffToChat };
}
