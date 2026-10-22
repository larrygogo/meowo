import { describe, expect, it } from "vitest";
import { buildFileTree, diffLineClass, sessionTouchedPaths, touchedByPath } from "./gitDiff";

describe("diffLineClass", () => {
  it("classifies additions and deletions", () => {
    expect(diffLineClass("+added line")).toBe("is-add");
    expect(diffLineClass("-removed line")).toBe("is-del");
  });

  it("treats file headers as meta, not add/del", () => {
    expect(diffLineClass("+++ b/src/a.ts")).toBe("is-meta");
    expect(diffLineClass("--- /dev/null")).toBe("is-meta");
    expect(diffLineClass("--- a/src/a.ts")).toBe("is-meta");
  });

  it("treats diff preamble lines as meta", () => {
    expect(diffLineClass("diff --git a/x b/x")).toBe("is-meta");
    expect(diffLineClass("index 1234567..89abcde 100644")).toBe("is-meta");
    expect(diffLineClass("new file mode 100644")).toBe("is-meta");
    expect(diffLineClass("deleted file mode 100644")).toBe("is-meta");
    expect(diffLineClass("Binary files a/x and b/x differ")).toBe("is-meta");
  });

  it("classifies hunk headers", () => {
    expect(diffLineClass("@@ -1,3 +1,4 @@ fn main()")).toBe("is-hunk");
  });

  it("leaves context and other lines unclassed", () => {
    expect(diffLineClass(" context line")).toBe("");
    expect(diffLineClass("")).toBe("");
    expect(diffLineClass("\\ No newline at end of file")).toBe("");
  });
});

describe("buildFileTree", () => {
  const file = (path: string, status = "M") => ({ path, status });

  it("returns an empty tree for no files", () => {
    expect(buildFileTree([])).toEqual([]);
  });

  it("keeps root-level files at the top level", () => {
    expect(buildFileTree([file("README.md")])).toEqual([
      { name: "README.md", path: "README.md", file: file("README.md") },
    ]);
  });

  it("groups nested directories and collapses single-child chains", () => {
    const tree = buildFileTree([
      file("src/components/Button.tsx"),
      file("src/components/Input.tsx"),
      file("src/api.ts"),
    ]);
    expect(tree).toEqual([
      {
        name: "src",
        path: "src",
        children: [
          {
            name: "components",
            path: "src/components",
            children: [
              { name: "Button.tsx", path: "src/components/Button.tsx", file: file("src/components/Button.tsx") },
              { name: "Input.tsx", path: "src/components/Input.tsx", file: file("src/components/Input.tsx") },
            ],
          },
          { name: "api.ts", path: "src/api.ts", file: file("src/api.ts") },
        ],
      },
    ]);
  });

  it("collapses deep single-child directory chains into one node", () => {
    const tree = buildFileTree([file("a/b/c/d.txt")]);
    expect(tree).toEqual([
      {
        name: "a/b/c",
        path: "a/b/c",
        children: [{ name: "d.txt", path: "a/b/c/d.txt", file: file("a/b/c/d.txt") }],
      },
    ]);
  });

  it("does not collapse a directory whose only child is a file", () => {
    const tree = buildFileTree([file("docs/guide.md")]);
    expect(tree).toEqual([
      {
        name: "docs",
        path: "docs",
        children: [{ name: "guide.md", path: "docs/guide.md", file: file("docs/guide.md") }],
      },
    ]);
  });

  it("sorts folders before files, alphabetically within each kind", () => {
    const tree = buildFileTree([
      file("zebra.ts"),
      file("beta/x.ts"),
      file("alpha.ts"),
      file("delta/y.ts"),
    ]);
    expect(tree.map((n) => n.name)).toEqual(["beta", "delta", "alpha.ts", "zebra.ts"]);
  });

  it("preserves per-file status letters", () => {
    const tree = buildFileTree([file("a.ts", "A"), file("d/gone.ts", "D")]);
    expect(tree[0].children![0].file?.status).toBe("D");
    expect(tree[1].file?.status).toBe("A");
  });
});

describe("sessionTouchedPaths / touchedByPath", () => {
  const tool = (kind: string, path: string) => ({ type: "tool_use", detail: { kind, path } });

  it("只收 Edit/Write 的路径,相对路径按会话 cwd 补全", () => {
    expect(sessionTouchedPaths([
      tool("edit", "C:\\Repo\\app\\src\\A.ts"),
      tool("read", "C:\\Repo\\README.md"),
      tool("write", "docs/new.md"),
      { type: "user_text", detail: null },
    ], "C:\\Repo\\app")).toEqual(["c:/repo/app/src/a.ts", "c:/repo/app/docs/new.md"]);
  });

  it("面板 cwd 是仓库子目录也能对上;同名文件在别的仓不算", () => {
    const touched = ["c:/repo/app/src/a.ts", "d:/other/src/b.ts"];
    expect(touchedByPath(touched, "C:\\Repo\\app", "app/src/a.ts")).toBe(true);
    expect(touchedByPath(touched, "C:\\Repo", "app/src/a.ts")).toBe(true);
    expect(touchedByPath(touched, "C:\\Repo", "src/b.ts")).toBe(false);
    expect(touchedByPath(touched, "C:\\Repo", "app/src/c.ts")).toBe(false);
  });
});
