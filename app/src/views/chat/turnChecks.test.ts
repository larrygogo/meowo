import { describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { checkKindsOf, collectTurnChecks, isDocPath, parseOutput, splitCommand } from "./turnChecks";

let seq = 0;
function run(command: string, result?: { text?: string; is_error?: boolean; interrupted?: boolean; exit_code?: number } | null, viaSummary = false): ChatItem[] {
  const id = `t${(seq += 1)}`;
  const use = {
    type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command,
    detail: viaSummary ? null : { kind: "command", command, description: null },
  } as ChatItem;
  if (result === null) return [use];
  const res = { type: "tool_result", id: `${id}r`, timestamp: null, tool_use_id: id, text: result?.text ?? "", is_error: result?.is_error ?? false, interrupted: result?.interrupted, exit_code: result?.exit_code } as ChatItem;
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
    ["npx -y tsc", ["typecheck"]],
    ["bunx --bun vitest run", ["test"]],
    ["pnpm -C app test", ["test"]],
    ["npm --prefix app test", ["test"]],
    ["pnpm --filter web test", ["test"]],
    ["yarn workspace app test", ["test"]],
    ["bun run --cwd app test", ["test"]],
    ["npm -w app run lint", ["lint"]],
    ["./node_modules/.bin/vitest run", ["test"]],
    ["make test", ["test"]],
    ["node --test", ["test"]],
    ["deno check main.ts", ["typecheck"]],
  ])("%s", (command, kinds) => {
    expect(checkKindsOf(command)).toEqual(kinds);
  });

  it.each(["grep -rn vitest src", "cat tsconfig.json", "git log --grep=test", "rg tsc", "ls tests", "Get-Content package.json", "echo npm test", "tsc --version", "npx tsc --init", "pytest --version", "vitest --help", "cargo build --help", "ruff format .", "npx webpack serve", "webpack-dev-server", "bunx vitest --watch"])("不是检查：%s", (command) => {
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
    expect(parseOutput(["lint"], "✖ 2 problems (0 errors, 2 warnings)")).toBeNull();
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
    // 几类合成的通过行不带计数：10 只是测试的用例数。
    expect(checks[0].note).toEqual({ kind: "passed", count: null });
  });

  it("前面的段失败、检查没跑到：看不出，不冤枉检查", () => {
    const [check] = collectTurnChecks(run("bun install && bunx vitest run", { is_error: true, text: "install failed" }));
    expect(check.status).toBe("unknown");
  });

  it("gemini / opencode：命令跑完不标 is_error，非零退出码照样算失败", () => {
    const checks = collectTurnChecks([
      ...run("npm run build", { exit_code: 1, text: "vite error\nExit Code: 1" }),
      ...run("cargo test", { exit_code: 101, text: "error[E0425]: x" }),
    ]);
    expect(checks.map((c) => [c.kinds[0], c.status])).toEqual([["build", "fail"], ["test", "fail"]]);
  });

  it("用例全过但测试文件没加载起来 / 有未处理的异常：算失败", () => {
    expect(collectTurnChecks(run("bunx vitest run", { is_error: true, text: " Test Files  1 failed | 3 passed (4)\n      Tests  30 passed (30)" }))[0].status).toBe("fail");
    expect(collectTurnChecks(run("bunx vitest run", { is_error: true, text: "      Tests  30 passed (30)\n     Errors  1 error" }))[0].status).toBe("fail");
    expect(collectTurnChecks(run("npx jest", { is_error: true, text: "Test Suites: 1 failed, 4 passed, 5 total\nTests:       40 passed, 40 total" }))[0].status).toBe("fail");
    // 汇总行写着全过、但退出码是 1：不写通过。
    expect(collectTurnChecks(run("bunx vitest run", { is_error: true, text: "Tests  30 passed (30)" }))[0].status).toBe("fail");
  });

  it("用户拒绝、超时：没跑完，不算失败", () => {
    const checks = collectTurnChecks([
      ...run("npm test", { is_error: true, text: "The user doesn't want to proceed with this tool use. The tool use was rejected." }),
      ...run("bun run lint", { is_error: true, text: "Command timed out after 2m 0.0s" }),
    ]);
    expect(checks.map((c) => [c.status, c.note.kind])).toEqual([["unknown", "notRun"], ["unknown", "notRun"]]);
  });

  it("tsc && vitest，tsc 报错：类型检查失败、测试没跑到，分两行；之后单独跑 tsc 通过只替换类型检查那一行", () => {
    const first = collectTurnChecks(run("bunx tsc --noEmit && bunx vitest run", { is_error: true, text: "a.ts(1,1): error TS2304: x" }));
    expect(first.map((c) => [c.kinds, c.status, c.note.kind])).toEqual([[["typecheck"], "fail", "failed"], [["test"], "unknown", "notRun"]]);
    const later = collectTurnChecks([...run("bunx tsc --noEmit && bunx vitest run", { is_error: true, text: "a.ts(1,1): error TS2304: x" }), ...run("bunx tsc --noEmit")]);
    expect(later.map((c) => [c.kinds, c.status])).toEqual([[["test"], "unknown"], [["typecheck"], "pass"]]);
  });

  it("PowerShell 用 ; 串起来：认出汇总的测试照样算通过，类型检查看不出", () => {
    const checks = collectTurnChecks(run("cd app; bunx tsc --noEmit; bunx vitest run", { text: "Tests  5 passed (5)" }));
    expect(checks.map((c) => [c.kinds, c.status])).toEqual([[["typecheck"], "unknown"], [["test"], "pass"]]);
  });

  it("做空第二轮：-v 是详细输出不是版本，照样认", () => {
    expect(checkKindsOf("pytest -v")).toEqual(["test"]);
    expect(checkKindsOf("go test -v ./...")).toEqual(["test"]);
    expect(checkKindsOf("cargo test serve")).toEqual(["test"]);
    expect(checkKindsOf("pnpm -w test")).toEqual(["test"]);
    expect(checkKindsOf("npm exec -- vitest run")).toEqual(["test"]);
    const [check] = collectTurnChecks(run("go test -v ./...", { exit_code: 1, text: "--- FAIL: TestA (0.00s)\nFAIL" }));
    expect(check.status).toBe("fail");
  });

  it("做空第二轮：测试输出里的 timed out 是失败，不是没跑完", () => {
    const [check] = collectTurnChecks(run("bunx vitest run", { is_error: true, text: "Error: Test timed out in 5000ms.\n      Tests  1 failed | 9 passed (10)" }));
    expect([check.status, check.note]).toEqual(["fail", { kind: "failed", count: 1, unit: "tests" }]);
  });

  it("做空第二轮：后一段的失败不算到前一段头上", () => {
    const clippy = collectTurnChecks(run("cargo check && cargo clippy -- -D warnings", { is_error: true, text: "error: unused\nerror: could not compile `x` due to 1 previous error" }));
    // check 与 clippy 都会打印 could not compile，分不清是谁：都写看不出，不冤枉 check。
    expect(clippy.map((c) => [c.kinds, c.status])).toEqual([[["typecheck", "lint"], "unknown"]]);
    const two = collectTurnChecks(run("cargo test -p a && cargo test -p b", { is_error: true, text: "test result: ok. 3 passed; 0 failed;\ntest result: FAILED. 1 passed; 2 failed;" }));
    expect(two.map((c) => [c.kinds, c.status])).toEqual([[["test"], "fail"]]);
    const lintFirst = collectTurnChecks(run("bun run lint && bunx vitest run", { is_error: true, text: "Tests  2 failed | 8 passed (10)" }));
    expect(lintFirst.map((c) => [c.kinds, c.status])).toEqual([[["lint"], "pass"], [["test"], "fail"]]);
  });

  it("做空第二轮：结尾的 & 是丢到后台，不算通过", () => {
    expect(collectTurnChecks(run("npx vitest run &", {}))[0].note.kind).toBe("background");
  });

  it("做空第二轮：字符串形态的 shell 包装拆开认", () => {
    expect(collectTurnChecks(run(`bash -lc 'cd app && bunx vitest run'`, { text: "Tests  3 passed (3)" }))[0].note).toEqual({ kind: "passed", count: 3 });
    expect(collectTurnChecks(run(`powershell -NoProfile -Command "npx tsc --noEmit"`, {}))[0].kinds).toEqual(["typecheck"]);
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
  it.each([["README.md", true], ["docs/guide.mdx", true], ["requirements.txt", false], ["CMakeLists.txt", false], [".lead/x/SPEC.md", true], ["assets/a.png", true], ["src/a.ts", false], ["styles.css", false]])("%s", (path, doc) => {
    expect(isDocPath(path)).toBe(doc);
  });
});
