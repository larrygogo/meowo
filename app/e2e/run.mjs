// E2E 编排器：一条命令跑通「构建 E2E 专用二进制 → 跑 WDIO → 清理」。跨平台（node + spawnSync）。
//
// 为什么不是直接 `wdio run`：E2E 需要一个特制的 app 二进制——注入了 WDIO 插件（--features e2e）、
// 打开了 withGlobalTauri、且前端带了 @wdio/tauri-plugin 桥（VITE_E2E=1）。这些**绝不能进生产**，
// 故都以构建期开关注入，并在此临时拷入 WDIO capability、跑完即删。
import { spawnSync } from "node:child_process";
import { copyFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, ".."); // app/
const capDst = join(appDir, "src-tauri", "capabilities", "wdio.json");

// Node 版本：webdriverio#15265（Node ≥ 26 的 undici v8 拒收 wdio 手动设的 Content-Length /
// Connection 头）已在 webdriver 9.30 修掉，@wdio/* 升到 9.32 后 Node 26 实测可跑，原先的
// 「Node ≥ 26 直接拒跑」预检随之去掉。

function run(bin, args, env = {}) {
  console.log(`\n▶ ${bin} ${args.join(" ")}`);
  const r = spawnSync(bin, args, {
    cwd: appDir,
    stdio: "inherit",
    env: { ...process.env, ...env },
    shell: process.platform === "win32", // Windows 下解析 bun/.cmd 需要 shell
  });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`退出码 ${r.status}: ${bin} ${args.join(" ")}`);
}

// WDIO 权限只在 e2e 构建存在：若把 wdio.json 常驻 capabilities/，非 e2e 构建下 `wdio:default`
// 权限未定义会直接编译失败。故临时拷入、跑完必清理。
const cleanup = () => {
  try {
    rmSync(capDst, { force: true });
  } catch {
    /* noop */
  }
};
// 正常/异常退出走下面的 finally；但 finally 不覆盖信号中断（Ctrl-C 的 SIGINT / SIGTERM）——
// 若在耗时数分钟的构建期间被中断而不清理，残留的 wdio.json 会让后续普通构建因 `wdio:default`
// 权限未定义而编译失败。故显式捕获信号清理后再退出。
for (const sig of ["SIGINT", "SIGTERM"]) {
  process.on(sig, () => {
    cleanup();
    process.exit(1);
  });
}
// 兜底：清理任何上一次被硬中断遗留的残留，再拷入本次的。
cleanup();
copyFileSync(join(here, "wdio.capability.json"), capDst);
try {
  // 构建 E2E 二进制：
  //  --features e2e            注入 tauri-plugin-wdio(-webdriver)
  //  --config …tauri.e2e…      合并 withGlobalTauri=true（不改生产 tauri.conf.json）
  //  --debug --no-bundle       出到 target/debug 的裸可执行（wdio.conf 指向此处），不打安装包
  //  VITE_E2E=1                前端注入 @wdio/tauri-plugin 桥 + board-changed 观测计数
  run(
    "bun",
    ["x", "tauri", "build", "--debug", "--no-bundle", "--features", "e2e", "--config", "src-tauri/tauri.e2e.conf.json"],
    { VITE_E2E: "1" },
  );
  // 跑 WDIO（embedded provider：首次会自动匹配/下载 msedgedriver）。
  // 用 Node（process.execPath）跑 wdio 本体，不用 bun：当初 bun 的 fetch 也拒收那两个头
  // （实测创建会话即 UND_ERR_INVALID_ARG）。webdriver 9.30 起不再设它们，bun 下没复测过，
  // 不冒这个险。`bun x` 只是装包/解析便利。
  const require = createRequire(import.meta.url);
  // @wdio/cli 的 exports 只放 "."，bin 不能直接 resolve——从主入口推包根再进 bin。
  const wdioBin = join(dirname(require.resolve("@wdio/cli")), "..", "bin", "wdio.js");
  run(process.execPath, [wdioBin, "run", "e2e/wdio.conf.ts"]);
} finally {
  cleanup();
}
