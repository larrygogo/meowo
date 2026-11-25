#!/usr/bin/env node
// 团队托管文件（app/src-tauri/src/team_assets）的个人内容扫描：命中就退出码 1，CI 里跑。
//
// 为什么要有：这些文件随 Meowo 以 MIT 公开发布，推上去就留在 git 历史和别人的 fork 里收不回来。
// 团队最初是从一个人的私人配置长出来的，功底清单里混着本机路径、用户原话的出处日期和项目名，
// 人工看一遍会漏（初版扫到 7 处项目名、20 多处出处）。
//
// 通用规则写在这里；项目名、公司名这类不能写进公开仓库的词，放在本机的私有词表里
// （环境变量 TEAM_PRIVATE_TERMS 指向的文件，默认 ~/.meowo/team/private-terms.txt，一行一个，不区分大小写）。
// CI 上没有私有词表，只跑通用规则；改团队文件的人在本机提交前跑一次带词表的。

import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { join, relative, resolve, dirname } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const DEFAULT_ROOT = resolve(here, "../app/src-tauri/src/team_assets");

/** 通用规则：[名字, 正则]。占位写法（`<名字>`、`{{PERSONAL_DIR}}`）不算命中。 */
export const RULES = [
  ["本机用户目录（Windows）", /[A-Za-z]:[\\/]+Users[\\/]+(?!<)[^\\/\s`'"]+/],
  ["本机用户目录（macOS）", /(?<![\w.])\/Users\/(?!<)[^/\s`'"]+/],
  ["本机用户目录（Linux）", /(?<![\w.])\/home\/(?!<)[^/\s`'"]+/],
  ["个人工作区路径", /[\\/]Desktop[\\/]+workspace\b/i],
  // 出处日期：「用户 2026-11-24 同意」「（2026-11-23）」这类。规则要写清它在防什么，不写谁哪天说的。
  ["出处日期", /\b20\d\d-\d{1,2}-\d{1,2}\b|\b20\d\d年\d{1,2}月/],
  ["邮箱", /[\w.+-]+@[\w-]+\.[\w.]+/],
];

export function loadPrivateTerms(file = process.env.TEAM_PRIVATE_TERMS || join(homedir(), ".meowo", "team", "private-terms.txt")) {
  if (!existsSync(file)) return { file, terms: [] };
  const terms = readFileSync(file, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
  return { file, terms };
}

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else yield path;
  }
}

/** 返回命中列表：{ file, line, rule, text }。 */
export function scan(root, terms = []) {
  const hits = [];
  // 英文词按词边界匹配：短项目名（三个字母）当子串会误中 node 之类的常用词；中文没有词边界，按子串。
  const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const patterns = terms.map((term) => {
    const head = /^\w/.test(term) ? "(?<![A-Za-z0-9_])" : "";
    const tail = /\w$/.test(term) ? "(?![A-Za-z0-9_])" : "";
    return new RegExp(head + escape(term) + tail, "i");
  });
  for (const file of walk(root)) {
    const lines = readFileSync(file, "utf8").split(/\r?\n/);
    lines.forEach((text, index) => {
      for (const [rule, pattern] of RULES) {
        if (pattern.test(text)) hits.push({ file: relative(root, file), line: index + 1, rule, text: text.trim() });
      }
      patterns.forEach((pattern, i) => {
        // 私有词本身不打印全文，只打印第几个词：输出会进 CI 日志，日志是公开的。
        if (pattern.test(text)) hits.push({ file: relative(root, file), line: index + 1, rule: `私有词表第 ${i + 1} 个词`, text: "" });
      });
    });
  }
  return hits;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(process.argv[2] || DEFAULT_ROOT);
  const { file, terms } = loadPrivateTerms();
  const hits = scan(root, terms);
  for (const hit of hits) console.log(`${hit.file}:${hit.line} [${hit.rule}] ${hit.text}`);
  console.log(
    terms.length
      ? `私有词表：${file}（${terms.length} 个词）`
      : "没有私有词表，只跑了通用规则（本机提交前请配 ~/.meowo/team/private-terms.txt 再跑一次）",
  );
  if (hits.length) {
    console.error(`团队托管文件里有 ${hits.length} 处个人内容，这些文件会公开发布，请改成通用说法或占位符。`);
    process.exit(1);
  }
  console.log("团队托管文件个人内容扫描：0 命中");
}
