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
- 补看场景的 `steps`（`click` / `rclick` / `hover` / `type` / `key` / `tick` / `log` …）是截图前替用户做的真实指针与键盘动作，见 `shoot.mjs` 的 `runSteps`；`?ui=1.5` 拍界面缩放。
- `VISUAL_EXTRA=<仓库外的 .mjs>`：临时追加一批补看场景（文件同样导出 `QA_SHOTS`）。量对齐用的 `log` 步骤、一次性的排查场景放那里，不用改仓库里的 `qa-scenes.mjs`。
- `?max=1`：右侧面板最大化（`panel-*` 与 `flow-md` 等带面板的场景都认）。
- `--jobs <n>`：并发页数，默认 4。
- `--engine webkit`：换 WebKit 拍（默认 Chromium，对应 Windows 的 WebView2）。
- `compare.mjs --tolerance <0~255>`：单通道允许的差值，默认 0（严格一致）；`--out <目录>` 指定差异图位置。

全量 384 张（`--list` 看确切数），本机三分钟左右。

## 场景矩阵

文件名格式：`<场景>__<语言>__<主题>__w<宽>__dpr<倍率>.png`。

| 场景 | 内容 | 宽度 |
|---|---|---|
| `chat` | 对话窗：侧栏、长内容会话、底部审批卡 | 620 / 1280 / 2560 |
| `chat-long` | 同上，滚到表格/代码块处并展开工具组（长输出、报错工具卡） | 620 / 1280 / 2560 |
| `panel-changes` | 右侧面板「更改」：仓库、提交框、推送、PR 与 CI、文件清单 | 620 / 1280 / 2560 |
| `panel-diff` | 「更改」里点开 `LoginForm.tsx` 的 diff | 620 / 1280 / 2560 |
| `settings-<分区>` | 设置窗九个分区：`general` `sessions` `appearance` `shortcuts` `remote`（开着，带二维码）`network`（自定义代理）`account`（带多账号列表）`extensions` `about` | 620 / 1280 / 2560 |
| `settings-search` / `-noresults` | 设置搜索：有命中（各分区平铺）/ 无结果 | 620 / 1280 / 2560 |
| `onboarding` | 引导页第一步（拉宽拉高的压力测试） | 620 / 1280 / 2560 |
| `newsession-page` / `-page-wt` | 对话窗里的新建会话页（只搭主列容器，不含侧栏与标题栏）；`-wt` 勾上「在新 worktree 里开」 | 620 / 1280 / 2560 |
| `settingswin-<分区>` | 同 `settings-*`，按真实窗口尺寸拍 | 620×460 |
| `onboarding-1` ～ `-4` | 引导四步，真实窗口尺寸 | 460×600 |
| `updater-checking` / `-latest` / `-error` | 更新窗紧凑态：检查中 / 已最新 / 失败 | 400×252 |
| `updater-available` / `-downloading` / `-installing` | 更新窗增高态：发现新版（带更新说明）/ 下载中 42% / 安装中 | 400×500 |
| `confirm` / `-danger` / `-alert` / `-long` | 确认框：普通 / 危险 / 单按钮提示 / 多行长文 | 420×132（长文 420×400） |
| `newsession-window` / `-window-full` | 新建会话独立窗；`-full` 带 worktree 选项、重名警告与接入项警告 | 460×520 |
| `crash` | 窗口级崩溃兜底页 + DEV 角标 | 620×460 |
| `island-compact` / `-panel` / `-alert` | 灵动岛收起 / 展开面板 / 审批提醒，垫在固定灰底上 | 700 |
| `island-compact-calm` / `-many` / `-idle` / `-notch` | 收起（概览样式）：只有在跑和跑完的 / 十二个会话折成 +N / 没有会话 / macOS 刘海两翼 | 700 |
| `island-focus` / `island-minimal`（各带 `-idle` / `-many` / `-notch`） | 收起的另外两种样式 | 700 |
| `island-strip` | 极简细条：四种状态各一条，外加没有会话的一条 | 1000 |
| `island-panel-single` / `-empty` / `-menubar`、`island-usage` | 面板：单页不出翻页 / 空态 / macOS 菜单栏落点 / 额度页 | 700 |
| `island-alert-error` / `-done` / `-menubar` | 提醒：出错（长标题长详情）/ 跑完 / 菜单栏落点 | 700 |
| `mobile-list` / `-chat` / `-changes` | 手机远程页：会话列表 / 对话 + 审批卡 / 「更改」页 | 375 / 430 |
| `flow-states` / `-mid` / `-top` | 对话流的各种条目：长用户消息渐隐、思考、编辑卡与命令卡（展开）、子任务、Markdown 全家桶、本轮改动、跨会话消息、压缩分隔条、回合报错、运行行；分别停在底部 / 中段 / 顶部 | 620 / 1280 |
| `flow-question` | 问答卡（一道单选带说明 + 一道多选） | 620 / 1280 |
| `flow-empty` | 空会话欢迎面板 | 620 / 1280 |
| `pop-palette` / `pop-shortcuts` | 命令面板（Ctrl+K）、快捷键速查表（?） | 620 / 1280 |
| `pop-add` / `pop-progress` | 输入框「+」菜单、停靠栏进度面板 | 620 / 1280 |
| `panel-files` | 右侧面板「文件」页签 | 620 / 1280 |
| `mobile-settings` / `-new` / `-gate*` / `-boot` / `-keys` / `-list-empty` / `-list-error` | 手机页其余状态：设置屏、新建会话浮层、配对门各态、加载占位、终端画面兜底、列表空态与首载失败（只拍 dpr 2） | 375（部分加 430） |
| `mobile-question` / `mobile-tasks` | 手机对话页换成问答卡（矮屏下选项可见性，量测脚本用，不进矩阵）/ 派过后台 Bash（一条在跑、三条跑完）与一个在跑的子代理（`qa-m-tasks*`、`qa-m-subagents` 从它点开后台任务面板） | 375 / 430 |
| `mobile-*-kbd` / `-land` / `-ipad` / `-ipadland` | 同一场景换视口：键盘弹出（375×476、375×376）、横屏 812×375、iPad 820×1180 与 1180×820。覆盖 `mobile-list` / `-chat` / `-changes` / `-settings`。**只拍 dpr 2**，文件名带高度（`w375h376`），按 `dpr1` 筛基础矩阵时看不到它们 | 见文件名 |
| `qa-*` | 补看场景（`qa-scenes.mjs`）：要靠真实点击、悬停、键盘才出现的弹层与状态（斜杠菜单、@ 补全、模型菜单与模型市场、截图标注、重命名、Lead 关卡卡、Broker 审批、屏幕识别的五种交互卡、同步中断、worktree setup、排队消息、灯箱、区内搜索、侧栏各态、会话管理页、右侧面板的 Markdown / 浏览器 / 终端页签、托管终端页签、设置窗的更新状态 / 下拉 / 聚焦 / 保存失败、手机底部面板），以及缩放 150%、英文、2560 超宽。假数据在 `qaFlow.ts`（harness 场景名 `flow-<名字>`） | 见各条 `size` |
| `qa-card-*` / `qa-m-card-*` | 五家的审批卡、提问卡、提示卡（harness 场景名 `flow-card-<名字>` 与 `mobile-card-<名字>`，数据在 `cardScenes.ts`，桌面与手机同一份）：五家各一张审批卡、Claude 屏幕兜底的多选项卡、Kimi 与 Gemini 的提问表单、Gemini 额度耗尽、OpenCode 的二次确认页、长命令在矮窗与键盘弹出下的卡内滚动。样本用测试里那份（`src/test/requestSamples.ts`，各自注明出处），能力表用测试夹具的 `chatUi(provider)`，所以与真实会话走同一条识别链 | 620×760、375×812（长命令 620×480、375×476） |

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
- 正式开拍前有一遍预热（每个场景用 800×600 的小视口各开一次，只为让 vite 把模块转译完）。预热不检查关键元素、也不留现场图：这个宽度下侧栏是收起的，找不到侧栏里的元素是正常的。
- 安全区（刘海、home 横条）：Playwright 的设备模拟里 `env(safe-area-inset-*)` 恒为 0。场景上写 `safeArea: { top, bottom, left, right }`，`shoot.mjs` 用 Chromium 的 CDP（`Emulation.setSafeAreaInsetsOverride`）覆盖这组值，`qa-m-changes-safe` 就是这么拍的。只在 Chromium 下生效，`--engine webkit` 时跳过；这只验证 CSS 取到了值，不等于真机验证。
- 桌面 harness 关掉了 WebGL，xterm 退回 DOM 渲染：WebGL 画布在截图那一帧是空的，托管终端场景（`qa-term*`）会整片漆黑。所以这些图验证的是终端的排版、配色、搜索命中与链接提示，不验证 WebGL 渲染器本身。
- 托管终端场景（`qa-term*`）的浅色图整窗是深色，不是 harness 没切主题：产品规定终端页签不论主题恒为深色（`styles.css` 的 `.chat-window.is-terminal`）。浅色图与深色图只差一个 DEV 角标。

## 注意

- 基线只能和同一台机器、同一个 Playwright/浏览器版本拍的图比。换机器或升级 Playwright 后先重拍基线。字体走 `@fontsource` 打包的 Inter 与 Noto Sans SC，系统字体只在回退时用到。
- 拍基线和改完重拍都要用同一个命令和参数，不要一份 `--jobs 1`、一份 `--jobs 8` 混着比（理论上应该一致，但这样排查问题更省事）。
- 端口默认 5191，被占用时脚本会直接报错退出，不会换端口。多个 worktree 并行拍图时用环境变量 `VISUAL_PORT=5192` 之类各用一个端口，依赖缓存目录也随端口分开。
- 截图里的 `DEV` 角标是 vite 开发模式带出来的，两边都有，不影响对比。
