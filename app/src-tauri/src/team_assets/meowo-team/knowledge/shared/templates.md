<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 工程化模板

所有角色的交付都按这些栏目写，不同项目里做出来的东西长得一样，用户看起来快。栏目不适用的写「不适用」，不要删栏目。业界参考附在后面。

## 团队交付说明模板（开发必用，设计和审查按需取用栏目）

```
# <一句话：做了什么，结果如何>

## 结论（三句以内）
## 为什么做 / 对应规格和验收项
## 考虑过的做法                    ← 必填：关键选择至少两种做法，写被否掉的那种和理由（设计两次）
## 改了什么                        ← 提交号；整理（重命名、搬代码）和行为改动分开提交，分别列出
## 怎么验证的                      ← 命令和结果、截图路径；至少一处变异检查（改坏哪里、哪个测试失败）
## 运维与回滚                      ← 必填：上线后怎么判断正常（看哪个日志、哪个指标）、出问题怎么回滚、数据迁移能否回退
## 没验证什么 / 剩余风险           ← 推理时意识到、但这次没处理的问题（例如「这个竞态还会出现」）必须写在这里
## 需要谁确认什么                  ← 设计师、用户各要定的事，附推荐答案
```

（「考虑过的做法」「运维与回滚」两栏必填：上限评分里最常见的打回原因，就是没写被否掉的方案、没写怎么判断正常和怎么回滚。）

## 业界参考

### 1. PR / 改动描述

- **Google CL 描述**：第一行一句话说做了什么（祈使句、独立成句）；空一行；正文写为什么、背景、取舍、局限、参考链接。[Writing good CL descriptions](https://google.github.io/eng-practices/review/developer/cl-descriptions.html)
- **常见 PR 模板栏目**（GitHub 官方模板机制 + 业界常见组合）：变更摘要、动机/关联需求、改动类型、怎么验证（命令、截图）、风险与回滚、检查清单、审查重点。[GitHub：PR templates](https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/creating-a-pull-request-template-for-your-repository)
- **Conventional Commits**：`<type>(<scope>)!: <description>`，正文，脚注（`BREAKING CHANGE:`、`Refs:`）；type 常用 feat、fix、docs、refactor、perf、test、build、ci、chore。[conventionalcommits.org v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/)
- **Conventional Comments**（审查评论）：`<label> [decorations]: <subject>`，label 有 praise、nitpick、suggestion、issue、todo、question、thought、chore、note，decoration 标 blocking / non-blocking / if-minor。[conventionalcomments.org](https://conventionalcomments.org/)

### 2. 设计文档 / RFC

- **Google 设计文档**：背景与范围；目标与非目标；设计（系统上下文图、API、数据存储、关键代码或伪代码、约束程度）；考虑过的替代方案；横切关注点（安全、隐私、可观测）。建议长度 10~20 页，小改动用 1~3 页的「迷你设计文档」。[Design Docs at Google](https://www.industrialempathy.com/posts/design-docs-at-google/)
- **Rust RFC**：Summary、Motivation、Guide-level explanation（给使用者看的讲解）、Reference-level explanation（实现细节）、Drawbacks、Rationale and alternatives、Prior art、Unresolved questions、Future possibilities。[0000-template.md](https://github.com/rust-lang/rfcs/blob/master/0000-template.md)
- **Kubernetes KEP**：Summary；Motivation（Goals / Non-Goals）；Proposal（用户故事、注意事项、风险与缓解）；Design Details（**Test Plan、毕业标准、升级/降级策略、版本偏差策略**）；**生产就绪审查问卷**（如何开启与回滚、上线与监控、依赖、扩展性、排障）；Drawbacks；Alternatives。KEP 是「把可运维写进设计」做得最完整的公开模板。[KEP template](https://github.com/kubernetes/enhancements/blob/master/keps/NNNN-kep-template/README.md)
- **ADR（架构决策记录）**：标题、状态、背景、决策、后果。一页纸，记录「为什么这么定」。[Michael Nygard 2011](https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
- **Oxide RFD**：带状态机（prediscussion → ideation → discussion → published → committed / abandoned），讨论在 PR 里进行。[RFD 1](https://rfd.shared.oxide.computer/rfd/0001)
- **Amazon PR/FAQ**：先写对外新闻稿（客户、问题、方案、引言、如何开始）再写 FAQ（对外 / 对内），倒推产品。[Working Backwards](https://www.workingbackwards.com/resources/working-backwards-pr-faq)

### 3. 测试计划

- **IEEE 829（经典结构）**：测试计划标识、范围与被测项、要测和不测的特性、方法、通过/失败判据、暂停与恢复条件、交付物、环境、职责、进度、风险与应急。[Software test documentation（IEEE 829 概述）](https://en.wikipedia.org/wiki/Software_test_documentation)
- **Google ACC（属性—组件—能力）**：列产品属性（如「安全」「快」）× 组件，在交叉格里写「能力」，按风险（频率 × 影响）上色，据此分配测试。出自《How Google Tests Software》。[ACC 说明](https://code.google.com/archive/p/test-analytics/wikis/AccExplained.wiki)
- **KEP 的 Test Plan 栏**：先说明前置测试改动，再分单元 / 集成 / e2e 列出测试和链接，加上毕业标准。
- **基于会话的探索测试**：每次会话写章程（测什么、用什么资源、找什么风险），时间盒 60~120 分钟，产出笔记、问题、缺陷和覆盖比例。[SBTM](https://www.satisfice.com/download/session-based-test-management)

### 4. 上线与运维

- **Google SRE 上线检查清单**：架构、容量、故障模式、客户端行为、流程与自动化、开发流程、外部依赖、上线计划。[Launch Checklist](https://sre.google/sre-book/launch-checklist/)
- **复盘模板**：摘要、影响、根因、触发因素、处理过程、检测方式、行动项（类型、负责人）、经验（做得好的、做得不好的、侥幸之处）、时间线。[Example Postmortem](https://sre.google/sre-book/example-postmortem/)

---

