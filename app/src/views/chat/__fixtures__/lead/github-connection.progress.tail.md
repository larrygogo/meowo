pi graphql -f query='{repository(owner:"cli",name:"cli"){pullRequests(last:1){nodes{number mergeStateStatus}}}}'`，默认、`-H "Accept: application/vnd.github.merge-info-preview+json"`、`-H "Accept: application/json"` 三种都返回 `{"number":15089,"mergeStateStatus":"BLOCKED"}`。结论：github.com 现在不需要该预览头；仍按 gh 的做法在 /graphql 请求带上（merge-info-preview + vnd.github+json），防老账号或企业设置收紧。
5. 401 归属（2801f1c4）：内置实现每个回包报一次令牌好坏，auth_result_with 只在所用令牌等于当前令牌时改「要重新登录」，之后 2xx 清掉；内存缓存避免每次读文件。测试 1 条覆盖旧令牌 401、当前令牌 401、重复 401、成功后清除。
6. 自动按主机（2801f1c4）：Executor::Auto(token)——github.com（链接主机 / --repo / api / 目录远端为 github.com）走内置实现，其它主机走本机 gh。测试：dispatch_gh 四种方式 + 自动分流，targets_github_com 各种参数与目录（真实 git 沙箱，github.com / github.example.com / 非仓库）。
7. macOS（8f7033e1）：写钥匙串改为 `security -i` 从标准输入传命令，令牌不进进程参数；-i 退出码不反映单条命令，写后读回核对。写入行是纯函数，各平台单测。macOS 分支仍依赖 CI。
8. 并发（2801f1c4）：begin_login_with 先占代次再要代码，期间被取消/再次登录即返回 github/cancelled（前端不当错误）；set_mode_with 用 MODE_WRITE 串行写设置与内存，写失败不动内存。测试各 1 条。
9. 补测：finish_login 取消路径、connected_gh 分派（dispatch_gh）、forge 缓存作废（invalidate_connection 清 auth/too_old/methods/prs/queued）、logout_with 删凭据与记录并回调、set_mode_with 回调。

验证（修复后）
- cargo clippy --workspace --all-targets -- -D warnings：通过。
- cargo test -p meowo-app --lib forge：81 通过；github/remote/settings/worktree/credential：182 通过。
- bunx tsc --noEmit -p .、bun run lint：通过。bunx vitest run src/views src/i18n：一次全过（3978），另一次 ChatWindow.adapterModel「模型保存需要 CLI 新确认行」在满载下超时失败，单独重跑 25/25 通过，与本改动无关（计时敏感）。
- 未推送；macOS 编译待协调者推分支跑 CI。
- G6：CI run 44242518730（8f7033e1）全部 success，含 macOS 与 E2E。G7 只读真机核对交给实现 agent（用 gh 的令牌只读对比，不写操作）。

## 2026-11-18 G7 只读真机核对（commit 41b46f52）
方法：forge/tests/builtin.rs 新增默认 #[ignore] 的 live_对照_gh。运行：
`$env:GH_TOKEN = (gh auth token); cargo test -p meowo-app --lib live_对照_gh -- --ignored --nocapture; Remove-Item Env:GH_TOKEN`
令牌只在环境变量里，不落文件、不打印（测试断言报告里没有令牌）。内置执行器走真实 HTTP（RealHttp，含重定向处理），与本机 gh 2.96 对同一条命令各跑一次，解析成 JSON 逐字段比较（含数组条数、字段有无、值）。只发只读请求，没有建 PR、合并、评论，没有发起设备码登录。本机到 GitHub 的 TLS 握手常超时，两边各最多重试 6 次。

结果（修复 per_page 后的最终一轮）：
- `pr view 192 --repo larrygogo/meowo --json <PR_FIELDS>`：差异 0；检查 5 项；forge 的 parse_pr_full 结果相同。
- 同上加 `,comments,reviews`：差异 0。
- `pr view fix/scm-switch-flash --repo larrygogo/meowo --json number,state`：差异 0。
- `pr view meowo/no-such-branch-for-g7 ...`：两边都退出码 1，文案都是 `no pull requests found for branch "meowo/no-such-branch-for-g7"`。
- `repo view larrygogo/meowo --json mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed`：差异 0。
- `api user`：差异 0。`auth status --hostname github.com`：两边都成功。
- `pr view 15086 --repo cli/cli --json <PR_FIELDS>`（cli/cli 最近合并的 PR）：差异 0；检查 28 项；forge 解析相同。
- `api repos/larrygogo/meowo/pulls/192/files --paginate --slurp`：
  - 第一轮发现分页边界不同：gh 每页 100 条（34 个文件 1 页），内置 30 条（2 页）。已修：分页时没写 per_page 就补 per_page=100（41b46f52，单测钉住），复测条数一致。
  - 余 3 处 patch 文本差异，不修，无害：源码里有字面的 `\u0000`（例如 app/src/views/chat/SourceControl.tsx:240 ``key={`open\u0000${key}\u0000${open.kind}`}``）。内置给出的就是源码原样；gh 输出成 `\^@`（按我的理解是 gh 为终端显示做的控制字符净化，误把转义反斜杠后的 u0000 当成 NUL 转成脱字符写法）。我们的数据更准确，review.rs 显示 diff 用的正是这段文本。

## 2026-11-18 安装版 TLS 偶发失败修复 + G7 真机写路径验证
提交：
7e76875b G7 写路径驱动：写操作只发一次，传输失败先读回确认再决定是否重发
65e02afc 内置 gh 支持 pr ready，并留一个默认跳过的真机写路径驱动（G7）
0c1d4507 登录与读取遇到偶发 TLS 失败时自动重试，不再第一步就报连不上 GitHub

### 登录第一步偶发 TLS 失败（0c1d4507）
leader 安装版经 TUN 代理，点「登录 GitHub」报 `https://github.com/login/device/code: Connection Failed: tls connection init failed: unexpected end of file`。本机同样常见（本节 git push 前 4 次都是 `schannel: failed to receive handshake`，第 5 次成功）。
修法：RealHttp 每一跳对传输层失败（还没收到任何回包）最多再试 3 次，间隔 0.5s / 1s / 2s，全部失败才报，错误带最后一次原因并注明 `retried 3 times`。只重试可安全重发的：GET/HEAD、GraphQL 查询、POST login/device/code、POST login/oauth/access_token；REST 写（建 PR、合并、评论、评审）与 GraphQL mutation（入队、转可评审）只发一次。收到任何回包（含 5xx）不重试；不许去的主机直接拒、不重试。测试：四类安全请求失败两次后成功（睡眠 0.5s、1s）、全部失败共 4 次调用且退避 0.5/1/2s、设备码申请的错误提示保留原因、四类写操作只调一次、收到 502 不重试。

### G7 写路径（leader 授权，仓库 larrygogo/meowo，只动临时分支）
- 开始前核对：`git ls-remote origin 'refs/heads/meowo-g7-*'` 为空，`gh pr list --search "Meowo G7" --state all` 为空。main = a256dd6a0f43f65cb8138f932e31bf8c70b26628。
- 临时分支：用底层命令（临时 index 在 scratchpad，不切换工作区、不建本地分支）从 origin/main 建 `meowo-g7-base-20261118135026`（= main）与 `meowo-g7-head-20261118135026`（7a74c268，只加一个一行文字的 meowo-g7-20261118135026.txt），推到远端。
- 驱动：`live_写路径_g7`（#[ignore]，65e02afc 加入；7e76875b 改为写步骤只发一次）。内置执行器真实 HTTP，令牌来自 `GH_TOKEN=(gh auth token)`，只在环境变量，未落盘、未打印。
- 第一次运行：驱动的读重试把 pr create 也包进去了，全程网络连不上，失败退出；按 head 分支核对没有建出 PR。随后改为写步骤只发一次、传输失败先读回确认（7e76875b）。
- 第二次运行结果（与 gh 输出形状对照）：
  1. `pr create --draft --base <base> --head <head> --repo larrygogo/meowo`：第 1 次在 TLS 初始化时失败（未发出请求，读回确认没有 PR），第 2 次成功，stdout 为 `https://github.com/larrygogo/meowo/pull/193`（gh 同样是只打一行 PR 链接）。只建出 #193 一个 PR。
  2. 读回草稿：内置 `pr view 193 --json <PR_FIELDS>` 正常解析，state=draft、headRefOid=7a74c268；这一次 gh 自己的读取没拿到数据（网络，gh 输出不是 JSON），未能逐字段对照，最终状态的对照见第 7 步。
  3. 再建一次：退出码 1，`a pull request for branch "…head…" into branch "…base…" already exists:` 下一行是 #193 的链接，与 gh 的写法一致；forge 抠出的链接 = #193。
  4. `pr comment 193 --body …`：stdout 为评论链接 `…/pull/193#issuecomment-6970474579`（gh 同样打评论链接）；内置与 gh 读回 comments 都看得到这条评论。
  5. `pr ready 193`：`✓ Pull request #193 is marked as "ready for review"`（gh 文案同形）。
  6. 头提交不符的合并（`--match-head-commit 0000…`）：退出码 1，`Head branch was modified. Review and try the merge again. (HTTP 409)`，forge 归类 `gh/head_changed`。
  7. `pr merge 193 --squash --match-head-commit 7a74c268 --repo larrygogo/meowo`：`✓ Merged pull request #193`。读回 `pr view 193 --json <PR_FIELDS>` 与 gh 逐字段差异 0 处。gh 核对：state=MERGED、mergedAt=2026-11-18T05:58:01Z、base=meowo-g7-base-20261118135026。
- 收尾：两个远端分支已删（`git push origin --delete`，第 2 次成功）；`git ls-remote` 只剩 main，main 仍为 a256dd6a（未变）；本地没有 meowo-g7-* 分支。PR #193 留在仓库里，状态为 MERGED，合并目标是已删除的临时分支，没有碰 main。
- leader accept C（G8）。本机无 github.json/凭据条目，G7 登录证据仍缺，标 blocked 等 leader 登录一次。
- D 关卡 approve 已 consume；本机仍无登录记录。phase 改 blocked：G7 只差 leader 在 #194 安装包里实际完成一次 Meowo 登录（或给出失败报错），其余全部通过。
- G7：leader 在集成包里用正式 Client ID 完成 Meowo 登录（截图 + github.json：larrygogo、needsRelogin=false），全部 8 项通过，phase=done。
