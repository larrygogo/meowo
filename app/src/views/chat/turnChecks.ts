// 回复末尾「本轮结果」里的检查段：这一轮跑过哪些测试、类型检查、lint、构建，最后一次各是什么结果。
//
// 全部从会话记录里推：命令原文（tool_use 的 command 详情）+ 回执（is_error、exit_code 与输出）。不听 AI 自己说
// 「测试都过了」——录屏、自述都只能证明做过，不能证明对（调研 landscape.md 第 3 节）。
// 五家的命令调用都归一成 kind=run。失败的标记各家不一样：claude、codex 标 is_error；gemini 与 opencode 命令
// 跑完就不标 is_error，非零退出码只在 exit_code 里（做空第一轮实测）。两个都看。
//
// 退出码不总可信：`vitest run | tail -20` 的退出码是 tail 的，`cargo test || true` 永远是 0，
// `tsc; echo done` 取的是 echo 的。这些情况改从输出里认常见的失败信号（vitest/jest/cargo/pytest/bun/go 的汇总行、
// tsc 的 error TS、eslint 的 ✖ N problems），认不出就如实写「看不出结果」，不猜。
// 宁可写「看不出」也不把没通过的写成「通过」：这一段存在的意义就是那个「✓」可信。
import type { ChatItem } from "../../api";
import type { ToolResultItem } from "./shared";

export type CheckKind = "typecheck" | "test" | "lint" | "build";
export type CheckStatus = "pass" | "fail" | "unknown";
/** 结果说明：通过/失败时给计数，看不出时给原因。oneFailed：几项用 && 串着、整条失败，但分不清是哪一项。 */
export type CheckNote =
  | { kind: "passed"; count: number | null }
  | { kind: "failed"; count: number | null; unit: "tests" | "errors" | "problems" | null }
  | { kind: "unclear" | "interrupted" | "background" | "missing" | "notRun" | "oneFailed" };

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

/** 认出来也不算检查：问版本、看帮助、初始化配置、监听模式、格式化（改文件不是检查）。
 *  区分大小写：`-V` 是版本；`-v` 对 pytest / go test 是详细输出（做空第二轮），只对下面这些工具才是版本。 */
const NOT_A_CHECK = /\s(?:--version|-V|--help|-h|--init|--watch)(?:\s|$)|^(?:npx\s+|bunx\s+)?ruff\s+format(?!.*--check)/;
/** `-v` 是问版本的工具（做空第三轮：`npx tsc -v` 被当成类型检查通过）。 */
/// 只列 `-v` 确实是版本的；cargo、go、mypy、pytest 的 `-v` 是详细输出，不在这里。且 `-v` 要紧跟工具名。
const VERSION_V = /^(?:\S+\s+)*?(?:tsc|vue-tsc|tsgo|eslint|oxlint|biome|vitest|jest|webpack|vite|next|rollup|stylelint|prettier)\s+-v(?:\s|$)/i;

/** 不会失败、也不改变结果的前置段：切目录、设环境变量。它们排在检查前面时，失败一定来自检查。 */
const SETUP = /^(?:(?:cd|pushd|popd|push-location|pop-location|set-location|sl|export|set|setlocal)\b|\$env:\w+\s*=|\$ErrorActionPreference\s*=|\w+=\S*$)/i;

type Connector = "&&" | "||" | ";" | null;
type Segment = { text: string; piped: boolean; background: boolean; next: Connector; prev: Connector };

/** 把一条命令切成段：记下每段前后的连接符、有没有被管道接走、有没有丢到后台。引号里的符号不切。
 *  sequentialAmp：cmd 里单独的 `&` 是「依次执行」，bash 里是「丢到后台」。 */
export function splitCommand(command: string, sequentialAmp = false): Segment[] {
  const parts: { text: string; sep: "&&" | "||" | ";" | "|" | "&" | null }[] = [];
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
    // `2>&1 |`、`|&` 都按管道算。
    if (ch === "|") {
      parts.push({ text: buf, sep: "|" });
      buf = "";
      if (command[i + 1] === "&") i += 1;
      continue;
    }
    // 单独的 `&`：重定向（`2>&1`、`&>`）和 PowerShell 开头的调用符不算；其余是后台（cmd 里是依次执行）。
    if (ch === "&" && command[i - 1] !== ">" && command[i + 1] !== ">" && buf.trim() !== "") {
      parts.push({ text: buf, sep: sequentialAmp ? ";" : "&" });
      buf = "";
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
    if (!current) current = { text: part.text.trim(), piped: false, background: false, next: null, prev };
    if (part.sep === "|") {
      current.piped = true;
      continue;
    }
    if (part.sep === "&") current.background = true;
    current.next = part.sep === "&" ? ";" : part.sep;
    if (current.text) segments.push(current);
    prev = current.next;
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
    // 只去掉脚本名之前的选项；遇到第一个不是选项的词（run/test/vitest…）后原样保留。
    if (out.length > 1 && out[out.length - 1].toLowerCase() !== "run") {
      out.push(...tokens.slice(i));
      break;
    }
    if (token === "--") continue;
    // pnpm 的 -w 是「在工作区根目录」，不带值；npm 的 -w 带工作区名。
    if (pm === "pnpm" && lower === "-w") continue;
    if (lower === "workspace" && pm === "yarn") {
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
  if (NOT_A_CHECK.test(` ${head}`) || NOT_A_CHECK.test(head) || VERSION_V.test(head)) return [];
  return KIND_ORDER.filter((kind) => PATTERNS[kind].some((re) => re.test(head)));
}

/** 输出里的一条信号：from 是它可能出自哪几类检查（`Found 3 errors` 是 tsc、mypy、ruff 都会打印的）。 */
type Marker = { from: CheckKind[]; failed: number; passed: number | null; unit: "tests" | "errors" | "problems" | null };

function sumAll(text: string, re: RegExp): number | null {
  let total: number | null = null;
  for (const match of text.matchAll(re)) total = (total ?? 0) + Number(match[1]);
  return total;
}

/** 从输出里认出所有信号。 */
function markersOf(output: string): Marker[] {
  const text = output.replace(/\x1b\[[0-9;]*m/g, "");
  const found: Marker[] = [];
  const test = (failed: number, passed: number | null, unit: Marker["unit"] = "tests") => found.push({ from: ["test"], failed, passed, unit });
  // 测试文件没加载起来（语法错、import 坏了）、未处理的异常：用例数可能全过，但这一轮没通过。
  // vitest：`Test Files  1 failed | 3 passed`、`Errors  1 error`；jest：`Test Suites: 1 failed, …`
  const broken = Number(/^\s*Test (?:Files|Suites):?\s+.*?(\d+)\s+failed/im.exec(text)?.[1] ?? 0)
    + Number(/^\s*Errors\s+(\d+)\s+errors?/im.exec(text)?.[1] ?? 0);
  // vitest：`Tests  3 failed | 159 passed (162)`；jest：`Tests:       1 failed, 161 passed, 162 total`
  const line = /^\s*Tests:?\s+(.+)$/im.exec(text)?.[1];
  const pytest = /^=+ (.*\b(?:passed|failed)\b.*) in [\d.]+s/m.exec(text)?.[1];
  const bunPass = /^\s*(\d+) pass\s*$/m.exec(text);
  const bunFail = /^\s*(\d+) fail\s*$/m.exec(text);
  if (line && /\b(?:passed|failed)\b/.test(line)) {
    const failed = Number(/(\d+)\s+failed/.exec(line)?.[1] ?? 0);
    const passed = Number(/(\d+)\s+passed/.exec(line)?.[1] ?? 0);
    if (failed > 0) test(failed, passed);
    else if (broken > 0) test(broken, passed, null);
    else test(0, passed);
  } else if (broken > 0) {
    test(broken, null, null);
  } else if (/test result: (?:ok|FAILED)\./.test(text)) {
    // cargo：每个测试二进制一行 `test result: ok. 45 passed; 0 failed; …`，加总。
    test(sumAll(text, /test result: \w+\. \d+ passed; (\d+) failed/g) ?? 0, sumAll(text, /test result: \w+\. (\d+) passed/g));
  } else if (pytest) {
    // pytest：`===== 3 failed, 159 passed in 2.31s =====`
    test(Number(/(\d+) failed/.exec(pytest)?.[1] ?? 0) + Number(/(\d+) errors?/.exec(pytest)?.[1] ?? 0), Number(/(\d+) passed/.exec(pytest)?.[1] ?? 0));
  } else if (bunPass || bunFail) {
    // bun test：` 162 pass` / ` 0 fail`
    test(Number(bunFail?.[1] ?? 0), Number(bunPass?.[1] ?? 0));
  } else if (/^(?:--- FAIL|FAIL\s)/m.test(text)) {
    // go test：有 `--- FAIL` 或行首 FAIL 即失败；全是 ok 即通过。
    test((text.match(/^--- FAIL/gm) ?? []).length || 1, null);
  } else if (/^ok\s+\S+\s+[\d.]+s/m.test(text)) {
    test(0, null);
  }
  // tsc 独有的 `error TS1234:`。
  const tsErrors = (text.match(/\berror TS\d+:/g) ?? []).length;
  if (tsErrors > 0) found.push({ from: ["typecheck"], failed: tsErrors, passed: null, unit: "errors" });
  // `Found N errors`：tsc、mypy、ruff、biome 都会打印，分不清是类型检查还是代码检查。
  const foundErrors = /\bFound (\d+) errors?\b/.exec(text);
  if (foundErrors && tsErrors === 0) found.push({ from: ["typecheck", "lint"], failed: Number(foundErrors[1]), passed: null, unit: "errors" });
  // eslint：`✖ 5 problems (3 errors, 2 warnings)`；只有警告不算失败。
  const problems = /✖ (\d+) problems? \((\d+) errors?/.exec(text);
  if (problems && Number(problems[2]) > 0) found.push({ from: ["lint"], failed: Number(problems[2]), passed: null, unit: "problems" });
  // cargo check/build/clippy/test 编译失败都打印 `could not compile`。
  if (/^error(?:\[E\d+\])?: /m.test(text) && /could not compile/.test(text)) {
    found.push({ from: ["typecheck", "lint", "build", "test"], failed: sumAll(text, /due to (\d+) previous errors?/g) ?? 1, passed: null, unit: "errors" });
  }
  return found;
}

type Parsed = { failed: number; passed: number | null; unit: Marker["unit"] } | null;

/** 只有这几类检查时，输出里认出的结果（有失败的优先）。认不出返回 null（不等于通过）。 */
export function parseOutput(kinds: CheckKind[], output: string): Parsed {
  const mine = markersOf(output).filter((marker) => marker.from.some((kind) => kinds.includes(kind)));
  const pick = mine.find((marker) => marker.failed > 0) ?? mine[0];
  return pick ? { failed: pick.failed, passed: pick.passed, unit: pick.unit } : null;
}

const BACKGROUND = /running in background|in the background with ID|background shell|已在后台/i;
/** 没跑完：用户拒绝执行、工具层超时。只认各家工具层的固定文案——测试输出里的「Test timed out in 5000ms」
 *  是测试失败，不是没跑完（做空第二轮）。codex 的超时是小写的 `command timed out after 120208 milliseconds`。
 *  并且只在输出里认不出失败时才用。 */
const NOT_RUN = /The user doesn't want to proceed with this tool use|tool use was rejected|^(?:Error: )?Command timed out after|^Command was rejected|用户拒绝了这次工具调用/im;

/** 字符串形态的 shell 包装：`bash -lc '…'`、`powershell -Command "…"`、`cmd /c …`。数组形态 codex 后端已拆。
 *  引号必须正好包住到整条命令末尾、中间没有同种引号：`bash -c "npm run build" ; echo $?` 外面还接着别的，
 *  拆开会把外层的 `;` 吞掉（做空第三轮），这种不拆。返回 cmd 表示里面的 `&` 是依次执行。 */
function unwrapShell(command: string): { text: string; cmd: boolean } {
  const quoted = /^(?:\S*[\\/])?(?:bash|sh|zsh|pwsh|powershell)(?:\.exe)?\s+(?:-[\w-]+\s+)*?-(?:l?c|Command)\s+(?:"([^"]*)"|'([^']*)')\s*$/i.exec(command);
  if (quoted) return { text: quoted[1] ?? quoted[2], cmd: false };
  const cmd = /^(?:\S*[\\/])?cmd(?:\.exe)?\s+(?:\/[dqs]\s+)*\/[ck]\s+(?:"([^"]*)"|([^"]+))\s*$/i.exec(command);
  if (cmd) return { text: cmd[1] ?? cmd[2], cmd: true };
  return { text: command, cmd: false };
}

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
const PASSED: Verdict = { status: "pass", note: { kind: "passed", count: null } };

/** 从 i 走到 j（不含 j）一路都用 && 接着、没被管道接走、没丢后台：i 成功才会跑到 j。 */
function chainedAnd(segments: Segment[], from: number, to: number): boolean {
  for (let k = from; k < to; k += 1) if (segments[k].next !== "&&" || segments[k].piped || segments[k].background) return false;
  return true;
}

/**
 * 一条命令里各检查段的结论（非检查段为 null）。
 *
 * 整条命令只有一份输出、一个退出码，要分给几段（做空第二、三轮）。原则：
 * 1. 输出里的每条信号先看它可能出自哪几类（Marker.from）。只有一段对得上时才归给它：
 *    它有失败就是失败；它前面用 && 接着的段一定跑过且成功；它后面用 && 接着的没跑到。
 * 2. 没归出结论的段看退出码：整条成功、这段的退出码落得到（没管道、后面只有 && 或什么都没有）就算通过；
 *    整条失败、这段是最后一段且一定跑到了，失败就是它的。
 * 3. 整条失败却归不到任何一段：如果这些检查段用 && 串成一条链、链外只有切目录这类不会失败的段，
 *    那一定是其中一项没过，写「其中一项没过」，不写「看不出」把失败藏掉。
 */
function judgeCommand(segments: Segment[], kindsBySegment: CheckKind[][], result: ToolResultItem): (Verdict | null)[] {
  const verdicts: (Verdict | null)[] = segments.map(() => null);
  const checks = kindsBySegment.flatMap((kinds, index) => (kinds.length > 0 ? [index] : []));
  const failed = runFailed(result);
  const owners = (marker: Marker) => checks.filter((index) => kindsBySegment[index].some((kind) => marker.from.includes(kind)));
  // 每段独占的信号。
  const own = new Map<number, Marker[]>();
  for (const marker of markersOf(result.text)) {
    const who = owners(marker);
    if (who.length === 1) own.set(who[0], [...(own.get(who[0]) ?? []), marker]);
  }
  const failing = checks.filter((index) => (own.get(index) ?? []).some((marker) => marker.failed > 0));
  for (const index of failing) {
    const marker = own.get(index)!.find((m) => m.failed > 0)!;
    verdicts[index] = { status: "fail", note: { kind: "failed", count: marker.failed, unit: marker.unit } };
  }
  for (const index of checks) {
    if (verdicts[index]) continue;
    const segment = segments[index];
    if (segment.background) {
      verdicts[index] = { status: "unknown", note: { kind: "background" } };
      continue;
    }
    if (failing.some((f) => f < index && chainedAnd(segments, f, index))) {
      verdicts[index] = NOT_REACHED;
      continue;
    }
    if (failing.some((f) => f > index && chainedAnd(segments, index, f))) {
      verdicts[index] = PASSED;
      continue;
    }
    // 这段独占的、0 失败的汇总（计数从这里来）。
    const summary = (own.get(index) ?? []).find((marker) => marker.failed === 0 && marker.unit === "tests");
    const ownsExit = !segment.piped && segment.prev !== "||";
    if (!failed) {
      // 整条成功、这段后面只用 && 接着（或什么都没有）：它一定跑过且成功。后面是 ; 或 || 时它失败了整条也可能成功。
      if (ownsExit && (segment.next === null || segment.next === "&&")) verdicts[index] = { status: "pass", note: { kind: "passed", count: summary?.passed ?? null } };
      // 退出码不算数，但输出里认出了这段独占的「0 个失败」测试汇总：照输出算通过。
      else if (summary) verdicts[index] = { status: "pass", note: { kind: "passed", count: summary.passed } };
      else verdicts[index] = UNCLEAR;
      continue;
    }
    // 整条失败、这段是最后一段、退出码是它的，且它一定跑到了：前面是 ;（不管前面成败都会跑），
    // 或前面全是切目录这类、已判通过的段。
    const last = index === segments.length - 1;
    const reached = segment.prev === ";" || segments.slice(0, index).every((seg, at) => SETUP.test(headOf(seg.text)) || verdicts[at]?.status === "pass");
    if (ownsExit && last && reached && failing.length === 0 && NOT_RUN.test(result.text)) verdicts[index] = NOT_REACHED;
    else if (ownsExit && last && reached && (failing.length === 0 || segment.prev === ";")) verdicts[index] = { status: "fail", note: { kind: "failed", count: null, unit: null } };
    else verdicts[index] = UNCLEAR;
  }
  // 整条失败却没有一段认定失败：工具层拒绝、超时写「没跑完」；检查段用 && 串成一条链时写「其中一项没过」。
  if (failed && !verdicts.some((verdict) => verdict?.status === "fail")) {
    if (NOT_RUN.test(result.text)) {
      for (const index of checks) verdicts[index] = NOT_REACHED;
    } else {
      const open = checks.filter((index) => verdicts[index]?.status === "unknown" && verdicts[index]?.note.kind === "unclear");
      const others = segments.every((seg, at) => kindsBySegment[at].length > 0 || SETUP.test(headOf(seg.text)));
      const chain = segments.every((seg, at) => at === segments.length - 1 || (seg.next === "&&" && !seg.piped && !seg.background));
      if (open.length > 0 && others && chain && open.length === checks.length) {
        for (const index of open) verdicts[index] = { status: "fail", note: { kind: "oneFailed" } };
      }
    }
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
    const unwrapped = unwrapShell(commandOf(item).trim());
    const command = unwrapped.text.trim();
    if (!command) return;
    const segments = splitCommand(command, unwrapped.cmd);
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
      if (same.verdict.status === "pass") same.verdict = PASSED;
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
    // 「其中一项没过」是对整组说的：拆掉一部分类别后就不成立了，改写成看不出。
    const note = check.note.kind === "oneFailed" && fresh.length < check.kinds.length ? UNCLEAR : null;
    kept.unshift({ ...check, kinds: fresh, ...(note ?? {}) });
  }
  return kept;
}

/** 文档、图片这类改了也不需要跑检查的文件。`.txt` 不算：requirements.txt、CMakeLists.txt 都会影响构建。 */
export function isDocPath(path: string): boolean {
  return /\.(?:md|mdx|markdown|rst|adoc|png|jpe?g|gif|webp|ico)$/i.test(path) || /(?:^|\/)\.lead\//.test(path) || /(?:^|\/)(?:LICENSE|CHANGELOG|AUTHORS)(?:\.\w+)?$/i.test(path);
}
