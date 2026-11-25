<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 抬高上限的机制（证据）

为什么团队这样运作：哪些做法被证明能抬上限、证据有多强。来源：公开研究与一线团队实践调研。

## 四、「抬高上限」的机制证据

证据等级：**A** = 对照实验或元分析；**B** = 有数据但条件特殊（基准任务、单一实验）；**C** = 经验之谈或案例。

### 1. 多候选再选优（生成 N 个，用可靠信号挑）

- 【A/B】**Self-Consistency**：同一推理问题采样多条推理路径投票，GSM8K 上比贪心解码 +17.9 个百分点（PaLM-540B 56.5%→74.4%）。[Wang et al., ICLR 2023](https://arxiv.org/abs/2203.11171)
- 【B】**CodeT**：让模型生成测试，用「候选代码 × 测试」的一致性挑代码，HumanEval pass@1 从 47.0% 提到 65.8%。[Chen et al., ICLR 2023](https://arxiv.org/abs/2207.10397)
- 【B】**Large Language Monkeys**：采样越多，「至少一个对」的覆盖率持续上升，但**没有自动检验器时，挑选环节会成为瓶颈**。[Brown et al. 2024](https://arxiv.org/abs/2407.21787)
- 【A】**人做设计：平行原型**：33 人对照实验，同时做多个原型再迭代的一组，在所有指标（含真实点击率）上胜过串行迭代组，方案更多样，对批评更开放。[Dow et al., TOCHI 2010](https://hci.stanford.edu/publications/2010/parallel-prototyping/ParallelPrototyping2010-submitted.pdf)（注：2022 年工程设计领域的复现结果不一致）
- 【A】**给评审者看多个方案**：只看一个设计时，用户打分偏高、不愿批评；并排看三个，批评更多更具体。[Tohidi, Buxton et al., CHI 2006](https://www.microsoft.com/en-us/research/publication/getting-the-right-design-and-the-design-right-testing-many-is-better-than-one/)
- **对我们的含义**：设计师的「2~3 版并排」有实证支持；开发的关键方案也应至少设计两次。挑选必须靠外部信号（测试、截图对照、评分标准），否则多候选的收益会被挑选环节吃掉。

### 2. 独立评审（生成者和评审者分开）

- 【B】**自我修正不靠谱**：没有外部反馈时，模型修正推理答案的效果不升反降；早期报告的提升多依赖「知道答案对不对」的标签。[Huang et al., ICLR 2024](https://arxiv.org/abs/2310.01798)
- 【B】**模型偏爱自己的输出**：LLM 评审存在自我偏好、位置偏好、长度偏好。[Zheng et al., MT-Bench, NeurIPS 2023](https://arxiv.org/abs/2306.05685)；[Panickssery et al. 2024](https://arxiv.org/abs/2404.13076)
- 【C】**Anthropic 实践**：agent「给自己打分稳定偏高」；拆出独立评审并调成怀疑态度，比让生成者自我批评容易得多。开箱即用的 Claude 是弱 QA：会找到真问题再说服自己放行。调法是读评审日志，找它和人判断分歧的地方，改提示。复古游戏编辑器的例子：单 agent 20 分钟 9 美元，核心功能不能用；完整流程 6 小时 200 美元，能玩。[Anthropic 工程博客](https://www.anthropic.com/engineering/harness-design-long-running-apps)
- 【A】**人的代码审查**：主要价值在可演进性（75% 的发现），并起到知识传递作用。[Mäntylä & Lassenius 2009](https://mmantyla.github.io/wiki/What+Types+of+Defects+Are+Really+Discovered+in+Code+Reviews%253F.html)；[Bacchelli & Bird 2013](https://www.microsoft.com/en-us/research/publication/expectations-outcomes-and-challenges-of-modern-code-review/)
- **对我们的含义**：现有「开发不给自己验收」的规矩方向正确；还要补两点——评审先读规格再看实现，评审提示要定期用「人和评审分歧的案例」来校准。

### 3. 评分标准（rubric）自评与按维度评审

- 【A】**人**：评分标准对学业成绩的元分析效应 g=0.45（21 项研究，已校正发表偏倚）；对自我调节和自我效能的效应小且不确定。[Panadero et al., Educational Psychology Review 2023](https://link.springer.com/article/10.1007/s10648-023-09823-4)
- 【B】**LLM**：给定细粒度评分标准时，评审模型和人工打分的相关系数达到 0.897（GPT-4 为 0.882）。说明「写清每档分长什么样」能让 AI 评审接近人的判断。[Prometheus, ICLR 2024](https://arxiv.org/abs/2310.08491)
- 【C】**Anthropic**：把「好设计」写成 4 个可打分的维度，并加权原创性和整体性，是把产出推出「AI 模板感」的关键；迭代 5~15 轮，分数上升后趋平，作者有时更喜欢中间某一轮。
- 【C】**先约定「做完」的标准**：生成者和评审者在动手前先商定这一段的验收标准（Anthropic 的 sprint contract，一个阶段有 27 条判据），弥合规格和可测实现之间的差距。
- **对我们的含义**：rubric 由评审方打分最有效；生成方用 rubric 自查是辅助，不能代替独立评分。rubric 每档要写成可观察的描述（本文件第一到三节就是这种写法）。

### 4. 对照范例（few-shot exemplars）

- 【A】**大模型少样本学习**：给几个范例就能显著改变输出形式和质量。[Brown et al., GPT-3, 2020](https://arxiv.org/abs/2005.14165)
- 【C】**用范例校准评审**：Anthropic 用「带分项打分拆解的范例」校准评审 agent，减少了跨轮次的打分漂移，使评审和作者品味对齐。
- 【A】**反面证据：设计固着**：给设计者看范例，会让他们不自觉地照抄范例的特征，包括范例里的缺陷。[Jansson & Smith, Design Studies 1991](https://doi.org/10.1016/0142-694X(91)90003-F)
- **对我们的含义**：范例主要给**评审**用（校准「5 分长什么样」）；给生成者看范例时要多样、只讲原理，并要求「不得复刻其版式」，避免固着。

### 5. 带外部反馈的自我修正与反思

- 【B】**Self-Refine**：同一模型生成→给反馈→改写，7 个任务平均提升约 20 个百分点（绝对值）。[Madaan et al., NeurIPS 2023](https://arxiv.org/abs/2303.17651)
- 【B】**Reflexion**：把执行结果（如测试失败）转成文字反思存进记忆，HumanEval pass@1 91%（同期 GPT-4 为 80%）。[Shinn et al., NeurIPS 2023](https://arxiv.org/abs/2303.11366)
- 注意和第 2 条的冲突：Self-Refine 的收益在推理题上不稳定，Huang 等人指出提升多依赖外部信号。**结论：反思有效的前提是有真实反馈（测试、运行结果、截图、人的纠正）。**

### 6. 经验库与复盘

- 【A】**人：复盘（debrief）**：46 个样本的元分析，复盘让个人和团队绩效平均提升约 25%（d=0.67），在真实和模拟场景、医疗和非医疗领域效果相近。[Tannenbaum & Cerasoli, Human Factors 2013](https://pubmed.ncbi.nlm.nih.gov/23516804/)
- 【B】**AI：工作流记忆**：从过往任务中提炼可复用的工作流存入记忆，Mind2Web 相对提升 24.6%、WebArena 相对提升 51.1%。[Agent Workflow Memory, ICML 2025](https://arxiv.org/abs/2409.07429)；同类工作 [ExpeL](https://arxiv.org/abs/2308.10144)、[Voyager 技能库](https://arxiv.org/abs/2305.16291)。
- **对我们的含义**：个人层的 lessons.md 和品味清单就是经验库；要提高命中率，每条经验要写成「触发场景 → 做法 → 反例」，并在开工时按场景检索，而不是全文通读。

### 7. 清单（守下限的对照组）

- 【A】WHO 手术安全清单：8 家医院，主要并发症 11.0%→7.0%，死亡率 1.5%→0.8%。证明清单降低失误，但不提高「最好能做到多好」。[Haynes et al., NEJM 2009](https://www.nejm.org/doi/full/10.1056/NEJMsa0810119)
- 【A】变异测试是「检验测试的清单」能升级为质量信号的例子：开发者接触后写更多、更好的测试。[Petrović et al. 2021](https://arxiv.org/abs/2103.07189)

### 机制效果汇总（按证据强弱）

- 有实证：多候选+可靠检验、并排展示多方案、按维度的评分标准、复盘、外部反馈驱动的修正、变异测试。
- 有数据但依赖条件：LLM 自我修正（需外部信号）、经验库（在网页任务上验证）、LLM 评审（有自我偏好）。
- 经验之谈：Anthropic 的生成—评审流程（单个案例，没有对照）、范例校准评审、预先约定验收标准、Linear/Stripe 的质量文化。

---


## 六、怎么用到团队里（建议，供主会话取舍）

- 每个角色加 `excellence.md`：直接用本文件的 D1~D8、R1~R6、G1~G7，每档写成可观察的描述；craft.md 不动（仍是下限）。
- 审查员按维度给开发和设计打分，附证据；任何维度 ≤ 2 分打回；设计的 G1、G2 加权（参照 Anthropic 的做法）。
- 建 `exemplars/`：每个维度挑 1~2 个公开的 5 分案例（如 KEP、Stripe 迁移、Linear 界面）写「为什么是 5 分」，主要给审查员校准用；给设计师看时要求只学原理、不复刻版式。
- 关键方案「设计两次」；界面「2~3 版并排」，挑选靠评分标准和截图对照，不靠生成者自评。
- 每次用户验收后做 5 分钟复盘，把分歧写进 lessons.md（格式：触发场景 → 做法 → 反例），下次开工按场景检索。
- 定期抽查评审：拿用户的判断和审查员的打分比对，分歧大的案例用来改评审提示（Anthropic 的调法）。

