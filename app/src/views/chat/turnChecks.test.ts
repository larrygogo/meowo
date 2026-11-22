import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { checkKindsOf, collectTurnChecks, isDocPath, parseOutput, splitCommand } from "./turnChecks";

let seq = 0;
function run(command: string, result?: { text?: string; is_error?: boolean; interrupted?: boolean } | null, viaSummary = false): ChatItem[] {
  const id = `t${(seq += 1)}`;
  const use = {
    type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command,
    detail: viaSummary ? null : { kind: "command", command, description: null },
  } as ChatItem;
  if (result === null) return [use];
  const res = { type: "tool_result", id: `${id}r`, timestamp: null, tool_use_id: id, text: result?.text ?? "", is_error: result?.is_error ?? false, interrupted: result?.interrupted } as ChatItem;
  return [use, res];
}

describe("认检查类别", () => {
  it.each([
    ["bunx tsc --noEmit", ["typecheck"]],
    ["npx vue-tsc -b", ["typecheck"]],
    ["cargo check -p meowo-agent", ["typecheck"]],
    ["bun run typecheck", ["typecheck"]],
    ["bunx vitest run src/views", ["test"]],
    ["npm test", ["test"]],
    ["pnpm run test:unit", ["test"]],
    ["cargo test --lib", ["test"]],
    ["go test ./...", ["test"]],
    ["python -m pytest -q", ["test"]],
    ["uv run pytest", ["test"]],
    ["bun test", ["test"]],
    ["npx playwright test", ["test"]],
    ["bun run lint", ["lint"]],
    ["npx eslint src", ["lint"]],
    ["cargo clippy --all-targets", ["lint"]],
    ["ruff check .", ["lint"]],
    ["cargo build --release", ["build"]],
    ["npm run build", ["build"]],
    ["CI=1 bunx vitest run", ["test"]],
    ["& npx tsc --noEmit", ["typecheck"]],
  ])("%s", (command, kinds) => {
    expect(checkKindsOf(command)).toEqual(kinds);
  });

  it.each(["grep -rn vitest src", "cat tsconfig.json", "git log --grep=test", "rg tsc", "ls tests", "Get-Content package.json", "echo npm test"])("不是检查：%s", (command) => {
    expect(checkKindsOf(command)).toEqual([]);
  });
});

describe("切命令", () => {
  it("&& ; || 管道各自记下，引号里不切", () => {
    const segments = splitCommand(`cd app && bunx vitest run 2>&1 | tail -5; echo "a && b"`);
    expect(segments.map((s) => [s.text, s.piped, s.prev, s.next])).toEqual([
      ["cd app", false, null, "&&"],
      ["bunx vitest run 2>&1", true, "&&", ";"],
      [`echo "a && b"`, false, ";", null],
    ]);
  });
});

describe("从输出认汇总", () => {
  it("vitest / jest / cargo / pytest / bun / go", () => {
    expect(parseOutput(["test"], " Test Files  3 passed (3)\n      Tests  3 failed | 159 passed (162)")).toEqual({ failed: 3, passed: 159, unit: "tests" });
    expect(parseOutput(["test"], "Tests:       1 failed, 161 passed, 162 total")).toEqual({ failed: 1, passed: 161, unit: "tests" });
    expect(parseOutput(["test"], "test result: ok. 40 passed; 0 failed; 0 ignored\ntest result: FAILED. 5 passed; 2 failed;")).toEqual({ failed: 2, passed: 45, unit: "tests" });
    expect(parseOutput(["test"], "===== 3 failed, 159 passed in 2.31s =====")).toEqual({ failed: 3, passed: 159, unit: "tests" });
    expect(parseOutput(["test"], " 162 pass\n 0 fail\n")).toEqual({ failed: 0, passed: 162, unit: "tests" });
    expect(parseOutput(["test"], "--- FAIL: TestX (0.00s)\nFAIL\tpkg\t0.1s")?.failed).toBe(1);
    expect(parseOutput(["test"], "ok  \tpkg/a\t0.12s")).toEqual({ failed: 0, passed: null, unit: "tests" });
  });

  it("tsc 与 eslint 的错误数；只有警告不算失败", () => {
    expect(parseOutput(["typecheck"], "src/a.ts(3,1): error TS2304: x\nsrc/b.ts(1,1): error TS2322: y")).toEqual({ failed: 2, passed: null, unit: "errors" });
    expect(parseOutput(["lint"], "✖ 5 problems (3 errors, 2 warnings)")).toEqual({ failed: 3, passed: null, unit: "problems" });
    expect(parseOutput(["lint"], "✖ 2 problems (0 errors, 2 warnings)")).toEqual({ failed: 0, passed: null, unit: "problems" });
  });

  it("带颜色码的输出照样认", () => {
    expect(parseOutput(["test"], "\x1b[2m      Tests \x1b[22m \x1b[1m\x1b[32m162 passed\x1b[39m\x1b[22m")).toEqual({ failed: 0, passed: 162, unit: "tests" });
  });
});

describe("这一轮的检查", () => {
  it("退出码可信：成功即通过，失败即失败", () => {
    const checks = collectTurnChecks([...run("bunx tsc --noEmit"), ...run("cd app && cargo test", { is_error: true, text: "error" })]);
    expect(checks.map((c) => [c.kinds, c.status])).toEqual([[["typecheck"], "pass"], [["test"], "fail"]]);
  });

  it("管道、|| true、; echo：退出码不算数，从输出认，认不出写看不出", () => {
    const checks = collectTurnChecks([
      ...run("bunx vitest run 2>&1 | tail -3", { text: "Tests  2 failed | 10 passed (12)" }),
      ...run("cargo clippy || true"),
      ...run("bunx tsc --noEmit; echo done"),
    ]);
    expect(checks.map((c) => [c.kinds[0], c.status, c.note.kind])).toEqual([
      ["test", "fail", "failed"],
      ["lint", "unknown", "unclear"],
      ["typecheck", "unknown", "unclear"],
    ]);
  });

  it("输出显示有失败时，哪怕退出码是 0 也算失败", () => {
    const [check] = collectTurnChecks(run("npm test", { text: "Tests:       1 failed, 3 passed, 4 total" }));
    expect(check.status).toBe("fail");
    expect(check.note).toEqual({ kind: "failed", count: 1, unit: "tests" });
  });

  it("被打断、后台运行、没有回执都不算通过", () => {
    const checks = collectTurnChecks([
      ...run("bunx tsc --noEmit", { interrupted: true, is_error: true }),
      ...run("cargo test", { text: "Command running in background with ID: b1" }),
      ...run("bun run lint", null),
    ]);
    expect(checks.map((c) => [c.status, c.note.kind])).toEqual([["unknown", "interrupted"], ["unknown", "background"], ["unknown", "missing"]]);
  });

  it("同一类跑了几次只留最后一次；一条命令里的几类合成一行", () => {
    const checks = collectTurnChecks([
      ...run("bunx vitest run", { is_error: true, text: "Tests  1 failed | 9 passed (10)" }),
      ...run("bunx tsc --noEmit && bunx vitest run", { text: "Tests  10 passed (10)" }),
    ]);
    expect(checks).toHaveLength(1);
    expect(checks[0].kinds).toEqual(["typecheck", "test"]);
    expect(checks[0].status).toBe("pass");
    expect(checks[0].note).toEqual({ kind: "passed", count: 10 });
  });

  it("前面的段失败、检查没跑到：看不出，不冤枉检查", () => {
    const [check] = collectTurnChecks(run("bun install && bunx vitest run", { is_error: true, text: "install failed" }));
    expect(check.status).toBe("unknown");
  });

  it("PowerShell 先设环境变量再跑：失败归到检查上", () => {
    const [check] = collectTurnChecks(run("$env:CARGO_TARGET_DIR='C:\\t'; cargo test", { is_error: true, text: "boom" }));
    expect(check.status).toBe("fail");
  });

  it("没有命令详情（只有摘要）时用摘要认", () => {
    expect(collectTurnChecks(run("cargo test", {}, true))[0].kinds).toEqual(["test"]);
  });

  it("不是检查的命令不出现", () => {
    expect(collectTurnChecks([...run("git status"), ...run("grep -rn vitest .")])).toEqual([]);
  });
});

describe("文档类文件", () => {
  it.each([["README.md", true], ["docs/guide.mdx", true], [".lead/x/SPEC.md", true], ["assets/a.png", true], ["src/a.ts", false], ["styles.css", false]])("%s", (path, doc) => {
    expect(isDocPath(path)).toBe(doc);
  });
});
