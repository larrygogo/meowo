# 架构收敛路线

本轮重构的目标是收敛跨组件协议与重复状态机，同时保持现有 Agent 插件架构、数据库边界和 UI 行为。
所有阶段均须可独立测试、合并和回滚，不进行一次性大重写。

## 基线（2026-07-19）

- 工作区包含一组尚未提交的 ChatWindow、托管 PTY、审批与 Agent transcript 改动；重构必须原样保留。
- `cargo test --workspace` 通过。
- `cd app && bun x tsc --noEmit` 通过。
- 前端 Vitest/Vite 在受限沙箱中需要沙箱外执行权限；CI 仍以完整 Vitest、build 为门禁。

## 阶段

1. 新增 `meowo-protocol::ipc`，由 Rust DTO 生成 TypeScript 类型。
2. 新增 `meowo-protocol::broker`，统一 app/reporter 的发现、attach、claim 与 approval 协议。
3. 将 Provider 日志解析结果与最终聊天渲染模型拆成 `TranscriptEvent -> ChatReducer -> ChatPatch`。
4. 用显式 `ApprovalConsumer` 租约和带 `operationId` 的登录结果替代窗口/布尔状态猜测。
5. 收敛前端 subscribe-first 资源读取和登录 hook；后端 Tauri command 降为服务适配层。
6. 在行为稳定后按职责拆分 `lib.rs`、`terminal.rs` 与 `config.rs`。

## 当前进度（2026-07-19）

- 已完成：共享 IPC DTO/TypeScript 契约、broker v1/v2 兼容协议、审批消费者租约、登录 operationId 状态机。
- 已完成：聊天增量合并纯 reducer、统一 Tauri 事件订阅生命周期 hook。
- 已完成：Claude/Codex/Kimi 原始日志统一规范化为 `TranscriptEvent`，IPC `ChatItem` 只在中央边界生成。
- 已完成：聊天历史、会话列表/角标/分页查询下沉到应用服务；进程快照缓存由查询服务封装。
- 已完成：托管终端/审批 command 与会话重命名、归档、便签 command 移出 crate 根。
- 已完成：新会话、账号卡片和多账号列表统一使用页面级 `useLoginOperations`；切换 Agent 不再遗失等待中的 operationId，profile 登录也具备事件匹配与取消语义。
- 已完成：审批 broker 保留 Agent 原生 `permission_suggestions`；GUI 直接展示完整工具参数，并可回传“一次允许”或 Agent 提供的持久权限更新。
- `config.rs` 保持现状：它虽体量较大，但已是无 I/O 的纯格式转换层；暂不为缩短文件而机械拆分。

## 不做

- 不引入 Redux/Zustand。
- 不为 `meowo-store` 增加只转发调用的 Repository 层。
- 不重写已经稳定的 `AgentPlugin` 能力槽。
- 不在协议迁移期间同步重做 UI。
- 不批量格式化或覆盖与当前阶段无关的用户改动。
