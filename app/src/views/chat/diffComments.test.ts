import { describe, expect, it } from "vitest";
import { diffLineAnchors, diffLineNumbers, formatReviewComments, type ReviewComment } from "./diffComments";

describe("diffLineNumbers", () => {
  it("删除行只有旧行号、新增行只有新行号、上下文两边都有；头部行没有", () => {
    const diff = ["diff --git a/x b/x", "--- a/x", "+++ b/x", "@@ -10,3 +20,3 @@", " keep", "-old", "+new", " tail"].join("\n");
    expect(diffLineNumbers(diff)).toEqual([
      null, null, null, null,
      { old: 10, new: 20 },
      { old: 11, new: null },
      { old: null, new: 21 },
      { old: 12, new: 22 },
    ]);
  });
});

describe("diffLineAnchors", () => {
  it("按 hunk 头起算行号:删除行记旧文件行号,新增与上下文行记新文件行号", () => {
    const diff = [
      "diff --git a/src/a.ts b/src/a.ts",
      "--- a/src/a.ts",
      "+++ b/src/a.ts",
      "@@ -10,3 +10,3 @@ fn",
      " keep",
      "-old",
      "+new",
      " tail",
      "\\ No newline at end of file",
      "@@ -40 +40,2 @@",
      "+added",
      "",
    ].join("\n");
    expect(diffLineAnchors(diff)).toEqual([
      null, null, null, null,
      { side: "new", line: 10, code: "keep" },
      { side: "old", line: 11, code: "old" },
      { side: "new", line: 11, code: "new" },
      { side: "new", line: 12, code: "tail" },
      null, null,
      { side: "new", line: 40, code: "added" },
      null,
    ]);
  });
});

describe("formatReviewComments", () => {
  const base: Omit<ReviewComment, "id" | "path" | "line" | "text"> = { dir: "C:\\repo", side: "new", code: "  const x = a + b;  " };
  const labels = { intro: "我对改动有几条意见:", removedLine: "删除的行" };

  it("一条一段:路径:行号、引用代码、意见;删除行标出来,附加目录写绝对路径", () => {
    const text = formatReviewComments([
      { ...base, id: "1", path: "src/a.ts", line: 42, text: "a 可能为空\n" },
      { ...base, id: "2", path: "src/b.ts", line: 7, side: "old", code: "", text: "别删" },
      { ...base, id: "3", dir: "D:\\lib", path: "x/y.rs", line: 3, text: "这里也改" },
    ], "C:\\repo", labels);
    expect(text).toBe([
      "我对改动有几条意见:",
      "",
      "src/a.ts:42",
      "> const x = a + b;",
      "a 可能为空",
      "",
      "src/b.ts:7 (删除的行)",
      "别删",
      "",
      "D:\\lib\\x\\y.rs:3",
      "> const x = a + b;",
      "这里也改",
    ].join("\n"));
  });
});
