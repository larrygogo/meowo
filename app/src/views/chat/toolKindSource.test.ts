import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

/// 扫描测试：前端不得再按工具名判断工具的类别。
///
/// 类别由后端各家插件按自己的工具名表给出（ChatItem 的 `kind`）。前端按名字去猜的规则是照
/// Claude 的命名调的：Gemini 的 read_file / replace 全落进「其他」，Codex 的 wait_agent 因为
/// 名字含 agent 被当成子任务，Claude 的 ToolSearch 被当成「搜索」。这里拦两类写法：
/// 1. 对工具条目的 `.name` 做子串、前缀、大小写归一、正则判断；
/// 2. 把 `.name` 与某个字面量比较（除了下面白名单里那几处——它们认的是**某一个具体工具的
///    协议**，不是在给工具归类）。
/// 只扫引用了工具条目（tool_use / ToolUseItem）的源码文件，测试文件不算。

const SRC = join(__dirname, "..", "..");
const ROOTS = ["views/chat", "chat", "mobile", "views/ChatWindow.tsx"];

/** 确需保留的按名字判断，逐条写理由。键是「相对路径 :: 该行去掉首尾空白后的片段」。 */
const ALLOWED: Array<{ file: string; snippet: string; reason: string }> = [
  {
    file: "views/chat/shared.ts",
    snippet: `item.name === "Skill"`,
    reason: "forked skill 的委派本体在调用时看不出是不是 fork，只能凭「Skill + 回执带结局统计」反认；认的是这一个工具，不是类别",
  },
  {
    file: "chat/transcriptTodos.ts",
    snippet: `item.name === "TaskCreate"`,
    reason: "从 Claude 的 TaskCreate 调用重建任务列表：解析的是这个工具的入参协议",
  },
  {
    file: "chat/transcriptTodos.ts",
    snippet: `item.name === "TaskUpdate"`,
    reason: "同上，TaskUpdate 的入参协议",
  },
];

const SUBJECTS = "(?:item|tool|entry|call|part|candidate|use|block|step)";
const BY_PATTERN = new RegExp(`\\b${SUBJECTS}\\.name\\s*\\.(?:includes|startsWith|endsWith|toLowerCase|toUpperCase|match|search|indexOf)\\(`);
const BY_REGEX = new RegExp(`\\.(?:test|exec)\\(\\s*${SUBJECTS}\\.name\\b`);
const BY_EQUALITY = new RegExp(`\\b${SUBJECTS}\\.name\\s*[!=]==\\s*["'\`]|["'\`]\\s*[!=]==\\s*${SUBJECTS}\\.name\\b`);
/** 已删除的按名字归类函数：不许以同名加回来。 */
const REMOVED = /\b(?:toolKind|friendlyToolName)\s*\(/;

function sourceFiles(path: string): string[] {
  const full = join(SRC, path);
  if (statSync(full).isFile()) return [full];
  return readdirSync(full).flatMap((name) => {
    const child = join(path, name);
    if (statSync(join(SRC, child)).isDirectory()) return sourceFiles(child);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [join(SRC, child)] : [];
  });
}

describe("工具类别只认后端给的 kind", () => {
  const offenders: string[] = [];
  const allowedHit = new Set<number>();
  for (const file of ROOTS.flatMap(sourceFiles)) {
    const text = readFileSync(file, "utf8");
    // 与工具条目无关的文件（目录补全、模型名匹配）里的 `.name` 不是工具名。
    if (!/tool_use|ToolUseItem/.test(text)) continue;
    const rel = relative(SRC, file).replace(/\\/g, "/");
    text.split(/\r?\n/).forEach((line, index) => {
      // 注释里举的例子不算。
      const code = line.replace(/\/\/.*$/, "");
      if (!BY_PATTERN.test(code) && !BY_REGEX.test(code) && !BY_EQUALITY.test(code) && !REMOVED.test(code)) return;
      const allowed = ALLOWED.findIndex((entry) => entry.file === rel && code.includes(entry.snippet));
      if (allowed >= 0) {
        allowedHit.add(allowed);
        return;
      }
      offenders.push(`${rel}:${index + 1}  ${line.trim()}`);
    });
  }

  it("渲染与归并代码里没有按工具名判断类别的写法", () => {
    expect(offenders, `改用条目上的 kind 字段（后端给）。确需认某个具体工具的，加进 ALLOWED 并写理由：\n${offenders.join("\n")}`).toEqual([]);
  });

  it("白名单没有过期条目", () => {
    const stale = ALLOWED.filter((_, index) => !allowedHit.has(index)).map((entry) => `${entry.file} :: ${entry.snippet}`);
    expect(stale, "这些写法已经不在源码里了，从 ALLOWED 里删掉").toEqual([]);
  });

  it("规则本身拦得住此前的几种写法", () => {
    const old = [
      `if (normalized === "task" || item.name.includes("agent")) return "agent";`,
      `const kind = toolKind(item.name);`,
      `if (tool.name.toLowerCase() === "bash") return "run";`,
      `return (item.name === "Bash" || item.name === "PowerShell") && outcomes.has(item.id);`,
      `if (/grep|glob/.test(item.name)) return "search";`,
    ];
    for (const line of old) {
      expect(BY_PATTERN.test(line) || BY_REGEX.test(line) || BY_EQUALITY.test(line) || REMOVED.test(line), line).toBe(true);
    }
  });
});
