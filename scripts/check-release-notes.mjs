#!/usr/bin/env node
// 检查发布说明是否符合 docs/release-notes/README.md 的写法：长度预算和不该出现的开发者内容。
// 用法：node scripts/check-release-notes.mjs <文件>...
// 发布说明会出现在应用内更新窗口、GitHub Release 和官网，读者是用户，不是开发者。
import { readFileSync } from "node:fs";

const LIMITS = { intro: 120, items: 20, itemChars: 50, total: 1500 };

// [正则, 说明]。匹配不区分大小写。
const BANNED = [
  [/测试底盘|用例全过|个用例|真机上验证|cargo |clippy|tsc --noEmit/i, "测试和验证情况放 PR 描述"],
  [/CVE-\d|RUSTSEC-\d|GHSA-/i, "不写漏洞编号，改成「修复了一个安全问题」"],
  // 「CI」本身可以是用户功能（「更改」面板看 PR 的 CI 结果），只拦写项目自己 CI 的说法。
  [/靠 CI|CI 矩阵|CI 改用|CI 的 (Windows|macOS)|\be2e\b|GitHub Actions|release\.yml/i, "CI 和构建改动用户看不到"],
  [/官网|README/i, "官网和文档改动不写进发布说明"],
];

// 中日韩字符算 1，其余连续的 ASCII 单词算 1，空白不算。粗略对应「读起来有多长」。
function length(text) {
  const cjk = text.match(/[　-鿿＀-￯]/g)?.length ?? 0;
  const words = text.replace(/[　-鿿＀-￯]/g, " ").match(/[A-Za-z0-9_.+#/-]+/g)?.length ?? 0;
  return cjk + words;
}

export function check(markdown) {
  const errors = [];
  // HTML 注释（草稿说明）不显示，不计入。
  const body = markdown.replace(/<!--[\s\S]*?-->/g, "").trim();
  const lines = body.split(/\r?\n/);

  const firstHeading = lines.findIndex((l) => /^#{1,6}\s/.test(l));
  const intro = (firstHeading === -1 ? lines : lines.slice(0, firstHeading)).join("").trim();
  if (length(intro) > LIMITS.intro) errors.push(`开头一段 ${length(intro)} 字，上限 ${LIMITS.intro}`);

  const items = lines.filter((l) => /^\s*[-*]\s+/.test(l));
  if (items.length > LIMITS.items) errors.push(`共 ${items.length} 条，上限 ${LIMITS.items}`);
  for (const item of items) {
    const n = length(item.replace(/^\s*[-*]\s+/, ""));
    if (n > LIMITS.itemChars) errors.push(`条目 ${n} 字，上限 ${LIMITS.itemChars}：${item.trim().slice(0, 40)}…`);
  }

  const total = length(body);
  if (total > LIMITS.total) errors.push(`全文 ${total} 字，上限 ${LIMITS.total}`);

  for (const [i, line] of lines.entries()) {
    for (const [re, why] of BANNED) {
      if (re.test(line)) errors.push(`第 ${i + 1} 行：${why}：${line.trim().slice(0, 40)}`);
    }
  }
  return errors;
}

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("用法：node scripts/check-release-notes.mjs <文件>...");
  process.exit(2);
}
let failed = false;
for (const file of files) {
  const errors = check(readFileSync(file, "utf8"));
  if (errors.length === 0) {
    console.log(`✓ ${file}`);
    continue;
  }
  failed = true;
  console.log(`✗ ${file}（写法见 docs/release-notes/README.md）`);
  for (const e of errors) console.log(`  - ${e}`);
}
process.exit(failed ? 1 : 0);
