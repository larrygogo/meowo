// 设计 token 门禁：扫 src 下的样式表与 ts/tsx 内联样式，拦截已删除的旧档 token。
//
// 为什么要拦：旧档与新档同名不同值（--r-md=6px、--radius-md=8px），引用错了不报错，
// 只会差 2px 或差一档灰度，贴纸、设置、对话窗细节对不齐就是这么来的（UI 调研 sys2）。
// 旧档在 2026-11 统一迁走并删除定义；删除后误引用会静默落到 initial 值（圆角变直角、
// 底色变透明），比差 2px 更难发现，所以在 lint 里直接拦。
//
// 规则只看「引用」和「定义」，注释里提到旧名字不算（历史说明需要能写出来）。
//
// 用法（app/ 下）：bun scripts/lint-tokens.mjs，已接进 `bun run lint`。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const APP = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(APP, "src");
const SKIP_DIRS = new Set(["generated", "node_modules"]);

// 旧档：圆角 --r-*、字号 --fs-*、悬停底 --cc-surface* / --cc-selected、阴影 --cc-shadow-*。
// 对照：--fs-9/10→--text-2xs、--fs-11/12→--text-xs、--fs-12-5→--text-sm、--fs-13→--text-base、
// --fs-15→--text-lg；--r-sm/md/lg/12/16/pill→--radius-xs/sm/md/lg/xl/full；
// --cc-surface/-hover/--cc-selected→--fill-hover/--fill-selected/--fill-selected-hover；
// --cc-shadow-popup→--shadow-menu、--cc-shadow-panel→--shadow-md。
const OLD = String.raw`--(?:r-[a-z0-9]+|fs-[0-9][0-9-]*|cc-surface[a-z0-9-]*|cc-selected|cc-shadow-[a-z]+)(?![\w-])`;
const RULES = [
  { name: "旧档 token 引用", re: new RegExp(String.raw`var\(\s*${OLD}`, "g") },
  { name: "旧档 token 定义", re: new RegExp(String.raw`(?:^|[;{\s])${OLD}\s*:`, "gm") },
  // ts/tsx 里 style.setProperty("--r-md", …) 之类的写法
  { name: "旧档 token 字符串", re: new RegExp(String.raw`["'\`]${OLD}["'\`]`, "g"), only: /\.tsx?$/ },
];

/** 把 CSS 注释换成等长空白（保留换行），行号不变、注释里的旧名字不误报。 */
function stripCssComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
}

function walk(dir, out) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(css|tsx?)$/.test(name)) out.push(p);
  }
  return out;
}

const problems = [];
for (const file of walk(SRC, [])) {
  const raw = readFileSync(file, "utf8");
  const text = file.endsWith(".css") ? stripCssComments(raw) : raw;
  for (const rule of RULES) {
    if (rule.only && !rule.only.test(file)) continue;
    for (const m of text.matchAll(rule.re)) {
      const line = text.slice(0, m.index).split("\n").length;
      problems.push(`${relative(APP, file).replaceAll("\\", "/")}:${line}  ${rule.name}：${m[0].trim()}`);
    }
  }
}

if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`\n设计 token 检查失败：${problems.length} 处。旧档已删除，按本脚本顶部的对照换成 --radius-* / --text-* / --fill-* / --shadow-*。`);
  process.exit(1);
}
console.log("设计 token 检查通过");
