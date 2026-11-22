// 回复末尾「本轮结果」里的检查段：这一轮跑过哪些测试、类型检查、lint、构建，最后一次各是什么结果。
//
// 全部从会话记录里推：命令原文（tool_use 的 command 详情）+ 回执（is_error、exit_code 与输出）。不听 AI 自己说
// 「测试都过了」——录屏、自述都只能证明做过，不能证明对（调研 landscape.md 第 3 节）。
// 五家的命令调用都归一成 kind=run。失败的标记各家不一样：claude、codex 标 is_error；gemini 与 opencode 命令
// 跑完就不标 is_error，非零退出码只在 exit_code 里（做空第一轮实测）。两个都看。
//
// 退出码不总可信：`vitest run | tail -20` 的退出码是 tail 的，`cargo test || true` 永远是 0，
// `tsc; echo done` 取的是 echo 的。这些情况改从输出里认常见的汇总行（vitest/jest/cargo/pytest/bun/go、
// tsc 的 error TS、eslint 的 ✖ N problems），认不出就如实写「看不出结果」，不猜。
// 宁可写「看不出」也不把没通过的写成「通过」：这一段存在的意义就是那个「✓」可信。
import type { ChatItem } from "../../api";
import type { ToolResultItem } from "./shared";

export type CheckKind = "typecheck" | "test" | "lint" | "build";
export type CheckStatus = "pass" | "fail" | "unknown";
/** 结果说明：通过/失败时给计数，看不出时给原因。 */
export type CheckNote =
  | { kind: "passed"; count: number | null }
  | { kind: "failed"; count: number | null; unit: "tests" | "errors" | "problems" | null }
  | { kind: "unclear" | "interrupted" | "background" | "missing" | "notRun" };

export type TurnCheck = {
  /** 这一行的检查类别（一条命令里结论相同的几类合成一行：`tsc && vitest run` 都过了）。 */
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

// 段开头（规范化之后，见 headOf）按「开头是什么」认类别。只看开头：`grep vitest`、`cat tsconfig.json`、
// `git log --grep=test` 都不是检查。包管理器的 `-C dir`、`--filter x`、`workspace x` 等先在 headOf 里去掉。
const RUNNER = String.raw`(?:(?:npx|bunx|pnpx|pnpm\s+(?:exec|dlx)|yarn\s+(?:exec|dlx)|bun\s+x|uv\s+run|poetry\s+run|python3?\s+-m|py\s+-m)\s+)?`;
const SCRIPT = String.raw`(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?`;
const PATTERNS: Record<CheckKind, RegExp[]> = {
  typecheck: [
    new RegExp(`^${RUNNER}(?:tsc|vue-tsc|tsgo|mypy|pyright|basedpyright)(?![\\w-])`, "i"),
    /^cargo\s+(?:\+\S+\s+)?check\b/i,
    /^(?:go\s+vet|deno\s+check)\b/i,
    new RegExp(`^${SCRIPT}(?:typecheck|type-check|check-types|types|tsc)(?![\\w-])`, "i"),
    /^make\s+(?:typecheck|check)\b/i,
  ],
  test: [
    new RegExp(`^${RUNNER}(?:vitest|jest|pytest|mocha|playwright\\s+test|ava|jasmine|phpunit|rspec)(?![\\w-])`, "i"),
    /^(?:cargo\s+(?:\+\S+\s+)?(?:test|nextest)|go\s+test|bun\s+test|deno\s+test|node\s+--test|dotnet\s+test|swift\s+test|mvn\s+(?:\S+\s+)*test|(?:\.\/)?gradlew?\s+(?:\S+\s+)*test)\b/i,
    new RegExp(`^${SCRIPT}(?:test|tests|e2e|test:[\\w:-]+)(?![\\w-])`, "i"),
    /^npm\s+t\b/i,
    new RegExp(`^${RUNNER}unittest\\b`, "i"),
    /^make\s+(?:test|tests|e2e)\b/i,
  ],
  lint: [
    new RegExp(`^${RUNNER}(?:eslint|oxlint|biome\\s+(?:check|lint|ci)|stylelint|ruff(?:\\s+check)?|flake8|pylint|golangci-lint|prettier\\s+(?:.*\\s)?--check)(?![\\w-])`, "i"),
    /^cargo\s+(?:\+\S+\s+)?(?:clippy|fmt\s+(?:.*\s)?--check)\b/i,
    /^deno\s+lint\b/i,
    new RegExp(`^${SCRIPT}(?:lint|lint:[\\w:-]+|format:check|fmt:check)(?![\\w-])`, "i"),
    /^make\s+lint\b/i,
  ],
  build: [
    /^cargo\s+(?:\+\S+\s+)?build\b/i,
    new RegExp(`^${SCRIPT}build(?::[\\w:-]+)?(?![\\w-])`, "i"),
    new RegExp(`^${RUNNER}(?:vite\\s+build|next\\s+build|tauri\\s+build|webpack(?![\\w-])(?!\\s+serve)|rollup\\s+-c)`, "i"),
    /^(?:go\s+build|dotnet\s+build|swift\s+build|mvn\s+(?:\S+\s+)*(?:package|compile)|(?:\.\/)?gradlew?\s+(?:\S+\s+)*build)\b/i,
    /^make\s+build\b/i,
  ],
};

/** 认出来也不算检查：问版本、看帮助、初始化配置、起开发服务器、格式化（改文件不是检查）。 */
const NOT_A_CHECK = /\s(?:--version|-V|--help|-h|--init|--watch|serve)(?:\s|$)|^(?:npx\s+|bunx\s+)?ruff\s+format(?!.*--check)/i;

/** 不会失败、也不改变结果的前置段：切目录、设环境变量。它们排在检查前面时，失败一定来自检查。 */
const SETUP = /^(?:(?:cd|pushd|popd|push-location|pop-location|set-location|sl|export|set|setlocal)\b|\$env:\w+\s*=|\$ErrorActionPreference\s*=|\w+=\S*$)/i;

type Connector = "&&" | "||" | ";" | null;
type Segment = { text: string; piped: boolean; next: Connector; prev: Connector };

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
  let prev: Connector = null;
  let current: Segment | null = null;
  for (const part of parts) {
    if (!current) current = { text: part.text.trim(), piped: false, next: null, prev };
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

// 包管理器自己的选项（不是脚本名）：带值的要连值一起去掉。
const PM_VALUE_FLAGS = new Set(["-c", "--prefix", "--filter", "--cwd", "-w", "--workspace", "--dir", "--cache-dir", "-p", "--package"]);

/** 去掉段开头不影响「这是什么命令」的包装：`time`、环境变量赋值、PowerShell 的调用符 `&`、
 *  `./node_modules/.bin/`、包管理器的目录/工作区选项（`pnpm -C app test` → `pnpm test`）。 */
function headOf(text: string): string {
  let head = text.trim();
  for (;;) {
    const next = head
      .replace(/^(?:time|command|exec|env)\s+/i, "")
      .replace(/^[A-Z_][A-Z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)\s+/i, "")
      .replace(/^&\s+/, "")
      .replace(/^\(\s*/, "")
      .replace(/^(?:\.[\\/])?node_modules[\\/]\.bin[\\/]/i, "");
    if (next === head) break;
    head = next;
  }
  const tokens = head.split(/\s+/);
  if (!/^(?:npm|pnpm|yarn|bun|npx|bunx|pnpx)$/i.test(tokens[0] ?? "")) return head;
  const out = [tokens[0]];
  for (let i = 1; i < tokens.length; i += 1) {
    const token = tokens[i];
    const lower = token.toLowerCase();
    // 只去掉脚本名之前的选项；遇到第一个不是选项的词（run/test/vitest…）后原样保留。
    if (out.length > 1 && out[out.length - 1].toLowerCase() !== "run") {
      out.push(...tokens.slice(i));
      break;
    }
    if (lower === "workspace" && tokens[0].toLowerCase() === "yarn") {
      i += 1;
      continue;
    }
    if (lower.startsWith("-")) {
      if (PM_VALUE_FLAGS.has(lower) && !token.includes("=")) i += 1;
      continue;
    }
    out.push(token);
  }
  return out.join(" ");
}

export function checkKindsOf(segment: string): CheckKind[] {
  const head = headOf(segment);
  if (NOT_A_CHECK.test(` ${head}`) || NOT_A_CHECK.test(head)) return [];
  return KIND_ORDER.filter((kind) => PATTERNS[kind].some((re) => re.test(head)));
}

type Parsed = { failed: number; passed: number | null; unit: "tests" | "errors" | "problems" | null } | null;

function sumAll(text: string, re: RegExp): number | null {
  let total: number | null = null;
  for (const match of text.matchAll(re)) total = (total ?? 0) + Number(match[1]);
  return total;
}

/** 从输出里认汇总行。认不出返回 null（不等于通过）。 */
export function parseOutput(kinds: CheckKind[], output: string): Parsed {
  const text = output.replace(/\x1b\[[0-9;]*m/g, "");
  if (kinds.includes("test")) {
    // 测试文件没加载起来（语法错、import 坏了）、未处理的异常：用例数可能全过，但这一轮没通过。
    // vitest：`Test Files  1 failed | 3 passed`、`Errors  1 error`；jest：`Test Suites: 1 failed, …`
    const brokenFiles = Number(/^\s*Test (?:Files|Suites):?\s+.*?(\d+)\s+failed/im.exec(text)?.[1] ?? 0);
    const unhandled = Number(/^\s*Errors\s+(\d+)\s+errors?/im.exec(text)?.[1] ?? 0);
    const broken = brokenFiles + unhandled;
    // vitest：`Tests  3 failed | 159 passed (162)`；jest：`Tests:       1 failed, 161 passed, 162 total`
    const line = /^\s*Tests:?\s+(.+)$/im.exec(text)?.[1];
    if (line && /\b(?:passed|failed)\b/.test(line)) {
      const failed = Number(/(\d+)\s+failed/.exec(line)?.[1] ?? 0);
      const passed = Number(/(\d+)\s+passed/.exec(line)?.[1] ?? 0);
      return failed > 0 ? { failed, passed, unit: "tests" } : broken > 0 ? { failed: broken, passed, unit: null } : { failed: 0, passed, unit: "tests" };
    }
    if (broken > 0) return { failed: broken, passed: null, unit: null };
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
    if (problems && Number(problems[2]) > 0) return { failed: Number(problems[2]), passed: null, unit: "problems" };
  }
  if (kinds.some((kind) => kind === "typecheck" || kind === "build" || kind === "lint" || kind === "test")) {
    // cargo check/build/clippy/test：`error: could not compile`
    if (/^error(?:\[E\d+\])?: /m.test(text) && /could not compile/.test(text)) {
      return { failed: sumAll(text, /due to (\d+) previous errors?/g) ?? 1, passed: null, unit: "errors" };
    }
  }
  return null;
}

const BACKGROUND = /running in background|in the background with ID|background shell|已在后台/i;
/** 没跑完：用户拒绝执行、超时。不是检查没过。 */
const NOT_RUN = /doesn't want to proceed|tool use was rejected|was rejected by the user|user denied|permission denied by user|timed out|timeout exceeded|已拒绝|超时/i;

function commandOf(item: Extract<ChatItem, { type: "tool_use" }>): string {
  return item.detail?.kind === "command" ? item.detail.command : item.summary;
}

/** 整条命令有没有失败：is_error，或者非零退出码（gemini、opencode 跑完的命令不标 is_error）。 */
function runFailed(result: ToolResultItem): boolean {
  return result.is_error || (result.exit_code != null && result.exit_code !== 0);
}

type Verdict = { status: CheckStatus; note: CheckNote };
const UNCLEAR: Verdict = { status: "unknown", note: { kind: "unclear" } };
const NOT_REACHED: Verdict = { status: "unknown", note: { kind: "notRun" } };

/**
 * 一段检查的结论。verdicts 是前面各段已经判好的（非检查段为 null）。
 * 原则：输出里认出了失败就是失败；要判「通过」必须退出码能落到这一段上且为成功；
 * 失败要归到这一段，前面的段必须确定没失败（切目录这类、或已判通过的检查）。
 */
function judge(segments: Segment[], index: number, kinds: CheckKind[], result: ToolResultItem, verdicts: (Verdict | null)[]): Verdict {
  const segment = segments[index];
  const before = segments.slice(0, index);
  // 前面用 && 接着、且前面有段已判失败：这段没跑到（`tsc && vitest`，tsc 报错时 vitest 根本没跑）。
  const earlierFailed = verdicts.slice(0, index).some((verdict) => verdict?.status === "fail");
  if (earlierFailed && segment.prev === "&&") return NOT_REACHED;
  const parsed = parseOutput(kinds, result.text);
  if (parsed && parsed.failed > 0) return { status: "fail", note: { kind: "failed", count: parsed.failed, unit: parsed.unit } };
  const failed = runFailed(result);
  const cleanBefore = before.every((seg, at) => SETUP.test(headOf(seg.text)) || verdicts[at]?.status === "pass");
  const isLast = index === segments.length - 1;
  // 退出码能不能落到这一段上：没被管道接走、前面不是 ||（可能根本没跑）。
  const ownsExit = !segment.piped && segment.prev !== "||";
  if (!failed) {
    // 整条成功、这段后面只用 && 接着（或什么都没有）：它一定跑过且成功。后面是 ; 或 || 时它失败了整条也可能成功。
    if (ownsExit && (segment.next === null || segment.next === "&&")) return { status: "pass", note: { kind: "passed", count: parsed?.passed ?? null } };
    // 退出码不算数，但输出里认出了「0 个失败」的测试汇总：照输出算通过。
    if (parsed && parsed.unit === "tests") return { status: "pass", note: { kind: "passed", count: parsed.passed } };
    return UNCLEAR;
  }
  // 整条失败：这段是最后一段、退出码是它的、前面确定没失败，失败就是它的。
  if (ownsExit && isLast && cleanBefore) return { status: "fail", note: { kind: "failed", count: null, unit: null } };
  return UNCLEAR;
}

/**
 * 这一段回复里的检查：每类只留最后一次（先跑失败、修好再跑通过，读者要的是现在的状态）。
 * 一条命令里结论相同的几类合成一行，结论不同的分行（`tsc && vitest`：类型检查失败、测试没跑到）。
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
    const kindsBySegment = segments.map((segment) => checkKindsOf(segment.text));
    if (kindsBySegment.every((kinds) => kinds.length === 0)) return;
    const result = results.get(item.id);
    const whole: Verdict | null = !result ? { status: "unknown", note: { kind: "missing" } }
      : result.interrupted ? { status: "unknown", note: { kind: "interrupted" } }
      : NOT_RUN.test(result.text) && runFailed(result) ? NOT_REACHED
      : !runFailed(result) && BACKGROUND.test(result.text) ? { status: "unknown", note: { kind: "background" } }
      : null;
    const verdicts: (Verdict | null)[] = [];
    const found = new Map<CheckKind, Verdict>();
    segments.forEach((_, index) => {
      const kinds = kindsBySegment[index];
      if (kinds.length === 0) {
        verdicts.push(null);
        return;
      }
      const verdict = whole ?? judge(segments, index, kinds, result!, verdicts);
      verdicts.push(verdict);
      for (const kind of kinds) found.set(kind, verdict);
    });
    // 结论相同的几类合成一行。
    const groups: { kinds: CheckKind[]; verdict: Verdict }[] = [];
    for (const kind of KIND_ORDER) {
      const verdict = found.get(kind);
      if (!verdict) continue;
      // 都通过的合成一行（计数取有的那个：`tsc && vitest` 写「162 个通过」）；其余结论完全相同才合。
      const same = groups.find((group) => (group.verdict.status === "pass" && verdict.status === "pass") || JSON.stringify(group.verdict) === JSON.stringify(verdict));
      if (!same) {
        groups.push({ kinds: [kind], verdict });
        continue;
      }
      same.kinds.push(kind);
      if (same.verdict.note.kind === "passed" && same.verdict.note.count == null && verdict.note.kind === "passed") same.verdict = verdict;
    }
    for (const group of groups) all.push({ kinds: group.kinds, ...group.verdict, command, toolId: item.id, at });
  });
  // 每类最后一次：倒着走，某行的类别全被更晚的行盖过就丢掉；部分被盖过的只留没被盖的类别。
  const covered = new Set<CheckKind>();
  const kept: TurnCheck[] = [];
  for (let index = all.length - 1; index >= 0; index -= 1) {
    const check = all[index];
    const fresh = check.kinds.filter((kind) => !covered.has(kind));
    if (fresh.length === 0) continue;
    fresh.forEach((kind) => covered.add(kind));
    kept.unshift({ ...check, kinds: fresh });
  }
  return kept;
}

/** 文档、图片这类改了也不需要跑检查的文件。`.txt` 不算：requirements.txt、CMakeLists.txt 都会影响构建。 */
export function isDocPath(path: string): boolean {
  return /\.(?:md|mdx|markdown|rst|adoc|png|jpe?g|gif|webp|ico)$/i.test(path) || /(?:^|\/)\.lead\//.test(path) || /(?:^|\/)(?:LICENSE|CHANGELOG|AUTHORS)(?:\.\w+)?$/i.test(path);
}
