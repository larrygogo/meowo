# E2E 测试（WebdriverIO + Tauri）

驱动**真实运行的 Meowo 桌面应用**做端到端回归。用于验证靠单测覆盖不到的、跨「后端文件监听 ↔ Tauri IPC ↔ 前端刷新」的整链路行为——首个用例就是「贴纸看板空闲期不再自持刷新」。

## 目录结构

```
app/e2e/
├── wdio.conf.ts            # WDIO 配置（tauri service + 内嵌 WebDriver + 指向构建产物）
├── tsconfig.json          # E2E 专用 TS 配置（wdio/mocha 类型）
├── wdio.capability.json   # WDIO 插件权限（构建期临时拷入 capabilities/，见下）
├── run.mjs                # 编排器：构建 E2E 二进制 → 跑 wdio → 清理
├── specs/
│   └── board-refresh.e2e.ts   # 空闲刷新回归
└── README.md
```

## 为什么需要"特制构建"（与生产完全隔离）

WDIO 要能驱动 Tauri，需要把一个内嵌 WebDriver 服务器 + JS 执行/命令 mock 桥塞进 app。**这些绝不能进生产发行版**，因此全部以构建期开关注入，生产构建零影响：

| 注入点 | 机制 | 生产是否包含 |
|---|---|---|
| `tauri-plugin-wdio` / `-webdriver` | Cargo feature `e2e`（`Cargo.toml [features]` + `lib.rs` 里 `#[cfg(feature="e2e")]` 注册） | ❌ |
| `withGlobalTauri: true` | `tauri build --config src-tauri/tauri.e2e.conf.json` 合并覆盖 | ❌ |
| `@wdio/tauri-plugin` 前端桥 + `board-changed` 观测计数 | `VITE_E2E=1` 构建，Vite 死代码消除 | ❌ |
| `wdio:default` capability 权限 | `run.mjs` 构建前临时拷 `wdio.capability.json → capabilities/wdio.json`，跑完即删（gitignored） | ❌ |

## 运行

```bash
cd app
bun run test:e2e     # = node e2e/run.mjs：构建 E2E 二进制 → 跑 wdio → 清理
```

> ⚠️ **必须在有真实图形环境的机器上运行**（Windows/macOS/Linux 桌面）。它会启动真实的 app 窗口，没有 headless 模式；纯无头 CI（无显示器）需配虚拟显示（如 Linux 的 `xvfb`）。

### 前置依赖

- **Node 22 LTS**。⚠️ Node ≥ 26 与 WebdriverIO 不兼容（[webdriverio#15265](https://github.com/webdriverio/webdriverio/issues/15265)：Node 26 的 undici v8 拒绝 wdio 设的 Content-Length/Connection 头），`run.mjs` 会预检拦截并提示。用 `fnm`/`nvm` 切到 22，或用便携版 Node 22 的 `node.exe` 执行 `e2e/run.mjs`。
- **Rust 工具链**（构建 app）。
- **Windows**：`@wdio/tauri-service` 会按本机 WebView2 版本自动下载匹配的 `msedgedriver`（`autoDownloadEdgeDriver: true`）。需要 WebView2 Runtime（Win11 自带）。
- **Linux**：`webkit2gtk-driver`（`sudo apt-get install -y webkit2gtk-driver` 等）。
- **macOS**：内嵌 provider 原生支持，无需外置 driver。

## 首个用例：board-refresh

断言**空闲时看板不再被反复刷新**。`App` 在 `VITE_E2E` 构建下把收到的 `board-changed` 次数累计到 `window.__MEOWO_BOARD_CHANGED__`；测试等其平静后空闲观察 6 秒，断言增量 ≤ 2。修复前该增量会是几十（每 1–2 秒一次的自持刷新循环）。

对应修复：`db-watcher` 持久连接 + `PRAGMA data_version` 门控（只在别的连接真正提交写入时才发 `board-changed`）+ 排除 `-shm` 文件事件；前端另加结构相等守卫兜底。

## 备注

- `wdio.conf.ts` 里的二进制路径指向 workspace 根（app/src-tauri）的 `target/debug/meowo-app(.exe)`（`--no-bundle` 产物，名字取自 cargo package）。
- 首次运行会下载 driver、并做一次 debug 构建，较慢；之后走增量。
- `package.json` 的 `overrides` 把 `@wdio/native-utils` 钉到 `2.5.0`：`@wdio/tauri-service@1.2.0` 的代码 import 了 `installMockSyncOverride`，却把该依赖精确锁在缺此导出的 `2.4.0`（上游发布 bug），不覆盖会在启动 wdio 时报 `SyntaxError`。上游修版后可移除此 override。

## 验证记录

已在真实 Windows（WebView2/msedge 150）+ Node 22.23.1 跑通：`board-refresh` 用例通过（空闲 6s 内 board-changed 增量 ≤ 2），端到端确认了空闲刷新循环的修复。
