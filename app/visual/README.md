# 视觉回归截图

样式重构前后各拍一份，逐像素对比，确认「只改了想改的地方」。

- `shoot.mjs`：自己起 vite（127.0.0.1:5191，`--strictPort`），用 Playwright 按固定场景矩阵截图，拍完关掉 vite 和浏览器。
- `compare.mjs`：比较两个目录里的同名 PNG，打印差异像素数，给有差异的图生成差异图（灰底 + 品红标出变化的像素）。
- `desktop.html` / `desktop.tsx`、`mobile.html` / `mobile.tsx`：截图用的 harness 页，挂真实组件，后端全换成 `src/demo/mock` 的内存假数据；`fixtures.ts` 是额外的长内容和「更改」面板假数据。
- `vite.visual.config.mjs`：和演示页同样的 `define`（mock 挂到演示专用的全局名上），依赖预构建缓存放系统临时目录，不碰主仓库 `node_modules/.vite`。

不需要 Tauri、不启动 Meowo、不读写 `~/.meowo`。

## 用法

在 `app/` 下：

```bash
# 1. 改样式之前，拍基线（目录放仓库外）
bun run visual:shoot --out <基线目录>

# 2. 改完再拍一份
bun run visual:shoot --out <新目录>

# 3. 对比；有差异时退出码为 1，差异图默认写到 <新目录>/_diff/
bun run visual:compare <基线目录> <新目录>
```

常用参数：

- `--filter <正则>`：只拍名字匹配的图，例如 `--filter "^chat__"`、`--filter "dark__w1280"`。对比时加 `--subset`，基线里多出来的图不算缺图。
- `--list`：只列出会拍哪些图，不起 vite。
- `--jobs <n>`：并发页数，默认 4。
- `--engine webkit`：换 WebKit 拍（默认 Chromium，对应 Windows 的 WebView2）。
- `compare.mjs --tolerance <0~255>`：单通道允许的差值，默认 0（严格一致）；`--out <目录>` 指定差异图位置。

全量 164 张，本机约 50 秒。

## 场景矩阵

文件名格式：`<场景>__<语言>__<主题>__w<宽>__dpr<倍率>.png`。

| 场景 | 内容 | 宽度 |
|---|---|---|
| `chat` | 对话窗：侧栏、长内容会话、底部审批卡 | 620 / 1280 / 2560 |
| `chat-long` | 同上，滚到表格/代码块处并展开工具组（长输出、报错工具卡） | 620 / 1280 / 2560 |
| `panel-changes` | 右侧面板「更改」：仓库、提交框、推送、PR 与 CI、文件清单 | 620 / 1280 / 2560 |
| `panel-diff` | 「更改」里点开 `LoginForm.tsx` 的 diff | 620 / 1280 / 2560 |
| `settings-general` / `-appearance` / `-account` / `-remote` | 设置窗四个分区（远程分区开着，带二维码） | 620 / 1280 / 2560 |
| `onboarding` | 引导页第一步 | 620 / 1280 / 2560 |
| `island-compact` / `-panel` / `-alert` | 灵动岛收起 / 展开面板 / 审批提醒，垫在固定灰底上 | 700 |
| `mobile-list` / `-chat` / `-changes` | 手机远程页：会话列表 / 对话 + 审批卡 / 「更改」页 | 375 / 430 |
| `mobile-settings` / `-new` / `-gate*` / `-boot` / `-keys` / `-list-empty` / `-list-error` | 手机页其余状态：设置屏、新建会话浮层、配对门各态、加载占位、终端画面兜底、列表空态与首载失败（只拍 dpr 2） | 375（部分加 430） |
| `mobile-*-kbd` / `-land` / `-ipad` / `-ipadland` | 同一场景换视口：键盘弹出（375×476、375×376）、横屏 812×375、iPad 820×1180 与 1180×820 | 见文件名 |

每个场景都拍深色、浅色两种主题，dpr 1 和 2。语言默认只拍中文，`chat` 与 `mobile-chat` 另加英文。

## 怎么保证两次拍出来一模一样

- 假时钟：Playwright `clock.install` 后立即暂停，只按 100ms 一步推进 1.8 秒。`Date`、定时器、`requestAnimationFrame` 都归它管，轮询和防抖的触发次数固定。1.8 秒也短于灵动岛最短的眨眼间隔（2.5 秒）。
- `Math.random` 换成固定种子的伪随机。
- 注入样式把动画、过渡时长清零，隐藏文本光标；同时开 `prefers-reduced-motion`，截图用 `animations: "disabled"`。
- 时区固定 `Asia/Shanghai`，locale 跟语言走。
- 等字体分片全部加载完：连续两轮都没有 `loading` 状态的字体才往下走。
- Chromium 关 GPU 和 LCD 次像素抗锯齿，走软件光栅。
- 每张图都要连续两次截图字节完全一致才落盘，否则多推一步时钟重拍，6 次还不一致就报错。
- 页面报错、资源 4xx/5xx（比如字体被拒）、缺少场景的关键元素，都按失败处理，不会把坏图拍进基线。失败现场图在 `<输出目录>/_failed/`。

## 注意

- 基线只能和同一台机器、同一个 Playwright/浏览器版本拍的图比。换机器或升级 Playwright 后先重拍基线。字体走 `@fontsource` 打包的 Inter 与 Noto Sans SC，系统字体只在回退时用到。
- 拍基线和改完重拍都要用同一个命令和参数，不要一份 `--jobs 1`、一份 `--jobs 8` 混着比（理论上应该一致，但这样排查问题更省事）。
- 端口默认 5191，被占用时脚本会直接报错退出，不会换端口。多个 worktree 并行拍图时用环境变量 `VISUAL_PORT=5192` 之类各用一个端口，依赖缓存目录也随端口分开。
- 截图里的 `DEV` 角标是 vite 开发模式带出来的，两边都有，不影响对比。
