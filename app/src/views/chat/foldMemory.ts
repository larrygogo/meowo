/// 对话里各折叠块（思考、工具、活动汇总、子任务、命令输出）的开合记忆，按会话分份。
///
/// 为什么按会话、为什么要记：此前记忆要么在组件 state 里（换会话、流式重建一重挂载就丢），
/// 要么是一张全局表、换会话时整张清空——切到别的会话看一眼再回来，刚展开的工具输出全折回去。
/// 对照 Kimi：对话区注入一份 history-state，组件按 `thinking:open`、`tool:<id>:open` 这类
/// 键存取，最多留最近 4 个会话的那份。进会话仍然落在最新消息（见 useStickToBottom），
/// 只是「怎么展开的」原样还给用户。

import { createContext, useContext, useEffect, useRef, useState, type SyntheticEvent } from "react";

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

/// 「跳到这一条」：回复末尾的检查行点一下，回到过程里的那次命令。目标可能套在几层收起的折叠里
/// （回合折叠「工作了 2m」、活动汇总、工具行），收起时里面的内容根本没挂载，所以分两步：
/// 已挂载的折叠收到通知、认出自己装着目标就展开；随后才挂载的内层折叠在挂载时看一眼目标。
/// 目标只留一小会儿（REVEAL_TTL_MS），之后挂载的折叠不再被它弹开。
type RevealTarget = { id: string; keys: ReadonlySet<string>; until: number };
let revealTarget: RevealTarget | null = null;
const revealListeners = new Set<() => void>();
const REVEAL_TTL_MS = 1500;

function revealHits(key: string, contains?: (id: string) => boolean): boolean {
  const target = revealTarget;
  if (!target || Date.now() > target.until) return false;
  return target.keys.has(key) || !!contains?.(target.id);
}

/** 展开装着 id 的各层折叠：keys 是已知的折叠键（回合折叠、工具行），其余由折叠自己按 contains 认。 */
export function revealItem(id: string, keys: string[]): void {
  revealTarget = { id, keys: new Set(keys), until: Date.now() + REVEAL_TTL_MS };
  for (const listener of [...revealListeners]) listener();
}

/// 开合状态：首挂载先读记忆，没有才用 initial；每次改动写回记忆。
/// contains：这个折叠里装着哪些条目（「跳到这一条」用，见 revealItem）。
export function useRememberedOpen(key: string, initial: boolean, contains?: (id: string) => boolean): [boolean, (open: boolean) => void] {
  const scope = useContext(FoldScopeContext);
  const [open, setOpenState] = useState(() => {
    if (revealHits(key, contains)) {
      scope?.set(key, true);
      return true;
    }
    return scope?.get(key) ?? initial;
  });
  const setOpen = (next: boolean) => {
    scope?.set(key, next);
    setOpenState(next);
  };
  // contains 每次渲染都是新函数，放 ref 里：订阅只随 key 变，不在每次渲染时重挂（长会话里有几百个折叠）。
  const containsRef = useRef(contains);
  containsRef.current = contains;
  useEffect(() => {
    const listener = () => {
      if (!revealHits(key, containsRef.current)) return;
      scope?.set(key, true);
      setOpenState(true);
    };
    revealListeners.add(listener);
    return () => {
      revealListeners.delete(listener);
    };
  }, [key, scope]);
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
