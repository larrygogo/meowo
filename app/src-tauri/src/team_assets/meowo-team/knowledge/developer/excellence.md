<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 开发：卓越标准（上限）

craft.md 守下限（不达标就是问题），这份管上限：按维度看这次交付离「业界最好团队」还差多远。
- 打分的是审查员，不是干活的人自己：研究和实测都显示自评稳定偏高。干活的人交付前对照它自查、把明显的差距补上，但不给自己打分。
- 每一分都要有证据（测试、截图、复现命令、文件行号），没有证据按低一档算。
- 任何维度 ≤ 2 分打回；关键的方案选择要「设计两次」：认真想过至少两种做法，写出被否掉的那种和理由。
- 来源：业界公开做法调研（各维度附出处）。

## 一、开发的卓越标准（8 个维度）

打分口径：1 分 = 能交差但留坑；3 分 = 合格的资深工程师；5 分 = 业界最好团队的公开做法。每个维度给 5 分的对应出处。

### D1. 问题理解与方案取舍

- **1 分**：直接按字面需求动手，只有一种方案；做完才发现漏了场景（例如没想到多人合用账号、旧数据）。
- **3 分**：动手前写清目标、非目标和约束；列出 2 种方案并说明为什么选这个；不清楚的事实先查代码和文档。
- **5 分**：
  - 至少认真设计两次（Ousterhout「Design it twice」），把被否掉的方案和否掉的理由写进设计文档的「Alternatives considered」；
  - 明确写出「这个方案在什么条件下会失效」以及换方案的信号；
  - 改动规模和风险匹配：小改动不写长文档，跨团队/不可逆的改动先写设计文档并找人评审（Google 的做法是：设计文档的价值在于「在改动还便宜时暴露分歧」）。
- 出处：[Google 设计文档实践（Malte Ubl）](https://www.industrialempathy.com/posts/design-docs-at-google/)；John Ousterhout《A Philosophy of Software Design》第 11 章「Design it Twice」（[作者讲座](https://www.youtube.com/watch?v=bmSAYlu0NcY)）；[Rust RFC 模板的 Rationale and alternatives 栏](https://github.com/rust-lang/rfcs/blob/master/0000-template.md)。

### D2. 结构与可演进性

- **1 分**：功能堆在调用处；浅模块（接口和实现一样复杂）；复制粘贴；改一处要改很多处。
- **3 分**：职责清楚，命名准确；改动让代码比原来稍好（Google 的审查标准：只要 CL「改善了整体代码健康度」就该批准，不追求完美）；有测试兜底后才重构。
- **5 分**：
  - 深模块：接口简单、把复杂度吸收在内部，例如「把错误定义为不存在」（Ousterhout 的 Unix 文件 I/O 例子）；
  - 用「先整理再改」（Kent Beck《Tidy First?》）把结构调整和行为改动拆成不同提交，审查者可以分开看；
  - 演进路径明确：大改动用 Branch by Abstraction / Parallel Change，主干始终可发布；
  - 能说清这次改动对「设计耐力」的影响（Fowler：内部质量差的代码，几周内就会拖慢交付速度）。
- 出处：[Google：The Standard of Code Review](https://google.github.io/eng-practices/review/reviewer/standard.html)；[Fowler：Is High Quality Software Worth the Cost?](https://martinfowler.com/articles/is-quality-worth-cost.html)、[Design Stamina Hypothesis](https://martinfowler.com/bliki/DesignStaminaHypothesis.html)、[Branch by Abstraction](https://martinfowler.com/bliki/BranchByAbstraction.html)；Kent Beck《Tidy First?》（O'Reilly 2023）。

### D3. 正确性、边界与兼容

- **1 分**：只走了顺利路径；接口字段直接改名或删除；权限只在前端判断。
- **3 分**：边界值、空值、并发、重复提交、中途失败都处理；新字段可选、旧字段保留；权限在最终执行层校验（这些是现有 craft.md 的要求）。
- **5 分**：
  - 默认按 Hyrum 定律对待接口：「用户够多时，你的系统所有可观察行为都会被人依赖」，包括错误文本、顺序、时延；变更前先查调用方；
  - 对外接口有版本化策略，旧版本客户端的行为有测试保护（Stripe 的做法：每个 API 版本的差异写成「版本变更模块」，请求进来按账号固定的版本逐层转换）；
  - 数据迁移用四步法：双写 → 回填 → 切读 → 删旧（Stripe「Online migrations at scale」），每步可回滚；
  - 对不变量做性质测试（property-based testing）或模糊测试，而不是只列举例子。
- 出处：[Hyrum's Law](https://www.hyrumslaw.com/)；[Stripe：APIs as infrastructure — 版本化](https://stripe.com/blog/api-versioning)；[Stripe：Online migrations at scale](https://stripe.com/blog/online-migrations)；[Fowler：Parallel Change（expand/contract）](https://martinfowler.com/bliki/ParallelChange.html)。

### D4. 测试保护真实风险

- **1 分**：没有测试，或测试只为凑覆盖率；断言弱（只检查「不报错」）；改坏代码测试照样通过。
- **3 分**：新逻辑有单元测试；至少手工改坏一处确认测试会失败；在真实入口跑过一遍。
- **5 分**：
  - 测试按风险分配，而不是按代码行分配：先列「最可能坏、坏了最贵」的地方，再写测试；
  - 测试满足 Kent Beck 的 Test Desiderata：隔离、可组合、确定、快、可写、可读、只对行为敏感、对结构不敏感、自动、具体（失败时直接指出原因）、可预测、有启发；尤其「对结构不敏感」——重构不应打碎测试；
  - 用变异测试检验测试本身：Google 在代码审查中展示存活的变异体，研究显示接触过变异测试的开发者会写更多、更好的测试，且变异体与真实缺陷相关；
  - 没有不稳定（flaky）测试；发现即隔离并修。
- 出处：[Kent Beck：Test Desiderata](https://testdesiderata.com/)；[Petrović et al.，Does mutation testing improve testing practices?（ICSE 2021）](https://arxiv.org/abs/2103.07189)；[Practical Mutation Testing at Scale: A view from Google](https://research.google/pubs/practical-mutation-testing-at-scale-a-view-from-google/)。

### D5. 性能与可靠性

- **1 分**：没想过数据量；列表全量加载；N+1 查询；外部调用没有超时和重试上限。
- **3 分**：按目标规模估算过（例如「一个用户 100 台机器」）；关键路径有超时、有限重试、幂等；界面交互在 100ms 内有反馈。
- **5 分**：
  - 有明确的性能和可靠性目标（SLO），改动前后给出数据（基准测试、p95/p99 时延），而不是「感觉更快」；
  - 失败模式设计过：降级、熔断、背压，并说明「这个依赖挂了用户看到什么」；
  - 界面性能按 Core Web Vitals / RAIL 的阈值验收（INP ≤ 200ms 为「良好」，响应 100ms 内）；
  - 上线走渐进发布：功能开关、灰度、可一键回滚。
- 出处：[Google SRE：Service Level Objectives](https://sre.google/sre-book/service-level-objectives/)；[web.dev：INP](https://web.dev/articles/inp)、[RAIL 模型](https://web.dev/articles/rail)；[Pete Hodgson：Feature Toggles](https://martinfowler.com/articles/feature-toggles.html)。

### D6. 可观测与可运维

- **1 分**：出错只打一行 `error`；没有请求标识；问题要靠复现才能查。
- **3 分**：关键路径有结构化日志和请求 id；错误带上下文；有基本的指标（请求量、错误率、时延——Google 的「四个黄金信号」）。
- **5 分**：
  - 按 Charity Majors / Honeycomb 的「可观测性」理解来做：每个请求一条宽事件（wide event），带高基数字段（用户 id、构建版本、功能开关状态），能回答事先没想到的问题，而不只是看预设仪表盘；
  - 先想「线上出问题时我要问什么」，再决定打什么点（observability-driven development）；
  - 提交里附运维说明：怎么判断它正常、出问题看哪里、怎么回滚；
  - 告警基于用户感受到的症状（SLO 消耗速度），不基于内部原因。
- 出处：[Charity Majors：Observability — A Manifesto](https://www.honeycomb.io/blog/observability-a-manifesto)、[charity.wtf](https://charity.wtf/)；[Google SRE：Monitoring Distributed Systems（四个黄金信号）](https://sre.google/sre-book/monitoring-distributed-systems/)；[Google SRE Workbook：Alerting on SLOs](https://sre.google/workbook/alerting-on-slos/)。

### D7. 交付沟通（PR 和说明让人快速信任）

- **1 分**：PR 标题「fix」；描述空白或复述代码；一次 PR 混着重构、功能和格式化。
- **3 分**：说清改了什么、为什么、怎么验证、没验证什么（现有 craft.md 要求）；PR 规模适中。
- **5 分**：
  - PR 描述第一行是一句完整的「做了什么」，正文讲「为什么」和背景，审查者不看代码也能判断方向对不对（Google CL 描述指南）；
  - 小步提交：一个 PR 只做一件事，重构和行为改动分开（Google「Small CLs」：小改动审得更快、更彻底、更不容易引入缺陷）；
  - 给审查者「阅读顺序」和「重点看哪里」，并附可直接复制的验证命令和前后截图；
  - 主动列出风险和自己不确定的地方，而不是等审查者发现。
- 出处：[Google：Writing good CL descriptions](https://google.github.io/eng-practices/review/developer/cl-descriptions.html)；[Google：Small CLs](https://google.github.io/eng-practices/review/developer/small-cls.html)。

### D8. 持续改进与工程判断（可选的第 8 维）

- **1 分**：同类问题反复出现；被纠正后下次照犯。
- **3 分**：被纠正的点记进 lessons；出事后写复盘。
- **5 分**：无责复盘（blameless postmortem），把「怎么防止整类问题」落成机制（测试、守卫、lint 规则），而不是「下次注意」；用 DORA 四项指标（部署频率、变更前置时间、变更失败率、恢复时间）衡量交付能力。
- 出处：[Google SRE：Postmortem Culture](https://sre.google/sre-book/postmortem-culture/)；[DORA](https://dora.dev/)。

---

