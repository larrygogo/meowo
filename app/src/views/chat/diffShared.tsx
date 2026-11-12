// 改动面板与源代码管理共用的小件（同 pathMenu.tsx 的拆分理由：别让 SourceControl /
// SidePanel 为了几个小函数静态引整个 GitDiffView）。
import type { GitDiffSummaryDto, RepoChangesDto } from "../../api";
import { folderName } from "../../paths";
import { normPath } from "./gitDiff";
import { Icon } from "../../icons/Icon";
import { icFile } from "../../icons/glyphs.chat";
import { icFolder } from "../../icons/glyphs";

/** 本会话改过这个文件吗：仓库根是准的，严格按「根/相对路径」比（touchedByPath 允许面板目录在根下面，
 *  那是给单仓库子目录用的宽松规则，在多仓库里会把别的仓库的同名文件算进来，做空复现）。 */
export function touchedInRoot(touched: readonly string[], root: string, rel: string): boolean {
  const full = `${normPath(root)}/${normPath(rel)}`;
  // 折叠的未跟踪目录（rel 以 / 结尾）：本会话写过其中任何一个文件就算。
  if (rel.endsWith("/")) return touched.some((path) => normPath(path).startsWith(`${full}/`));
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
    <Icon glyph={icFolder} size={14} />
  );
}

export function FileIcon() {
  return (
    <Icon glyph={icFile} size={12} className="chat-diff-fileicon" />
  );
}
