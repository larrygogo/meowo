//! 手机桥 `/rpc/<command>` 各臂的参数结构体（remote.rs dispatch 逐臂解析）。
//!
//! 为什么搬到这里：原先定义在 meowo-app 的 remote.rs，前端 api.ts 的 invoke 传参只能靠人肉
//! 对齐，样例 payload 测试也是手抄的——api.ts 加了键、没人改样例时测试照样绿，手机端每次
//! 调用都被 deny_unknown_fields 打成 400（2026-08-31 newSession 加 extraDirs 事故）。放进
//! protocol 后 ts-rs 从同一份定义导出 `generated/contracts/bridge/*.ts`，api.ts 用
//! `satisfies XxxArgs` 校验实参：多一个键、少一个必填键 tsc 当场报错；导出走 CI 的
//! 「生成并校验前后端 IPC 契约」快速步骤（cargo test -p meowo-protocol + git diff）。
//!
//! 字段约定：
//! - JSON 键 = camelCase（宏统一加 `rename_all`），TS 键与之一致。
//! - `Option` 字段在 TS 里是 `k?: T | null`：serde 对缺键和显式 null 都认 None，api.ts 两种都发。
//! - i64/u64 逐字段覆盖成 `number`（ts-rs 默认 bigint；JSON 里就是数字，和 ipc.rs 同一做法）。
//! - 数组字段导出成 `readonly T[]`：后端只读，api.ts 的签名多是 `readonly string[]`，可变数组也能赋过去。

/// camelCase + deny_unknown_fields 由宏统一强制：新臂手写漏了 deny_unknown_fields 会退化成
/// 静默丢键，不许再有这个自由度。字段一律 pub，remote.rs 跨 crate 取值。
macro_rules! bridged_args {
    ($name:ident { $( $(#[$attr:meta])* $field:ident : $ty:ty ),* $(,)? }) => {
        #[derive(Debug, serde::Deserialize)]
        #[cfg_attr(test, derive(ts_rs::TS))]
        #[cfg_attr(
            test,
            ts(export, export_to = "../../../../src/generated/contracts/bridge/", optional_fields = nullable)
        )]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            $( $(#[$attr])* pub $field : $ty, )*
        }
    };
}

// sessionId 单参臂共用。
bridged_args!(SessionArg {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64
});

bridged_args!(GetLiveSessionsPageArgs {
    filter: String,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    before_last_event_at: Option<i64>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    before_id: Option<i64>,
    limit: usize,
    #[serde(default)]
    include_foreign: Option<bool>,
    #[serde(default)]
    include_pending: Option<bool>,
    // 对话侧栏按创建时间排时才发（api.ts 只在 "created" 时带这个键）。
    #[serde(default)]
    order: Option<String>
});

bridged_args!(GetSessionAdminPageArgs {
    #[cfg_attr(test, ts(type = "readonly string[]"))]
    workspaces: Vec<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    updated_before_days: Option<u32>,
    page: usize,
    page_size: usize
});

bridged_args!(SearchChatTranscriptsArgs { query: String });

bridged_args!(RecentCwdsArgs {
    limit: usize,
    #[serde(default)]
    include_hidden: Option<bool>
});

bridged_args!(HideWorkspaceArgs {
    cwd: String,
    /// true = 撤销刚才的移除（侧栏 5 秒撤销条，UI 调研 shell #15）；缺省 = 移除。
    #[serde(default)]
    restore: Option<bool>
});

bridged_args!(GitDiffSummaryArgs { cwd: String });

// 侧栏会话行的 PR / CI 与改动数（UI 优化 #54）：一次带上可见会话的全部工作目录（去重后）。
bridged_args!(SidebarGitBadgesArgs {
    #[cfg_attr(test, ts(type = "readonly string[]"))]
    cwds: Vec<String>
});

bridged_args!(FileMentionCandidatesArgs {
    cwd: String,
    query: String
});

bridged_args!(LeadTaskSnapshotArgs {
    cwd: String,
    #[serde(default)]
    session: Option<String>
});

bridged_args!(AnswerLeadGateArgs {
    cwd: String,
    gate_id: String,
    choice: String,
    note: String
});

bridged_args!(ForkSessionArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    cols: u16,
    rows: u16
});

bridged_args!(GitFileDiffArgs {
    cwd: String,
    path: String,
    untracked: bool,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitChangesOverviewArgs {
    cwd: String,
    #[cfg_attr(test, ts(type = "readonly string[]"))]
    touched: Vec<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "readonly string[] | null", optional))]
    commands: Option<Vec<String>>,
    #[serde(default)]
    detail: Option<String>
});

bridged_args!(GitPathsArgs {
    cwd: String,
    #[cfg_attr(test, ts(type = "readonly string[]"))]
    paths: Vec<String>,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitDirArgs {
    cwd: String,
    path: String,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitCommitArgs {
    cwd: String,
    message: String,
    all: bool,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "readonly string[] | null", optional))]
    paths: Option<Vec<String>>,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitRepoArgs {
    cwd: String,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitJobStatusArgs { job_id: String });

// 撤销一次放弃（walkthrough P-5）：undoId 是 git_discard 的返回值，只在桌面端进程内有效。
bridged_args!(GitDiscardUndoArgs {
    cwd: String,
    undo_id: String,
    #[serde(default)]
    session_cwd: Option<String>
});

// 提交 / 合并报 git/identity 时就地填名字和邮箱（walkthrough P-1），写全局 git 配置。
bridged_args!(GitSetIdentityArgs {
    cwd: String,
    name: String,
    email: String,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GitForgeStatusArgs {
    cwd: String,
    #[serde(default)]
    force: Option<bool>,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GhPrCreateArgs {
    cwd: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    draft: Option<bool>,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(GhPrMergeArgs {
    cwd: String,
    number: u32,
    method: String,
    expected_head_oid: String,
    #[serde(default)]
    session_cwd: Option<String>
});

// 多个 GitHub 远端时选 PR 开在哪个远端的仓库（forge_choose_repo）。
bridged_args!(ForgeChooseRepoArgs {
    cwd: String,
    remote: String,
    #[serde(default)]
    session_cwd: Option<String>
});

bridged_args!(RenameSessionArgs {
    #[serde(default)]
    cwd: Option<String>,
    session_id: String,
    title: String,
    #[serde(default)]
    provider: Option<String>
});

bridged_args!(SetArchivedArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    archived: bool
});

bridged_args!(WorktreeStatusArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64
});

bridged_args!(RemoveSessionWorktreeArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    expected_path: String,
    #[cfg_attr(test, ts(type = "readonly number[]"))]
    confirmed_pids: Vec<u32>
});

bridged_args!(WorktreePreviewArgs {
    repo_cwd: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    fallback_slug: Option<String>
});

bridged_args!(WorktreeCreateArgs {
    repo_cwd: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    fallback_slug: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    request_id: String
});

bridged_args!(WorktreeSetupArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    action: Option<String>
});

bridged_args!(WorktreeSetupTrustArgs {
    path: String,
    hash: String
});

bridged_args!(MoveSessionRankArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    above_id: Option<i64>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    below_id: Option<i64>
});

bridged_args!(SetWorkspaceOrderArgs {
    #[cfg_attr(test, ts(type = "readonly string[]"))]
    order: Vec<String>
});

bridged_args!(SetKeepAwakeArgs { enabled: bool });

bridged_args!(SetPushPausedArgs { paused: bool });

bridged_args!(StopManagedTerminalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64
});

bridged_args!(GetChatHistoryArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    #[cfg_attr(test, ts(type = "number"))]
    offset: u64,
    #[serde(default)]
    full: Option<bool>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    before: Option<u64>
});

bridged_args!(GetSubagentTranscriptArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    tool_use_id: String
});

bridged_args!(ManagedTerminalSnapshotArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    since: Option<u64>
});

bridged_args!(WriteManagedTerminalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    data: String
});

bridged_args!(ResizeManagedTerminalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    cols: u16,
    rows: u16
});

bridged_args!(StartManagedTerminalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    cols: u16,
    rows: u16,
    #[serde(default)]
    options: Option<std::collections::HashMap<String, String>>
});

bridged_args!(TakeoverManagedTerminalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    cols: u16,
    rows: u16,
    #[serde(default)]
    options: Option<std::collections::HashMap<String, String>>
});

bridged_args!(SendBackgroundPromptArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    text: String
});

bridged_args!(SetSessionLaunchSelectionArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    option: String,
    choice: String
});

bridged_args!(RegisterApprovalConsumerArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    consumer_id: String
});

bridged_args!(UnregisterApprovalConsumerArgs {
    consumer_id: String
});

bridged_args!(ResolvePendingApprovalArgs {
    #[cfg_attr(test, ts(type = "number"))]
    session_id: i64,
    request_id: String,
    choice: String
});

bridged_args!(SavePastedAttachmentArgs {
    file_name: String,
    data_base64: String
});

bridged_args!(AgentChatUiArgs {
    provider: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    session_id: Option<i64>
});

bridged_args!(AgentModelsArgs {
    provider: String,
    #[cfg_attr(test, ts(type = "number | null", optional))]
    session_id: Option<i64>
});

bridged_args!(AgentModelCatalogArgs {
    provider: String,
    #[cfg_attr(test, ts(type = "number | null", optional))]
    session_id: Option<i64>
});

bridged_args!(NewSessionArgs {
    cwd: String,
    provider: String,
    /// 旧前端兼容参数，桌面命令同样忽略。TS 侧不导出：新代码不该再发它。
    #[serde(default)]
    #[cfg_attr(test, ts(skip))]
    terminal: Option<String>,
    #[serde(default)]
    options: Option<std::collections::HashMap<String, String>>,
    // 附加目录已按用户要求去掉，新页面不再发它。字段仍收下并忽略：手机上没刷新的旧页面
    // 还会带 extraDirs，deny_unknown_fields 会把它们的每次新建都打成 400。TS 侧同样不导出。
    #[serde(default)]
    #[cfg_attr(test, ts(skip))]
    extra_dirs: Option<Vec<String>>
});

bridged_args!(CheckProviderHooksArgs { provider: String });

bridged_args!(RepairProviderHooksArgs { provider: String });

bridged_args!(ListSubdirectoriesArgs {
    #[serde(default)]
    path: Option<String>
});
