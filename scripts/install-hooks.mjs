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

const SPECS = [
  ["SessionStart", "*"],
  ["UserPromptSubmit", "*"],
  ["PostToolUse", "*"],
  ["Stop", "*"],
  ["SessionEnd", "*"],
  ["PermissionRequest", "*"],
  ["PreToolUse", "AskUserQuestion"],
  ["PreToolUse", "ExitPlanMode"],
];
// 写入的 command：双引号包住路径以防空格
const command = `"${reporter}"`;

for (const [event, matcher] of SPECS) {
  settings.hooks[event] ??= [];
  // 幂等识别:只移除「command 完全相同 且 matcher 相同」的旧条目,
  // 避免同事件多 matcher 条目互相误删(如 PreToolUse 的 AskUserQuestion 与 ExitPlanMode)。
  settings.hooks[event] = settings.hooks[event].filter(
    (entry) =>
      !(entry.matcher === matcher && (entry.hooks ?? []).some((h) => h.command === command)),
  );
  // 追加新条目；timeout=5s 给 Claude Code 一个上限，万一 reporter 卡住也不会无限阻塞会话
  settings.hooks[event].push({
    matcher,
    hooks: [{ type: "command", command, timeout: 5 }],
  });
}

writeFileSync(settingsPath, JSON.stringify(settings, null, 2));
console.log(`已写入 ${settingsPath}，挂载: ${SPECS.map(([e, m]) => `${e}:${m}`).join(", ")}`);
