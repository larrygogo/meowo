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
function needleVariants(needle) {
  const lower = needle.toLowerCase();
  const forms = new Set([lower]);
  if (lower.includes("'")) { forms.add(lower.replaceAll("'", "’")); forms.add(lower.replaceAll("'", "\\u2019")); }
  const hex = (c, width) => c.charCodeAt(0).toString(16).padStart(width, "0");
  // 打包器把非 ASCII 写成 `❯`；0x80–0xFF 常写成 `\xb7`（claude 的 `scroll \xB7 v to`，2.1.322）。
  forms.add([...lower].map((c) => (c.charCodeAt(0) > 127 ? `\\u${hex(c, 4)}` : c)).join(""));
  forms.add([...lower].map((c) => (c.charCodeAt(0) > 255 ? `\\u${hex(c, 4)}` : c.charCodeAt(0) > 127 ? `\\x${hex(c, 2)}` : c)).join(""));
  const out = [];
  for (const f of forms) { out.push(Buffer.from(f, "utf8")); out.push(Buffer.from(f, "utf16le")); }
  return out;
}

function scan(files, entries) {
  const found = new Map(entries.map((e) => [e, false]));
  const wanted = entries.filter((e) => e.needle).map((e) => [e, needleVariants(e.needle)]);
  for (const file of files) {
    let buf;
    try { buf = lowerAscii(fs.readFileSync(file)); } catch { continue; }
    for (const [entry, variants] of wanted) {
      if (found.get(entry)) continue;
      if (variants.some((v) => buf.indexOf(v) >= 0)) found.set(entry, true);
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
  const rows = agent.entries.map((e) => ({
    ...e,
    status: !e.needle ? "skip" : found.get(e) ? "found" : e.legacy ? "legacy-missing" : "missing",
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
    console.log(`   找到 ${count("found")}，缺失 ${count("missing")}，旧版兼容 ${count("legacy-missing")}，不搜 ${count("skip")}`);
    for (const row of r.rows) {
      if (row.status === "found") continue;
      console.log(`   [${label[row.status]}] ${row.rule}${row.needle && row.needle !== row.rule.replace(/^literal:/, "") ? `（搜「${row.needle}」）` : ""}`);
      console.log(`       规则位置：${row.source}`);
      if (row.note) console.log(`       说明：${row.note}`);
    }
  }
  console.log(`\n共 ${missingTotal} 条缺失${missingTotal ? "：先对照上面的规则位置取证再改规则，新旧格式都要认" : ""}`);
}
process.exitCode = missingTotal > 0 ? 1 : 0;
