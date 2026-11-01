/// 对话里各折叠块（思考、工具、活动汇总、子任务、命令输出）的开合记忆，按会话分份。
///
/// 为什么按会话、为什么要记：此前记忆要么在组件 state 里（换会话、流式重建一重挂载就丢），
/// 要么是一张全局表、换会话时整张清空——切到别的会话看一眼再回来，刚展开的工具输出全折回去。
/// 对照 Kimi：对话区注入一份 history-state，组件按 `thinking:open`、`tool:<id>:open` 这类
/// 键存取，最多留最近 4 个会话的那份。进会话仍然落在最新消息（见 useStickToBottom），
/// 只是「怎么展开的」原样还给用户。

import { createContext, useContext, useState, type SyntheticEvent } from "react";

/** 最多记几个会话的开合（取自 Kimi 的 history-state 上限）。再多的会话很少来回切，
 *  且每份都随长会话增长，不值得常驻。 */
export const FOLD_SESSION_LIMIT = 4;

export type FoldScope = Map<string, boolean>;

const scopes = new Map<number, FoldScope>();

/// 取某会话的那份记忆；取用即算最近使用，超出上限淘汰最久没碰的。
export function foldScope(sessionId: number): FoldScope {
  let scope = scopes.get(sessionId);
  if (scope) scopes.delete(sessionId);
  else scope = new Map();
  scopes.set(sessionId, scope);
  while (scopes.size > FOLD_SESSION_LIMIT) scopes.delete(scopes.keys().next().value!);
  return scope;
}

/** 测试用：清空全部会话的记忆。 */
export function resetFoldMemory(): void {
  scopes.clear();
}

/// Transcript 注入当前会话的那份；没有注入（单测、独立渲染）时记忆只活在组件里。
export const FoldScopeContext = createContext<FoldScope | null>(null);

/// 开合状态：首挂载先读记忆，没有才用 initial；每次改动写回记忆。
export function useRememberedOpen(key: string, initial: boolean): [boolean, (open: boolean) => void] {
  const scope = useContext(FoldScopeContext);
  const [open, setOpenState] = useState(() => scope?.get(key) ?? initial);
  const setOpen = (next: boolean) => {
    scope?.set(key, next);
    setOpenState(next);
  };
  return [open, setOpen];
}

/// 原生 <details> 的开合：open 由记忆给初值，用户点开/收起经 onToggle 写回。
/// 流式更新不会改 state，所以不会把用户收起的块弹开。
export function useRememberedDetails(key: string, initial = false, onChange?: (open: boolean) => void) {
  const [open, setOpen] = useRememberedOpen(key, initial);
  return {
    open: open || undefined,
    onToggle: (event: SyntheticEvent<HTMLDetailsElement>) => {
      const next = event.currentTarget.open;
      if (next !== open) setOpen(next);
      onChange?.(next);
    },
  };
}
