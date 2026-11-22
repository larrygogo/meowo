// 回复末尾「本轮结果」里的检查段：这一轮跑过哪些测试、类型检查、lint、构建，最后一次各是什么结果。
//
// 全部从会话记录里推：命令原文（tool_use 的 command 详情）+ 回执（is_error 与输出）。不听 AI 自己说
// 「测试都过了」——录屏、自述都只能证明做过，不能证明对（调研 landscape.md 第 3 节）。
// 五家的命令调用都归一成 kind=run，回执都有 is_error；退出码字段只有部分家有，所以不靠它。
//
// 退出码不总可信：`vitest run | tail -20` 的退出码是 tail 的，`cargo test || true` 永远是 0，
// `tsc; echo done` 取的是 echo 的。这些情况改从输出里认常见的汇总行（vitest/jest/cargo/pytest/bun/go、
// tsc 的 error TS、eslint 的 ✖ N problems），认不出就如实写「看不出结果」，不猜。
import type { ChatItem } from "../../api";
import type { ToolResultItem } from "./shared";

export type CheckKind = "typecheck" | "test" | "lint" | "build";
export type CheckStatus = "pass" | "fail" | "unknown";
/** 结果说明：通过/失败时给计数，看不出时给原因。 */
export type CheckNote =
  | { kind: "passed"; count: number | null }
  | { kind: "failed"; count: number | null; unit: "tests" | "errors" | "problems" | null }
  | { kind: "unclear" | "interrupted" | "background" | "missing" };

export type TurnCheck = {
  /** 这条命令认出来的检查类别（一条命令可以同时是类型检查和单测：`tsc && vitest run`）。 */
  kinds: CheckKind[];
  status: CheckStatus;
  note: CheckNote;
  command: string;
  /** 对应的 tool_use id：点这一行跳回过程里的那次命令。 */
  toolId: string;
  /** 在 items 里的位置：和之后的改动比先后。 */
  at: number;
};

const KIND_ORDER: CheckKind[] = ["typecheck", "test", "lint", "build"];

// 每段命令（按 && ; || 换行切开、再取管道的第一节）去掉开头的包装后，按「开头是什么」认类别。
// 只看开头：`grep vitest`、`cat tsconfig.json`、`git log --grep=test` 都不是检查。
const RUNNER = String.raw`(?:(?:npx|bunx|pnpx|pnpm\s+(?:exec|dlx)|yarn\s+(?:exec|dlx)|bun\s+x|uv\s+run|poetry\s+run|python3?\s+-m|py\s+-m)\s+)?`;
const SCRIPT = String.raw`(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?`;
const PATTERNS: Record<CheckKind, RegExp[]> = {
  typecheck: [
    new RegExp(`^${RUNNER}(?:tsc|vue-tsc|tsgo|mypy|pyright|basedpyright)\\b`, "i"),
    /^cargo\s+(?:\+\S+\s+)?check\b/i,
    /^go\s+vet\b/i,
    new RegExp(`^${SCRIPT}(?:typecheck|type-check|check-types|types|tsc)\\b`, "i"),
  ],
  test: [
    new RegExp(`^${RUNNER}(?:vitest|jest|pytest|mocha|playwright\\s+test|ava|jasmine|phpunit|rspec)\\b`, "i"),
    /^(?:cargo\s+(?:\+\S+\s+)?(?:test|nextest)|go\s+test|bun\s+test|deno\s+test|dotnet\s+test|swift\s+test|mvn\s+(?:\S+\s+)*test|(?:\.\/)?gradlew?\s+(?:\S+\s+)*test)\b/i,
    new RegExp(`^${SCRIPT}(?:test|tests|e2e|test:[\\w:-]+)\\b`, "i"),
    /^npm\s+t\b/i,
    new RegExp(`^${RUNNER}unittest\\b`, "i"),
  ],
  lint: [
    new RegExp(`^${RUNNER}(?:eslint|oxlint|biome\\s+(?:check|lint|ci)|stylelint|ruff(?:\\s+check)?|flake8|pylint|golangci-lint|prettier\\s+(?:.*\\s)?--check)\\b`, "i"),
    /^cargo\s+(?:\+\S+\s+)?(?:clippy|fmt\s+(?:.*\s)?--check)\b/i,
    new RegExp(`^${SCRIPT}(?:lint|lint:[\\w:-]+|format:check|fmt:check)\\b`, "i"),
  ],
  build: [
    /^cargo\s+(?:\+\S+\s+)?build\b/i,
    new RegExp(`^${SCRIPT}build(?::[\\w:-]+)?\\b`, "i"),
    new RegExp(`^${RUNNER}(?:vite\\s+build|next\\s+build|tauri\\s+build|webpack|rollup\\s+-c)\\b`, "i"),
    /^(?:go\s+build|dotnet\s+build|swift\s+build|mvn\s+(?:\S+\s+)*(?:package|compile)|(?:\.\/)?gradlew?\s+(?:\S+\s+)*build)\b/i,
  ],
};

/** 不会失败、也不改变结果的前置段：切目录、设环境变量。它们排在检查前面时，失败一定来自检查。 */
const SETUP = /^(?:(?:cd|pushd|popd|set-location|sl|export|set|setlocal)\b|\$env:\w+\s*=|\w+=\S*$)/i;

type Segment = { text: string; piped: boolean; next: "&&" | "||" | ";" | null; prev: "&&" | "||" | ";" | null };

/** 把一条命令切成段：记下每段前后的连接符、有没有被管道接走。引号里的符号不切。 */
export function splitCommand(command: string): Segment[] {
  const parts: { text: string; sep: "&&" | "||" | ";" | "|" | null }[] = [];
  let buf = "";
  let quote: string | null = null;
  for (let i = 0; i < command.length; i += 1) {
    const ch = command[i];
    if (quote) {
      buf += ch;
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      buf += ch;
      continue;
    }
    const two = command.slice(i, i + 2);
    if (two === "&&" || two === "||") {
      parts.push({ text: buf, sep: two });
      buf = "";
      i += 1;
      continue;
    }
    if (ch === ";" || ch === "\n") {
      parts.push({ text: buf, sep: ";" });
      buf = "";
      continue;
    }
    // `2>&1 |`、`|&` 都按管道算；单独的 `&`（后台）不切，留在段里由下面判后台。
    if (ch === "|") {
      parts.push({ text: buf, sep: "|" });
      buf = "";
      if (command[i + 1] === "&") i += 1;
      continue;
    }
    buf += ch;
  }
  parts.push({ text: buf, sep: null });
  // 合并管道：管道后面的节是过滤器，不单独成段，只把前一段标成「被接走」。
  const segments: Segment[] = [];
  let prev: Segment["prev"] = null;
  let current: Segment | null = null;
  for (const part of parts) {
    if (!current) {
      current = { text: part.text.trim(), piped: false, next: null, prev };
    }
    if (part.sep === "|") {
      current.piped = true;
      continue;
    }
    current.next = part.sep;
    if (current.text) segments.push(current);
    prev = part.sep;
    current = null;
  }
  return segments;
}

/** 去掉段开头不影响「这是什么命令」的包装：`time`、环境变量赋值、PowerShell 的调用符 `&`。 */
function headOf(text: string): string {
  let head = text.trim();
  for (;;) {
    const next = head
      .replace(/^(?:time|command|exec|env)\s+/i, "")
      .replace(/^[A-Z_][A-Z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)\s+/i, "")
      .replace(/^&\s+/, "")
      .replace(/^\(\s*/, "");
    if (next === head) return head;
    head = next;
  }
}

export function checkKindsOf(segment: string): CheckKind[] {
  const head = headOf(segment);
  return KIND_ORDER.filter((kind) => PATTERNS[kind].some((re) => re.test(head)));
}

type Parsed = { failed: number; passed: number | null; unit: "tests" | "errors" | "problems" } | null;

function sumAll(text: string, re: RegExp): number | null {
  let total: number | null = null;
  for (const match of text.matchAll(re)) total = (total ?? 0) + Number(match[1]);
  return total;
}

/** 从输出里认汇总行。认不出返回 null（不等于通过）。 */
export function parseOutput(kinds: CheckKind[], output: string): Parsed {
  const text = output.replace(/\x1b\[[0-9;]*m/g, "");
  if (kinds.includes("test")) {
    // vitest：`Tests  3 failed | 159 passed (162)`；jest：`Tests:       1 failed, 161 passed, 162 total`
    const line = /^\s*Tests:?\s+(.+)$/im.exec(text)?.[1];
    if (line && /\b(?:passed|failed)\b/.test(line)) {
      return { failed: Number(/(\d+)\s+failed/.exec(line)?.[1] ?? 0), passed: Number(/(\d+)\s+passed/.exec(line)?.[1] ?? 0), unit: "tests" };
    }
    // cargo：每个测试二进制一行 `test result: ok. 45 passed; 0 failed; …`，加总。
    if (/test result: (?:ok|FAILED)\./.test(text)) {
      return { failed: sumAll(text, /test result: \w+\. \d+ passed; (\d+) failed/g) ?? 0, passed: sumAll(text, /test result: \w+\. (\d+) passed/g), unit: "tests" };
    }
    // pytest：`===== 3 failed, 159 passed in 2.31s =====`
    const pytest = /^=+ (.*\b(?:passed|failed)\b.*) in [\d.]+s/m.exec(text)?.[1];
    if (pytest) {
      return { failed: Number(/(\d+) failed/.exec(pytest)?.[1] ?? 0) + Number(/(\d+) errors?/.exec(pytest)?.[1] ?? 0), passed: Number(/(\d+) passed/.exec(pytest)?.[1] ?? 0), unit: "tests" };
    }
    // bun test：` 162 pass` / ` 0 fail`
    const bunPass = /^\s*(\d+) pass\s*$/m.exec(text);
    const bunFail = /^\s*(\d+) fail\s*$/m.exec(text);
    if (bunPass || bunFail) return { failed: Number(bunFail?.[1] ?? 0), passed: Number(bunPass?.[1] ?? 0), unit: "tests" };
    // go test：有 `--- FAIL` 或行首 FAIL 即失败；全是 ok 即通过。
    if (/^(?:--- FAIL|FAIL\s)/m.test(text)) return { failed: (text.match(/^--- FAIL/gm) ?? []).length || 1, passed: null, unit: "tests" };
    if (/^ok\s+\S+\s+[\d.]+s/m.test(text)) return { failed: 0, passed: null, unit: "tests" };
  }
  if (kinds.includes("typecheck")) {
    const errors = (text.match(/\berror TS\d+:/g) ?? []).length;
    if (errors > 0) return { failed: errors, passed: null, unit: "errors" };
    const found = /Found (\d+) errors?/.exec(text);
    if (found) return { failed: Number(found[1]), passed: null, unit: "errors" };
  }
  if (kinds.includes("lint")) {
    // eslint：`✖ 5 problems (3 errors, 2 warnings)`；只有警告不算失败。
    const problems = /✖ (\d+) problems? \((\d+) errors?/.exec(text);
    if (problems) return { failed: Number(problems[2]), passed: null, unit: "problems" };
  }
  if (kinds.some((kind) => kind === "typecheck" || kind === "build" || kind === "lint")) {
    // cargo check/build/clippy：`error: could not compile`
    if (/^error(?:\[E\d+\])?: /m.test(text) && /could not compile/.test(text)) {
      return { failed: sumAll(text, /due to (\d+) previous errors?/g) ?? 1, passed: null, unit: "errors" };
    }
  }
  return null;
}

const BACKGROUND = /running in background|in the background with ID|background shell|已在后台/i;

function commandOf(item: Extract<ChatItem, { type: "tool_use" }>): string {
  return item.detail?.kind === "command" ? item.detail.command : item.summary;
}

/** 一条命令里各检查段的结论。 */
function judge(segments: Segment[], index: number, kinds: CheckKind[], result: ToolResultItem | undefined): { status: CheckStatus; note: CheckNote } {
  if (!result) return { status: "unknown", note: { kind: "missing" } };
  if (result.interrupted) return { status: "unknown", note: { kind: "interrupted" } };
  if (!result.is_error && BACKGROUND.test(result.text)) return { status: "unknown", note: { kind: "background" } };
  const parsed = parseOutput(kinds, result.text);
  if (parsed && parsed.failed > 0) return { status: "fail", note: { kind: "failed", count: parsed.failed, unit: parsed.unit } };
  if (parsed && parsed.unit === "tests") return { status: "pass", note: { kind: "passed", count: parsed.passed } };
  const segment = segments[index];
  // 被管道接走：退出码是最后一节过滤器的，只能信输出。
  if (segment.piped || segment.prev === "||") return { status: "unknown", note: { kind: "unclear" } };
  const tail = segments.slice(index + 1);
  if (!result.is_error) {
    // 整条成功：后面只用 && 接着（或什么都没有），这段一定跑过且成功。后面用 ; 或 || 接着时，它失败了整条也可能成功。
    return segment.next === null || segment.next === "&&"
      ? { status: "pass", note: { kind: "passed", count: null } }
      : { status: "unknown", note: { kind: "unclear" } };
  }
  // 整条失败：这段是最后一段，且前面只有切目录这类不会失败的段（或前面的检查已认定通过），失败就是它的。
  const before = segments.slice(0, index);
  const cleanBefore = before.every((seg) => SETUP.test(headOf(seg.text)) || checkKindsOf(seg.text).length > 0);
  if (tail.length === 0 && cleanBefore) return { status: "fail", note: { kind: "failed", count: parsed?.failed ?? null, unit: parsed?.unit ?? null } };
  return { status: "unknown", note: { kind: "unclear" } };
}

/**
 * 这一段回复里的检查：每类只留最后一次（先跑失败、修好再跑通过，读者要的是现在的状态）。
 * 一条命令同时含几类时合成一行。
 */
export function collectTurnChecks(items: ChatItem[]): TurnCheck[] {
  const results = new Map<string, ToolResultItem>();
  for (const item of items) {
    if (item.type === "tool_result" && item.tool_use_id) results.set(item.tool_use_id, item);
  }
  const all: TurnCheck[] = [];
  items.forEach((item, at) => {
    if (item.type !== "tool_use" || item.kind !== "run") return;
    const command = commandOf(item).trim();
    if (!command) return;
    const segments = splitCommand(command);
    const found = new Map<CheckKind, { status: CheckStatus; note: CheckNote }>();
    segments.forEach((segment, index) => {
      const kinds = checkKindsOf(segment.text);
      if (kinds.length === 0) return;
      const verdict = judge(segments, index, kinds, results.get(item.id));
      for (const kind of kinds) found.set(kind, verdict);
    });
    if (found.size === 0) return;
    const verdicts = [...found.values()];
    // 合成一行：有一段失败就算失败；全部通过才算通过；其余看不出。
    const failed = verdicts.find((v) => v.status === "fail");
    const unknown = verdicts.find((v) => v.status === "unknown");
    const passed = verdicts.filter((v) => v.note.kind === "passed").map((v) => (v.note as { count: number | null }).count).find((count) => count != null) ?? null;
    const verdict = failed ?? unknown ?? { status: "pass" as const, note: { kind: "passed" as const, count: passed } };
    all.push({ kinds: KIND_ORDER.filter((kind) => found.has(kind)), ...verdict, command, toolId: item.id, at });
  });
  // 每类最后一次：倒着走，某行的类别全被更晚的行盖过就丢掉。
  const covered = new Set<CheckKind>();
  const kept: TurnCheck[] = [];
  for (let index = all.length - 1; index >= 0; index -= 1) {
    const check = all[index];
    if (check.kinds.every((kind) => covered.has(kind))) continue;
    check.kinds.forEach((kind) => covered.add(kind));
    kept.unshift(check);
  }
  return kept;
}

/** 文档、图片这类改了也不需要跑检查的文件。 */
export function isDocPath(path: string): boolean {
  return /\.(?:md|mdx|markdown|txt|rst|adoc|png|jpe?g|gif|webp|ico)$/i.test(path) || /(?:^|\/)\.lead\//.test(path) || /(?:^|\/)(?:LICENSE|CHANGELOG|AUTHORS)(?:\.\w+)?$/i.test(path);
}
