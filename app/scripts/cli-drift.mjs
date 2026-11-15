#!/usr/bin/env node
// CLI 漂移检查：在本机已装的 claude / codex / kimi / gemini / opencode 的程序文件里，
// 逐条搜 Meowo 识别规则依赖的输出文案，缺了就报出来。
//
// 为什么：CLI 升级后改一句提示，Meowo 的识别规则就静默失效（2026-11-12 调研五个 CLI
// 共 16 条已失效，全是用户先撞上）。程序文件里搜模板字符串不用 spawn 会话、不触发自更新，
// 升级后跑一次就知道哪些规则要跟。
//
// 清单来自 app/src/generated/cli-text-deps.json，由 `cargo test -p meowo-agent --lib` 从插件
// 规则生成（meowo-agent/src/cli_text.rs），不手抄。只读：除了 `<cli> --version`（Meowo 启动时
// 也这样探版本）不运行任何 CLI。
//
// 四种条目：
// - 整串（needle）：程序文件里有原样的文案。
// - 分段近邻（parts + max_gap）：运行时拼接的文案，各段都在且相距不超过 max_gap 字节才算找到。
//   单搜一段泛词（`interrupt`、` to page`）哪个程序文件都有，永远报「找到」；整串又没有原样，
//   以前只能标「不搜」（做空第一轮 #7，2026-11-12）。片段带 `"` 表示要求它是 JS 字符串字面量。
// - 记录格式（rule 以 record: 开头）：transcript / wire / rollout 的事件名与字段，同样按整串搜。
// - 版本行（version_prefix）：`<cli> --version` 的真实首行要以它开头，不搜程序文件。版本行是命令名与
//   版本号运行时拼的，命令名在程序文件里的位置随链接布局变（codex 0.167.2 实测挪了 17 万字节），
//   按位置搜会在版本行没变时误报缺失。
//
// 用法：node app/scripts/cli-drift.mjs [--only claude,codex] [--json]
// 退出码：有非旧版条目缺失时为 1。

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const manifestPath = path.resolve(here, "../src/generated/cli-text-deps.json");
const args = process.argv.slice(2);
const asJson = args.includes("--json");
const onlyArg = args.find((a) => a.startsWith("--only"));
const only = onlyArg ? (onlyArg.includes("=") ? onlyArg.split("=")[1] : args[args.indexOf(onlyArg) + 1] ?? "").split(",").filter(Boolean) : [];
const isWin = process.platform === "win32";

/** PATH 上所有同名命令（Windows 用 where.exe，含 .exe/.cmd/无扩展名 shim）。 */
function whichAll(command) {
  const out = isWin
    ? spawnSync("where.exe", [command], { encoding: "utf8" })
    : spawnSync("sh", ["-c", `which -a ${command}`], { encoding: "utf8" });
  if (out.status !== 0 || !out.stdout) return [];
  return [...new Set(out.stdout.split(/\r?\n/).map((s) => s.trim()).filter(Boolean))];
}

const isScriptLike = (file) => {
  try {
    const st = fs.statSync(file);
    if (st.size > 256 * 1024) return false;
    const head = fs.readFileSync(file).subarray(0, 512).toString("latin1");
    return head.startsWith("#!") || /^@?echo|^@SETLOCAL|basedir|%~dp0|%dp0%/im.test(head) || /\.(cmd|ps1|bat)$/i.test(file);
  } catch { return false; }
};

/** 往上找包根（含 package.json 的目录）。 */
function packageRoot(file) {
  let dir = path.dirname(file);
  for (let i = 0; i < 6; i += 1) {
    if (fs.existsSync(path.join(dir, "package.json"))) return dir;
    const up = path.dirname(dir);
    if (up === dir) break;
    dir = up;
  }
  return path.dirname(file);
}

function walkJs(dir, out = [], depth = 0) {
  if (depth > 6) return out;
  let list = [];
  try { list = fs.readdirSync(dir, { withFileTypes: true }); } catch { return out; }
  for (const e of list) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) { if (e.name !== ".bin") walkJs(p, out, depth + 1); }
    else if (/\.(m?js|cjs)$/i.test(e.name)) out.push(p);
  }
  return out;
}

/** 从 PATH 命中项解析出要扫的程序文件：原生可执行直接扫；npm shim 跟到包里的 exe 或 JS bundle。 */
function resolveTargets(command) {
  const hits = whichAll(command);
  for (const hit of hits) {
    let real = hit;
    try { real = fs.realpathSync(hit); } catch { continue; }
    if (!isScriptLike(real)) {
      if (fs.statSync(real).size > 1024 * 1024) return { hit, files: [real], kind: "binary" };
      continue;
    }
    const text = fs.readFileSync(real, "utf8");
    const base = path.dirname(real);
    const refs = [...text.matchAll(/(?:\$basedir|%~?dp0%?|%dp0%)[\\/]+([^"'\s%$]+)/g)].map((m) => path.resolve(base, m[1].replace(/\\/g, "/")));
    for (const ref of refs) {
      if (!fs.existsSync(ref)) continue;
      if (/\.(m?js|cjs)$/i.test(ref)) {
        const root = packageRoot(ref);
        return { hit, files: walkJs(root), kind: "bundle", root };
      }
      if (/\.exe$/i.test(ref) || fs.statSync(ref).size > 1024 * 1024) return { hit, files: [ref], kind: "binary" };
    }
  }
  return null;
}

function version(command) {
  // Windows 上 npm 装的是 .cmd shim，要经 shell 才能起；命令名来自清单（插件 launch_stem），不含外部输入。
  const out = isWin
    ? spawnSync(`${command} --version`, { encoding: "utf8", timeout: 15_000, shell: true, windowsHide: true })
    : spawnSync(command, ["--version"], { encoding: "utf8", timeout: 15_000 });
  const line = `${out.stdout ?? ""}\n${out.stderr ?? ""}`.split(/\r?\n/).map((s) => s.trim()).find(Boolean);
  return line ?? "（读不到版本）";
}

/** 只把 ASCII 大写转小写：程序文件里的 UTF-8/UTF-16 字节照旧，搜索与大小写无关。 */
function lowerAscii(buf) {
  for (let i = 0; i < buf.length; i += 1) {
    const b = buf[i];
    if (b >= 65 && b <= 90) buf[i] = b + 32;
  }
  return buf;
}

/** 同一句文案在程序文件里可能的几种存法：UTF-8、UTF-16LE（bun 字节码常量表）、JS 转义、弯撇号。 */
function needleVariants(needle, encoding) {
  const lower = needle.toLowerCase();
  const forms = new Set([lower]);
  if (lower.includes("'")) { forms.add(lower.replaceAll("'", "’")); forms.add(lower.replaceAll("'", "\\u2019")); }
  const hex = (c, width) => c.charCodeAt(0).toString(16).padStart(width, "0");
  // 打包器把非 ASCII 写成 `❯`；0x80–0xFF 常写成 `\xb7`（claude 的 `scroll \xB7 v to`，2.1.322）。
  forms.add([...lower].map((c) => (c.charCodeAt(0) > 127 ? `\\u${hex(c, 4)}` : c)).join(""));
  forms.add([...lower].map((c) => (c.charCodeAt(0) > 255 ? `\\u${hex(c, 4)}` : c.charCodeAt(0) > 127 ? `\\x${hex(c, 2)}` : c)).join(""));
  const out = [];
  for (const f of forms) {
    if (encoding !== "utf16le") out.push(Buffer.from(f, "utf8"));
    if (encoding !== "utf8") out.push(Buffer.from(f, "utf16le"));
  }
  return out;
}

/** 片段在 buf 里所有出现位置（升序）。 */
function positions(buf, variants) {
  const out = [];
  for (const v of variants) {
    for (let at = buf.indexOf(v); at >= 0; at = buf.indexOf(v, at + 1)) out.push(at);
  }
  return out.sort((a, b) => a - b);
}

/** 有序数组里离 x 最近的距离。 */
function nearest(sorted, x) {
  let lo = 0;
  let hi = sorted.length;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (sorted[mid] < x) lo = mid + 1; else hi = mid; }
  let best = Infinity;
  if (lo < sorted.length) best = sorted[lo] - x;
  if (lo > 0) best = Math.min(best, x - sorted[lo - 1]);
  return best;
}

/**
 * 分段近邻：存在第一段的某次出现，其余每段都有一次出现离它不超过 gap 字节。UTF-8 与 UTF-16LE
 * 分开算（同一份文案不会两种编码混存），UTF-16 的间距按两倍算。
 */
function partsNear(buf, parts, gap) {
  for (const [encoding, scale] of [["utf8", 1], ["utf16le", 2]]) {
    const lists = parts.map((part) => positions(buf, needleVariants(part, encoding)));
    if (lists.some((list) => list.length === 0)) continue;
    const [first, ...rest] = lists;
    if (first.some((at) => rest.every((list) => nearest(list, at) <= gap * scale))) return true;
  }
  return false;
}

function scan(files, entries) {
  const found = new Map(entries.map((e) => [e, false]));
  const wanted = entries.filter((e) => e.needle).map((e) => [e, needleVariants(e.needle)]);
  const near = entries.filter((e) => !e.needle && e.parts?.length);
  for (const file of files) {
    let buf;
    try { buf = lowerAscii(fs.readFileSync(file)); } catch { continue; }
    for (const [entry, variants] of wanted) {
      if (found.get(entry)) continue;
      if (variants.some((v) => buf.indexOf(v) >= 0)) found.set(entry, true);
    }
    // 近邻按单个文件算：bundle 里分在两个 chunk 的片段不算挨着。
    for (const entry of near) {
      if (!found.get(entry) && partsNear(buf, entry.parts, entry.max_gap)) found.set(entry, true);
    }
  }
  return found;
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const report = [];
let missingTotal = 0;
for (const agent of manifest.agents) {
  if (only.length && !only.includes(agent.id)) continue;
  const target = resolveTargets(agent.command);
  if (!target) { report.push({ id: agent.id, name: agent.name, installed: false }); continue; }
  const ver = version(agent.command);
  const found = scan(target.files, agent.entries);
  // 版本行：拿真实首行比对（不区分大小写）。读不到版本就是缺失。
  for (const entry of agent.entries) {
    if (entry.version_prefix) found.set(entry, ver.toLowerCase().startsWith(entry.version_prefix.toLowerCase()));
  }
  const rows = agent.entries.map((e) => ({
    ...e,
    status: !e.needle && !e.parts?.length && !e.version_prefix ? "skip" : found.get(e) ? "found" : e.legacy ? "legacy-missing" : "missing",
  }));
  missingTotal += rows.filter((r) => r.status === "missing").length;
  report.push({ id: agent.id, name: agent.name, installed: true, version: ver, scanned: target.kind === "bundle" ? `${target.root}（${target.files.length} 个 JS 文件）` : target.files[0], declaredComplete: agent.declared_complete, rows });
}

if (asJson) {
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} else {
  const label = { found: "找到", missing: "缺失", "legacy-missing": "旧版兼容，缺失正常", skip: "不搜" };
  for (const r of report) {
    console.log(`\n== ${r.name}（${r.id}）`);
    if (!r.installed) { console.log("   本机没装，跳过"); continue; }
    console.log(`   版本：${r.version}`);
    console.log(`   扫描：${r.scanned}`);
    if (!r.declaredComplete) console.log("   提示：该插件的正则规则还没声明样例，下面只含自动抽取的字面量");
    const count = (s) => r.rows.filter((x) => x.status === s).length;
    const kinds = `其中分段近邻 ${r.rows.filter((x) => x.parts?.length).length}、记录格式 ${r.rows.filter((x) => x.rule.startsWith("record:") && !x.version_prefix).length}、版本行 ${r.rows.filter((x) => x.version_prefix).length}`;
    console.log(`   找到 ${count("found")}，缺失 ${count("missing")}，旧版兼容 ${count("legacy-missing")}，不搜 ${count("skip")}（${kinds}）`);
    for (const row of r.rows) {
      if (row.status === "found") continue;
      const how = row.version_prefix
        ? `（--version 首行应以「${row.version_prefix}」开头，实际是「${r.version}」）`
        : row.parts?.length
        ? `（搜${row.parts.map((p) => `「${p}」`).join("")}相距 ≤${row.max_gap} 字节）`
        : row.needle && row.needle !== row.rule.replace(/^(literal|record):/, "") ? `（搜「${row.needle}」）` : "";
      console.log(`   [${label[row.status]}] ${row.rule}${how}`);
      console.log(`       规则位置：${row.source}`);
      if (row.note) console.log(`       说明：${row.note}`);
    }
  }
  console.log(`\n共 ${missingTotal} 条缺失${missingTotal ? "：先对照上面的规则位置取证再改规则，新旧格式都要认" : ""}`);
}
process.exitCode = missingTotal > 0 ? 1 : 0;
