import { useCallback } from "react";
import { setArchived } from "../api";

/** 归档成功后在窗口上广播（detail.ids = 成功归档的会话）。侧栏据此提示「worktree 可以清理了」：
 *  归档入口有好几个（侧栏、对话窗标题菜单、批量），提示只放在侧栏一处（二期包 5「归档后提示清理」）。 */
export const SESSION_ARCHIVED_EVENT = "meowo:session-archived";

/**
 * 归档后「自动跳到下一条」的统一取序（C-13）。此前侧栏按本地 ordered 取首条幸存项、
 * 对话窗另发后端查询按默认排序取第一条，两个入口的跳转目标不一致、对用户不可预期。
 * 现在统一为：按**用户当前看到的列表顺序**（侧栏可见顺序或其镜像），取当前条之后的
 * 第一条幸存项，没有则往前找，再退到任意幸存首条；全被归档返回 null（留在原地——
 * 空窗比自作主张关窗好）。
 */
export function pickNextAfterArchive(
  visibleOrder: readonly number[],
  archivedIds: readonly number[],
  activeId: number
): number | null {
  const removed = new Set(archivedIds);
  const at = visibleOrder.indexOf(activeId);
  if (at >= 0) {
    for (let i = at + 1; i < visibleOrder.length; i += 1) {
      if (!removed.has(visibleOrder[i])) return visibleOrder[i];
    }
    for (let i = at - 1; i >= 0; i -= 1) {
      if (!removed.has(visibleOrder[i])) return visibleOrder[i];
    }
  }
  return visibleOrder.find((id) => !removed.has(id)) ?? null;
}

type ErrorMessage = string | ((reason: unknown) => string);

const resolveMessage = (message: ErrorMessage, reason: unknown) =>
  typeof message === "function" ? message(reason) : message;

/**
 * 归档相关的会话动作（C-13）：侧栏条目菜单/批量归档与对话窗标题菜单共用同一条
 * set_archived、同一个「下一条」取序、同一个 8s 撤销条。调用方只负责自己那部分
 * 乐观 UI（侧栏摘列表项、对话窗翻转 archived 标志）与失败兜底（重取/回滚）。
 */
export function useSessionActions({
  onNavigate,
  onError,
}: {
  /** 归档了当前打开的会话后跳「下一条」（取序见 pickNextAfterArchive）。 */
  onNavigate: (id: number) => void;
  /** 部分/全部失败或撤销失败时上报（文案经 errorMessage 参数给到）。 */
  onError: (message: string) => void;
}) {
  /**
   * 归档一批会话（单条归档走同一入口）。visibleOrder 是用户当前可见的列表顺序
   * （侧栏 ordered，或 ChatWindow 持有的侧栏顺序镜像）；归档的含当前打开的会话时，
   * 乐观地立即跳到取序后的下一条（镜像为空时退回 fallbackNext 现查）——与调用方
   * 乐观摘除列表项同一节奏，失败由 onError + onFailure 兜底。
   */
  const archive = useCallback(
    (args: {
      ids: number[];
      visibleOrder: readonly number[];
      activeId: number;
      /** 可见顺序为空（侧栏收起/镜像未建立）时兜底取「下一条」（现查后端）。 */
      fallbackNext?: () => Promise<number | null>;
      /** 部分/全部失败时的兜底：调用方拉回真实列表或回滚乐观更新。 */
      onFailure?: () => void;
      /** 失败提示文案；函数形式可拿到首个失败原因。 */
      errorMessage: ErrorMessage;
    }): Promise<void> => {
      const { ids, visibleOrder, activeId, fallbackNext, onFailure, errorMessage } = args;
      if (ids.length === 0) return Promise.resolve();
      if (ids.includes(activeId)) {
        const next = pickNextAfterArchive(visibleOrder, ids, activeId);
        if (next != null) onNavigate(next);
        else if (fallbackNext) {
          void fallbackNext()
            .then((id) => { if (id != null) onNavigate(id); })
            .catch(() => {});
        }
      }
      return Promise.allSettled(ids.map((id) => setArchived(id, true))).then((results) => {
        const ok = ids.filter((_, index) => results[index].status === "fulfilled");
        if (ok.length > 0) window.dispatchEvent(new CustomEvent(SESSION_ARCHIVED_EVENT, { detail: { ids: ok } }));
        if (ok.length < ids.length) {
          const reason = (results.find((entry) => entry.status === "rejected") as PromiseRejectedResult | undefined)?.reason;
          onError(resolveMessage(errorMessage, reason));
          onFailure?.();
        }
      });
    },
    [onNavigate, onError]
  );

  // 不再有归档撤销条（用户要求，2026-11-09：「不要显示这个」）。取回走侧栏「已完成」。
  return { archive };
}
