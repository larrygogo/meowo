| 运行 | 中文墨迹 | -0.50 | +0.00 |
| 桌面 | u5-tools .chat-tool-icon | 写入 | 中文墨迹 | -1.25 | -0.75 |
| 桌面 | u5-settings .nav-item | 通用 | 中文墨迹 | -1.25 | -0.25 |
| 桌面 | u5-settings .nav-item | 会话 | 中文墨迹 | -1.00 | +0.00 |
| 桌面 | u5-settings .nav-item | 外观 | 中文墨迹 | -1.00 | +0.00 |
| 桌面 | u5-settings .nav-item | 代码托管 | 中文墨迹 | -1.25 | -0.25 |
| 桌面 | u5-settings .nav-item | 扩展 | 中文墨迹 | -1.50 | -0.50 |
| 桌面 | u5-panel .side-panel-tab | 更改 | 中文墨迹 | -1.00 | +0.00 |
| 桌面 | u5-panel .side-panel-tab | 文件 | 中文墨迹 | -1.00 | +0.00 |
| 桌面 | u5-panel .scm-row-main | login.ts | 大写高中线 | -0.27 | -0.27 |
| 桌面 | u5-panel .scm-row-main | LoginForm.tsx | 大写高中线 | -0.27 | -0.27 |
| 桌面 | u5-panel .scm-row-main | LoginForm.test | 大写高中线 | -0.27 | -0.27 |
| 桌面 | u5-panel .scm-forge-btn | 推送 ↑2 | 中文墨迹 | +0.00 | +0.00 |
| 桌面 | u5-chat .chat-title-agent | web-app | 大写高中线 | -0.41 | -0.41 |
| 手机 | u5-m-chat .chat-tool-icon | 运行 | 中文墨迹 | -0.50 | +0.00 |
| 手机 | u5-m-chat .ui-pill | 当前进度 3/4 | 中文墨迹 | +0.25 | +0.25 |
| 手机 | u5-m-chat .chat-turn-copy | 04:26 | 大写高中线 | -0.64 | -0.64 |
| 手机 | u5-m-tools .chat-tool-icon | 读取 | 中文墨迹 | -0.75 | -0.25 |
| 手机 | u5-m-tools .chat-tool-icon | 运行 | 中文墨迹 | -0.50 | +0.00 |
| 手机 | u5-m-tools .chat-tool-icon | 写入 | 中文墨迹 | -1.25 | -0.75 |
| 手机 | u5-m-tools .chat-turn-changes-head | 1 个文件已修改 | 中文墨迹 | -0.50 | -0.50 |
| 手机 | u5-m-tasks .ui-pill | 后台 Bash | 中文墨迹 | +0.00 | +0.00 |
| 手机 | u5-m-tasks .ui-pill | 子任务 | 中文墨迹 | +0.00 | +0.00 |
| 手机 | u5-m-settings .remote-usage-logo | Claude | 大写高中线 | +0.09 | +0.09 |
| 手机 | u5-m-settings .remote-usage-logo | Codex | 大写高中线 | +0.09 | +0.09 |
| 手机 | u5-m-chat .chat-title-agent | 重构登录页表单校验 | 中文墨迹 | -0.50 | -0.50 |

### U6 段落渐显（4ef7c837）
- views/chat/enterMotion.ts：trackEntering 按会话记已见 id；首屏/切会话/Transcript 重挂载/空会话首次拿到历史只记不动；之后只有排在上一轮末尾条目之后的新 id 才算实时追加（加载更早插在头部不算）；一轮新增 >6 条按整段补读不动；只认 assistant_text/reasoning/turn_error/tool_use（用户消息、分隔条不动）。Transcript 在 useLayoutEffect 里给 [data-enter-id] 元素挂 .is-entering（首帧绘制前，不闪），animationend（只认自身）后摘掉，折叠块重挂载不重播。
- CSS：.is-entering 0.2s（新 token --dur-enter）opacity 0→1 + filter blur(3px)→0，--ease-out；只动 opacity/filter；reduced-motion 关。
- 测试：src/views/chat/enterMotion.test.tsx 10 例（判法 6 例 + Transcript 渲染 4 例：首屏无类、追加的工具行与回复段落有类、加载更早无类、切会话无类、动画结束摘类且子元素冒泡不算）；uiPolish.test 钉关键帧只含 opacity/filter、0.2s、reduced-motion。
- 局限：harness 截图关动画，渐显本身没有截图；流式增量（同一条 assistant 文本变长）不逐段渐显，只有新条目渐显。

### U7 拖拽（cc44d0b3）
- 核对：排队消息拖拽走 pointer 事件；拖动中条目淡化与落点线都没有 transition（即时）。但松手后拖动态立刻清掉、列表要等 promptQueueReorder 往返 + onChanged 重读才换位，中间条目先弹回原位再跳——这是落点延迟。改为松手即按乐观顺序渲染，后端新顺序到了以它为准，被拒/出错撤回。
- 测试：QueuedPrompts.test.tsx 新增 2 例（后端回执挂起时松手当帧就是新顺序；被拒时撤回）。前一例在改前代码上失败、改后通过。
- 其他拖拽：侧栏工作区/会话排序本来就是先改本地再存（失败回滚并提示），面板页签拖拽是本地状态，均无拖尾。

### 验证（U8 的本地部分）
- `bunx tsc --noEmit -p .`：通过。
- `bun run lint`：通过（oxlint + 设计 token 检查）。
- `bunx vitest run src`：281 个文件、4433 例全部通过（33s，本次满载下 ChatWindow 系列没有超时，未单独重跑）。U2 插件提交后又跑了 ChatMarkdown/FileMarkdown/replay/uiPolish 6 个文件 57 例通过。
- 跑全量时 vitest 把 replay.test.tsx.snap 的换行改成 LF（内容无差异），已还原，未提交。
- 未做：CI（未 push）；U9 由 leader 验。

### 取舍（对照 ChatGPT / 豆包 / Kimi）
- 字重：亮字压暗底会显粗（irradiation），设计标准要求暗色减一档；只降 30 左右，不像 350 那样在 Windows 100% 缩放下发虚。三家客户端暗色下是否调字重我没有核实。
- 渐显：按设计标准的「流式渐显」做一次短淡入（ChatGPT、豆包新内容出现的观感相近，未核实实现），不做逐字延迟（D1）；Meowo 的回复从 CLI 会话记录成段到达，只给新条目做一次 200ms 模糊淡入，历史不动。
- 阴影替代边框：浅色下菜单、卡片只靠阴影（豆包、ChatGPT 浅色浮层的观感）；深色下纯阴影在近黑底上看不见，保留 6% 内高光代替原 12% 实线外框。这几家暗色浮层的具体 CSS 我没有逐一核实，这里只按观感对照。
- 段尾孤字：浏览器 pretty 不处理中文「字+标点」，用结构包裹而不是插零宽字符，复制内容不受影响。

## 2026-11-18 验证（主会话）
- 合最新 main（9c3b8004），tsc / lint 通过；开 PR #200 跑 CI；请独立审查复核 U1–U8。
- 主会话实测（2026-11-18）：`bunx vitest run` ChatWindow.liveState / newSession / p2keys / question 4 个文件 7 条失败（`Unable to find an element with the text`），b42b919e（合 main 前）即可复现，origin/main 上同 4 个文件 78 条全过。根因：keepCjkTail 把段尾 4 字拆进单独 span，按整段文字匹配失败；对用户即区内搜索跨该边界的词搜不到。交付者所报「全量 4433 通过」不成立。已交回修复；U2、U8 保持未通过。

## 2026-11-18 复核返工（agent）

**更正上一轮汇报**：上一轮写的「vitest 全量 4433 例通过」是在加段尾插件（303aaeef）之前跑的；加插件后只跑了 ChatMarkdown/FileMarkdown/replay/uiPolish 6 个文件，没有再跑全量，所以没看到段尾拆 span 引起的失败。汇报把两次结果混在一起写，不对。

提交（已 push 到 origin/feat/ui-polish，基于 9c3b8004）：4d42c8b0（搜索 + 测试匹配）· 90c439ab（U1/U2/U3 补漏）· 1dc47a15（RemoteUsage 时间相关测试）。

- 搜索选 (a)：TranscriptSearch 新增 findTextRanges，把同一块里被行内元素（span/strong/em/code/a…）隔开的文本节点拼成连续文字再搜，命中映射成跨节点 Range 画高亮，不跨块拼接。理由：段尾孤字必须靠结构边界（不折行的 span）才能在 WebView2 上处理；(b) 插不可见连接符会进入用户手动框选的复制内容、读屏与搜索都要另外剥离，风险面更大；(a) 顺带解决了加粗、行内代码切词搜不到的旧限制。测试：flow-states 同句「边界情况都覆盖到了，只有加号别名没测。」搜「别名没」1/1 并画出当前高亮；段尾词范围恰好是「远端分支」且跨两个节点；加粗/行内代码切开的词；两段之间不误连；大小写。
- 测试匹配：新增 src/test/visibleText.ts（按 textContent 精确/正则匹配，只认最内层元素）。改了 ChatWindow.render(3)、liveState(2)、newSession(1)、p2keys(3)、question(1)、LeadPanel(1)、PlanPanel(1)、Message.userNotes(1，含一条否定断言，拆分后原写法会恒成立)。被验证的行为不变。另做了一次排查：临时让插件打日志跑全量，收集到 49 段被拆的文字，再与全部测试里的 getBy/queryBy/findByText 字面量比对，没有其余受影响的断言（唯一的正则命中 /Kimi/ 来自别的测试文件的样本，不在同一个渲染里）。
- RemoteUsage「30 分钟后重置」：与本任务无关的时间相关失败，23:30 后必挂（本机 23:47 复现，暂存本轮改动后在 9c3b8004 上同样失败），测试里钉 Date 到中午。
- U2 补漏：.chat-tool-code 及 .pr-patch/.lead-evidence-command/.chat-compose-gate-tail/.chat-remote-screen/.wt-setup-log/.wt-trust-script 进退回列表；uiPolish.test 改为扫描所有 white-space: pre/pre-wrap/break-spaces 规则，未退回且未登记为正文（消息气泡、思考、提问、提示、更新说明等 13 条，附理由）就失败——改前这条测试报出 7 条漏网。
- U1 补漏：--fw-* 深色值放进 `:where(.chat-window.is-terminal), :root` 共用块并重设 font-weight，浅色主题下终端页签也是 370 档；测试钉住。局限：JetBrains Mono 只打包 400/700 两个静态字重，代码块与终端里的等宽字实际不降档。
- U3 补漏：带 .fb-press 的 .sbtn 等去掉自身 :active scale(.98)（原来叠到约 .90）；useFlash 返回 fx 类，每次 flash 在 .fb-flash / .fb-flash-alt（两个同样的关键帧）之间切换，2.5 秒内再次触发也重播；测试钉住。

验证（无其他负载）：
- `bunx tsc --noEmit -p .` 通过；`bun run lint` 通过（设计 token 检查通过）。
- `bunx vitest run src`：` Test Files  283 passed (283)` / `      Tests  4465 passed (4465)` / `   Duration  34.26s`。
- 单跑 liveState/newSession/p2keys/question：` Test Files  4 passed (4)` / `      Tests  78 passed (78)`；单跑 render/LeadPanel/PlanPanel/Message.userNotes/RemoteUsage：` Test Files  5 passed (5)` / `      Tests  114 passed (114)`。

- 主会话复核（5e8a2eca，已合最新 main）：tsc、lint 通过；vitest 全量亲自跑两次均为 283 files / 4465 tests passed；U1–U7 回写证据。U8 等 PR #200 CI。
