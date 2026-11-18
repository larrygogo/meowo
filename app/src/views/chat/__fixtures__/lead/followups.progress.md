phase: done
- 2026-11-11 leader「继续做」：已合并 PR #163（13ba45b3），清理 12 个车道 worktree 与本地分支、删远端 feat/phase2-optimization；meowo-ui-polish 因 leader 开发版正在跑保留。开轻量档收尾四个顺手发现。
- 2026-11-11 车道回报：#1 7092fa3a（fill_line_counts 把仓库根相对路径拼到 cwd；新增 work_tree_root；测试修前 (0,0,2)→修后 (5,0,2)）；#2 095a6b18（task_notification_result 无 status 默认完成；resumed_task_id 识别续上，入队行翻回运行中、送达行只吞；修前 (0,1,0)→修后 (1,0,0)，实拍顺序回放最终完成、后台计数 0；改了 3 条旧断言）；#3 c107e4ad（标题栏走 forge 缓存，三处 60s 合计 1 次 pr view，修前 2 次）；#4 本机 4 条用户插话全是收工后续上，主会话无正文、无信号；覆盖需轮询读侧车（207 个，中位 1.0MB、p90 2.8MB、最大 11.4MB，首读整扫），交 leader 定。全量：vitest 1655、clippy 0、cargo test 1464 过。
  问题：#3 使 GitHub 企业版远端标题栏不再显示 PR（回退），已退回车道改为 github.com 走共用缓存、其他远端保留直查。
- 2026-11-11 企业版回退修复 12476876：forge 可解析的 github.com 走共用缓存，其余远端照旧目录直查 + 按目录 60s 缓存（出错也记）；测试修前空→修后有 PR、4 次查询 1 次 pr view、不调 auth status。主会话复验：forge 47、subdir 3、meowo-agent subagent 21 过。#1-#4 通过（#4 = 结论已写、摆关卡）。
- 推送 fix/ui-followups，开草稿 PR #164；起 lead-redteam（改动 374+/73- 超轻量档免做空门槛）；CI 在跑。
- 2026-11-11 leader 纠正 #4 理解：不是「子代理运行中直发」，而是托管终端里 CC 切到子代理标签页后，对话页发出的消息被 CC 交给了子代理，对话页只能标未送达。根本修法是发送前识别子代理视图并切回主会话（或明确提示）。追加验收 #6；业务事实写进 product.md。派子代理查 CC 切换子代理视图的按键与屏幕特征并实现（worktree meowo-subview，基于 fix/ui-followups，避开正在做空审查的 meowo-followups）。
- 2026-11-11 PR #164（12476876）CI run 42969784035 全绿（frontend、audit、check windows/macos）。
- 2026-11-11 做空（12476876）：#1-#5 验证通过、无回退；#6 不在此分支。新发现：中1 子代理「is waiting on its own background work」通知（status=completed、interim）使徽标提前完成（lead 会话 9372→实际 9446 结束，约 8 分钟）；中2 SendMessage 续上后完成通知的 tool-use-id 指向 SendMessage 调用，原委派挂运行中约 1.5h；低 untracked_file_diff 同 #1 拼路径。追加验收 #7 #8 #9，退回 followups 车道修；#5 待最终代码重验。
- 2026-11-11 车道回 #7 247d93c4、#8 e1c80407、#9 d80417e2，全量过（vitest 1655、clippy 0、cargo 1614）。#7 副作用：等后台后再无 finished 时会话永远运行中（本机 a6ca1514 一例）。定：徽标显示等待后台、busy 计数照减，退回车道改。
- 2026-11-11 #6 通过（fix/subagent-view 28f1358d、bb84acbb）：CC 2.1.322 子代理视图特征＝输入框上边框挂子代理描述、占位「❯ Message @…」、面板 main 行由「● main」变「◯ main」；切回＝↓ 进面板落 main 行、回车退出（不用 Esc：子代理在跑时 Esc 会中断它）。发送前（正文/贴图//btw）先切回并读屏确认（≤5 步），切不回退回输入框＋提示条「终端停在子代理「X」，发送会交给它」与「切回主会话」按钮。主会话复验：tsc 0、subagentView+SubagentViewNotice 13 例过；车道全量 vitest 181 文件 1668、clippy 0、cargo 全过。探针 1 次（隔离配置目录、未写 ~/.claude、未触发自更新）。版本漂移时退到提示条，不误发。
- 2026-11-11 #7 返工 5c3926b4：等后台通知 → SubagentOutcome.waiting=1，徽标「等待后台」（不转的等待色整环）、进度面板算未结束；busy 计数照减（会话不卡运行中）；finished 到达再完成。主会话复验：meowo-agent waiting 1 例、send_message_resume 2 例、untracked_file_diff_reads 1 例、subagentWaiting 2 例过。#7（徽标不提前收尾；计数按 decisions 取舍照减，关卡 C 向 leader 说明）#8 #9 通过。
- 2026-11-11 fix/subagent-view 合入 fix/ui-followups（5acda03f，无冲突）。合并后 tsc/lint 0，vitest 182 文件 1670 过，clippy 0，cargo test --workspace 无失败；契约按 CI 方式重生成后忽略换行无差异。推送并起做空第二轮（覆盖 #6～#9）。
- 2026-11-11 #5 通过：5acda03f CI run 42973759856 全绿（frontend、audit、check windows/macos）。等做空第二轮。
- 2026-11-11 做空第二轮（5acda03f，本任务最后一轮）：8/9 实测成立。中1 真实数据 9 次「等待后台」有 4 次再无收尾（434cc5a5×2、8e1c4e31、a6ca1514），子代理已交结论却永久挂进行中，比改前「提前几分钟完成」更伤；中2 终端停在子代理视图时对话页「中断」发 Esc 未先切回，会掐掉被看的子代理、主会话 5s 后报中断超时（撤回 /rewind 同未切回）；低1 面板不在时切回按钮无效（未取证）；低2 发送前切回无接线测试；低3 续上回执落轮末长假「思考过程」（老毛病）；低4「Message @」开头草稿可能误判（理论）。
  处理：#7 改为徽标「完成」+ 次行「后台仍在运行」、不算进行中，finished 到达后去掉次行（推翻上次「等待后台」取舍，关卡 C 向 leader 说明与验收原文的出入）；中断与撤回写 PTY 前先切回，切不回不发 Esc 改提示；五个写入口加接线测试。低1/低3/低4 记入关卡 C。退回车道。
- #6（中断/撤回未切回）#7（表现改）改回未通过，待车道修复后复验。
- 2026-11-11 车道修复：5738d729（#7 改为「已完成」+ 次行「后台仍在运行」，不算进行中，finished 到达去掉次行；busy 照减）、d571284d（中断/中断并处理/撤回写 PTY 前先切回，切不回不发 Esc 与 /rewind；ChatWindow.subagentWrites.test 8 例，中断/中断并处理/撤回 4 例修复前失败）、92828bb2（面板在且 main 行为「正在看」时作废占位证据，草稿「Message @」不再误判；面板不在时仍可能误判，后果仅多一次横幅，注释写明）。
  主会话复验：tsc/lint 0；vitest 全量连跑两次 184 文件 1680 例全过（车道首跑 4 例偶发、单跑与重跑均过）；clippy 0；cargo test --workspace 无失败；background_note 1 例过。#6 #7 通过（#7 与原文「仍为运行中」出入，关卡 C 说明）。#5 待新 CI。
- 2026-11-11 92828bb2 CI run 42979276318 全绿。#5 通过。PR #164 描述更新，发关卡 C-20261111-followups。
- 2026-11-11 关卡 C accept。#7「已完成 + 后台仍在运行」取舍由 leader 认可。PR #164 转正式待审，合并待 leader 同意。
