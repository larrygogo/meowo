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
/// 区分大小写：`-V` 是版本，`-v` 是 pytest / go test 的详细输出（做空第二轮：带 /i 时连 `pytest -v` 都不认了）。
const NOT_A_CHECK = /\s(?:--version|-V|--help|-h|--init|--watch)(?:\s|$)|^(?:npx\s+|bunx\s+)?ruff\s+format(?!.*--check)/;

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
  const pm = (tokens[0] ?? "").toLowerCase();
  if (!/^(?:npm|pnpm|yarn|bun|npx|bunx|pnpx)$/.test(pm)) return head;
  // `npm exec -- vitest run` 等于 `npx vitest run`。
  if (pm === "npm" && tokens[1]?.toLowerCase() === "exec") {
    return headOf(["npx", ...tokens.slice(2).filter((token) => token !== "--")].join(" "));
  }
  const out = [tokens[0]];
  for (let i = 1; i < tokens.length; i += 1) {
    const token = tokens[i];
    const lower = token.toLowerCase();
    if (token === "--") continue;
    // pnpm 的 -w 是「在工作区根目录」，不带值；npm 的 -w 带工作区名。
    if (pm === "pnpm" && lower === "-w") continue;
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
/** 没跑完：用户拒绝执行、工具层超时。只认各家工具层的固定文案——测试输出里的「Test timed out in 5000ms」
 *  是测试失败，不是没跑完（做空第二轮）。并且只在输出里认不出失败时才用。 */
const NOT_RUN = /The user doesn't want to proceed with this tool use|tool use was rejected|^Command timed out after|^Error: Command timed out|^Command was rejected|用户拒绝了这次工具调用/m;

/** 字符串形态的 shell 包装：`bash -lc '…'`、`powershell -Command "…"`、`cmd /c …`。数组形态 codex 后端已拆。 */
function unwrapShell(command: string): string {
  const quoted = /^(?:\S*[\\/])?(?:bash|sh|zsh|pwsh|powershell)(?:\.exe)?\s+(?:-[\w-]+\s+)*?-(?:l?c|Command|NoProfile\s+-Command)\s+(['"])([\s\S]*)\1\s*$/i.exec(command);
  if (quoted) return quoted[2];
  const cmd = /^(?:\S*[\\/])?cmd(?:\.exe)?\s+\/[cs]\s+([\s\S]+)$/i.exec(command);
  return cmd ? cmd[1].replace(/^"([\s\S]*)"$/, "$1") : command;
}

function commandOf(item: Extract<ChatItem, { type: "tool_use" }>): string {
  return unwrapShell(item.detail?.kind === "command" ? item.detail.command : item.summary);
}

/** 整条命令有没有失败：is_error，或者非零退出码（gemini、opencode 跑完的命令不标 is_error）。 */
function runFailed(result: ToolResultItem): boolean {
  return result.is_error || (result.exit_code != null && result.exit_code !== 0);
}

type Verdict = { status: CheckStatus; note: CheckNote };
const UNCLEAR: Verdict = { status: "unknown", note: { kind: "unclear" } };
const NOT_REACHED: Verdict = { status: "unknown", note: { kind: "notRun" } };

/** 段末单独的 `&`（不是 `&&`）：丢到后台跑，退出码与结果都不在这次回执里。 */
function inBackground(segment: Segment): boolean {
  return /(?:^|[^&])&\s*$/.test(segment.text);
}

/** 从 i 走到 j（不含 j）一路都用 && 接着：i 成功才会跑到 j。 */
function chainedAnd(segments: Segment[], from: number, to: number): boolean {
  for (let k = from; k < to; k += 1) if (segments[k].next !== "&&" || segments[k].piped) return false;
  return true;
}

/**
 * 一条命令里各检查段的结论（非检查段为 null）。
 *
 * 难点是整条命令只有一份输出、一个退出码，要分给几段（做空第二轮）：
 * - 输出里认出失败、且只有一段的类别对得上：失败归它；它前面用 && 接着的段一定跑过且成功；后面用 && 接着的没跑到。
 * - 几段都对得上（`cargo check && cargo clippy` 都会打印 could not compile）：同类的就都算失败（反正这一类没过），
 *   不同类的说不清是谁，写「看不出」，不把失败安到无辜的段上。
 * - 输出里没认出失败：整条成功时，退出码能落到的段算通过；计数只在汇总行只对得上一段时才给。
 *   整条失败时，只有最后一段、且前面只有切目录这类不会失败的段，失败才归它；工具层拒绝/超时写「没跑完」。
 */
function judgeCommand(segments: Segment[], kindsBySegment: CheckKind[][], result: ToolResultItem): (Verdict | null)[] {
  const verdicts: (Verdict | null)[] = segments.map(() => null);
  const checks = kindsBySegment.flatMap((kinds, index) => (kinds.length > 0 ? [index] : []));
  const parsed = kindsBySegment.map((kinds) => (kinds.length > 0 ? parseOutput(kinds, result.text) : null));
  const failing = checks.filter((index) => (parsed[index]?.failed ?? 0) > 0);
  const failed = runFailed(result);
  if (failing.length === 1) {
    const at = failing[0];
    const p = parsed[at]!;
    for (const index of checks) {
      verdicts[index] = index === at ? { status: "fail", note: { kind: "failed", count: p.failed, unit: p.unit } }
        : index < at && chainedAnd(segments, index, at) ? { status: "pass", note: { kind: "passed", count: null } }
        : index > at && chainedAnd(segments, at, index) ? NOT_REACHED
        : UNCLEAR;
    }
    return verdicts;
  }
  if (failing.length > 1) {
    const sameKinds = failing.every((index) => kindsBySegment[index].join() === kindsBySegment[failing[0]].join());
    for (const index of checks) {
      verdicts[index] = sameKinds && failing.includes(index) ? { status: "fail", note: { kind: "failed", count: null, unit: null } } : UNCLEAR;
    }
    return verdicts;
  }
  // 输出里没认出失败。
  if (failed && NOT_RUN.test(result.text)) {
    for (const index of checks) verdicts[index] = NOT_REACHED;
    return verdicts;
  }
  // 汇总行（计数、0 失败）只对得上一段时才归给它：两段都是测试时分不清 162 是谁的。
  const summaries = checks.filter((index) => parsed[index] != null);
  const attributable = summaries.length === 1 ? summaries[0] : -1;
  for (const index of checks) {
    const segment = segments[index];
    if (inBackground(segment)) {
      verdicts[index] = { status: "unknown", note: { kind: "background" } };
      continue;
    }
    // 退出码能不能落到这一段上：没被管道接走、前面不是 ||（可能根本没跑）。
    const ownsExit = !segment.piped && segment.prev !== "||";
    const own = index === attributable ? parsed[index] : null;
    if (!failed) {
      // 整条成功、这段后面只用 && 接着（或什么都没有）：它一定跑过且成功。后面是 ; 或 || 时它失败了整条也可能成功。
      if (ownsExit && (segment.next === null || segment.next === "&&")) {
        verdicts[index] = { status: "pass", note: { kind: "passed", count: own?.passed ?? null } };
      } else if (own && own.unit === "tests") {
        // 退出码不算数，但输出里认出了「0 个失败」的测试汇总：照输出算通过。
        verdicts[index] = { status: "pass", note: { kind: "passed", count: own.passed } };
      } else {
        verdicts[index] = UNCLEAR;
      }
      continue;
    }
    // 整条失败：这段是最后一段、退出码是它的、前面只有切目录这类不会失败的段，失败就是它的。
    const cleanBefore = segments.slice(0, index).every((seg) => SETUP.test(headOf(seg.text)));
    verdicts[index] = ownsExit && index === segments.length - 1 && cleanBefore
      ? { status: "fail", note: { kind: "failed", count: null, unit: null } }
      : UNCLEAR;
  }
  return verdicts;
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
      : !runFailed(result) && BACKGROUND.test(result.text) ? { status: "unknown", note: { kind: "background" } }
      : null;
    const verdicts = whole ? kindsBySegment.map((kinds) => (kinds.length > 0 ? whole : null)) : judgeCommand(segments, kindsBySegment, result!);
    // 类别按在命令里出现的先后排；同一类出现几次取最后一段的结论。
    const found = new Map<CheckKind, Verdict>();
    kindsBySegment.forEach((kinds, index) => {
      for (const kind of kinds) {
        found.delete(kind);
        found.set(kind, verdicts[index]!);
      }
    });
    // 结论相同的几类合成一行：都通过的合成一行，其余结论完全相同才合。
    const groups: { kinds: CheckKind[]; verdict: Verdict }[] = [];
    for (const [kind, verdict] of found) {
      const same = groups.find((group) => (group.verdict.status === "pass" && verdict.status === "pass") || JSON.stringify(group.verdict) === JSON.stringify(verdict));
      if (!same) {
        groups.push({ kinds: [kind], verdict });
        continue;
      }
      same.kinds.push(kind);
      // 几类合成的通过行不带计数：「类型检查 + 测试 162 个通过」会让人以为 162 是两类加起来的。
      if (same.verdict.status === "pass") same.verdict = { status: "pass", note: { kind: "passed", count: null } };
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
