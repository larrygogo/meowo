// 已合并 worktree 的清理：前端共用的判定读取、原因文案、删除流程和撤销条按钮。
// 判定全在后端（worktree.rs），这里只负责展示和两道确认：删除前一道（说清删哪个目录、哪个分支，
// 不能撤销）；有程序占着目录时再一道（列出程序名，确认后才结束它们）。
// 手机端同一套：appConfirm 在手机浏览器里落到 window.confirm（mobile/transport.ts）。
import { useEffect, useState } from "react";
import {
  getSessionAdminPage,
  listCleanableWorktrees,
  removeSessionWorktree,
  worktreeStatus,
  type WorktreeStatusDto,
} from "./api";
import { appConfirm } from "./confirm";
import { formatBackendError } from "./i18n/errors";
import type { Dict } from "./i18n/zh";
import { UNNAMED_SESSION_SENTINEL } from "./views/sticker/types";

/** 这些原因说明根本不是 worktree（或目录已经没了），菜单里不必出现「删除 worktree」这一项。 */
const NOT_APPLICABLE = new Set(["not_worktree", "not_repo", "dir_missing"]);

export function worktreeApplicable(status: WorktreeStatusDto | null): boolean {
  return status == null || status.eligible || !NOT_APPLICABLE.has(status.reason ?? "");
}

/** 被 git 忽略、只在本地的文件（.env、.lead/ 等）：删 worktree 时一起没掉，确认框里必须说出来。 */
function ignoredNote(t: Dict, ignored: string[] | undefined): string {
  return ignored && ignored.length > 0 ? "\n\n" + t.worktree.ignoredNote(ignored.join("\n")) : "";
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
    case "nested_repo": return r.nested_repo(n);
    case "hidden_changes": return r.hidden_changes(n);
    case "query_failed": return r.query_failed;
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
        // 查询失败不能当成「不是 worktree」藏掉菜单项：置灰并说明（规格要求常驻、置灰、写原因）。
        if (alive) setStatus({ path: null, branch: null, prNumber: null, eligible: false, reason: "query_failed", count: null, ignored: [] });
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
  status: Pick<WorktreeStatusDto, "path" | "branch" | "ignored">,
  askFirst = true,
): Promise<RemovalOutcome> {
  if (askFirst) {
    const yes = await appConfirm(t.worktree.confirmMessage(status.path ?? "", status.branch ?? "") + ignoredNote(t, status.ignored), {
      title: t.worktree.confirmTitle,
      danger: true,
      confirmLabel: t.worktree.remove,
    });
    if (!yes) return { kind: "cancelled" };
  }
  try {
    // 后端只结束用户在确认框里看过的那批进程：确认后又冒出新进程时它会再返回占用清单，
    // 这里就再问一次（最多三轮，防止有进程不停地重新拉起）。
    const expected = status.path ?? "";
    let confirmed: number[] = [];
    let result = await removeSessionWorktree(sessionId, expected, confirmed);
    for (let round = 0; !result.removed && result.blockers.length > 0; round += 1) {
      if (round >= 3) return { kind: "failed", message: t.worktree.busyLoop };
      const names = result.blockers.map((p) => `${p.name} (${p.pid})`).join("\n");
      const kill = await appConfirm(t.worktree.busyMessage(names), {
        title: t.worktree.busyTitle,
        danger: true,
        confirmLabel: t.worktree.busyConfirm,
      });
      if (!kill) return { kind: "cancelled" };
      confirmed = result.blockers.map((p) => p.pid);
      result = await removeSessionWorktree(sessionId, expected, confirmed);
    }
    if (!result.removed) return { kind: "failed", message: t.worktree.reason.unknown };
    return { kind: "removed", branchDeleted: result.branchDeleted };
  } catch (error) {
    return { kind: "failed", message: formatBackendError(error, t.locale) };
  }
}

/**
 * 批量清理已完成会话的 worktree（会话管理页和侧栏「已完成」页共用；手机端走后者）。
 * 清单整体确认一次，之后逐个删；某个 worktree 有程序占着时仍会单独问——结束哪个程序
 * 必须让用户看到名字。返回给用户看的结果文字。
 */
export async function runBatchCleanup(t: Dict): Promise<string> {
  try {
    const all = await listCleanableWorktrees();
    const ready = all.filter((item) => item.status.eligible);
    if (ready.length === 0) return t.worktree.batchNone;
    // 会话名：拉一页最近的已完成会话对上 id（最多 200 条），对不上的只显示目录。
    const titles = new Map<number, string>();
    const page = await getSessionAdminPage({ workspaces: [], status: "done", updatedBeforeDays: null, page: 0, pageSize: 200 }).catch(() => null);
    for (const row of page?.items ?? []) {
      if (row.task_title && row.task_title !== UNNAMED_SESSION_SENTINEL) titles.set(row.session.id, row.task_title);
    }
    const lines = ready.map((item) => {
      const name = item.sessionIds.map((id) => titles.get(id)).find(Boolean);
      const extra = item.status.ignored.length > 0 ? `\n    ${t.worktree.ignoredInline(item.status.ignored.join(", "))}` : "";
      return `• ${name ? `${name} — ` : ""}${item.status.path ?? ""} (${item.status.branch ?? ""})${extra}`;
    });
    const yes = await appConfirm(t.worktree.batchMessage(lines.join("\n"), all.length - ready.length), {
      title: t.worktree.batchTitle,
      danger: true,
      confirmLabel: t.worktree.remove,
    });
    if (!yes) return "";
    let ok = 0;
    const failures: string[] = [];
    for (const item of ready) {
      const outcome = await runWorktreeRemoval(t, item.sessionIds[0], item.status, false);
      if (outcome.kind === "removed") ok += 1;
      else if (outcome.kind === "failed") failures.push(t.worktree.batchFailedItem(item.status.path ?? "", outcome.message));
    }
    return [t.worktree.batchDone(ok, failures.length), ...failures].join("\n");
  } catch (error) {
    return formatBackendError(error, t.locale);
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
