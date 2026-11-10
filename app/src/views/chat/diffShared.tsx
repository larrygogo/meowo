// 改动面板与源代码管理共用的小件（同 pathMenu.tsx 的拆分理由：别让 SourceControl /
// SidePanel 为了几个小函数静态引整个 GitDiffView）。
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { folderName } from "../../paths";
import { normPath } from "./gitDiff";

/** 本会话改过这个文件吗：仓库根是准的，严格按「根/相对路径」比（touchedByPath 允许面板目录在根下面，
 *  那是给单仓库子目录用的宽松规则，在多仓库里会把别的仓库的同名文件算进来，做空复现）。 */
export function touchedInRoot(touched: readonly string[], root: string, rel: string): boolean {
  const full = `${normPath(root)}/${normPath(rel)}`;
  return touched.some((path) => normPath(path) === full);
}

/** 只有单仓库摘要、overview 还没到时，把摘要包成一个仓库交给 SourceControl（形态不变，不重挂）。 */
export function singleRepo(cwd: string, summary: GitDiffSummaryDto): RepoChangesDto[] {
  return [{ root: cwd, name: folderName(cwd), isCwdRepo: true, summary, base: null, baseLabel: null, committed: [] }];
}

const DIFF_LAYOUT_KEY = "meowo-diff-layout";
export function readDiffLayout(): "list" | "tree" {
  try { return localStorage.getItem(DIFF_LAYOUT_KEY) === "tree" ? "tree" : "list"; } catch { return "list"; }
}
export function saveDiffLayout(layout: "list" | "tree") {
  try { localStorage.setItem(DIFF_LAYOUT_KEY, layout); } catch { /* 隐私模式：只是不记 */ }
}

/** 文件或合计的 `+12 −3`；后端数不出来（二进制等）时两项都是 null，不占位。 */
export function LineCounts({ file }: { file: { additions: number | null; deletions: number | null } | undefined }) {
  if (!file || file.additions == null || file.deletions == null) return null;
  return (
    <span className="chat-diff-lines">
      {file.additions > 0 && <span className="is-add">+{file.additions}</span>}
      {file.deletions > 0 && <span className="is-del">−{file.deletions}</span>}
    </span>
  );
}

export function FolderIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  );
}

export function FileIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6" />
    </svg>
  );
}
