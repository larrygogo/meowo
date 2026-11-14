import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const SRC = dirname(fileURLToPath(import.meta.url));

/**
 * 前端不得按 agent 身份**分支**。
 *
 * 与后端 `host_code_does_not_branch_on_agent_identity` 同一条纪律：agent 的能力差异
 * 由插件声明、经 descriptor / chatUi 下发，前端只消费。历史上清理过两批——
 * `NetworkSection` 自维护的代理能力名单、`terminalAttention` 按 `provider ===` 的
 * 整句识别规则。
 *
 * 这类回潮**没有任何失败信号**：不会有测试变红、也不会报错，只会让下一个接进来的
 * agent 悄悄少一块能力（用户侧表现为「为什么 gemini 没有这个」），所以只能靠守卫拦。
 *
 * 只禁**比较**（`=== "claude"`、`includes("kimi")`），不禁数据声明
 * （`provider: "claude"`、`["claude"]`）——后者是首帧占位、默认文法自述、预览页固件，
 * 都会被后端下发的真值覆盖，不构成能力判断。
 */
const AGENT_IDS = ["claude", "codex", "kimi", "gemini", "opencode"];

/**
 * 守卫覆盖面：src/ 下**全部**生产 ts/tsx，自动枚举——手工名单必然漂移（曾只列 6 个
 * 文件，RelayAccess/NewSessionPanel/AccountSection 都在盲区）。排除的目录各有理由：
 * 测试与 test/（固件按 id 造数据是本分）、generated/（后端产物）、demo/ 与 poster/
 * （营销物料，用写死的 agent 数据演戏）。
 */
const EXCLUDED_DIRS = new Set(["generated", "test", "demo", "poster"]);

function guardedFiles(dir: string = SRC): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!EXCLUDED_DIRS.has(entry.name)) out.push(...guardedFiles(path));
      continue;
    }
    if (!/\.tsx?$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) continue;
    out.push(relative(SRC, path).replace(/\\/g, "/"));
  }
  return out;
}

/**
 * 身份**比较**的形态。数据声明（`x: "claude"` / `["claude"]`）刻意不在内。
 *
 * `[!=]=+` 而不是 `[!=]==`：松散相等 `p == "kimi"` 同样是分支，只写三等号会漏。
 * 字符串方法一并覆盖 startsWith/endsWith/indexOf/match/test——它们都能拿来做身份判断。
 */
const COMPARISONS = (id: string) => [
  new RegExp(`[!=]=+\\s*["']${id}["']`),
  new RegExp(`["']${id}["']\\s*[!=]=+`),
  new RegExp(`\\.(includes|has|startsWith|endsWith|indexOf|match|test)\\(\\s*["']${id}["']`),
  new RegExp(`\\bcase\\s+["']${id}["']`),
];

/**
 * 通过**中间变量**做身份判断的形态：`const P = "claude"; p === P`。
 *
 * 单看每一行都合法（一行是数据声明、另一行是变量比较），只有连起来看才是身份分支。
 * 故先收集「值为 agent id 的常量名」，再把它们当作 id 的别名重新扫一遍。
 */
function agentIdAliases(source: string): Map<string, string> {
  const aliases = new Map<string, string>();
  const declaration = /\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]+)?=\s*["']([^"']+)["']/g;
  for (let m = declaration.exec(source); m; m = declaration.exec(source)) {
    if (AGENT_IDS.includes(m[2])) aliases.set(m[1], m[2]);
  }
  return aliases;
}

/** 变量别名的比较形态（`p === P`、`switch` 的 `case P`）。 */
const ALIAS_COMPARISONS = (name: string) => [
  new RegExp(`[!=]=+\\s*${name}\\b`),
  new RegExp(`\\b${name}\\s*[!=]=+`),
  new RegExp(`\\.(includes|has|startsWith|endsWith|indexOf)\\(\\s*${name}\\s*\\)`),
  new RegExp(`\\bcase\\s+${name}\\b`),
];

/**
 * 带 Provider 前缀的标识（屏幕识别规则 id `claude:plan-approval`、说明键 `gemini.approval.default`）
 * 是按身份分支的另一种写法：比的不是 `"claude"` 本身，效果一样。`[.:]` 之后至少一个字符。
 */
const PREFIXED = (id: string) => `["']${id}[.:][^"']+["']`;
const PREFIXED_COMPARISONS = (id: string) => [
  new RegExp(`[!=]=+\\s*${PREFIXED(id)}`),
  new RegExp(`${PREFIXED(id)}\\s*[!=]=+`),
  new RegExp(`\\.(includes|has|startsWith|endsWith|indexOf|match|test)\\(\\s*["']${id}[.:]`),
  new RegExp(`\\bcase\\s+${PREFIXED(id)}`),
];

/**
 * 白名单：按带前缀标识分支、此刻确需保留的写法（文件 → 标识 → 理由）。每条都要写理由；
 * 标了 TODO(合并后) 的是落在别的并行分支负责的区域里、本轮不动的，合并后要清掉。
 */
const PREFIXED_ALLOWED: Record<string, Record<string, string>> = {
  "views/ChatWindow.tsx": {
    // TODO(合并后)：通用终端提示卡的标题与正文按 claude:* 规则 id 拼（走查输入 A3、B13）。
    // 卡片组件归「卡片」一路，它把标题、徽章改成由请求种类决定后，这三条要删掉。
    "claude:long-session-resume": "TODO(合并后) 通用提示卡标题按规则 id 拼，卡片一路改成按请求种类",
    "claude:plan-approval": "TODO(合并后) 同上",
    "claude:command-approval": "TODO(合并后) 同上",
  },
};

/**
 * 以 Provider 为键的查表（`{ claude: …, kimi: … }`、`{ "gemini.approval.default": … }`）同样是按身份
 * 分支，只是写成了数据。只允许纯展示用途（图标、品牌色、显示名），逐个文件列出并写理由。
 */
const KEYED_TABLE = new RegExp(`^\\s*["']?(${AGENT_IDS.join("|")})(?:[.:][\\w.:-]+)?["']?\\s*:\\s`);
const KEYED_ALLOWED: Record<string, string> = {
  "providers.tsx": "各家的品牌图标与底色：纯展示，不决定任何功能有无",
  "views/island/islandParts.tsx": "灵动岛用量环的品牌色：纯展示",
  "views/chat/modeChoices.ts": "模式短名的文案映射（键是后端声明的说明键）：只决定叫什么，不决定能不能做",
  "views/chatWindow.harness.tsx": "测试夹具（不叫 .test 所以被扫到）：按 id 造各家的终端画面样本",
};

describe("架构守卫", () => {
  it("前端不按带 Provider 前缀的标识分支（确需保留的逐条列白名单）", () => {
    const offences: string[] = [];
    const used = new Set<string>();
    for (const file of guardedFiles()) {
      const source = readFileSync(join(SRC, file), "utf8");
      source.split("\n").forEach((line, index) => {
        const code = line.trim();
        if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) return;
        for (const id of AGENT_IDS) {
          if (!PREFIXED_COMPARISONS(id).some((re) => re.test(code))) continue;
          const literal = new RegExp(`["'](${id}[.:][^"']+)["']`).exec(code)?.[1] ?? `${id}:`;
          if (PREFIXED_ALLOWED[file]?.[literal]) { used.add(`${file}|${literal}`); continue; }
          offences.push(`${file}:${index + 1} 按 ${literal} 分支 → ${code.slice(0, 140)}`);
        }
      });
    }
    expect(offences).toEqual([]);
    // 白名单里的写法已经不在代码里了：删掉对应条目，别让它替以后的回潮放行。
    const stale = Object.entries(PREFIXED_ALLOWED)
      .flatMap(([file, entries]) => Object.keys(entries).map((literal) => `${file}|${literal}`))
      .filter((key) => !used.has(key));
    expect(stale).toEqual([]);
  });

  it("以 Provider 为键的查表只用于纯展示（逐个文件列白名单并写理由）", () => {
    const hits = new Map<string, number>();
    for (const file of guardedFiles()) {
      // 字典按后端下发的键取文案（t.modes.desc[键]），是翻译资产，不是分支。
      if (file.startsWith("i18n/")) continue;
      const source = readFileSync(join(SRC, file), "utf8");
      for (const line of source.split("\n")) {
        const code = line.trim();
        if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) continue;
        if (KEYED_TABLE.test(line)) hits.set(file, (hits.get(file) ?? 0) + 1);
      }
    }
    expect([...hits.keys()].filter((file) => !KEYED_ALLOWED[file])).toEqual([]);
    expect(Object.keys(KEYED_ALLOWED).filter((file) => !hits.has(file))).toEqual([]);
    for (const reason of Object.values(KEYED_ALLOWED)) expect(reason.length).toBeGreaterThan(5);
  });

  it("守卫认得出这几种写法（自检：规则失灵会让守卫空转）", () => {
    const branch = (code: string) => AGENT_IDS.some((id) => COMPARISONS(id).some((re) => re.test(code)));
    const prefixed = (code: string) => AGENT_IDS.some((id) => PREFIXED_COMPARISONS(id).some((re) => re.test(code)));
    expect(branch(`if (agent === 'claude') {`)).toBe(true);
    expect(branch(`provider === "kimi" ? a : b`)).toBe(true);
    expect(branch(`case "codex":`)).toBe(true);
    expect(branch(`["gemini"].includes(provider)`)).toBe(false); // 数组字面量是数据声明，见上
    expect(branch(`ids.includes("opencode")`)).toBe(true);
    expect(prefixed(`attention.id === "claude:plan-approval"`)).toBe(true);
    expect(prefixed(`id.startsWith("kimi:")`)).toBe(true);
    expect(prefixed(`case "gemini:usage-limit":`)).toBe(true);
    expect(prefixed(`id: "claude:plan-approval",`)).toBe(false);
    expect(KEYED_TABLE.test(`  claude: { Icon: ClaudeMark },`)).toBe(true);
    expect(KEYED_TABLE.test(`  "gemini.approval.default": "askChanges",`)).toBe(true);
    expect(KEYED_TABLE.test(`  provider: "claude",`)).toBe(false);
  });

  it("前端不按 agent 身份分支（能力一律由后端下发）", () => {
    const files = guardedFiles();
    // 枚举失灵（目录结构变了/路径算错）会静默把守卫变成空转——先钉住下限。
    expect(files.length).toBeGreaterThan(30);
    const offences: string[] = [];
    for (const file of files) {
      const source = readFileSync(join(SRC, file), "utf8");
      const aliases = agentIdAliases(source);
      source.split("\n").forEach((line, index) => {
        const code = line.trim();
        // 注释里举例说明是正常的。
        if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) return;
        for (const id of AGENT_IDS) {
          if (COMPARISONS(id).some((re) => re.test(code))) {
            offences.push(`${file}:${index + 1} 按 ${id} 分支 → ${code}`);
          }
        }
        for (const [name, id] of aliases) {
          if (ALIAS_COMPARISONS(name).some((re) => re.test(code))) {
            offences.push(`${file}:${index + 1} 经变量 ${name} 按 ${id} 分支 → ${code}`);
          }
        }
      });
    }
    expect(offences).toEqual([]);
  });
});
