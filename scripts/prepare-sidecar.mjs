#!/usr/bin/env bun
// 编译 cc-reporter 并按 Tauri sidecar 命名规则放进 app/src-tauri/binaries/：
//   cc-reporter-<target-triple>[.exe]
// 这样 tauri.conf.json 的 bundle.externalBin 才能把它打进安装包（装到主程序同目录，
// 供 ccsetup 无感接线时找到）。
//
// 用法：bun scripts/prepare-sidecar.mjs [--universal]
//   默认       —— 构建目标 triple：优先 TAURI_ENV_TARGET_TRIPLE（tauri 的 before 钩子
//                 会注入），否则取 rustc 宿主 triple（本地 dev / build）。
//   --universal —— macOS 双架构分别编译后 lipo 合并为 cc-reporter-universal-apple-darwin
//                 （CI 的 --target universal-apple-darwin 构建用）。
import { execSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "app", "src-tauri", "binaries");
mkdirSync(outDir, { recursive: true });

const run = (cmd) => execSync(cmd, { cwd: root, stdio: "inherit" });
const hostTriple = () =>
  execSync("rustc --print host-tuple").toString().trim();

const triple = process.argv.includes("--universal")
  ? "universal-apple-darwin"
  : process.env.TAURI_ENV_TARGET_TRIPLE || hostTriple();

if (triple === "universal-apple-darwin") {
  const arches = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const t of arches) {
    run(`cargo build --release -p cc-reporter --target ${t}`);
  }
  const out = join(outDir, "cc-reporter-universal-apple-darwin");
  const slices = arches
    .map((t) => `"${join(root, "target", t, "release", "cc-reporter")}"`)
    .join(" ");
  run(`lipo -create -output "${out}" ${slices}`);
  chmodSync(out, 0o755); // lipo 按 umask 建文件，不保证可执行位
  console.log(`sidecar 就绪: ${out}`);
} else {
  run(`cargo build --release -p cc-reporter --target ${triple}`);
  const ext = triple.includes("windows") ? ".exe" : "";
  const src = join(root, "target", triple, "release", `cc-reporter${ext}`);
  const dst = join(outDir, `cc-reporter-${triple}${ext}`);
  copyFileSync(src, dst);
  console.log(`sidecar 就绪: ${dst}`);
}
