// 前端依赖审计门禁：跑 `bun audit --json`，只有「生产依赖」里出现 high/critical
// 且不在 audit-allowlist.json 豁免清单里时才失败；其余全部照常打印，不吞。
//
// 为什么自己算生产依赖：bun 1.3.14 的 audit 没有 --prod 一类开关，直接按退出码判会把
// devDependency（vite/wdio/sharp 等构建期工具）的告警一起算进来——2026-11-10 实测 93 条里
// 生产依赖链上一条 high+ 都没有，原先只能 continue-on-error 整步吞掉。
// 为什么不先 bun install：audit 只需要 package.json + bun.lock（无 node_modules 的空目录
// 实测照样出结果），装依赖在 CI 上走 bun.lock 里的国内镜像地址曾卡满 900s。
// 本脚本零依赖（CI 不装 node_modules），版本范围用最小实现比较。
//
// 用法（app/ 下）：BUN_CONFIG_REGISTRY=https://registry.npmjs.org bun scripts/audit-gate.mjs
// 本机 ~/.npmrc 指向 npmmirror 时 audit 接口返回 404，必须用环境变量临时指回 npmjs。
//
// package.json 的 overrides 把 tinypool 钉到 ^2.1.2：vitest 最新的 3.x（3.2.7）仍声明
// tinypool ^1.1.1，命中两条 critical（GHSA-5gmw-xhrv-c9v3 / GHSA-85c8-ppgw-ccpr）；
// 2026-11-10 实测 tinypool 2.2.0 下前端全量 1407 用例全过。升 vitest 4 需要 vite ≥6，另议。
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appDir = join(dirname(fileURLToPath(import.meta.url)), "..");
const GATE = new Set(["high", "critical"]);

// ── bun.lock 是带尾逗号的 JSON；去掉对象/数组收尾前的逗号即可解析（字符串里不会出现 `,}`）──
function readLock() {
  const raw = readFileSync(join(appDir, "bun.lock"), "utf8");
  return JSON.parse(raw.replace(/,(\s*[}\]])/g, "$1"));
}

// "a/b/@s/c" → ["a","b","@s/c"]：bun.lock 用 父包路径/子包名 表示嵌套安装的版本
function splitKey(key) {
  const segs = key.split("/");
  const out = [];
  for (let i = 0; i < segs.length; i++) {
    if (segs[i].startsWith("@")) out.push(`${segs[i]}/${segs[++i]}`);
    else out.push(segs[i]);
  }
  return out;
}

// 按 node 解析规则从最近的嵌套层往外找依赖实际落在哪个 lock 键上
function resolveKey(packages, fromKey, dep) {
  const chain = fromKey ? splitKey(fromKey) : [];
  for (let n = chain.length; n >= 0; n--) {
    const k = [...chain.slice(0, n), dep].join("/");
    if (packages[k]) return k;
  }
  return null;
}

// 生产依赖闭包：workspace 的 dependencies/optionalDependencies 起步，沿包的
// dependencies/optionalDependencies/peerDependencies 传递。返回 包名 → 已装版本集合
function prodClosure(lock) {
  const packages = lock.packages;
  const ws = lock.workspaces[""];
  const roots = { ...ws.dependencies, ...ws.optionalDependencies };
  const seen = new Set();
  const stack = Object.keys(roots).map((d) => resolveKey(packages, "", d)).filter(Boolean);
  while (stack.length) {
    const key = stack.pop();
    if (seen.has(key)) continue;
    seen.add(key);
    const meta = packages[key][2] ?? {};
    const deps = { ...meta.dependencies, ...meta.optionalDependencies, ...meta.peerDependencies };
    for (const d of Object.keys(deps)) {
      const k = resolveKey(packages, key, d);
      if (k && !seen.has(k)) stack.push(k);
    }
  }
  const byName = new Map();
  for (const key of seen) {
    const spec = packages[key][0]; // "name@1.2.3"
    const at = spec.lastIndexOf("@");
    const name = spec.slice(0, at);
    if (!byName.has(name)) byName.set(name, new Set());
    byName.get(name).add(spec.slice(at + 1));
  }
  return byName;
}

// ── 最小版本范围判定：advisory 的 vulnerable_versions 形如 ">=7.0.0 <7.29.0"、"<=6.2.0"、
//    带 "||"。忽略预发布标签（审计范围不会落在预发布上）──
const num = (v) => v.replace(/^v/, "").split(/[-+]/)[0].split(".").map((x) => Number(x) || 0);
function cmp(a, b) {
  const x = num(a);
  const y = num(b);
  for (let i = 0; i < 3; i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  return 0;
}
function satisfies(version, range) {
  return range.split("||").some((set) =>
    set
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .every((c) => {
        const m = /^(<=|>=|<|>|=)?\s*(.+)$/.exec(c);
        if (!m) return false;
        const d = cmp(version, m[2]);
        switch (m[1]) {
          case "<": return d < 0;
          case "<=": return d <= 0;
          case ">": return d > 0;
          case ">=": return d >= 0;
          default: return m[2] === "*" || d === 0;
        }
      }),
  );
}

// AUDIT_REPORT_FILE：喂一份现成的 audit JSON，用来验证门禁本身会拦（不发网络请求）
const res = process.env.AUDIT_REPORT_FILE
  ? { stdout: readFileSync(process.env.AUDIT_REPORT_FILE, "utf8"), stderr: "" }
  : spawnSync("bun", ["audit", "--json"], { cwd: appDir, encoding: "utf8", shell: process.platform === "win32" });
let report;
try {
  report = JSON.parse(res.stdout);
} catch {
  // 接口不通（例如走了不支持审计接口的镜像，返回 404）时不能当作「没有漏洞」放行
  console.error("bun audit 没有返回 JSON，判失败：\n" + res.stdout + res.stderr);
  process.exit(2);
}

// 豁免清单：[{ "advisory": "GHSA-…", "package": "包名", "reason": "为什么现在修不了、何时复查" }]
const allow = JSON.parse(readFileSync(join(appDir, "audit-allowlist.json"), "utf8"));
const noReason = allow.filter((e) => !e.advisory || !e.reason?.trim());
if (noReason.length) {
  console.error("audit-allowlist.json 每条都要写 advisory 和 reason：\n" + JSON.stringify(noReason, null, 2));
  process.exit(2);
}
const allowById = new Map(allow.map((e) => [e.advisory, e]));
const prod = prodClosure(readLock());
const counts = {};
const blocking = [];
const allowedHit = new Set();
for (const [name, advisories] of Object.entries(report)) {
  for (const a of advisories) {
    const ghsa = a.url.split("/").pop();
    const prodHit = [...(prod.get(name) ?? [])].filter((v) => satisfies(v, a.vulnerable_versions));
    const scope = prodHit.length ? "prod" : "dev";
    counts[`${scope}/${a.severity}`] = (counts[`${scope}/${a.severity}`] ?? 0) + 1;
    const allowed = allowById.has(ghsa);
    if (allowed) allowedHit.add(ghsa);
    const mark = scope === "prod" && GATE.has(a.severity) ? (allowed ? "豁免" : "拦截") : "提示";
    console.log(`[${mark}] ${scope} ${a.severity} ${name}@${a.vulnerable_versions} ${ghsa} ${a.title}`);
    if (mark === "拦截") blocking.push(`${name}@${prodHit.join(",")} ${ghsa}`);
  }
}
console.log("\n统计：", JSON.stringify(counts));
for (const e of allow) {
  if (!allowedHit.has(e.advisory)) console.log(`豁免清单里的 ${e.advisory}（${e.package}）已不再命中，可以删掉`);
}
if (blocking.length) {
  console.error(`\n生产依赖有 ${blocking.length} 条 high 及以上漏洞未豁免：\n  ${blocking.join("\n  ")}`);
  console.error("升级依赖修掉；确实无法修时写进 app/audit-allowlist.json 并写明原因。");
  process.exit(1);
}
console.log("\n生产依赖无未豁免的 high 及以上漏洞。");
