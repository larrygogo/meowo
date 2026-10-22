import { diffLineClass } from "./gitDiff";

/** 改动面板里对某一行 diff 写的评论。`dir` 是面板当时的激活仓（主仓或某个附加目录），
 *  `path` 相对它；删除行只在旧文件里存在，所以 `side` 区分行号属于旧文件还是新文件。 */
export type ReviewComment = {
  id: string;
  dir: string;
  path: string;
  side: "old" | "new";
  line: number;
  code: string;
  text: string;
};

export type DiffLineAnchor = { side: "old" | "new"; line: number; code: string };

/// 给统一 diff 的每一行标上真实行号：按 `@@ -a,b +c,d @@` 起算，删除行走旧文件行号，
/// 新增和上下文行走新文件行号（评论发给 agent 时它要按行号找回当前文件里的位置）。
/// meta/hunk 头/空行没有可评论的代码，记 null。
export function diffLineAnchors(diff: string): (DiffLineAnchor | null)[] {
  let oldNo = 0;
  let newNo = 0;
  return diff.split("\n").map((line) => {
    const cls = diffLineClass(line);
    if (cls === "is-hunk") {
      const match = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(line);
      if (match) {
        oldNo = Number(match[1]);
        newNo = Number(match[2]);
      }
      return null;
    }
    if (cls === "is-meta" || line === "" || oldNo + newNo === 0) return null;
    const code = line.slice(1);
    if (cls === "is-del") return { side: "old", line: oldNo++, code };
    if (cls === "is-add") return { side: "new", line: newNo++, code };
    // `\ No newline at end of file` 不占行号。
    if (line.startsWith("\\")) return null;
    oldNo += 1;
    return { side: "new", line: newNo++, code };
  });
}

/// 拼成插进输入框的一段文字：一条评论一段「路径:行号 / 引用那行代码 / 意见」。
/// 纯文本，五家 CLI 都能直接读，不依赖任何一家的评论协议。附加目录里的文件写绝对路径，
/// 否则 agent 会在主仓里找同名相对路径。
export function formatReviewComments(
  comments: readonly ReviewComment[],
  mainDir: string | null,
  labels: { intro: string; removedLine: string },
): string {
  const blocks = comments.map((comment) => {
    const path = mainDir && comment.dir !== mainDir ? joinPath(comment.dir, comment.path) : comment.path;
    const where = `${path}:${comment.line}` + (comment.side === "old" ? ` (${labels.removedLine})` : "");
    const quote = comment.code.trim() ? `\n> ${comment.code.trim()}` : "";
    return `${where}${quote}\n${comment.text.trim()}`;
  });
  return `${labels.intro}\n\n${blocks.join("\n\n")}`;
}

function joinPath(dir: string, rel: string): string {
  const sep = dir.includes("\\") ? "\\" : "/";
  return dir.replace(/[\\/]+$/, "") + sep + (sep === "\\" ? rel.replace(/\//g, "\\") : rel);
}
