/**
 * 对话里的文件修改 → 右侧面板「更改」页签里这个文件的 diff（用户要求 2026-11-18：「对话里面的文件修改
 * 可以点击到侧边面板里看」）。
 *
 * 对话这边只拿到工具记录里的路径（绝对或相对会话目录），面板那边按仓库分组、按相对仓库根的路径列文件。
 * 这里做两边的换算，纯逻辑、不带 React 组件，好让工具行和改动卡（首屏包）引用时不把面板拖进来。
 */
import { createContext } from "react";
import type { RepoChangesDto } from "../../api";
import type { GitChangedFileDto } from "../../generated/contracts/GitChangedFileDto";
import { normPath } from "./gitDiff";

/**
 * 「在面板里看这个文件的改动」。对话窗只在桌面端提供；手机远程页没有右侧面板（只有全屏的改动页），
 * 不提供，入口就不渲染——不能留一个点了没反应的按钮。
 */
export const RevealChangeContext = createContext<((path: string) => void) | null>(null);

/** 是不是绝对路径（盘符、UNC、以 / 开头）。 */
const isAbsolute = (path: string) => /^([a-zA-Z]:)?[\\/]/.test(path);

/** 工具记录里的路径换成绝对路径：相对路径按会话目录补全（各家 agent 有的给相对路径）。 */
export function absolutePath(path: string, cwd: string | null): string {
  const clean = path.replace(/\\/g, "/");
  if (isAbsolute(path) || !cwd) return clean;
  return `${cwd.replace(/\\/g, "/").replace(/\/+$/, "")}/${clean.replace(/^\.\//, "")}`;
}

/** path 在 root 下面时返回相对 root 的路径（保留原大小写），否则 null。 */
function relativeTo(path: string, root: string): string | null {
  const base = normPath(root);
  const full = path.replace(/\\/g, "/").replace(/\/+$/, "");
  if (!normPath(full).startsWith(`${base}/`)) return null;
  return full.slice(base.length + 1);
}

export type ChangeGroup = "staged" | "work" | "committed";
/** 定位结果：找到了就是哪个仓库、哪一组、哪个文件；没找到时 root 是文件所在仓库（不在任何仓库里为 null）。 */
export type ChangeTarget =
  | { found: true; root: string; group: ChangeGroup; rel: string; file: GitChangedFileDto }
  | { found: false; root: string | null };

/**
 * 在「更改」面板的仓库清单里找这个文件。
 * - 仓库按根目录最长匹配（worktree 可能嵌在主仓库目录里）。
 * - 优先看未暂存的改动（VS Code 从别处跳进来打开的也是工作区这一份），没有再看暂存区，再看「本分支已提交」。
 * - 落在折叠的未跟踪目录里（后端只给一条 `dir/`）：按未跟踪文件补一条，面板照样能读盘显示。
 */
export function locateChange(repos: readonly RepoChangesDto[], path: string): ChangeTarget {
  let repo: RepoChangesDto | null = null;
  let rel: string | null = null;
  for (const item of repos) {
    const inside = relativeTo(path, item.root);
    if (inside === null) continue;
    if (!repo || normPath(item.root).length > normPath(repo.root).length) {
      repo = item;
      rel = inside;
    }
  }
  if (!repo || rel === null) return { found: false, root: null };
  const key = normPath(rel);
  // 先认大小写完全一致的，再退到不分大小写（Windows 上工具记录里的大小写可能和 git 给的不同）。
  const pick = (files: readonly GitChangedFileDto[]) => files.find((file) => file.path === rel) ?? files.find((file) => normPath(file.path) === key);
  const files = repo.summary.files as (GitChangedFileDto & { indexStatus?: string | null; worktreeStatus?: string | null })[];
  const hit = pick(files.filter((file) => !file.path.endsWith("/")));
  if (hit) {
    // 旧后端不分暂存区（两个字段都不给），一律当未暂存（同 SourceControl.workRows）。
    const staged = !!hit.indexStatus && !hit.worktreeStatus;
    return { found: true, root: repo.root, group: staged ? "staged" : "work", rel: hit.path, file: hit };
  }
  const folded = files.find((file) => file.path.endsWith("/") && key.startsWith(normPath(file.path) + "/"));
  if (folded) {
    return { found: true, root: repo.root, group: "work", rel, file: { path: rel, status: "U", additions: null, deletions: null, indexStatus: null, worktreeStatus: "U", untrackedFiles: null } as GitChangedFileDto };
  }
  const committed = pick(repo.committed);
  if (committed) return { found: true, root: repo.root, group: "committed", rel: committed.path, file: committed };
  return { found: false, root: repo.root };
}

/**
 * 没有改动可看时，能不能在「文件」页签打开它：「文件」浏览的是会话目录，或「更改」里列出的别的仓库（worktree）。
 * 返回要切到的根目录和相对它的路径；都不在就是 null（不给这个出路）。
 */
export function filesTarget(path: string, cwd: string, repos: readonly RepoChangesDto[]): { root: string; rel: string } | null {
  const inCwd = relativeTo(path, cwd);
  if (inCwd !== null) return { root: cwd, rel: inCwd };
  for (const repo of repos) {
    if (repo.isCwdRepo) continue;
    const rel = relativeTo(path, repo.root);
    if (rel !== null) return { root: repo.root, rel };
  }
  return null;
}
