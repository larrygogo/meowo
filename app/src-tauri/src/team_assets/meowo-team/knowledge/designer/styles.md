<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 设计师：风格库（第 2 层）

同一个设计师能做出不同风格，靠的是这里的 8 种风格方向和每个项目一份「风格档」。进项目先读项目的风格档；没有就按 5.4 的流程先产出一份，用 2~3 个差异明显的高保真版本对照，用户定了再写入项目。

## 三、风格库（8 种）

说明：每种风格都给出可以落到 token 上的取值倾向。项目选定一种主风格后，可以从相邻风格借 1–2 个特征，但不要拼成大杂烩。

### 3.1 克制工具风（Refined Utility）

- **适用**：生产力工具、开发者工具、SaaS 控制台；面向专业用户，每天高频使用，需要长时间专注。
- **配色**：中性色占 90% 以上，冷灰或带极轻色相的灰；只有一个强调色，只用在主操作和选中态；深色模式是一等公民。
- **字体**：一套无衬线字体（系统字体或 Geist、Inter Display 一类），字号阶梯紧凑（12–14–16–20–24），字重 400/500/600；代码和数字用等宽字体。
- **圆角**：小，4–8px。
- **阴影/描边**：以 1px 低对比描边为主，阴影只用于浮层。
- **密度**：中高；行高 32–40px。
- **动效**：快，100–200ms；几乎不做装饰动效；键盘优先，提供 ⌘K 命令面板。
- **插画/图标**：线性图标，1.5px 线宽、16px 尺寸；基本不用插画。
- **代表**：[Linear](https://linear.app/)、[Vercel / Geist](https://vercel.com/geist/introduction)、Raycast。
- **翻车点**：
  - 灰得没有层次，所有东西一样重。
  - 对比度太低（浅灰次要文字不过 4.5:1）。
  - 照抄 Linear 的深色加紫光，看起来像 AI 模板。
- **最易混淆**：信息密集专业风。区别在于克制风追求「少」，密集风追求「多而可扫读」。

### 3.2 信息密集专业风（Dense Professional / Data-heavy）

- **适用**：后台管理、运营看板、交易和金融终端、数据分析；面向需要同时看大量数据的专家。
- **配色**：中性底色；语义色（涨跌、状态）系统化而且克制；图表用经过色盲测试的分类色板。
- **字体**：无衬线字体，正文 12–14px；数字一律使用 tabular-nums；表头小写或小号字配次要色。
- **圆角**：2–6px。
- **阴影/描边**：用描边和斑马纹或分隔线组织表格；基本不用阴影。
- **密度**：高，表格行高 28–36px；支持紧凑和舒适两种密度切换。
- **动效**：几乎没有，只保留状态变化的高亮闪烁。
- **插画/图标**：小号图标、状态点、迷你图（sparkline）。
- **代表**：Bloomberg Terminal、[Stripe Dashboard](https://stripe.com/)、Grafana、Airtable。
- **翻车点**：
  - 用营销页的大留白做后台，一屏只能看 5 行。
  - 状态全靠颜色（色盲用户看不出）。
  - 表格没有冻结列和排序。
  - 卡片化：把表格拆成卡片网格，反而无法比较。
- **最易混淆**：克制工具风（见上）。也容易被误做成通用的「后台模板风」（Ant Design 默认样式），缺少针对任务的取舍。

### 3.3 温暖消费风（Warm Consumer / Friendly）

- **适用**：生活方式、健康、记账、社交、教育类 C 端 App；面向普通人，偶发使用，需要被鼓励。
- **配色**：暖色或柔和色作底，主色饱和但不刺眼，常配辅助色块；大面积浅色底。
- **字体**：圆润或人文主义无衬线字体（例如 SF Rounded、Nunito、Circular 一类），标题字号大、字重粗。
- **圆角**：大，12–24px；按钮可用胶囊形。
- **阴影/描边**：柔和的大范围扩散阴影，或者无阴影的色块分层。
- **密度**：低，单列为主，每屏聚焦一件事。
- **动效**：有弹性的弹簧动画、庆祝动画（完成时撒彩纸），触感反馈丰富。
- **插画/图标**：自定义插画或角色、填充式图标。
- **代表**：Airbnb、Headspace、Monzo。
- **翻车点**：
  - 可爱过头显得幼稚。
  - 柔和色对比度不够。
  - 每屏都放插画，信息被淹没。
  - 千篇一律的 Corporate Memphis（扁平大手大脚的人物插画）。
- **最易混淆**：趣味游戏风。区别在于温暖风是「友好可信」，游戏风是「刺激好玩」。

### 3.4 趣味游戏风（Playful / Gamified）

- **适用**：游戏化学习、儿童产品、轻娱乐、社区活动页。
- **配色**：高饱和多色，对比强烈；每个功能或等级配一个颜色。
- **字体**：粗圆体或个性展示字体（例如 Duolingo 的 Feather Bold），标题极粗。
- **圆角**：大，并且常带「厚底」立体按钮（底部加 4px 深色边，按下时下沉）。
- **阴影/描边**：硬阴影（无模糊的偏移阴影）、粗描边。
- **密度**：低到中。
- **动效**：夸张、有挤压拉伸，有角色动画（Rive / Lottie）、音效和触感；奖励反馈强。
- **插画/图标**：角色 IP、徽章、进度条、连胜火焰。
- **代表**：Duolingo、Nintendo Switch 系统界面、Kahoot!。
- **翻车点**：
  - 动效太多导致操作慢，高频路径也在播放动画。
  - 颜色多而无系统，缺少语义。
  - 无障碍被忽略（闪烁、对比度、减弱动态效果）。
- **最易混淆**：温暖消费风（见上）；也容易和新粗野主义（neo-brutalism）混淆，后者是硬阴影加粗描边加原色，但带反叛和「土」的态度，不一定好玩。

### 3.5 编辑杂志风（Editorial）

- **适用**：内容、阅读、博客、媒体、文档、作品集；作品以文字为主角。
- **配色**：纸白或米白底，加墨黑文字，加一个点缀色；照片承担色彩。
- **字体**：衬线字体用于标题和正文（例如 Tiempos、GT Sectra、Newsreader；中文可用思源宋体），或者衬线配无衬线；字号对比极大（标题是正文的 3–5 倍）。
- **圆角**：0–2px。
- **阴影/描边**：细分隔线（hairline）、栏线，无阴影。
- **密度**：正文舒展，行宽严格控制在 60–75 字符或 30–40 个汉字；版面可以使用多栏不对称栅格。
- **动效**：极少，主要是滚动叙事。
- **插画/图标**：大幅摄影、编辑插画、首字下沉、引文块。
- **代表**：[The Pudding](https://pudding.cool/)、Medium（早期）、Stripe Press、The New York Times。
- **翻车点**：
  - 把杂志版式套到工具界面上，可操作性差。
  - 衬线字体在 Windows 小字号下发虚。
  - Anthropic 的 frontend-design skill 已把「报纸风（细线 + 零圆角 + 密集栏）」列为模型的新套路，题材不需要时不要用。
- **最易混淆**：品牌表现风。区别在于编辑风为阅读服务，品牌风为第一印象服务。

### 3.6 品牌表现风（Expressive Brand / Marketing）

- **适用**：官网、发布页、活动页、作品集首页；目标是在 5 秒内记住这个品牌。
- **配色**：大胆，一个主导色加尖锐的点缀色，可以用全屏色块、渐变或真实材质；明暗主题都可能使用。
- **字体**：个性展示字体加干净的正文字体；超大标题（80–200px），字重对比走极端（200 对 800）。
- **圆角**：按品牌定，关键是全站统一。
- **阴影/描边**：按品牌定；常用层叠、遮罩和 3D。
- **密度**：低，一屏只讲一件事。
- **动效**：一个精心编排的入场加滚动驱动的叙事；WebGL 或 3D 可以用在一处。
- **插画/图标**：定制插画、3D 渲染、产品实拍和录屏。
- **代表**：[Stripe 官网](https://stripe.com/)、Apple 产品页、Arc 浏览器官网。
- **翻车点**：
  - 套模板（hero 区大标题加渐变字加三列特性卡片），这正是「AI 味」的主要来源。
  - 动效过多，首屏性能差（LCP 慢）。
  - 卖点文案空洞。
  - Anthropic 的建议是「把大胆集中在一处」，其余部分保持克制。
- **最易混淆**：编辑杂志风（见上）、温暖消费风。

### 3.7 质感拟物风（Tactile / Material Depth）

- **适用**：创意工具、音频和硬件控制、高端个人工具、需要强烈「实体感」的 App；以及 2025 年后的 Apple 平台（Liquid Glass）。
- **配色**：带光影的中性色，色彩来自材质（金属、玻璃、纸、皮革）；高光和投影形成体积感。
- **字体**：系统字体或精致的无衬线字体，本身不抢戏。
- **圆角**：中到大，并使用连续曲率圆角。
- **阴影/描边**：多层阴影（一层环境光加一层接触阴影）、内阴影、顶部 1px 高光边、背景模糊（backdrop-filter）。
- **密度**：中。
- **动效**：物理感强，有弹簧、惯性、按下下沉，并配触感反馈。
- **插画/图标**：精细的高光图标、3D 图标。
- **代表**：Teenage Engineering 的软件界面、Apple iOS 26 Liquid Glass、Things 3（轻质感）。
- **翻车点**：
  - 玻璃拟态（glassmorphism）滥用，导致文字压在模糊背景上，对比度不稳定。
  - 阴影单层发灰，显得廉价。
  - 质感在深色模式下塌掉。
  - 性能问题（大面积模糊）。
- **最易混淆**：品牌表现风的 3D 元素；也容易和系统原生风混淆（Apple 自己就在用材质）。

### 3.8 系统原生风（Platform Native）

- **适用**：工具类桌面应用（Tauri）、iOS 工具 App、系统偏好类、需要「像系统自带」才可信的产品；面向希望零学习成本的用户。
- **配色**：直接使用系统语义色（iOS `label` / `systemBackground`、Windows 强调色、macOS accent），自动跟随深浅色和用户设定的强调色。
- **字体**：系统字体（SF Pro / Segoe UI Variable / Roboto），遵循各平台的字号阶梯和 Dynamic Type。
- **圆角**：完全使用平台默认值（Windows 8px 和 4px，iOS 连续圆角）。
- **阴影/描边**：系统材质（Mica、vibrancy、Liquid Glass）。
- **密度**：遵循平台惯例（macOS 紧凑，iOS 宽松）。
- **动效**：系统默认转场。
- **插画/图标**：SF Symbols / Fluent System Icons / Material Symbols。
- **代表**：Apple 备忘录 / 提醒事项、Windows 11 设置、[Tot](https://tot.rocks/)、Ivory。
- **翻车点**：
  - 一套 Web 样式跑三端，结果在每个平台上都显得「不对劲」。
  - 只做一个平台的规范（在 Windows 上放 macOS 红绿灯）。
  - 原生过头而没有品牌记忆点。
  - 做法：在原生骨架上用一个品牌色加一个签名式细节来建立辨识度。
- **最易混淆**：克制工具风。区别在于原生风跟随系统，克制风自己定义一套统一的克制语言。

### 3.9 风格选择速查

- 每天用几个小时、给专业用户 → 克制工具风或信息密集专业风。
- 给普通人、偶发使用、需要情绪价值 → 温暖消费风；面向孩子或需要强激励 → 趣味游戏风。
- 内容是主角 → 编辑杂志风。
- 要被记住的落地页 → 品牌表现风。
- 桌面或 iOS 小工具 → 系统原生风打底，加 1 个签名细节；需要实体感 → 质感拟物风。
- 一个项目可以分区使用不同风格（官网用品牌表现风，产品内用克制工具风），但这要写进项目约定，并共享同一套基础 token。

---


## 五、跨项目一致地专业、但各项目风格不同

### 5.1 结论

把「专业」放进**不随项目变的基础层**（刻度、状态、可访问性、动效时长、组件行为），把「风格」压缩成**少量品牌旋钮**，通过 token 分层把旋钮的值传到每个组件。AI 设计师进入新项目时，先读项目的风格档（旋钮取值），再套用自己的基础层，就能做到两者兼顾。

### 5.2 Token 三层（业界通行）

| 层 | 内容 | 例子 | 谁可以改 |
|---|---|---|---|
| Primitive / 原始层（reference） | 原始值，没有语义 | `blue-600: oklch(...)`、`space-4: 16px`、`radius-2: 8px` | 品牌（每个项目可以换） |
| Semantic / 语义层（system） | 用途，引用原始层 | `color.bg.surface`、`color.text.secondary`、`color.accent`、`color.danger`、`space.gap.inline` | 基础层定义名称，项目映射值 |
| Component / 组件层 | 组件专用，引用语义层 | `button.primary.bg`、`input.border.focus` | 组件库 |

- Material 3 的命名就是这三层：`md.ref.*`（参考色板）→ `md.sys.*`（系统角色，例如 `md.sys.color.primary`）→ `md.comp.*`（组件）（[M3 Design tokens](https://m3.material.io/foundations/design-tokens/overview)）。
- 格式标准：W3C Design Tokens Community Group 在 2025 年 10 月发布第一个稳定版 2025.10（Format + Resolver 模块），可以作为 token 文件格式（[公告](https://www.w3.org/community/design-tokens/2025/10/28/design-tokens-specification-reaches-first-stable-version/)、[规范](https://www.designtokens.org/tr/2025.10/)）。注意它不是 W3C 正式标准轨道。
- 规则：组件只能引用语义层或组件层，不能直接引用原始层。深色模式和品牌切换都只改「语义 → 原始」的映射。

### 5.3 品牌层与基础层分离：参考案例

- **Linear**：把每个主题需要定义的 98 个变量缩减为 3 个输入（底色、强调色、对比度），在 LCH 色彩空间中自动生成全部表面和文字色；高对比度主题也是自动生成的（[How we redesigned the Linear UI part II](https://linear.app/now/how-we-redesigned-the-linear-ui)）。
- **Radix Colors**：12 级色阶的每一级都有固定用途，换品牌色时组件不用改（[Understanding the scale](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale)）。
- **shadcn/ui**：组件只引用 `--background / --foreground / --primary / --muted / --border / --radius` 等语义 CSS 变量，换主题只换变量（[Theming](https://ui.shadcn.com/docs/theming)）。这也是 AI 味的来源之一：变量不换，所有项目就长一个样。
- **Material 3 动态配色**：从一个种子色用 HCT 色彩空间生成整套色调色板和角色色（[M3 Dynamic color](https://m3.material.io/styles/color/dynamic/overview)）。
- **Stripe**：在感知均匀的色彩空间里构建色阶，保证各色相同级对比度一致，可访问性由系统兜底（[Designing accessible color systems](https://stripe.com/blog/accessible-color-systems)）。

### 5.4 给 AI 设计师的落地建议：「风格旋钮」

**基础层**（第 1 层功底，跨项目固定，写在设计师自己的知识库中）：
- 间距刻度（4px 基数）、字号阶梯算法、语义色角色名、状态层规则、焦点样式规则、动效时长和缓动档位、点击区、对比度门槛、各平台差异表。

**风格档**（每个项目一个文件，放在项目约定中，只填旋钮取值）：
1. 风格方向：从第三节 8 种中选一种主风格，最多借用一种副风格的 1–2 个特征。
2. 色彩：底色、强调色、对比度（Linear 式三输入），以及中性灰的冷暖倾向和语义色。
3. 字体：西文和中文字体、正文字号（14/15/16）、字号对比强度（温和或极端）。
4. 圆角：一个基础值（0/4/8/12/16），其余按刻度推出；外圆角和内圆角同心。
5. 密度：紧凑、标准、宽松（影响行高、控件高度、间距倍率）。
6. 层级表达：描边、阴影、色调还是材质。
7. 动效性格：利落（100–200ms、无弹性）、柔和（200–300ms、ease-out）或弹性（spring）。
8. 图形语言：图标库及线宽、插画风格（或明确不用）。
9. 语气：专业简洁、亲切口语或俏皮。
10. 签名元素：唯一大胆的一处是什么。
11. 禁用清单：本项目明确不要的东西（例如「不用渐变」）。

**进项目的流程**：
1. 读项目约定和风格档；没有风格档就先产出一份，用 2~3 个差异明显的高保真版本对照，确定后写入。
2. 用基础层加风格档生成 token。
3. 设计。
4. 用第一节清单加第四节症状清单自查。
5. 按第二节模板交付。

**成长**：用户的纠正按层归档。违反硬规范的写进第 1 层；对某个项目风格的修正写进该项目的风格档；跨项目都成立的审美偏好写进用户的品味清单（个人层，由 team-learn 记录）。

---


## 出处汇总

**规范与标准**
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/)
- [Apple Human Interface Guidelines](https://developer.apple.com/design/human-interface-guidelines)
- [Material 3](https://m3.material.io/)
- [Fluent 2](https://fluent2.microsoft.design/)
- [Windows app design（圆角、Mica、触控目标）](https://learn.microsoft.com/windows/apps/design/)
- [W3C clreq 中文排版需求](https://www.w3.org/TR/clreq/)
- [中文文案排版指北](https://github.com/sparanoid/chinese-copywriting-guidelines)
- [Design Tokens 2025.10](https://www.designtokens.org/tr/2025.10/)

**方法与原则**
- [Refactoring UI](https://www.refactoringui.com/)
- [Nielsen 10 Heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/)
- [Laws of UX](https://lawsofux.com/)
- [NN/g Design Critiques](https://www.nngroup.com/articles/design-critiques/)
- [Discussing Design](https://www.oreilly.com/library/view/discussing-design/9781491902394/ch02.html)
- [IDEO I like, I wish](https://www.ideo.com/blog/build-your-creative-confidence-i-like-i-wish)
- [Figma design crit](https://www.figma.com/solutions/design-crit/)

**一线团队实践**
- [Vercel Web Interface Guidelines](https://vercel.com/design/guidelines)
- [Linear UI redesign](https://linear.app/now/how-we-redesigned-the-linear-ui)
- [Stripe accessible color systems](https://stripe.com/blog/accessible-color-systems)
- [Radix Colors](https://www.radix-ui.com/colors)
- [Rauno: Interaction design details](https://rauno.me/craft/interaction-design)

**AI 味**
- [Anthropic Cookbook: Prompting for frontend aesthetics](https://platform.claude.com/cookbook/coding-prompting-for-frontend-aesthetics)
- [Anthropic: Improving frontend design through Skills](https://claude.com/blog/improving-frontend-design-through-skills)
- [anthropics/skills frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design)
- [prg.sh: Why your AI keeps building the same purple gradient website](https://prg.sh/ramblings/Why-Your-AI-Keeps-Building-the-Same-Purple-Gradient-Website)

**待核实**
- Windows 触控目标的具体数值（表中写的约 40×40epx）。
- macOS 控件的具体尺寸。
- Figma 内部「6 种 crit 形式」的原文（未能取到原文）。

这几项用在硬性验收前，先打开官方页面确认。

