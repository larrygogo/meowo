<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 测试/审查：卓越标准（上限）

craft.md 守下限（不达标就是问题），这份管上限：按维度看这次交付离「业界最好团队」还差多远。
- 打分的是审查员，不是干活的人自己：研究和实测都显示自评稳定偏高。干活的人交付前对照它自查、把明显的差距补上，但不给自己打分。
- 每一分都要有证据（测试、截图、复现命令、文件行号），没有证据按低一档算。
- 任何维度 ≤ 2 分打回；审查员自己的分由技术负责人或用户对照结果来打（审查漏掉、后来被发现的问题直接拉低 R1/R6）。
- 来源：业界公开做法调研（各维度附出处）。

## 二、审查员的卓越标准

### 业界对「审查做得好不好」的分级依据

- **审查主要发现的是可演进性问题，不是功能缺陷。** Mäntylä & Lassenius 分析了 759 个审查缺陷：75% 不影响可见功能，而是影响理解和修改。说明只盯「能不能跑」的审查员漏掉了审查的主要价值。[TSE 2009](https://mmantyla.github.io/wiki/What+Types+of+Defects+Are+Really+Discovered+in+Code+Reviews%253F.html)
- **审查的期望和实际不一致。** 微软研究发现开发者期待审查找 bug，但实际产出主要是代码改进和知识传递；审查最大的难点是「理解改动」。[Bacchelli & Bird, ICSE 2013](https://www.microsoft.com/en-us/research/publication/expectations-outcomes-and-challenges-of-modern-code-review/)
- **评论有没有用可以量化。** 微软对约 150 万条审查评论的研究按「是否引起代码改动」判定有用性，引用方报告约 1/3 评论无用；有经验的审查者有用率更高。[Bosu, Greiler, Bird, MSR 2015](https://www.microsoft.com/en-us/research/publication/characteristics-of-useful-code-reviews-an-empirical-study-at-microsoft/)
- **误报会让整套审查失去信任。** Google 的分析平台 Tricorder 给每条发现配「没用」按钮，并把「有效误报率」（开发者看到后没采取行动的比例）压在 10% 以下，超标的检查会被下线。[Sadowski et al., Tricorder, ICSE 2015](https://research.google/pubs/tricorder-building-a-program-analysis-ecosystem/)；[Lessons from Building Static Analysis Tools at Google, CACM 2018](https://cacm.acm.org/research/lessons-from-building-static-analysis-tools-at-google/)
- **好的缺陷报告的核心是复现步骤。** 对 Apache、Eclipse、Mozilla 开发者的调查：最需要的是复现步骤、堆栈和测试用例，最大的障碍是复现步骤不准。[Bettenburg/Zimmermann et al., TSE 2010](https://thomas-zimmermann.com/publications/files/zimmermann-tse-2010.pdf)
- **严重度要显式标注。** Google 要求把非阻塞意见标成 `Nit:`、`Optional:`、`FYI:`；Netlify 用「山 / 巨石 / 卵石 / 沙 / 尘」五级区分阻塞程度；Conventional Comments 用 `issue (blocking)`、`suggestion (non-blocking)` 等标签。[Google：How to write code review comments](https://google.github.io/eng-practices/review/reviewer/comments.html)；[Netlify Feedback Ladders](https://www.netlify.com/blog/2020/03/05/feedback-ladders-how-we-encode-code-reviews-at-netlify/)；[Conventional Comments](https://conventionalcomments.org/)
- **测试本身要被检验。** 变异测试：Google 在审查中展示存活的变异体，开发者据此补测试；约 1700 万个变异体、76 万次改动的规模化实践。[Practical Mutation Testing at Scale](https://research.google/pubs/practical-mutation-testing-at-scale-a-view-from-google/)
- **探索式测试有方法。** James Bach 的启发式测试策略模型（覆盖维度 SFDPOT：结构、功能、数据、平台、操作、时间）和基于会话的测试管理（每次限定时间、目标和记录）。[Heuristic Test Strategy Model](https://www.satisfice.com/download/heuristic-test-strategy-model)；[Session-Based Test Management](https://www.satisfice.com/download/session-based-test-management)

### 审查员 6 个维度的 1 / 3 / 5 分

**R1. 发现的层次**
- 1 分：只找表面问题（命名、格式、拼写），或只确认「测试通过」。
- 3 分：找到功能缺陷和边界遗漏，能对照规格判断「目的达不成」。
- 5 分：找到设计和风险层面的问题——接口会被谁依赖（Hyrum 定律）、迁移能否回滚、权限在哪一层最终执行、失败时用户看到什么、实现是否偷偷缩小了规格野心；格式类问题交给 lint，不占报告篇幅。

**R2. 可复现与证据**
- 1 分：「我觉得这里可能有问题」，没有复现。
- 3 分：每个失败项有复现步骤和证据位置。
- 5 分：复现最小化（一条命令或一个最小用例）；在指定提交上复现；把复现用例转成回归测试交给开发；区分「已复现 / 静态推断 / 无法判定」。

**R3. 严重度与优先级**
- 1 分：所有意见同等语气，开发不知道哪些必须改。
- 3 分：标注阻塞 / 非阻塞。
- 5 分：按「影响面 × 发生概率 × 可逆性」评级并给理由（参考 [Chromium 安全严重度指南](https://chromium.googlesource.com/chromium/src/+/HEAD/docs/security/severity-guidelines.md) 的分级写法）；误报率自查——过去被判「没用」的意见类型下次不再报（Tricorder 的 10% 原则）。

**R4. 改法的可执行性**
- 1 分：只说「这里不好」。
- 3 分：给出改法方向。
- 5 分：给出具体改法（必要时附补丁草案或测试用例），并说明替代方案的取舍；对设计问题给「最小可接受改法」和「理想改法」两档，让负责人决定。

**R5. 检验测试本身**
- 1 分：把测试通过当作正确。
- 3 分：挑关键防线手工改坏一次，确认测试会失败。
- 5 分：系统性变异检查（对改动行跑变异测试或逐条手工变异），报告存活变异体；检查桩是否掩盖真实行为、测试数据是否真实；指出缺失的测试类别（并发、旧数据、权限越界）。

**R6. 覆盖与独立性**
- 1 分：只看实施者给的截图和日志。
- 3 分：自己在真实入口跑，按状态清单逐项走。
- 5 分：先读规格再看实现；用风险模型（SFDPOT 或威胁建模 STRIDE）规划覆盖，说清「测了什么、没测什么、为什么」；有时间盒的探索式测试记录。

---

