/user 回查改用 Client::probe，不回报令牌好坏（不多写文件、不多发事件）。测试覆盖四种情况 + 合并 401 后最后一次回报仍是「被拒」那次。
8. 叫法与截图（be4765d0、0f020a70）：检查汇总按平台换词（GitLab「流水线 N 项作业失败」，与底部「流水线有失败」一致；Bitbucket 构建；Gitee 检查），GitHub 原句不变（逐字对照测试）。截图见下。
9. 真机核对自证（d04bd1fd）：检查状态改用测试里独立写死的对照表（GITLAB_EXPECT / GITEA_EXPECT / BITBUCKET_EXPECT / GITEE_RUN_EXPECT），表外的原始值直接失败。

### Gitee（gitee.com）
- 依据：官方 Swagger `https://gitee.com/api/v5/doc_json`（2026-11-18 取）+ 公开仓库 openharmony/docs 不带令牌只读实测。
  - 认证：文档写 access_token 查询参数；实测 `Authorization: token <令牌>` 被识别（不带头回「登录失效…40001」，带假令牌回「401 Unauthorized: Access token does not exist」），用请求头，令牌不进地址。权限 user_info、projects、pull_requests、notes；生成页 https://gitee.com/profile/personal_access_tokens。
  - 按分支：`pulls?state=open&head=<分支>`（head 只认分支名；state=all/merged 配 head 实测回 500）→ 退回最近 50 条按 head.ref + head 仓库筛。
  - 检查：`commits/{sha}/check-runs`（status/conclusion 同 GitHub CheckRun）；仓库没接就空列表、不显示检查。Gitee 没有 commit status 接口。
  - 合并：merge_method merge/squash/rebase，**没有头提交保护**：合并前再读一次核对 head.sha；拒绝按原话归类（冲突、审查、方式）。
  - 评论：comment_type pr_comment / diff_comment（path + new_line）；行内评论 position = 这一行在该文件 diff 里的序号（files 接口的 patch.diff 推算）。files 的 additions/deletions 是字符串。
  - mergeable 只是布尔不给原因 → UNKNOWN，不挡合并。
  - 只认 gitee.com；Gitee 企业版与私有部署本轮不做（添加账号时报 forge/gitee_com_only）。
- 测试：forge::tests::platforms 的 gitee_* 5 条（样例 forge/fixtures/platforms/gitee_*.json，openharmony/docs !95569 及列表、评论、文件、空 check-runs、/user 401）+ check_target / auth_for。
- 前端：账号添加多一块 Gitee（主机固定 gitee.com），平台名 Gitee，编号写 !号，叫法 Pull Request；链接按主机与 Gitea 区分。

### M7 只读真机核对（含 Gitee）
`cargo test -p meowo-app --lib live_只读_各平台 -- --ignored --nocapture`（原 live_只读_三个平台 改名；不带令牌、只 GET）。本轮结果：
- gitlab.com gitlab-org/cli !4471（草稿）：头提交 c56411df… 一致；流水线 3082670697 success，16 个 job 按独立对照表逐项一致（govulncheck=manual→skipped）。
- codeberg.org forgejo/forgejo #14947：头提交一致，2 项 status（skipped、success）一致。
- bitbucket.org atlassian/atlassian-connect-express #604：短 hash e0949d3373fd 一致，3 项 SUCCESSFUL 一致。
- gitee.com openharmony/docs !95569（fork liuyongkai2/docs_lisiteng，分支 0704）：按分支与按链接都是 !95569 open，头提交 5f61c05b… 与原始一致，原始 mergeable=false → UNKNOWN，check_runs=0 → 不显示检查。

### 验证
- `cargo clippy --workspace --all-targets -- -D warnings` 通过；`--lib forge` 133（含 forge_accounts 14、platforms 25）、github 39、worktree 50、remote 29、settings 71 全过（新文件与改动的测试文件只跑了 rustfmt）。
- `bunx tsc --noEmit -p .`、`bun run lint` 通过；`bunx vitest run src`：277 文件 4388 条中 ChatWindow.interject 1 条满载失败，单独重跑 22/22 过（未改 ChatWindow）。
- 截图（深浅主题）：C:\Users\user0\AppData\Local\Temp\claude\7d909b5a4c1b8b4f\0ebbc4dc-b6dc-4020-8903-bbb2780cd634\scratchpad\shots-forge——qa-panel-codeberg / -bitbucket / -gitee / -gitlab-wide（PR 区块 + 侧栏角标）、qa-title-gitlab / -codeberg / -bitbucket / -gitee（2560 宽，标题栏 PR 胶囊 + 侧栏角标 + 面板）、qa-panel-gitlab（「流水线 1 项作业失败」与「流水线有失败」）、qa-set-forge-*（含 add-gitee、列表里的 Gitee 账号）。说明：visual 的 `chat` 场景在本分支基线上本来就报 `t.chat.turnToolSummaries[kind] is not a function`（与本次改动无关），标题栏截图改用 2560 宽的 panel-changes。

### 仍然的风险
- 写操作（建 PR、合并、评论、行内评论、设备码、刷新令牌）仍只在假 HTTP 上测过；Gitee 与 Bitbucket 合并没有服务端头提交保护（核对与合并间有短窗口）；Gitee 合并被拒的原话官方没列，按关键词归类。
- Gitee 很老的已合并 / 已关闭 PR 可能找不到（head 只能配 state=open 用）。

## 2026-11-18 复核第二轮修复（feat/multi-forge，基于 f9585035，已 push 到 bfaab13f）
- A 8abcee2c：GitLab 续期只有 400/401 且 error=invalid_grant/invalid_client 才算被拒标重登；5xx、429、维护页、非 JSON 当暂时失败。回归测试 `续期遇到服务器暂时故障不标重登`（503/502/429/500/200 非 JSON/400 invalid_request 不标，invalid_grant/invalid_client 标）；修复前 503 得到 Rejected。
- B 1ed437be：Gitea 405「Please try again later」（services/pull/check.go 冲突/检查中/出错都回 ErrNotMergeableState）改为 `forge/not_mergeable: <PR 链接>`，中英文案「平台判定暂时不能合并（可能有冲突或仍在检查），请到网页查看」，面板给「在 <平台> 打开」。后端测试修复前得到 gh/checks_pending；前端测试修复前失败。
- C 3aa0ac9c：Gitee 只有实测会 500 的组合（带 head 且 state≠open）当空列表，其余 5xx 照常报错；回归测试覆盖按分支找、建 PR 查重（不发 POST）、行内评论不误报 invalid_line，修复前按分支找得到 Ok(None)。
- cec81e56：GitLab 大文件拿不到 diff（too_large/collapsed）时行内评论只带面板给的一侧；修复前同时带 old_line/new_line。
- bfaab13f：「更改」面板开着的 PR 改写「已打开」，与标题栏、侧栏 chat.prState 一致（新增一致性测试）；tests/builtin.rs 的 side=RIGHT 直传测试旁注明它不是回归测试。
- 验证：clippy -D warnings 通过；forge 135、github 39、worktree 50、remote 29、settings 71 全过；tsc、lint 通过；`bunx vitest run src/views src/i18n` 230 文件中 ChatWindow.interject 1 条满载失败，单独重跑 23/23 过（未改 ChatWindow）。

## 2026-11-18 取消 gh（D3）、gitlab.com 只用令牌（D4）、设置「代码托管」分区（D5）

- 提交：ce263812（后端：删本机 gh 执行器与全部调用路径、取消 GitLab 设备码 / OAuth 刷新、git_tool）、150cf045（前端：代码托管分区 Git 组 + 平台账号同列表、去掉 GitHub 四种方式与 GithubSection、gh 文案）。已 push origin/feat/multi-forge（PR #197）。
- 迁移：旧 github_connection 四种值一样处理（字段不再读写）：有 Meowo GitHub 登录（github.json + 凭据库令牌）就是 github.com 账号，否则视为未添加；旧清单里 oauth=true 的 GitLab 账号读到时标 needsRelogin、删访问令牌与 #refresh 两条凭据，只迁一次。
- 验证：cargo clippy -p meowo-app --all-targets 无警告；cargo test -p meowo-app --lib 962 passed / 0 failed；cargo test -p meowo-protocol 200 passed；tsc 通过；lint 通过；vitest 278 文件 4403 passed。
- 截图（深浅、中英）：C:\Users\user0\AppData\Local\Temp\claude\7d909b5a4c1b8b4f\0ebbc4dc-b6dc-4020-8903-bbb2780cd634\scratchpad\shots-forge\nogh\

## 2026-11-18 复核修正（取消 gh 之后）

- 提交 504db0f3，已 push origin/feat/multi-forge。
- 设备码行按钮换到说明下方、可换行；g7 与 drill 恢复为只靠令牌（#[ignore]，未运行）；git 路径按 std resolve_exe 只认 git.exe、先程序目录与系统目录；remote 注释统一；注释中 gh 残留清理。
- 验证：clippy 无警告；cargo test -p meowo-app --lib 963 passed / 0 failed / 23 ignored；meowo-protocol 200 passed；tsc、lint 通过；vitest 4400 passed，3 条 ChatWindow 用例全量下偶发失败、单独重跑 65/65 通过。
- 截图：C:\Users\user0\AppData\Local\Temp\claude\7d909b5a4c1b8b4f\0ebbc4dc-b6dc-4020-8903-bbb2780cd634\scratchpad\shots-forge\nogh2\

## 2026-11-18 验证（lead）
- 独立复核三轮：问题均已修（1315887c）；M1–M5、M7、M9 写回证据。
- M6：#197 CI audit / frontend / macOS 通过；Windows 失败于 detect::evaluating_a_full_screen_stays_cheap（计时 1.13ms > 1ms，本 PR 未改 detect.rs，本机连跑通过，#198 同样失败），已重跑。
- M8：本地集成安装包（main + #197 + #198，1cd790e8…b5d9）待 leader 添加非 GitHub 账号试用。
- M6 通过：#197 CI 全部通过（Windows detect 计时测试重跑后通过）。agent 验收项全部通过。
- 发布 C 关卡 C-20261118-multi-forge-1（publication-1），冻结 9 项，待 leader 在本地集成安装包上验 M8。

- leader 接受 C-20261118-multi-forge-1（accept），任务完成。待 leader 决定合并 #197、#198。
