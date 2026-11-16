#!/usr/bin/env node
// 检查发布说明是否符合 docs/release-notes/README.md 的写法：长度预算和不该出现的开发者内容。
// 用法：node scripts/check-release-notes.mjs <文件>...
// 发布说明会出现在应用内更新窗口、GitHub Release 和官网，读者是用户，不是开发者。
// 规则只拦「几乎不可能是用户功能」的说法：Meowo 是开发者工具，CI、README、cargo 这些词
// 本身可能出现在正当的功能描述里（「更改」面板看 CI 结果、预览 README），不按单词拦。
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export const LIMITS = { intro: 120, items: 20, itemChars: 50, total: 1500 };

// [正则, 说明]。匹配不区分大小写。
const BANNED = [
  [/测试底盘|用例(全过|全部通过)|\d+ ?个(测试|用例)|测试全过|真机.{0,6}验证|clippy|tsc --noEmit|vitest|cargo (test|audit)|tests? (all )?pass/i,
    "测试和验证情况放 PR 描述"],
  [/CVE-?\d|RUSTSEC|GHSA-|security advisory/i, "不写漏洞编号，改成「修复了一个安全问题」"],
  [/靠 CI|CI (矩阵|全绿|改用|通过)|CI 的 (Windows|macOS)|持续集成|GitHub Actions 改|release\.yml|\bCI (matrix|is green)\b/i,
    "项目自己的 CI 和构建改动用户看不到"],
  [/(依赖|插件|Tauri|Rust|Node|SQLite|crate).{0,8}(升级|升到|升至|bump)|(upgrade|bump)(d|s)? .{0,20}(dependenc|to v?\d)/i,
    "依赖升级不写，除非用户因此要做什么"],
  [/(官网|website|README).{0,6}(改版|更新|改成|重写|换成)|(更新|改版)了?.{0,4}(官网|website|README|文档)/i,
    "官网和文档改动不写进发布说明"],
  [/轮询|间隔(改|调)成?|\d+ ?(ms|毫秒)\b|polling/i, "实现参数用户感知不到"],
];

const LIST_ITEM = /^\s*(?:[-*+]|\d+[.)])\s+/;

// 中日韩字符算 1，其余连续的 ASCII 单词算 1，空白不算。粗略对应「读起来有多长」。
export function length(text) {
  const CJK = /[　-鿿＀-￯]/g;
  const cjk = text.match(CJK)?.length ?? 0;
  const words = text.replace(CJK, " ").match(/[A-Za-z0-9_.+#/-]+/g)?.length ?? 0;
  return cjk + words;
}

export function check(markdown) {
  const errors = [];
  // HTML 注释（草稿说明）不显示，不计入。
  const body = markdown.replace(/<!--[\s\S]*?-->/g, "").trim();
  const lines = body.split(/\r?\n/);

  const firstBlock = lines.findIndex((l) => /^#{1,6}\s/.test(l) || LIST_ITEM.test(l));
  const intro = (firstBlock === -1 ? lines : lines.slice(0, firstBlock)).join("").trim();
  if (length(intro) > LIMITS.intro) errors.push(`开头一段 ${length(intro)} 字，上限 ${LIMITS.intro}`);

  const items = lines.filter((l) => LIST_ITEM.test(l));
  if (items.length > LIMITS.items) errors.push(`共 ${items.length} 条，上限 ${LIMITS.items}`);
  for (const item of items) {
    const n = length(item.replace(LIST_ITEM, ""));
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

function main(files) {
  if (files.length === 0) {
    console.error("用法：node scripts/check-release-notes.mjs <文件>...");
    return 2;
  }
  // 在 GitHub Actions 里用 ::error:: 输出，原因会直接显示在运行摘要上。
  const inActions = process.env.GITHUB_ACTIONS === "true";
  let failed = false;
  for (const file of files) {
    const errors = check(readFileSync(file, "utf8"));
    if (errors.length === 0) {
      console.log(`✓ ${file}`);
      continue;
    }
    failed = true;
    console.log(`✗ ${file}（写法见 docs/release-notes/README.md）`);
    for (const e of errors) console.log(inActions ? `::error file=${file}::${e}` : `  - ${e}`);
  }
  return failed ? 1 : 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exit(main(process.argv.slice(2)));
}
