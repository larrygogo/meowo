/// 后端错误消息的展示层映射。Rust 侧的 Err(String) 是中文 sentinel(全量 i18n 需要错误码
/// 重构,工程量与收益不成比——务实路线:高频可见错误在这里做双语与口语化,未命中的原样
/// 显示;英文界面下含汉字的未命中项降级为 "Operation failed: {raw}",不让中文直漏)。
/// 注意:这里的 match 串是与后端约定的 sentinel,改 Rust 文案必须同步改这里。

type ErrorEntry = {
  /** 匹配串:exact 为空时按前缀匹配(带参数的错误,如「同步会话失败：{e}」)。 */
  m: string;
  /** 精确匹配(默认按前缀)。 */
  exact?: boolean;
  /** 子串匹配:后端把 agent id 拼在句首(「claude 的中转地址…」)时前缀够不着,按包含匹配。 */
  anywhere?: boolean;
  /** 非精确匹配时把匹配串之后的原文(通常是「：具体原因」)追加到译文后,不丢排障信息。 */
  tail?: boolean;
  /** 缺省 = 原文照放(原文本就是中文,且句首常带 agent id 等有用前缀,替换反而丢信息)。 */
  zh?: string;
  en: string;
};

/** 按声明顺序匹配:精确项放前、长前缀放短前缀前。导出仅供对账测试(errors.parity.test.ts)。 */
export const ENTRIES: ErrorEntry[] = [
  { m: "conversation/previous_process_unverified", exact: true, zh: "尚未确认上次会话进程已退出，已暂停恢复", en: "The previous session process has not been confirmed stopped. Recovery is paused." },
  { m: "conversation/exit_unverified", exact: true, zh: "尚未确认会话的全部进程已退出，请重启 Meowo 后再恢复", en: "Some session processes have not been confirmed stopped. Restart Meowo before resuming." },
  { m: "conversation/adoption_unverified", exact: true, zh: "无法确认原生会话已交出控制权，请先在原终端结束会话", en: "Control of the native session could not be verified. End it in its original terminal first." },
  { m: "conversation/auth_required", exact: true, zh: "原生会话尚未完成认证，请检查当前账号的 CLI 登录状态", en: "The native session is not authenticated. Check the CLI sign-in status for this account." },
  { m: "conversation/native_auth_failed", exact: true, zh: "原生 CLI 拒绝了认证，请检查当前账号或 API Key", en: "The native CLI rejected authentication. Check this account or API key." },
  { m: "conversation/native_rate_limited", exact: true, zh: "原生 Provider 已限流或配额不足，请稍后重试", en: "The native provider is rate-limited or out of quota. Try again later." },
  { m: "conversation/native_busy", exact: true, zh: "服务繁忙，请稍后重试或换个模型", en: "The service is busy. Try again later or switch models." },
  { m: "conversation/native_network_failed", exact: true, zh: "原生 CLI 的网络请求失败，请检查网络或代理", en: "The native CLI network request failed. Check the network or proxy." },
  { m: "conversation/native_context_limit", exact: true, zh: "原生会话已达到上下文限制，请新建或压缩会话", en: "The native session reached its context limit. Start or compact a session." },
  { m: "conversation/client_persistence_failed", exact: true, zh: "无法保存发送回执，请允许本地存储后重试", en: "Could not save the delivery receipt. Allow local storage and try again." },
  { m: "conversation/gate_task_cancelled", exact: true, zh: "任务已结束，本次关卡投递已取消", en: "The task has ended. This gate delivery was cancelled." },
  { m: "conversation/gate_owner_changed", exact: true, zh: "关卡归属已变，本次投递已取消", en: "Gate ownership changed. This delivery was cancelled." },
  { m: "conversation/gate_changed", exact: true, zh: "关卡已更新，本次旧回答未发送", en: "The gate changed. The previous answer was not sent." },
  { m: "conversation/gate_answer_changed", exact: true, zh: "关卡回答已变，本次旧回答未发送", en: "The gate answer changed. The previous answer was not sent." },
  { m: "conversation/gate_read_failed", exact: true, zh: "无法确认当前关卡，回答已保留且未发送", en: "The current gate could not be verified. The answer was preserved and not sent." },
  { m: "conversation/lease_failed", exact: true, zh: "无法建立私有会话监管记录，会话未启动", en: "Could not create the private supervision record. The session was not started." },
  { m: "conversation/lease_missing", exact: true, zh: "会话监管记录不完整，已暂停恢复以保留原会话", en: "The supervision record is incomplete. Recovery is paused to preserve the original session." },
  { m: "conversation/supervisor_failed", exact: true, zh: "会话监管程序异常，请检查投递记录后重启 Meowo", en: "The session supervisor failed. Check delivery records and restart Meowo." },
  { m: "conversation/platform_supervision_unsupported", exact: true, zh: "此平台暂不支持托管会话，请使用原生 CLI 入口", en: "Managed conversations are not supported on this platform yet. Use the native CLI entry." },
  { m: "conversation/reporter_missing", exact: true, zh: "找不到会话监管程序，请重新安装 Meowo", en: "The session supervisor is missing. Reinstall Meowo." },
  { m: "conversation/interaction_stale", exact: true, zh: "审批请求已经失效，请查看当前会话中的请求", en: "This approval request expired. Check the current request in the conversation." },
  { m: "conversation/runtime_conflict", exact: true, zh: "此会话已由另一个运行实例控制", en: "Another running instance controls this conversation." },
  { m: "conversation/unsupported", exact: true, zh: "此 CLI 版本暂不支持托管会话", en: "This CLI version does not support managed conversations yet." },
  { m: "conversation/cli_missing", exact: true, zh: "未找到 CLI，请先安装并完成登录", en: "CLI not found. Install it and sign in first." },
  { m: "conversation/profile_missing", exact: true, zh: "此会话使用的账号配置已不存在", en: "The account profile used by this conversation no longer exists." },
  { m: "conversation/ready_timeout", exact: true, zh: "原生会话启动超时，消息仍保存在投递列表中", en: "The native session timed out during startup. Messages remain in the delivery list." },
  { m: "conversation/process_exited", exact: true, zh: "原生会话进程已退出，请检查投递状态后恢复", en: "The native session process exited. Check delivery status before resuming." },
  { m: "conversation/stopped_during_delivery", exact: true, zh: "会话在送达过程中停止，请核对消息是否已收到", en: "The session stopped during delivery. Check whether the message was received." },
  { m: "conversation/command_queue_full", exact: true, zh: "会话请求过多，请稍后重试", en: "The session request queue is full. Try again shortly." },
  { m: "conversation/", tail: true, zh: "托管会话操作失败：", en: "Managed conversation operation failed: " },
  { m: "lead/gate_ambiguous", exact: true, zh: "多个任务使用了相同的关卡编号，已停止送达，请先修正编号", en: "Multiple tasks share this gate ID. Delivery stopped until the IDs are corrected." },
  { m: "lead/delivery_chain_limit", exact: true, zh: "这条回答已多次人工重试，请在会话中核对投递记录", en: "This answer has been retried many times. Review its delivery records in the conversation." },
  { m: "lead/session_paused", exact: true, zh: "回答已保存；会话已暂停，点击继续送达可恢复", en: "Answer saved. The session is paused; continue delivery to resume." },
  { m: "lead/delivery_uncertain", exact: true, zh: "回答已保存，送达结果不确定；请在会话中核对后处理", en: "Answer saved, but delivery is uncertain. Check the conversation before taking action." },
  { m: "lead/delivery_failed", exact: true, zh: "回答已保存，本次投递未完成；请在会话中查看详情并处理", en: "Answer saved, but delivery did not complete. Check delivery details in the conversation." },
  { m: "lead/owner_not_managed", exact: true, zh: "此关卡归属原生终端会话，请从原终端继续", en: "This gate belongs to a native terminal session. Continue from its original terminal." },
  { m: "lead/owner_mismatch", exact: true, zh: "关卡归属与会话不一致，已停止送达", en: "Gate ownership does not match the conversation. Delivery stopped." },
  { m: "lead/owner_invalid", exact: true, zh: "关卡归属记录无效，回答已保留", en: "The gate ownership record is invalid. Your answer was preserved." },
  { m: "lead/gate_not_answered", exact: true, zh: "当前关卡还没有可送达的回答", en: "This gate has no answer ready for delivery." },
  // ── 远程桥(手机端) ──
  // 7M-7：远程传输层的错误此前是硬编码中文串、直接上屏，既不进这张表也不双语。
  // 改成 `remote/<code>` 结构化 reason（与后端「模块/代码」同规），文案落回这里。
  // 不说「正在重试」：轮询会自己重试，但发消息等一次性请求超时就是失败了，那样说是误导。
  { m: "remote/timeout", exact: true, zh: "连不上桌面端，请求超时", en: "Couldn't reach the desktop app. The request timed out." },
  // 远程桥 git 只读命令的收口（remote.rs ensure_session_dir / 改动清单校验）。
  { m: "remote/not_session_dir", exact: true, zh: "只能查看会话目录里的改动", en: "Only changes in the session folder can be viewed" },
  { m: "remote/not_changed_file", exact: true, zh: "这个文件不在改动列表里", en: "This file is not in the change list" },
  { m: "remote/unauthorized", exact: true, zh: "配对已失效，请重新扫码", en: "Pairing expired; scan the code again" },
  { m: "remote/not_json", exact: true, zh: "响应异常，网络可能被中间设备拦截", en: "Unexpected response; the network may be intercepting requests" },
  // tail：状态码与命令名是排障的全部线索，译文吞掉它等于比改前更难查（复核指出）。
  { m: "remote/bad_payload", tail: true, zh: "远程请求参数不合法", en: "Invalid request payload for the remote bridge" },
  // 桥自身（remote.rs）产生的 reason，与上面前端侧的同一命名空间。
  { m: "remote/unknown_command", exact: true, zh: "这个操作在手机端不可用", en: "This action isn't available from the phone" },
  { m: "remote/not_ready", exact: true, zh: "远程服务还没就绪，请稍后重试", en: "The remote service isn't ready yet; try again shortly" },
  { m: "remote/bad_args", tail: true, zh: "远程请求参数不合法", en: "Invalid arguments for the remote request" },
  { m: "remote/http_", tail: true, zh: "远程请求失败", en: "Remote request failed" },
  // 远程服务启动时绑定端口失败（remote.rs serve）。占用多半是另一个 Meowo 实例开着远程。
  { m: "remote/port_in_use", exact: true, zh: "端口被其它程序占用，换个端口或关掉占用它的程序", en: "The port is in use; pick another or close the program using it" },
  // 「更改」面板的 git 写操作（git.rs git_stage / git_unstage / git_discard / git_commit）。
  { m: "git/locked", exact: true, zh: "另一个 git 操作正在进行（可能是 agent 在跑），稍后再试", en: "Another git operation is in progress (maybe the agent). Try again shortly." },
  { m: "git/identity", exact: true, zh: "还没配置 git 用户名和邮箱，先在终端里运行 git config 设置", en: "Git user name and email aren't set. Run git config in a terminal first." },
  { m: "git/conflict", exact: true, zh: "有未解决的合并冲突，先在终端或编辑器里解决再提交", en: "There are unresolved merge conflicts. Resolve them in a terminal or editor first." },
  { m: "git/nothing_to_commit", exact: true, zh: "没有可提交的更改", en: "Nothing to commit" },
  { m: "git/empty_message", exact: true, zh: "请填写提交信息", en: "Enter a commit message" },
  { m: "git/not_in_changes", exact: true, zh: "文件已不在更改列表里，刷新后再试", en: "The file is no longer in the change list. Refresh and try again." },
  // 放弃更改时未跟踪文件进回收站失败（git.rs trash_to_recycle_bin）。文件原样保留、不会永久删。
  { m: "git/trash_failed", tail: true, zh: "没能移到回收站，文件已保留", en: "Couldn't move the file to the trash. It was kept." },
  // 整目录放弃（折叠的未跟踪目录，git.rs untracked_dir_to_trash）：里面有链接或嵌套仓库就不动。
  { m: "git/dir_has_links", exact: true, zh: "目录里有链接，没有放弃", en: "The folder contains links, so nothing was discarded." },
  { m: "git/dir_has_repo", exact: true, zh: "目录里有另一个仓库，没有放弃", en: "The folder contains another repository, so nothing was discarded." },
  { m: "git/failed", tail: true, zh: "git 操作失败", en: "Git operation failed" },
  // 推送 / 开 PR / 合并（forge.rs）。原因码之后的原文（钩子输出、服务器原话）带上，排障要用。
  { m: "git/no_remote", exact: true, zh: "没有远端仓库，推不了", en: "No remote to push to" },
  { m: "git/detached", exact: true, zh: "不在分支上，推不了", en: "Not on a branch, so there's nothing to push" },
  { m: "git/push_auth", exact: true, zh: "推送认证失败，检查 git 凭据", en: "Push failed to sign in. Check your git credentials." },
  { m: "git/push_rejected", exact: true, zh: "远端有新提交，先拉取再推", en: "The remote has new commits. Pull first." },
  { m: "git/push_protected", tail: true, zh: "分支受保护，推送被拒", en: "The branch is protected. Push rejected." },
  { m: "git/push_hook", tail: true, zh: "推送钩子没通过", en: "A pre-push hook failed" },
  { m: "git/push_timeout", exact: true, zh: "推送超时", en: "Push timed out" },
  { m: "git/push_network", exact: true, zh: "连不上远端，检查网络", en: "Can't reach the remote. Check the network." },
  { m: "git/push_failed", tail: true, zh: "推送失败", en: "Push failed" },
  { m: "git/job_not_found", exact: true, zh: "推送结果过期了，刷新看看", en: "The push result expired. Refresh to check." },
  { m: "gh/not_installed", exact: true, zh: "没装 gh", en: "gh isn't installed" },
  { m: "gh/not_authed", exact: true, zh: "gh 没登录", en: "gh isn't signed in" },
  { m: "gh/too_old", exact: true, zh: "gh 版本太旧，升级后再合并", en: "gh is too old. Update it to merge." },
  { m: "gh/checks_failed", exact: true, zh: "CI 有失败，不能合并", en: "CI failed. Can't merge." },
  { m: "gh/checks_pending", exact: true, zh: "检查还没完成，稍后再试", en: "Checks aren't done yet. Try again shortly." },
  { m: "gh/merge_conflict", zh: "有冲突，去网页处理", en: "There are conflicts. Resolve them on GitHub." },
  { m: "gh/head_changed", exact: true, zh: "PR 有新提交，看过再合并", en: "The PR has new commits. Review them first." },
  { m: "gh/method_not_allowed", exact: true, zh: "仓库不允许这种合并方式", en: "This repository doesn't allow that merge method" },
  { m: "gh/review_required", exact: true, zh: "还需要审核通过才能合并", en: "An approving review is required first" },
  { m: "gh/merge_blocked", tail: true, zh: "GitHub 拒绝了合并", en: "GitHub blocked the merge" },
  { m: "gh/pr_exists", zh: "这个分支已经有 PR 了", en: "This branch already has a PR" },
  { m: "gh/ambiguous_repo", exact: true, zh: "有多个 GitHub 远端，先选仓库", en: "Several GitHub remotes. Choose a repository first." },
  { m: "gh/failed", tail: true, zh: "gh 操作失败", en: "gh failed" },
  { m: "remote/bind_failed", tail: true, zh: "无法监听这个端口", en: "Couldn't listen on this port" },
  // ── 诊断日志（diaglog.rs）──
  { m: "diag/bad_extension", exact: true, zh: "只能导出成 .txt 或 .log 文件", en: "Export only to .txt or .log files" },
  { m: "diag/no_log", exact: true, zh: "还没有诊断日志，开发版不写日志", en: "No diagnostic log yet. Dev builds don't write one." },
  { m: "diag/write_failed", tail: true, zh: "日志没能导出", en: "Couldn't export the log" },
  // ── 导出会话（fsutil.rs export_text_file）──
  { m: "export/bad_extension", exact: true, zh: "只能导出成 .md 或 .txt 文件", en: "Export only to .md or .txt files" },
  { m: "export/write_failed", tail: true, zh: "文件没能写入", en: "Couldn't write the file" },
  // ── 用编辑器打开目录（editors.rs）──
  { m: "editor/dir_missing", exact: true, zh: "项目目录不存在", en: "Project directory not found" },
  { m: "editor/not_found", exact: true, zh: "没找到这个编辑器，可能已卸载", en: "Editor not found; it may have been uninstalled" },
  // 对话窗右侧面板（browser_pane.rs / shell_terminal.rs）。
  { m: "browser_pane/blocked_url", exact: true, zh: "面板浏览器只能打开 http/https 网页", en: "The panel browser only opens http/https pages" },
  { m: "browser_pane/bad_url", exact: true, zh: "网址格式不对", en: "That doesn't look like a valid address" },
  { m: "browser_pane/too_many", exact: true, zh: "浏览器页签开太多了（最多 16 个），先关掉一些", en: "Too many browser tabs (16 max). Close some first." },
  { m: "browser_pane/download_missing", exact: true, zh: "文件已经不在了，可能被移动或删除", en: "The file is gone. It may have been moved or deleted." },
  { m: "browser_pane/not_found", exact: true, zh: "这个浏览器页签已经关掉了", en: "This browser tab has been closed" },
  { m: "shell_terminal/not_found", exact: true, zh: "这个终端已经关掉了", en: "This terminal has been closed" },
  { m: "shell_terminal/exited", exact: true, zh: "这个终端里的程序已经退出了", en: "The program in this terminal has exited" },
  { m: "editor/spawn_failed", tail: true, zh: "编辑器没能打开", en: "Couldn't open the editor" },
  // ── 已合并 worktree 的清理（worktree.rs）。判定原因码前端另有展示文案，这里兜删除时的失败 ──
  { m: "worktree/busy", exact: true, zh: "目录还被占用（可能有终端停在里面），什么都没删", en: "The folder is still in use (maybe a terminal is open in it). Nothing was deleted." },
  { m: "worktree/no_cwd", exact: true, zh: "这个会话没有记录工作目录", en: "This session has no working folder on record" },
  { m: "worktree/not_repo", exact: true, zh: "这个目录不在 git 仓库里", en: "This folder isn't in a git repository" },
  { m: "worktree/not_worktree", exact: true, zh: "这是仓库主目录，不是 worktree", en: "This is the main checkout, not a worktree" },
  { m: "worktree/dir_missing", exact: true, zh: "目录已经不在了", en: "The folder is already gone" },
  { m: "worktree/gh_unavailable", exact: true, zh: "没装 gh 或没登录，判断不了 PR 有没有合并", en: "gh isn't installed or signed in, so the PR status is unknown" },
  { m: "worktree/no_pr", exact: true, zh: "这个分支没有 PR", en: "This branch has no PR" },
  { m: "worktree/not_merged_local", exact: true, zh: "没装 gh，本地看分支还没合进主分支", en: "Without gh, git shows this branch isn't in the main branch yet" },
  { m: "worktree/pr_open", exact: true, zh: "PR 还没合并", en: "The PR isn't merged yet" },
  { m: "worktree/pr_closed", exact: true, zh: "PR 已关闭，没有合并", en: "The PR was closed without merging" },
  { m: "worktree/dirty", exact: true, zh: "还有文件改动没提交", en: "There are uncommitted changes" },
  { m: "worktree/untracked", exact: true, zh: "还有新文件没提交", en: "There are new files not committed" },
  { m: "worktree/nested_repo", exact: true, zh: "目录里还有别的仓库或 worktree", en: "There are other repos or worktrees inside this folder" },
  { m: "worktree/hidden_changes", exact: true, zh: "有文件被标记成不跟踪改动，判断不了有没有改过", en: "Some files are marked to hide changes, so changes can't be checked" },
  { m: "worktree/unpushed", exact: true, zh: "有提交还没推到远端", en: "Some commits haven't been pushed" },
  { m: "worktree/submodule_dirty", exact: true, zh: "子模块里有改动", en: "A submodule has changes" },
  { m: "worktree/submodule_unpushed", exact: true, zh: "子模块里有提交还没推", en: "A submodule has commits that haven't been pushed" },
  { m: "worktree/shared_session", exact: true, zh: "还有进行中的会话在用这个目录", en: "An open session is using this folder" },
  { m: "worktree/detached", exact: true, zh: "当前不在任何分支上", en: "Not on a branch" },
  { m: "worktree/locked", exact: true, zh: "这个 worktree 被锁定了", en: "This worktree is locked" },
  { m: "worktree/target_changed", exact: true, zh: "目录和确认时看到的不一样了，没有删除，请重新检查", en: "The folder no longer matches what you confirmed. Nothing was deleted; check again." },
  { m: "worktree/partial_removed", tail: true, zh: "删到一半失败：目录里部分文件已经删掉，本地分支保留", en: "Deletion failed partway: some files in the folder were deleted; the local branch was kept" },
  { m: "worktree/still_exists", exact: true, zh: "删完后目录还在，请手动检查", en: "The folder is still there after deleting; check it manually" },
  { m: "worktree/unlink_failed", tail: true, zh: "拆除目录里的链接失败", en: "Couldn't remove links inside the folder" },
  { m: "worktree/remove_failed", tail: true, zh: "删除目录失败（目录里的链接已拆掉，链接指向的内容没动）", en: "Couldn't delete the folder (links inside it were removed; what they pointed to is untouched)" },
  { m: "worktree/git_failed", tail: true, zh: "git 没能运行", en: "Couldn't run git" },
  { m: "worktree/probe_restore", tail: true, zh: "检查占用时改名后没能改回，请手动检查目录", en: "The folder was renamed during the check and couldn't be restored; check it manually" },
  // ── 在新 worktree 里开会话（wtnew）。撞名时后缀是建议名，新建页会单独给「改用」按钮 ──
  { m: "worktree/dir_exists", tail: true, zh: "目录已存在，可改用", en: "The folder already exists. Try" },
  { m: "worktree/branch_exists", tail: true, zh: "分支已存在，可改用", en: "The branch already exists. Try" },
  { m: "worktree/branch_checked_out", tail: true, zh: "这个分支已在别处检出", en: "This branch is checked out in another folder" },
  { m: "worktree/branch_invalid", exact: true, zh: "分支名不合法", en: "Invalid branch name" },
  { m: "worktree/add_failed", tail: true, zh: "worktree 没建成", en: "Couldn't create the worktree" },
  { m: "worktree/trust_stale", exact: true, zh: "配置刚被改过，请重新查看", en: "The config just changed; review it again" },
  { m: "worktree/no_task", exact: true, zh: "找不到这次 setup，可能 Meowo 重启过", en: "This setup run is gone; Meowo may have restarted" },
  { m: "worktree/bad_action", exact: true, zh: "不支持的操作", en: "Unsupported action" },
  { m: "worktree/setup_shell_unsupported", exact: true, zh: "需要 PowerShell 7 或 Git Bash", en: "Needs PowerShell 7 or Git Bash" },
  // ── 手机推送（push.rs）──
  { m: "push/invalid_url", exact: true, zh: "推送地址无效，请从 Bark App 里复制", en: "Invalid push URL; copy it from the Bark app" },
  { m: "push/missing_key", exact: true, zh: "推送地址缺少设备 key", en: "The push URL is missing the device key" },
  { m: "push/http_", tail: true, zh: "推送服务器拒绝了请求", en: "The push server rejected the request" },
  { m: "push/send_failed", tail: true, zh: "推送没发出去，检查网络或代理", en: "Push failed; check the network or proxy" },
  // ── 会话 / 终端 ──
  { m: "无效 session_id", exact: true, zh: "会话已失效，请重新打开", en: "Session is no longer valid; reopen it" },
  { m: "会话不存在", exact: true, zh: "会话不存在", en: "Session not found" },
  { m: "目录不存在", exact: true, zh: "项目目录不存在", en: "Project directory not found" },
  // 远程新建会话的页内目录浏览(list_subdirectories):手机端可见,英文界面下需双语。
  { m: "目录不存在或不可读", exact: true, zh: "目录不存在或不可读", en: "Folder doesn't exist or isn't readable" },
  { m: "不是目录", exact: true, zh: "这不是一个目录", en: "Not a folder" },
  { m: "目录不可读", exact: true, zh: "目录不可读", en: "Folder isn't readable" },
  { m: "未知 agent", exact: true, zh: "未知的 Agent", en: "Unknown agent" },
  { m: "当前平台不支持", exact: true, zh: "当前平台不支持此操作", en: "Not supported on this platform" },
  { m: "当前平台暂不支持托管终端", exact: true, zh: "当前平台暂不支持托管终端", en: "Managed terminals aren't supported on this platform" },
  { m: "该 Agent 不支持恢复会话", exact: true, zh: "该 Agent 不支持恢复会话", en: "This agent can't resume sessions" },
  { m: "该会话尚未由 Meowo 接管", exact: true, zh: "会话尚未接管", en: "Session not taken over yet" },
  { m: "PTY 会话未运行", exact: true, zh: "会话终端未在运行", en: "The session terminal is not running" },
  { m: "PTY 已结束", exact: true, zh: "会话终端已结束", en: "The session terminal has ended" },
  { m: "PTY 输入通道已关闭", exact: true, zh: "会话终端已结束", en: "The session terminal has ended" },
  { m: "单次 PTY 输入过大", exact: true, zh: "输入内容过大", en: "Input too large" },
  // pty.rs 的 INPUT_BACKLOGGED / RESIZE_BUSY 常量:attach 循环把它们当临时状态跳帧,
  // 但 broker 直返调用方时会落到 UI——不进映射的话英文界面降级出中文原文。
  { m: "Agent 未在读取输入，输入已积压，请稍后重试", exact: true, en: "The agent isn't reading input; input has backed up — try again in a moment" },
  { m: "PTY 尺寸通道忙（可能已僵死），本次调整跳过", exact: true, en: "The PTY resize channel is busy (possibly wedged); this resize was skipped" },
  { m: "原会话仍在运行，未重新打开", exact: true, zh: "原会话仍在运行，未重新打开", en: "The original session is still running; nothing was reopened" },
  { m: "无法结束原会话进程", exact: true, zh: "无法结束原会话进程", en: "Couldn't stop the original session process" },
  { m: "会话进程已变化，请刷新后重试", exact: true, zh: "会话进程已变化，请刷新后重试", en: "The session process changed; refresh and retry" },
  // tail:后缀有两种——「请先在终端页选择接管」(terminal.rs)与「不能重复接管」(managed_terminal.rs),
  // 都是可执行指引,裁掉就只剩一句干巴巴的现状陈述。
  { m: "会话仍在外部终端运行", tail: true, zh: "会话仍在外部终端运行", en: "The session is still running in an external terminal" },
  { m: "外部终端已在线，但没能带到前台，请手动切换", exact: true, zh: "外部终端已在线，但没能带到前台，请手动切换", en: "External terminal is online but couldn't be focused; switch manually" },
  { m: "打开外部同步终端失败", zh: "打开外部终端失败", en: "Couldn't open the external terminal" },
  { m: "打开外部终端失败", zh: "无法打开外部终端", en: "Couldn't open the external terminal" },
  { m: "同步会话失败", tail: true, zh: "同步会话失败", en: "Couldn't sync the session" },
  // tail 透传:后端秒退探测的报错带输出尾部(「…（退出码 N）：tail」),zh 必须原样保留;
  // 英文界面下该消息本就是英文,不会命中此中文前缀。
  { m: "Agent 启动后立即退出", tail: true, zh: "Agent 启动后立即退出", en: "The agent exited right after starting; check the Terminal tab —" },
  { m: "该会话所属账号", zh: "会话所属账号已被删除，无法恢复", en: "This session's account was deleted; it can't be resumed" },
  { m: "请选择工作目录", exact: true, zh: "请选择工作目录", en: "Pick a working directory" },
  { m: "无法定位当前账号目录", exact: true, zh: "找不到当前账号的数据目录", en: "Couldn't locate the current account directory" },
  { m: "内部状态异常", zh: "内部状态异常，请重启 Meowo", en: "Internal error — restart Meowo" },
  // ── 审批 ──
  { m: "审批请求已结束", exact: true, zh: "审批请求已结束", en: "The approval request has ended" },
  { m: "审批请求不属于该会话", exact: true, zh: "该审批属于另一条会话", en: "This approval belongs to another session" },
  { m: "Agent 已不再等待审批", exact: true, zh: "Agent 已不再等待审批", en: "The agent is no longer waiting for approval" },
  { m: "无效的审批选项", exact: true, zh: "该审批选项已失效", en: "That approval option is no longer valid" },
  { m: "审批选项已失效", exact: true, zh: "该审批选项已失效", en: "That approval option is no longer valid" },
  // ── 后台会话(bgpty / managed_terminal)──
  { m: "该后台会话未接入", exact: true, zh: "后台会话未连接", en: "Background session not attached" },
  { m: "后台会话已结束或被 Agent 收回", exact: true, zh: "后台会话已结束或被 Agent 收回", en: "The background session has ended or was reclaimed by the agent" },
  { m: "后台会话不接受终端按键", zh: "后台会话不接受终端按键，请在对话页发消息", en: "Background sessions ignore terminal keys; send from the chat page" },
  { m: "找不到 Agent 的控制通道", zh: "连不上后台会话的控制通道", en: "Couldn't reach the background session's control channel" },
  { m: "连接后台会话", tail: true, zh: "无法连接后台会话", en: "Couldn't connect to the background session" },
  { m: "写后台会话", tail: true, zh: "消息没能送进后台会话，请重试", en: "Couldn't deliver to the background session; try again" },
  { m: "向后台会话发送失败", tail: true, zh: "消息没能送进后台会话，请重试", en: "Couldn't deliver to the background session; try again" },
  { m: "等待后台会话回执失败", tail: true, zh: "后台会话未确认收到消息", en: "The background session didn't acknowledge the message" },
  { m: "后台会话未确认收到消息", zh: "后台会话未确认收到消息，请稍后重试", en: "The background session didn't acknowledge the message; try again" },
  { m: "后台会话拒绝了这条消息", tail: true, zh: "后台会话拒绝了这条消息", en: "The background session rejected this message" },
  { m: "消息为空", exact: true, zh: "消息为空", en: "Empty message" },
  // ── 切换引擎(handoff)──
  { m: "该 Agent 不提供结构化会话记录，无法携带上下文切换", exact: true, zh: "该 Agent 无结构化记录，无法携带上下文切换", en: "This agent has no structured transcript; can't switch with context" },
  { m: "会话记录为空，没有可交接的上下文", exact: true, zh: "会话记录为空，没有可交接的内容", en: "The transcript is empty; nothing to hand off" },
  { m: "未找到会话记录文件", exact: true, zh: "找不到会话记录文件", en: "Transcript file not found" },
  { m: "找不到会话记录文件", exact: true, zh: "找不到会话记录文件", en: "Transcript file not found" },
  { m: "会话缺少工作目录", zh: "会话缺少工作目录，无法在原目录启动", en: "The session has no working directory; can't start it in place" },
  { m: "写入交接文件失败", tail: true, zh: "写入交接文件失败", en: "Couldn't write the handoff file" },
  // ── 对话记录 ──
  { m: "该 Agent 不提供结构化会话记录", exact: true, zh: "该 Agent 不提供结构化会话记录", en: "This agent doesn't provide a structured transcript" },
  { m: "找不到该子任务的记录", exact: true, zh: "找不到该子任务的记录", en: "No record found for this subtask" },
  // ── 账号 / 登录 / 安装 ──
  { m: "没有这个账号", exact: true, zh: "账号不存在", en: "Account not found" },
  { m: "账号名不能为空", exact: true, zh: "账号名不能为空", en: "Account name can't be empty" },
  { m: "该 agent 不支持多账号", exact: true, zh: "该 Agent 不支持多账号", en: "This agent doesn't support multiple accounts" },
  { m: "该 agent 不支持 API 中转", exact: true, zh: "该 Agent 不支持 API 中转", en: "This agent doesn't support API relay" },
  { m: "该 agent 不支持 API Key 登录", exact: true, zh: "该 Agent 不支持 API Key 登录", en: "This agent doesn't support API-key sign-in" },
  { m: "该 agent 未声明登录入口", exact: true, zh: "该 Agent 没有登录入口", en: "This agent has no sign-in method" },
  { m: "登录操作已结束或已被替换", exact: true, zh: "本次登录已结束或被新的登录替换", en: "The sign-in attempt has ended or was replaced" },
  { m: "启动终端失败：无法拉起登录流程", exact: true, zh: "无法打开终端进行登录", en: "Couldn't open a terminal to start sign-in" },
  { m: "解析不到该账号的目录", zh: "找不到该账号的数据目录", en: "Couldn't locate this account's data directory" },
  { m: "找不到该账号的数据目录", exact: true, zh: "找不到该账号的数据目录", en: "Couldn't locate this account's data directory" },
  { m: "找不到默认账号的数据目录", exact: true, zh: "找不到默认账号的数据目录", en: "Couldn't locate the default account's data directory" },
  { m: "解析不到该 agent 的安装实况", zh: "无法确认该 Agent 的安装状态", en: "Couldn't determine this agent's install state" },
  // ── 多账号结构化 reason 码(profile/mod.rs 的 coded(),S-9)──
  // 前后端约定:错误串 = `profile/<code>: <detail>`,tail 把 detail(路径/错误原文)透传保留排障信息。
  // 长前缀在前:delete-dir-failed-unrestored 会被 delete-dir-failed 抢先命中。
  { m: "profile/delete-dir-failed-unrestored", tail: true, zh: "删除账号目录失败，且恢复账号入口也失败，请手动检查账号列表", en: "Couldn't delete the account folder, and restoring the entry also failed — check the account list" },
  { m: "profile/delete-dir-failed", tail: true, zh: "删除账号目录失败，账号入口已恢复，可关闭占用进程后重试", en: "Couldn't delete the account folder; the entry was restored — close whatever is using it and retry" },
  { m: "profile/has-live-sessions", tail: true, zh: "该账号还有进行中的会话，请先结束这些会话再合并；进行中会话数", en: "End this account's live sessions before merging; live sessions" },
  { m: "profile/create-dir-failed", tail: true, zh: "创建账号目录失败", en: "Couldn't create the account folder" },
  { m: "profile/read-dir-failed", tail: true, zh: "读取账号目录失败", en: "Couldn't read the account folder" },
  { m: "profile/copy-failed", tail: true, zh: "复制账号数据失败", en: "Couldn't copy account data" },
  { m: "profile/read-file-failed", tail: true, zh: "读取账号数据失败", en: "Couldn't read account data" },
  { m: "profile/write-file-failed", tail: true, zh: "写入账号数据失败", en: "Couldn't write account data" },
  { m: "该 agent 没有可用的一键安装命令", exact: true, zh: "该 Agent 不支持一键安装", en: "One-click install isn't available for this agent" },
  { m: "未找到该 agent 的可执行目录", exact: true, zh: "找不到该 Agent 的安装目录", en: "Couldn't find this agent's install directory" },
  { m: "下载失败", tail: true, zh: "下载失败", en: "Download failed" },
  { m: "下载安装脚本失败", tail: true, zh: "下载安装脚本失败", en: "Couldn't download the install script" },
  { m: "执行安装失败", tail: true, zh: "安装失败", en: "Installation failed" },
  { m: "启动安装失败", tail: true, zh: "无法启动安装", en: "Couldn't start the installation" },
  { m: "安装程序以退出码", zh: "安装程序执行失败", en: "The installer exited with an error" },
  // ── API 中转(relay.rs)──
  // 后端常把 agent id 拼在句首(「claude 的中转地址…」「claude 插件不支持…」),
  // 前缀够不着,这批用 anywhere;zh 一律缺省(原文即中文,保留句首的 agent id)。
  { m: "未知中转 agent", tail: true, zh: "未知的中转 Agent", en: "Unknown relay agent" },
  { m: "插件不支持 API 中转", anywhere: true, en: "This agent's plugin doesn't support API relay" },
  { m: "当前安装版本不支持 API 中转", exact: true, zh: "当前安装版本不支持 API 中转", en: "The installed version doesn't support API relay" },
  { m: "中转地址必须以 http:// 或 https:// 开头", anywhere: true, en: "Relay URL must start with http:// or https://" },
  { m: "中转地址格式无效", anywhere: true, en: "Invalid relay URL" },
  { m: "中转地址缺少主机名", anywhere: true, en: "Relay URL is missing a host" },
  // validate_http_url 的明文守卫:被「{id} 的{e}」包在句中,anywhere 够中;整句作 m,无 tail 可带。
  { m: "http:// 明文地址仅允许本机回环（127.0.0.1、::1、localhost），远程中转请改用 https://", anywhere: true, en: "Plain http:// relay URLs are loopback-only (127.0.0.1, ::1, localhost); use https:// for remote relays" },
  { m: "中转地址与已保存的中转配置不一致", anywhere: true, zh: "中转地址与已保存的配置不一致，请先保存中转地址", en: "Relay URL doesn't match the saved relay config; save the relay URL first" },
  { m: "中转模型不能为空", anywhere: true, en: "Relay model can't be empty" },
  // 覆盖「请先保存中转密钥」与「请先保存 {id} 的中转密钥」两种形态。
  { m: "中转密钥", anywhere: true, en: "Save the relay key first" },
  { m: "安装形态", anywhere: true, en: "Couldn't determine the agent's install type" },
  { m: "中转返回的模型列表不是有效 JSON", exact: true, zh: "中转返回的模型列表不是有效 JSON", en: "The relay returned invalid JSON for the model list" },
  { m: "中转返回的模型列表格式不受支持", exact: true, zh: "中转返回的模型列表格式不受支持", en: "Unsupported model-list format from the relay" },
  { m: "中转没有返回可用模型", exact: true, zh: "中转没有返回可用模型", en: "The relay returned no models" },
  // 这两条会被「查询模型失败：{e}」包一层,anywhere+tail 把内层译出来、参数照带;
  // 必须排在「查询模型失败」前缀项之前。
  { m: "中转返回 HTTP", anywhere: true, tail: true, en: "The relay returned HTTP" },
  { m: "连接中转失败", anywhere: true, tail: true, en: "Couldn't reach the relay" },
  { m: "查询模型失败", tail: true, zh: "查询模型失败", en: "Couldn't fetch models" },
  // ── 代理 ──
  { m: "已选「自定义代理」，但代理地址为空", exact: true, zh: "已选自定义代理，请填写代理地址", en: "Custom proxy selected — enter a proxy address" },
  { m: "代理地址为空", exact: true, zh: "代理地址为空", en: "Proxy address is empty" },
  { m: "代理地址不能含空格", exact: true, zh: "代理地址不能含空格", en: "Proxy address can't contain spaces" },
  // tail 带上「{scheme}」与可用协议列表,排障信息不丢(同「代理端口无效」的处理)。
  { m: "不支持的代理协议", tail: true, zh: "不支持的代理协议", en: "Unsupported proxy protocol" },
  { m: "代理地址缺少主机", exact: true, zh: "代理地址缺少主机", en: "Proxy address is missing a host" },
  { m: "代理端口无效", tail: true, zh: "代理端口无效", en: "Invalid proxy port" },
  { m: "代理地址无效", tail: true, zh: "代理地址无效", en: "Invalid proxy address" },
  { m: "host:port:user:pass 四段都不能为空", zh: "host:port:user:pass 四段都不能为空", en: "All four host:port:user:pass parts are required" },
  // ── 更新 ──
  { m: "请先检查更新", exact: true, zh: "请先检查更新", en: "Check for updates first" },
  { m: "更新尚未下载完成", exact: true, zh: "更新还没下载完", en: "The update hasn't finished downloading" },
  // 下载 60 秒没有新数据，被看门狗中止（lib.rs stall_watchdog）。下一轮检查会重新下载。
  { m: "update/stalled", exact: true, zh: "下载没有进展，已停止", en: "The download stalled and was stopped" },
  // ── 文件 / git / 打开方式 ──
  { m: "附件过大", zh: "附件过大（上限 32MB）", en: "Attachment too large (max 32MB)" },
  { m: "空附件", exact: true, zh: "附件是空的", en: "Empty attachment" },
  { m: "不是文件", exact: true, zh: "所选路径不是文件", en: "The selected path is not a file" },
  { m: "不是目录", exact: true, zh: "所选路径不是目录", en: "The selected path is not a directory" },
  { m: "路径不存在", exact: true, zh: "路径不存在", en: "Path not found" },
  { m: "路径超出工作目录", exact: true, zh: "路径超出工作目录", en: "Path is outside the working directory" },
  { m: "无法读取文件", tail: true, zh: "无法读取文件", en: "Couldn't read the file" },
  { m: "无法读取目录", tail: true, zh: "无法读取目录", en: "Couldn't read the directory" },
  { m: "不是支持的图片格式", exact: true, zh: "不支持的图片格式", en: "Unsupported image format" },
  { m: "git status 执行失败", zh: "git status 执行失败", en: "git status failed" },
  { m: "git diff 执行失败", zh: "git diff 执行失败", en: "git diff failed" },
  { m: "未检测到该编辑器", exact: true, zh: "未检测到该编辑器", en: "Editor not found" },
  { m: "不允许的链接", exact: true, zh: "不允许打开该链接", en: "This link isn't allowed" },
  { m: "无效链接", exact: true, zh: "链接无效", en: "Invalid link" },
  { m: "只支持 http/https 链接", exact: true, zh: "只支持 http/https 链接", en: "Only http/https links can be opened" },
  { m: "该文件类型没有关联应用", exact: true, zh: "该文件类型没有关联应用", en: "No app is associated with this file type" },
  { m: "该打开方式已不可用", exact: true, zh: "该打开方式已不可用", en: "That app is no longer available" },
  { m: "terminal/process_not_exclusive", exact: true, zh: "这个进程可能还连着别的会话，或者认不出归属，Meowo 没有结束它，以免一起关掉。在原终端退出这个会话后再试", en: "This process may also serve other sessions, or its owner could not be confirmed, so Meowo left it running to avoid closing them. Exit this session in its original terminal, then try again" },
  { m: "terminal/composer_occupied", exact: true, zh: "终端输入框已有草稿，已原样保留。请先在终端发送或清空该草稿，再重试此消息", en: "The terminal already contains a draft, which was preserved. Send or clear it in the terminal, then retry this message" },
  { m: "terminal/composer_unknown", exact: true, zh: "无法确认终端输入框是否为空，消息原文已保留。请查看终端，确认输入框就绪后重试", en: "Could not confirm that the terminal input is empty. Your message was preserved; check the terminal input and retry when ready" },
  { m: "terminal/composer_partial", exact: true, zh: "上条消息的正文可能仍在终端输入框，暂未重复写入。请在终端检查并提交原有内容，消息原文已保留", en: "The previous message may still be in the terminal input. It was not written again. Check and submit the existing text in the terminal; your original message is preserved" },
  // ── Lead 工作流（reporter lead 模块的结构化 reason）──
  { m: "lead/claude_missing", exact: true, zh: "没找到 Claude Code 的配置目录，请先安装并登录 Claude Code", en: "Claude Code's config folder wasn't found. Install and sign in to Claude Code first" },
  { m: "lead/reporter_missing", exact: true, zh: "找不到 meowo-reporter，请重新安装 Meowo", en: "meowo-reporter is missing. Reinstall Meowo" },
  { m: "lead/user_file_exists", exact: true, zh: "该账号已有你自己的同名 Lead 技能或审查角色，已停止安装以保留原文件", en: "This account already has your own Lead skill or reviewer role. Installation stopped to preserve your files" },
  { m: "lead/settings_invalid", exact: true, zh: "CLI 配置格式不正确，未修改该配置", en: "The CLI configuration is malformed and was left unchanged" },
  { m: "lead/provider_missing", exact: true, zh: "未找到该 CLI，请先安装并完成设置", en: "This CLI was not found. Install it and complete its setup first" },
  { m: "lead/provider_unsupported", exact: true, zh: "该 CLI 暂不支持安装 Lead 工作流", en: "Lead installation is not supported for this CLI yet" },
  { m: "lead/version_unsupported", exact: true, zh: "尚未验证此 CLI 版本的 Lead 接入，未修改配置", en: "Lead support has not been verified for this CLI version. Its configuration was left unchanged" },
  { m: "lead/variant_unsupported", exact: true, zh: "该 CLI 变体暂不支持 Lead 接入", en: "Lead is not supported for this CLI variant yet" },
  { m: "lead/profile_missing", exact: true, zh: "该账号配置已不存在，请刷新账号列表后重试", en: "This account configuration no longer exists. Refresh the account list and retry" },
  { m: "lead/reviewer_disabled", exact: true, zh: "CLI 已禁用审查角色所需的能力，请先在其设置中启用", en: "The CLI has disabled the capability required by the reviewer role. Enable it in the CLI settings first" },
  { m: "lead/config_changed", exact: true, zh: "CLI 配置在安装期间发生变化，请重试", en: "The CLI configuration changed during installation. Please retry" },
  { m: "lead/read_failed", exact: true, zh: "无法读取 Lead 配置，请检查目录权限后重试", en: "Could not read the Lead configuration. Check folder permissions and retry" },
  { m: "lead/write_failed", exact: true, zh: "无法保存 Lead 配置，请检查目录权限后重试", en: "Could not save the Lead configuration. Check folder permissions and retry" },
  { m: "lead/backup_failed", exact: true, zh: "无法备份 CLI 原配置，已停止安装", en: "Could not back up the original CLI configuration. Installation stopped" },
  { m: "lead/rollback_incomplete", exact: true, zh: "安装失败且部分配置未能还原，请检查备份后再重试", en: "Installation failed and some configuration could not be restored. Check the backup before retrying" },
  { m: "lead/reporter_path_invalid", exact: true, zh: "Lead 运行程序路径无效，请重新安装 Meowo", en: "The Lead runtime path is invalid. Reinstall Meowo" },
  { m: "lead/asset_path_invalid", exact: true, zh: "Lead 安装文件路径无效，请重新安装 Meowo", en: "A Lead installation file path is invalid. Reinstall Meowo" },
  { m: "lead/native_hooks_unsupported", exact: true, zh: "此 CLI 暂无可用的原生 Lead 事件接入", en: "This CLI does not yet provide a supported native Lead event integration" },
  { m: "lead/worker_failed", exact: true, zh: "Lead 设置未能完成，请重试", en: "Lead setup could not be completed. Please retry" },
  { m: "lead/gate_missing", exact: true, zh: "关卡已经不在了", en: "This gate no longer exists" },
  { m: "lead/gate_invalid", exact: true, zh: "关卡文件格式不对", en: "The gate file is malformed" },
  { m: "lead/gate_changed", exact: true, zh: "关卡已经更新，请看最新的卡片", en: "The gate has changed; check the latest card" },
  { m: "lead/gate_bad_choice", exact: true, zh: "这个选项不属于当前关卡，请刷新后重选", en: "That option doesn't belong to this gate; refresh and pick again" },
  { m: "lead/gate_closed", exact: true, zh: "这个关卡已经回答过了", en: "This gate has already been answered" },
  // ── 窗口 ──
  { m: "创建对话窗口失败", tail: true, zh: "打开对话窗口失败", en: "Couldn't open the chat window" },
  { m: "打开对话窗口失败", tail: true, zh: "打开对话窗口失败", en: "Couldn't open the chat window" },
  { m: "切换会话失败", tail: true, zh: "切换会话失败", en: "Couldn't switch session" },
];

/** 是否含汉字(CJK 统一表意文字基本区,后端 sentinel 均落在此区间)。 */
const hasHan = (text: string): boolean => /[一-鿿]/.test(text);

/**
 * 把后端错误(通常是 `invoke` reject 出来的 String)格式化成当前语言的用户文案。
 * @param raw    任意错误值(Error / string / unknown),内部先转字符串。
 * @param locale 当前界面 locale(取 `t.locale`),zh 开头视为中文界面。
 */
export function formatBackendError(raw: unknown, locale: string): string {
  const text = raw instanceof Error ? raw.message : String(raw ?? "");
  const isZh = locale.startsWith("zh");
  for (const entry of ENTRIES) {
    const idx = entry.anywhere
      ? text.indexOf(entry.m)
      : entry.exact
        ? (text === entry.m ? 0 : -1)
        : (text.startsWith(entry.m) ? 0 : -1);
    if (idx < 0) continue;
    // zh 缺省:原文本就是中文,整句照放(不做 tail 拼接,保留句首前缀)。
    const base = isZh ? entry.zh : entry.en;
    if (base === undefined) return text;
    // tail:把「：具体原因」原样带上——译文负责可读,原因负责排障。
    return entry.tail && !entry.exact ? base + text.slice(idx + entry.m.length) : base;
  }
  // 未命中:中文界面原样显示(原文本就是中文);英文界面遇到中文原文时降级为通用前缀,
  // 保留原文便于反馈排查——比一句纯中文可读。
  if (!isZh && hasHan(text)) return `Operation failed: ${text}`;
  return text;
}
