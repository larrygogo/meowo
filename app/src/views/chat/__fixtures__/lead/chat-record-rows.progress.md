phase: done

- 2026-11-09 定位根因：.chat-tool-name 宽度随文字；Message 先渲染 records 后 notifications；拍板卡 .chat-approval-copy 是限高 flex 列，子项可收缩致摘要被压扁
- 21:36 leader 追加 IMG_2094（拍板卡压扁）、IMG_2095（Stop hook 反馈变成用户气泡）。后者根因：同步拦截写 isMeta + 纯文本「Stop hook feedback:」，后端只拦 system-reminder/Skill 说明等，未拦它。追加验收 9、10
- 21:36 证据：bunx vitest run localCommand/LeadPanel/DockWork 44 通过（1–4 解析与渲染、7 文案）
- 21:45 证据 #8：bun run test 111 文件 1336 条通过；tsc 通过
- 21:47 证据 #9/#10：cargo test -p meowo-agent chat_parser 15 通过（含 chat_parser_wraps_meta_hook_feedback）；clippy --workspace --all-targets -D warnings 零警告（worktree 补了被忽略的 binaries/、dist-mobile/）
- 21:50 证据 #5：Playwright msedge 390x844，同一消息内各标签摘要 x：zh 全为 171，en 全为 206；回退新规则（--old）后 zh 143/171/143、en 200/78/177
- 21:50 实测新发现：通用 .chat-tool>summary 允许换行，长摘要掉第二行、箭头掉第三行；加 nowrap + flex:1 1 0% 修复
- 21:52 证据 #6：回退规则时摘要 45px/应有 191px、意见框 26/52px（复现 IMG_2094）；修后 191/52。另把卡片正文上限放到 70vh：收起态选项、意见框、提交全可见，展开态正文滚动
- 测试页与 vite 已收掉
- 22:05 做空完成：可交付 + 3 条问题，已修（提交 6e04cdb9）。挂关卡 C
- 关卡 C：leader accept，phase 改为 done
