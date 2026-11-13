// 从矢量母图 src-tauri/icons/icon.svg 重新生成全套应用图标：
//   src-tauri/icons 下的 png / ico / icns（Windows 托盘图标就是 icon.ico，window.rs 取默认窗口图标）、
//   README 的 docs/images/logo.png、官网的 site/public/logo.png 与 favicon.ico。
// 前端里的 logo（设置页、引导示意图、手机页、演示页）都是 import 这里的 128x128*.png，跟着就换了。
// 不管的：macOS 菜单栏的单色模板（gen-menubar-template.mjs）、NSIS 安装器位图（仓库根 scripts/generate-nsis-bitmaps.py）。
//
// 做法：render-icon.mjs（Playwright）把 svg 渲成 1024 母图 → `tauri icon` 出各尺寸到临时目录 →
// 只把仓库里已经跟踪的那几个文件名拷回去（tauri icon 还会吐 android/ ios/ 两个目录，用不上）。
// 用法：node app/scripts/gen-icons.mjs      （必须用 node：Playwright 在 bun 下跑不了）
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";
import sharp from "sharp";

const dir = path.dirname(fileURLToPath(import.meta.url));
const app = path.join(dir, "..");
const repo = path.join(app, "..");
const iconsDir = path.join(app, "src-tauri/icons");
const work = mkdtempSync(path.join(tmpdir(), "meowo-icons-"));

try {
  const master = path.join(work, "master.png");
  execFileSync(process.execPath, [path.join(dir, "render-icon.mjs"), path.join(iconsDir, "icon.svg"), master, "1024"], { stdio: "inherit" });

  const out = path.join(work, "out");
  const tauriCli = path.join(app, "node_modules/@tauri-apps/cli/tauri.js");
  execFileSync(process.execPath, [tauriCli, "icon", master, "-o", out], { stdio: "inherit", cwd: app });

  // 只更新已有的文件：icons 目录里还有 svg 母图、菜单栏模板、NSIS 位图，不归这里管。
  const generated = new Set(readdirSync(out));
  const updated = [];
  for (const name of readdirSync(iconsDir)) {
    if (!/\.(png|ico|icns)$/.test(name) || !generated.has(name)) continue;
    copyFileSync(path.join(out, name), path.join(iconsDir, name));
    updated.push(name);
  }
  console.log("src-tauri/icons:", updated.join(" "));

  // README 与官网用 256 的 logo；favicon 与应用 ico 是同一份。
  const logo256 = await sharp(master).resize(256, 256).png().toBuffer();
  for (const rel of ["docs/images/logo.png", "site/public/logo.png"]) {
    const target = path.join(repo, rel);
    if (!existsSync(target)) continue;
    await sharp(logo256).toFile(target);
    console.log("wrote", rel);
  }
  const favicon = path.join(repo, "site/public/favicon.ico");
  if (existsSync(favicon)) {
    copyFileSync(path.join(iconsDir, "icon.ico"), favicon);
    console.log("wrote site/public/favicon.ico");
  }
} finally {
  rmSync(work, { recursive: true, force: true });
}
