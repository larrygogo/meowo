// scripts/install-hooks.mjs
// 用法：bun scripts/install-hooks.mjs <cc-reporter 可执行文件绝对路径> [settingsPath]
// 把 cc-kanban 的 hooks 幂等合并进 settings.json，不破坏已有配置。
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";

const reporter = process.argv[2];
if (!reporter) {
  console.error("用法: bun scripts/install-hooks.mjs <cc-reporter 可执行文件绝对路径> [settingsPath]");
  process.exit(1);
}

// 优先级：命令行第 2 参 > 环境变量 > 默认 ~/.claude/settings.json
const settingsPath =
  process.argv[3] ||
  process.env.CC_KANBAN_SETTINGS ||
  join(homedir(), ".claude", "settings.json");

mkdirSync(dirname(settingsPath), { recursive: true });

let settings = {};
if (existsSync(settingsPath)) {
  try {
    settings = JSON.parse(readFileSync(settingsPath, "utf8"));
  } catch (e) {
    console.error(`解析 ${settingsPath} 失败: ${e.message}`);
    process.exit(1);
  }
}
settings.hooks ??= {};

const EVENTS = ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "SessionEnd"];
// 幂等识别：command 字段包含此字串即视为我们加的条目
const MARKER = "cc-reporter";

for (const event of EVENTS) {
  settings.hooks[event] ??= [];
  // 过滤掉我们之前写入的条目（command 含 MARKER），保留其它条目
  settings.hooks[event] = settings.hooks[event].filter(
    (entry) => !(entry.hooks ?? []).some((h) => (h.command ?? "").includes(MARKER)),
  );
  // 追加新条目，command 用双引号包住路径以防空格
  settings.hooks[event].push({
    matcher: "*",
    hooks: [{ type: "command", command: `"${reporter}"` }],
  });
}

writeFileSync(settingsPath, JSON.stringify(settings, null, 2));
console.log(`已写入 ${settingsPath}，挂载事件: ${EVENTS.join(", ")}`);
