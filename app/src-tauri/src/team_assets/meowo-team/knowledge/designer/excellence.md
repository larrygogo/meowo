<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 设计师：卓越标准（上限）

craft.md 守下限（不达标就是问题），这份管上限：按维度看这次交付离「业界最好团队」还差多远。
- 打分的是审查员，不是干活的人自己：研究和实测都显示自评稳定偏高。干活的人交付前对照它自查、把明显的差距补上，但不给自己打分。
- 每一分都要有证据（测试、截图、复现命令、文件行号），没有证据按低一档算。
- 任何维度 ≤ 2 分打回；原创性和整体一致单独计分并加权，否则产出会收敛到 AI 模板感。
- 来源：业界公开做法调研（各维度附出处）。

## 三、设计师的卓越标准（7 个维度）

### 业界怎么评价「卓越」

- **Apple Design Awards 2025** 的 6 个类别：Delight and Fun（愉悦与趣味）、Innovation（创新）、Interaction（交互）、Inclusivity（包容）、Social Impact（社会影响）、Visuals and Graphics（视觉与图形）。2025 年交互类获奖的是淘宝。[Apple Newsroom](https://www.apple.com/newsroom/2025/06/apple-unveils-winners-and-finalists-of-the-2025-apple-design-awards/)；[ADA 官网](https://developer.apple.com/design/awards/)
- **Anthropic 的设计评审 4 个维度**（生成—评审双 agent 实验）：设计质量（「是一个整体还是一堆零件」）、原创性（「有没有自己的决定，还是模板、库默认值和 AI 套路」）、工艺（排版层级、间距一致、配色和对比度——大部分合理实现默认能过）、功能（不看美观的可用性）。**前两项加权更高**，才把产出推向更大胆。[Anthropic：Harness design for long-running application development](https://www.anthropic.com/engineering/harness-design-long-running-apps)
- **Linear**：质量是业务核心；「质量不需要变慢」；为创作者做工具、拒绝无意义的忙碌、先简单再强大。[Linear Method](https://linear.app/method)；[Why is quality so rare](https://linear.app/now/why-is-quality-so-rare)；[Karri Saarinen 的 10 条规则](https://www.figma.com/blog/karri-saarinens-10-rules-for-crafting-products-that-stand-out/)
- **Stripe**：把可访问性做成系统能力（自建感知均匀的色彩系统，保证任意组合的对比度），而不是逐屏修补。[Designing accessible color systems](https://stripe.com/blog/accessible-color-systems)
- **Dieter Rams 十条**：创新、有用、美观、易懂、不张扬、诚实、耐久、细节周全、环保、尽可能少的设计。[Vitsœ：Good design](https://www.vitsoe.com/us/about/good-design)
- **IDEO**：好的方案同时满足「用户想要（desirability）、技术可行（feasibility）、商业可持续（viability）」。[IDEO U](https://www.ideou.com/blogs/inspiration/how-to-prototype-a-new-business)
- **Don Norman 的情感设计三层**：本能层（第一眼）、行为层（用起来顺）、反思层（用完愿意认同和推荐）。《Emotional Design》(2004)。
- **交互细节派**：Rauno Freiberg「交互设计里看不见的细节」、Vercel Web Interface Guidelines——卓越往往体现在按下反馈、手势连续性、动效时机这类细节。[rauno.me](https://rauno.me/craft/interaction-design)；[Vercel guidelines](https://vercel.com/design/guidelines)
- **度量**：Google HEART（愉悦、参与、采用、留存、任务成功）把「好不好」接到数据上。[Rodden et al. 2010](https://research.google/pubs/measuring-the-user-experience-on-a-large-scale-user-centered-metrics-for-web-applications/)

### 7 个维度的 1 / 3 / 5 分

**G1. 整体性（一个整体，还是一堆零件）**
- 1 分：组件库默认样式拼起来；每屏风格各异。
- 3 分：配色、字体、间距成体系，一致但没有个性。
- 5 分：颜色、字体、版式、细节共同形成可辨认的气质；遮住 logo 也认得出是谁家的产品（Linear、Stripe 的公认水平）。出处：Anthropic 评审「设计质量」维度；Apple「Visuals and Graphics」。

**G2. 原创性与产品特点**
- 1 分：通用「列表 + 卡片」外壳；看得出 AI 模板套路（紫色渐变、千篇一律的英雄区）。
- 3 分：有若干定制决定，但核心界面换个产品也能用。
- 5 分：界面形态来自这个产品自己的核心任务（例如记账 App 的首页直接就是「记一笔」这件事，而不是通用的列表加卡片）；至少有一个让人记住的设计决定，并能说出它服务哪个用户目标。出处：Anthropic「原创性」；Apple「Innovation」。自查两个问题：第一眼哪里让人眼前一亮？明天为什么还想打开？

**G3. 信息层级与清晰（易懂、尽可能少）**
- 1 分：所有元素一样重；多个主按钮；靠堆标签解释。
- 3 分：每屏一个主操作；层级用字重、颜色、尺寸拉开（craft.md 已有）。
- 5 分：做过减法后无可再减（Rams「尽可能少的设计」）；模糊测试下第一眼就落在主任务；数字和状态自己会说话，不需要说明文字。

**G4. 交互与反馈（行为层）**
- 1 分：点了没反应或反应慢；弹层没有出口；状态缺失。
- 3 分：所有状态齐全，100ms 内有反馈，失败可回退。
- 5 分：交互有物理感和连续性（手势可中断、动效跟手、过渡说明空间关系）；先给结果再兜失败（乐观更新）；键盘、触控都顺手。出处：Apple「Interaction」；Rauno Freiberg；Vercel guidelines；[web.dev RAIL](https://web.dev/articles/rail)。

**G5. 工艺与细节**
- 1 分：对齐错位、字号杂乱、深色模式没看过。
- 3 分：过完 craft.md 全部【硬】项。
- 5 分：放大到 200% 截图逐像素看也没有瑕疵：圆角内外一致、光学对齐、等宽数字、中英文间距、动效曲线和时长统一；深浅两种模式都像是专门设计的，而不是反色。出处：Rams「细节周全」；Anthropic「工艺」；Stripe 色彩系统。

**G6. 包容与可访问**
- 1 分：只在设计师自己的屏幕和默认字号下成立。
- 3 分：对比度、字号放大、键盘可达达到 WCAG AA。
- 5 分：把可访问性做进系统（色彩 token 保证任何组合达标、Dynamic Type 下布局仍好看）；考虑不同用户和权限组合（只开了部分功能、离线和在线两种模式、多人合用一个账号）。出处：Apple「Inclusivity」；Stripe 色彩系统；[WCAG 2.2](https://www.w3.org/TR/WCAG22/)。

**G7. 愉悦与反思层（明天为什么还想打开）**
- 1 分：能用，但没有任何情绪。
- 3 分：顺手，没有让人烦的地方。
- 5 分：至少一处恰到好处的愉悦（完成时的反馈、空状态的文案、一个巧妙的默认值），而且不打扰核心任务；用户愿意截图分享。出处：Apple「Delight and Fun」；Norman 反思层；Kano 模型的「魅力质量」。

---

