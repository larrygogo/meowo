# 决策

- 2026-11-16，leader：在调研后答复「按你说的做」，授权按建议路线实施；不解释为提交、推送、发布或全局配置修改授权。
- 2026-11-16，实施方案：保留文件化任务和原生 TUI 路线，集中关卡更新，增加版本化验收与恢复记录，不立即迁移数据库唯一真源。
- 2026-11-16，待 leader 决策：旧 pending C 关卡没有冻结版本，不能在保持旧式自动批量勾选的同时保证只接受看到的内容。通过 B 关卡确认兼容策略。
- 2026-11-16，leader 在 B-20261116-lead-trust-legacy-c 选择 reissue（无附加意见）：旧 pending C 必须重新发起带验收范围与版本的新卡才可接受；已有 answered/consumed 记录保留、不重放业务。

- leader 在 Q-20261116-lead-trust-reviewer 选择 readonly_review（无附加意见）：授权 explore 执行只读独立审查及允许的隔离测试，禁止源码修改、真实 CLI、活动会话数据访问与提交推送。此决定只替代审查角色，不豁免验收项或接受当前交付。

- leader 在 B-20261116-lead-trust-publish-contract 选择 explicit_publish（无附加意见）：确认草稿编辑后显式发布，发布/撤回与作答统一排序，决定独立持久化，不由宿主覆盖草稿。后续需完整迁移及兼容验证，不代表已完成。
- 用户追加：进一步优化 Lead 面板；对话内容中的文件修改面板可以查看详情；修复手机插入队列不能拖拽排序。先处理新增交互，再继续未完成的发布协议工作，原验收范围不删除。

- D-20261116-final-verification-environment（publication-2）：leader 选择 approve。允许在 dev 实例关闭后完成完整 workspace 测试，并在隔离临时目录/端口进行真实 HTTP 与官方 Kimi TUI Lead 闭环；不提交、推送、发布，不改全局 CLI 配置，不访问共享活动数据，验证后清理实例。拒绝路径未选择，L8 不豁免。

- C-20261116-lead-trust-final-r2（publication-3）：leader 已 accept 原范围，已消费。随后 leader 实拍指出验收箭头单独换行及决策/进度排版不足，新增 L15/L16，不将原接受扩大为后续界面改动验收。

- C-20261116-lead-trust-final-r3（publication-4）：leader 对包含 L15/L16 界面修正的完整冻结范围选择 accept，已按 status 核对并 consume。交付版本 e4c32d824c2b4105d3898f8332fdc6f7d327e434，probe 0feeefc95903eaf266c5b7eaba70a91e82861d91，ChatWindow-CTYbN-6D.js。

- C-20261116-lead-trust-final-r4（publication-5）：leader 接受包含未认领任务入口、移动表格、时间轴和验收编号的完整冻结范围，已 consume。交付版本 f1173bd9212c5d2410a70983b505f23dd62e7638，ChatWindow-CE_jRwuh.js。
