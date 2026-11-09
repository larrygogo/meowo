// 已合并 worktree 的清理：前端共用的判定读取、原因文案、删除流程和撤销条按钮。
// 判定全在后端（worktree.rs），这里只负责展示和两道确认：删除前一道（说清删哪个目录、哪个分支，
// 不能撤销）；有程序占着目录时再一道（列出程序名，确认后才结束它们）。
// 手机端同一套：appConfirm 在手机浏览器里落到 window.confirm（mobile/transport.ts）。
import { useEffect, useState } from "react";
import {
  removeSessionWorktree,
  worktreeStatus,
  type WorktreeStatusDto,
} from "./api";
import { appConfirm } from "./confirm";
import { formatBackendError } from "./i18n/errors";
import type { Dict } from "./i18n/zh";

/** 这些原因说明根本不是 worktree（或目录已经没了），菜单里不必出现「删除 worktree」这一项。 */
const NOT_APPLICABLE = new Set(["not_worktree", "not_repo", "dir_missing"]);

export function worktreeApplicable(status: WorktreeStatusDto | null): boolean {
  return status == null || status.eligible || !NOT_APPLICABLE.has(status.reason ?? "");
}

/** 不可清理的原因 → 一句人话。 */
export function worktreeReasonText(t: Dict, status: WorktreeStatusDto): string {
  const r = t.worktree.reason;
  const n = status.count ?? 0;
  switch (status.reason) {
    case "gh_unavailable": return r.gh_unavailable;
    case "no_pr": return r.no_pr;
    case "pr_open": return r.pr_open;
    case "pr_closed": return r.pr_closed;
    case "dirty": return r.dirty(n);
    case "untracked": return r.untracked(n);
    case "unpushed": return r.unpushed;
    case "submodule_dirty": return r.submodule_dirty;
    case "submodule_unpushed": return r.submodule_unpushed;
    case "shared_session": return r.shared_session(n);
    case "detached": return r.detached;
    case "locked": return r.locked;
    default: return r.unknown;
  }
}

/** 取一次判定（gh 要联网，约 1 秒）。sessionId 为 null 时不查。查询失败按「不可清理」处理。 */
export function useWorktreeStatus(sessionId: number | null): WorktreeStatusDto | null {
  const [status, setStatus] = useState<WorktreeStatusDto | null>(null);
  useEffect(() => {
    setStatus(null);
    if (sessionId == null) return;
    let alive = true;
    worktreeStatus(sessionId)
      .then((s) => { if (alive) setStatus(s); })
      .catch(() => {
        if (alive) setStatus({ path: null, branch: null, prNumber: null, eligible: false, reason: "not_repo", count: null });
      });
    return () => { alive = false; };
  }, [sessionId]);
  return status;
}

export type RemovalOutcome =
  | { kind: "removed"; branchDeleted: boolean }
  | { kind: "cancelled" }
  | { kind: "failed"; message: string };

/**
 * 完整删除流程。`askFirst=false` 用于批量清理——清单已经整体确认过，不再逐个问；
 * 但占用进程那一道照样逐个问：结束哪个程序必须让用户看到名字。
 */
export async function runWorktreeRemoval(
  t: Dict,
  sessionId: number,
  status: Pick<WorktreeStatusDto, "path" | "branch">,
  askFirst = true,
): Promise<RemovalOutcome> {
  if (askFirst) {
    const yes = await appConfirm(t.worktree.confirmMessage(status.path ?? "", status.branch ?? ""), {
      title: t.worktree.confirmTitle,
      danger: true,
      confirmLabel: t.worktree.remove,
    });
    if (!yes) return { kind: "cancelled" };
  }
  try {
    let result = await removeSessionWorktree(sessionId, false);
    if (!result.removed && result.blockers.length > 0) {
      const names = result.blockers.map((p) => `${p.name} (${p.pid})`).join("\n");
      const kill = await appConfirm(t.worktree.busyMessage(names), {
        title: t.worktree.busyTitle,
        danger: true,
        confirmLabel: t.worktree.busyConfirm,
      });
      if (!kill) return { kind: "cancelled" };
      result = await removeSessionWorktree(sessionId, true);
    }
    if (!result.removed) return { kind: "failed", message: t.worktree.reason.unknown };
    return { kind: "removed", branchDeleted: result.branchDeleted };
  } catch (error) {
    return { kind: "failed", message: formatBackendError(error, t.locale) };
  }
}

/**
 * 归档撤销条上的「删除 worktree」按钮：可清理才出现。点完原地换成结果文字，
 * 不另开提示条——撤销条本身就是这次归档的上下文。
 */
export function WorktreeRemoveButton({
  t,
  sessionId,
  className,
  textClassName,
  onEngage,
  onSettled,
}: {
  t: Dict;
  sessionId: number;
  className: string;
  textClassName?: string;
  /** 点下按钮时：宿主暂停撤销条的自动消失。 */
  onEngage?: () => void;
  /** 流程结束（删完、失败或取消）：宿主恢复计时。 */
  onSettled?: () => void;
}) {
  const status = useWorktreeStatus(sessionId);
  const [state, setState] = useState<"idle" | "busy" | "done" | "doneKeepBranch" | { error: string }>("idle");
  if (!status?.eligible) return null;
  if (state === "done" || state === "doneKeepBranch") {
    return (
      <span className={textClassName} role="status">
        {state === "done" ? t.worktree.removed : t.worktree.removedKeepBranch}
      </span>
    );
  }
  if (typeof state === "object") {
    return <span className={textClassName} role="alert" title={state.error}>{state.error}</span>;
  }
  return (
    <button
      type="button"
      className={className}
      disabled={state === "busy"}
      onClick={() => {
        setState("busy");
        onEngage?.();
        void runWorktreeRemoval(t, sessionId, status).then((outcome) => {
          if (outcome.kind === "removed") setState(outcome.branchDeleted ? "done" : "doneKeepBranch");
          else if (outcome.kind === "failed") setState({ error: outcome.message });
          else setState("idle");
          onSettled?.();
        });
      }}
    >
      {state === "busy" ? t.worktree.removing : t.worktree.remove}
    </button>
  );
}
