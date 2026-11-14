/// 会话级状态：只属于「对话窗此刻打开的这个会话」的界面状态——转圈、等着的选择、超时计时器、
/// 一次性的提示条。换会话时它们必须归零，不然旧会话的转圈和报错会挂到新会话上。
///
/// 为什么要有：归零原先靠 ChatWindow 的 resetTo 里手工列一串 `setXxx(...)`，声明在几千行之外，
/// 新增一个状态很容易忘了补（走查输入 A5：用终端菜单选模型的途中切会话，转圈一直转，12 秒后
/// 超时报错挂到新会话上，错误条的「重试」还会在新会话里按旧 id 选模型）。
///
/// 做法：把「归零」和「声明」写在同一行。用下面几个 hook 声明的状态，`scope.reset()` 时自动
/// 回到初始值、计时器自动取消。resetTo 只调一次 `scope.reset()`，不再逐个列举。
/// **新增的会话级状态一律用这组 hook 声明**；resetTo 里的旧清单随手迁进来，不再往里加新行。

import { useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";

export type SessionScope = {
  /// 登记一条归零动作。key 是声明处的固定身份，同一处重复登记只留最后一次（每次渲染都会登记）。
  register: (key: symbol, reset: () => void) => void;
  /// 换会话时调用：所有登记过的状态归零。
  reset: () => void;
};

export function useSessionScope(): SessionScope {
  const scopeRef = useRef<SessionScope | null>(null);
  if (!scopeRef.current) {
    const resets = new Map<symbol, () => void>();
    scopeRef.current = {
      register: (key, reset) => { resets.set(key, reset); },
      reset: () => { for (const reset of resets.values()) reset(); },
    };
  }
  return scopeRef.current;
}

function useKey(): symbol {
  return useRef(Symbol("session-scoped")).current;
}

/// 同 useState，换会话时回到初始值。
export function useSessionState<T>(scope: SessionScope, initial: T): [T, Dispatch<SetStateAction<T>>] {
  const [value, setValue] = useState(initial);
  const initialRef = useRef(initial);
  scope.register(useKey(), () => setValue(initialRef.current));
  return [value, setValue];
}

/// 同 useRef，换会话时回到初始值。
export function useSessionRef<T>(scope: SessionScope, initial: T): MutableRefObject<T> {
  const ref = useRef(initial);
  const initialRef = useRef(initial);
  scope.register(useKey(), () => { ref.current = initialRef.current; });
  return ref;
}

export type SessionTimer = {
  /// 重新计时（上一次没到点的作废）。
  start: (run: () => void, ms: number) => void;
  cancel: () => void;
};

/// 一个只为当前会话计时的超时器：换会话时自动取消，到点的回调不会落到别的会话上。
export function useSessionTimer(scope: SessionScope): SessionTimer {
  const idRef = useRef<number | undefined>(undefined);
  const timerRef = useRef<SessionTimer | null>(null);
  if (!timerRef.current) {
    const cancel = () => { window.clearTimeout(idRef.current); idRef.current = undefined; };
    timerRef.current = {
      start: (run, ms) => { cancel(); idRef.current = window.setTimeout(() => { idRef.current = undefined; run(); }, ms); },
      cancel,
    };
  }
  const timer = timerRef.current;
  scope.register(useKey(), timer.cancel);
  return timer;
}
