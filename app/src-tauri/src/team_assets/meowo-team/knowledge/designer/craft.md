<!-- 由 Meowo 安装和管理。升级会覆盖本文件；要长期保留的改动写进个人层 {{PERSONAL_DIR}}。 -->
# 设计师：专业功底（第 1 层）

跨项目通用，换任何风格都不能丢。交付前按第一节逐条自查，标【硬】的不达标就是客观问题，必须修。来源：业界公开规范与一线团队实践调研（各条附出处，见文末）。

## 一、专业功底自查清单

每条都写成能直接判断「是 / 否」的形式。【硬】表示有标准出处、违反就算客观问题；【建议】表示业界的强共识，可以有项目级例外，但例外要写进项目约定。

### 1.1 信息层级

- [ ] 【硬】每屏只有一个主操作（primary button），其余操作降级为次要、文字按钮或收进菜单。（Refactoring UI「Not every button needs a background」；Apple HIG 按钮样式）
- [ ] 【硬】层级同时靠字重、颜色深浅和尺寸拉开，不能只靠放大字号。次要信息用更浅的文字色（例如 secondary/tertiary 文字 token），不要只缩小字号。（Refactoring UI「Hierarchy is everything」「Size isn't everything」）
- [ ] 【建议】先做减法：能省的标签就省（例如「邮箱：」加地址，直接写地址本身就够了），数据自己能说明含义时不加 label。（Refactoring UI「Labels are a last resort」）
- [ ] 【建议】眯眼测试或模糊测试：把截图模糊到看不清字，第一眼落点应该是主任务，而不是装饰、插画或侧栏。
- [ ] 【硬】同一类信息在全产品里位置、样式一致（Nielsen 启发式第 4 条「一致性和标准」）。
- [ ] 【建议】列表和表格中，最常扫读的列放左侧，数字列右对齐并使用等宽数字（`font-variant-numeric: tabular-nums`）。
- [ ] 【建议】不靠堆分隔线做分组：优先用间距，其次用背景色差，最后才用描边。（Refactoring UI「Use fewer borders」；格式塔接近原则）

出处：[Refactoring UI](https://www.refactoringui.com/)（书 + 作者推文合集）、[Nielsen 10 条启发式](https://www.nngroup.com/articles/ten-usability-heuristics/)、[Laws of UX](https://lawsofux.com/)（接近律 Law of Proximity、冯·雷斯托夫效应 Von Restorff）。

### 1.2 排版（西文）

- [ ] 【硬】正文字号：Web ≥ 16px；iOS 默认 Body 为 17pt；Windows 正文为 14px（Segoe UI Variable）；Android Body Large 为 16sp。（[Apple HIG Typography](https://developer.apple.com/design/human-interface-guidelines/typography)、[Fluent 2 Typography](https://fluent2.microsoft.design/typography)、[M3 Type scale](https://m3.material.io/styles/typography/type-scale-tokens)）
- [ ] 【硬】移动 Web 的 `<input>` 字号 ≥ 16px，否则 iOS Safari 会自动放大页面。（[Vercel Web Interface Guidelines](https://vercel.com/design/guidelines)）
- [ ] 【硬】用户可以放大到 200% 且不丢内容（WCAG 1.4.4）；iOS 支持 Dynamic Type，不能写死字号。
- [ ] 【建议】使用固定的字号阶梯（type scale），全产品不超过 6–8 档。行宽 45–75 字符，最多约 80 字符。（Bringhurst《The Elements of Typographic Style》；Anthropic frontend-design skill）
- [ ] 【建议】行高：正文 1.4–1.6，标题 1.1–1.3，字号越大行高越小。
- [ ] 【硬】WCAG 1.4.12 文本间距：用户把行高调到 1.5 倍、段距 2 倍、字距 0.12em、词距 0.16em 时，布局不能坏。
- [ ] 【建议】最多用两套字体；字重最多 3 档（例如 400/500/600 或 400/600/700）。
- [ ] 【建议】文案细节：用弯引号 “ ”、单字符省略号 …、数字和单位之间加不换行空格（10&nbsp;MB），计数用阿拉伯数字。（Vercel 指南）

### 1.3 中文排版（重点）

- [ ] 【硬】中文与英文、中文与阿拉伯数字之间加一个半角空格，例如「在 GitHub 上提交 3 次」。数字与单位之间加空格（「10 GB」），但度数和百分比不加（「90°」「15%」）。全角标点与其他字符之间不加空格。如果用了 CSS `text-autospace`（新属性，各浏览器支持度不一，用前先查 caniuse），就不要再手动加空格，否则会出现双倍间距。（[中文文案排版指北](https://github.com/sparanoid/chinese-copywriting-guidelines)）
- [ ] 【硬】中文语境下使用全角标点（，。！？：；「」），不混入半角逗号和句号。完整的英文句子或专有名词内部保留半角标点。不重复使用标点（不写「！！」「？？」）。（同上）
- [ ] 【硬】专有名词按官方大小写书写（GitHub、iOS、macOS、JavaScript），不写成 Github、IOS、Mac OS。（同上）
- [ ] 【建议】中文正文行高 1.6–1.8，比西文大，因为汉字是满格方块，行间需要更多留白。标题行高 1.3–1.4。（参考 [W3C 中文排版需求 clreq](https://www.w3.org/TR/clreq/)）
- [ ] 【建议】中文正文行长约 25–40 个汉字。超过 45 字，读者换行时容易找错行。
- [ ] 【硬】中文不使用斜体（中文字体没有真正的斜体，浏览器生成的伪斜体很难看）。强调用加粗、颜色或着重号（`text-emphasis`）。
- [ ] 【建议】中文正文不要加大 `letter-spacing`。大标题可以轻微调整（±0.02em 以内）。大量中文正文段落可以两端对齐（`text-align: justify`），但含长英文单词的窄栏要避免使用。
- [ ] 【硬】字体栈中西文字体写在前，中文字体写在后，让西文和数字落到西文字体上，例如 `font-family: "Inter", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", "Noto Sans SC", sans-serif`（Inter 只是示例，具体字体按风格档选）。
- [ ] 【建议】字重要看平台实际可用档位：微软雅黑只有 Light/Regular/Bold，写 500/600 在 Windows 上会落到 Regular 或 Bold。需要中间字重时，用思源黑体 / Noto Sans SC 等可变字重字体，或者接受只用两档。
- [ ] 【建议】中文最小字号 12px，正文 14–16px（桌面密集界面可以用 13–14px）。中文 12px 以下在 Windows 上会糊。
- [ ] 【建议】标点挤压：连续标点和行首标点可以用 CSS `text-spacing-trim` 处理，避免出现大片空白；`line-break: strict` 保证句号、逗号不出现在行首（避头尾）。
- [ ] 【建议】中文按钮文案 2–4 个字，动词开头（「保存」「新建项目」），不写「点击这里」。

出处：[中文文案排版指北](https://github.com/sparanoid/chinese-copywriting-guidelines)、[W3C Requirements for Chinese Text Layout](https://www.w3.org/TR/clreq/)。

### 1.4 间距与栅格

- [ ] 【硬】所有间距取自同一套刻度：4px 基数（4/8/12/16/24/32/48/64/96…），越往上间隔越大，不用 13px、17px 这种随手值。（Refactoring UI「Establish a spacing system」；M3 使用 4dp 网格，组件按 8dp；Windows 使用 4epx 基数）
- [ ] 【建议】先给过多留白，再往回收。组内间距 < 组间间距，这条决定分组是否清晰。（Refactoring UI「Start with too much white space」；接近律）
- [ ] 【硬】子元素圆角 ≤ 父元素圆角，嵌套圆角满足：外圆角 = 内圆角 + 内边距（同心）。（Vercel 指南）
- [ ] 【建议】做视觉对齐而不是数学对齐：图标、三角箭头、圆形头像要按视觉中心微调 ±1px。（Vercel 指南）
- [ ] 【硬】响应式至少检查手机、笔记本、超宽屏三个宽度；WCAG 1.4.10 要求 320 CSS px 宽时内容不出现横向滚动。
- [ ] 【建议】内容区设最大宽度，超宽屏不要把正文拉满。
- [ ] 【硬】iOS 和刘海屏考虑安全区（`env(safe-area-inset-*)` / SafeAreaView）。

### 1.5 色彩与对比度（WCAG 2.2 具体数值）

- [ ] 【硬】正文文字对比度 ≥ 4.5:1（AA，1.4.3）；大字（≥ 24px 常规，或 ≥ 18.66px 粗体）≥ 3:1。AAA 级分别为 7:1 和 4.5:1（1.4.6）。
- [ ] 【硬】非文字元素（输入框边框、图标、焦点环、图表线条、复选框）与相邻颜色的对比度 ≥ 3:1（1.4.11）。
- [ ] 【硬】不只用颜色传达信息（1.4.1），例如错误状态要同时有图标或文字，图表要同时有形状或标签。
- [ ] 【硬】placeholder 和禁用态以外的灰色文字也要过 4.5:1。最常见的翻车是浅灰次要文字放在浅灰底上。
- [ ] 【建议】每个色相做 9–12 级色阶，并规定每一级的用途（Radix 的 12 级：1–2 背景，3–5 组件底色和悬停/按下，6–8 边框，9–10 实色，11–12 文字）。（[Radix Colors scale](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale)）
- [ ] 【建议】在感知均匀的色彩空间（OKLCH/LCH）里生成色阶，保证不同色相在同一亮度下看起来一样亮。（[Linear 重设计 part II](https://linear.app/now/how-we-redesigned-the-linear-ui)、[Stripe: Designing accessible color systems](https://stripe.com/blog/accessible-color-systems)）
- [ ] 【建议】彩色背景上不要放灰色文字，改用同色相、降低饱和度的文字色。（Refactoring UI「Don't use grey text on colored backgrounds」）
- [ ] 【建议】主色只用在需要注意的地方；大面积用中性色。主色占比参考 60-30-10。
- [ ] 【建议】Vercel 推荐用 APCA 辅助判断对比度（APCA 对深色模式更准确），但合规仍以 WCAG 2.x 为准。

出处：[WCAG 2.2](https://www.w3.org/TR/WCAG22/)、[Understanding 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)、[Understanding 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html)。

### 1.6 交互状态（每个可交互组件都要覆盖）

- [ ] 【硬】每个可交互元素都有默认、悬停（桌面）、按下、焦点、禁用五态；选中、拖拽、加载态按组件需要补齐。交互态的对比度要比静止态高。（Vercel 指南；[M3 States](https://m3.material.io/foundations/interaction/states/overview)：悬停 8%、焦点 10%、按下 10%、拖拽 16% 的状态层不透明度）
- [ ] 【硬】焦点：用 `:focus-visible` 显示清晰的焦点环（WCAG 2.4.7）；焦点不能被吸顶栏或浮层遮住（2.4.11，AA）；焦点环建议 ≥ 2px、对比度 ≥ 3:1（2.4.13，AAA）。
- [ ] 【硬】加载：
  - 等待 0.1s 内无需反馈；1s 内保持用户的思路不中断；超过 10s 要显示进度和可取消。（[Nielsen 响应时间三阈值](https://www.nngroup.com/articles/response-times-3-important-limits/)）
  - 加载指示延迟 150–300ms 再出现，出现后至少显示 300–500ms，避免闪烁。（Vercel 指南）
  - 加载中的按钮保留原文案，并加 spinner 或「保存中…」。
  - 能预知形状的内容用骨架屏，未知时长的用进度提示。
  - 系统 400ms 内响应能让用户保持专注。（[Doherty 阈值](https://lawsofux.com/doherty-threshold/)）
- [ ] 【硬】空状态：说明这里会出现什么，并给出第一步操作（按钮）。首次使用、清空后、搜索无结果三种空状态要分别设计。
- [ ] 【硬】错误状态：写清发生了什么、怎么修，不道歉、不甩锅（Nielsen 第 9 条）。表单提交出错时，焦点跳到第一个错误，错误紧贴字段显示（WCAG 3.3.1）。
- [ ] 【硬】禁用态：说明为什么不可用（tooltip 或旁边的说明）。表单不要预先禁用提交按钮，让用户点了看到校验结果。（Vercel 指南）
- [ ] 【建议】破坏性操作优先提供撤销，而不是二次确认弹窗（Nielsen 第 3 条「用户控制与自由」）；不可撤销的才确认，并在确认按钮上写明动作（「删除 3 个项目」）。
- [ ] 【建议】乐观更新：低风险操作立即反映在界面上，失败再回滚并提示。
- [ ] 【硬】长列表和长内容的极端值都要测过：超长名字、0 条、1 条、1 万条、缺图、网络断开。

### 1.7 动效原则

- [ ] 【硬】动效必须有目的：表达因果（这是从哪儿来的）、空间关系、状态变化。纯装饰的入场动画最多一处。（Anthropic frontend-design skill；[M3 Motion](https://m3.material.io/styles/motion/overview)）
- [ ] 【建议】时长：微交互（按钮、开关）100–200ms；小面板和菜单 200–300ms；全屏转场 300–500ms；UI 动效一般不超过 400–500ms。进入用 ease-out（减速），离开用 ease-in（加速），离开比进入快。
- [ ] 【建议】移动端和原生端优先用弹簧（spring）动画，以便可以中途打断。iOS 原生就是弹簧；M3 Expressive（2025）也转向基于弹簧的 motion physics。
- [ ] 【硬】尊重 `prefers-reduced-motion` / iOS「减弱动态效果」：位移和缩放改为淡入淡出或直接切换。（WCAG 2.3.3）
- [ ] 【硬】自动播放超过 5 秒的动效要提供暂停（WCAG 2.2.2）；不能每秒闪烁 3 次以上（2.3.1）。
- [ ] 【硬】Web 只对 `transform` 和 `opacity` 做动画；不写 `transition: all`。（Vercel 指南）
- [ ] 【建议】高频操作（每天点几百次的）不加动效，或者把动效压到 100ms 以内，否则会让人觉得慢。（[Rauno Freiberg: Invisible Details of Interaction Design](https://rauno.me/craft/interaction-design)）

### 1.8 可访问性（上面没覆盖到的部分）

- [ ] 【硬】点击区：Web 最低 24×24 CSS px（WCAG 2.5.8 AA），推荐 44×44（2.5.5 AAA）；iOS 44×44pt（[HIG](https://developer.apple.com/design/human-interface-guidelines/accessibility)）；Android 48×48dp（[M3](https://m3.material.io/foundations/designing/structure)）；图标视觉尺寸小于这个值时扩大热区。
- [ ] 【硬】只有图标的按钮要有可读名称（`aria-label` / `accessibilityLabel`）；图片有替代文本，装饰图标 `aria-hidden`。
- [ ] 【硬】所有功能都能用键盘完成，Tab 顺序和视觉顺序一致，弹窗打开时把焦点困在弹窗内（focus trap），Esc 关闭，关闭后焦点回到触发按钮。
- [ ] 【硬】不禁用浏览器缩放，输入框不禁止粘贴。（Vercel 指南）
- [ ] 【硬】语义元素优先（button、a、label、原生 select），不用 div 模拟按钮。
- [ ] 【建议】用屏幕阅读器（VoiceOver / NVDA）把主流程走一遍。

### 1.9 深浅色模式

- [ ] 【硬】颜色全部走 semantic token（`bg/surface/text/border/accent`），组件里不写死色值，深色模式只换 token 的值。
- [ ] 【硬】深色模式不是简单反色：背景用深灰而不是纯黑（M3 推荐 #121212 一类的深色面；iOS 有 base / elevated 两套背景），层级越高表面越亮（不靠阴影表达高度）；饱和色要降低饱和度、提高亮度，否则会刺眼和「振动」。（[M3 Color roles](https://m3.material.io/styles/color/roles)、[Apple HIG Dark Mode](https://developer.apple.com/design/human-interface-guidelines/dark-mode)）
- [ ] 【硬】深色下重新检查所有对比度，包括图表、图标、焦点环、阴影（深色下阴影基本看不见，要改用描边或亮度差）。
- [ ] 【硬】Web 在 `<html>` 上设 `color-scheme`，并让 `<meta name="theme-color">` 跟随背景；Windows 上原生 `<select>` 要显式设置背景色和文字色。（Vercel 指南）
- [ ] 【建议】默认跟随系统，同时提供手动切换；切换时不能先闪一下白屏（FOUC）。
- [ ] 【建议】图片和插画准备深色版本，或者降低亮度；Logo 要有反白版。

### 1.10 各端平台规范差异（Web / 桌面 Tauri / iOS·RN）

先说原则：**跨平台产品保持品牌一致，但交互惯例跟随各平台**。这是 Apple、Google、Microsoft 三家规范共同的立场。

| 维度 | iOS（Apple HIG） | Android（Material 3） | Windows（Fluent 2） | macOS（HIG） | Web |
|---|---|---|---|---|---|
| 最小点击区 | 44×44pt | 48×48dp | 触控约 40×40epx（≈7.5mm）；鼠标可更小 | 无硬性要求，控件常见 20–28pt 高 | 24px（AA），触屏 44px |
| 系统字体 / 正文 | SF Pro，Body 17pt，Dynamic Type | Roboto / Roboto Flex，Body 14–16sp | Segoe UI Variable，Body 14px | SF Pro，Body 13pt | 自选；正文 ≥16px |
| 圆角 | 连续曲率圆角（squircle），大卡片 10–20pt；iOS 26 起的 Liquid Glass 控件更圆 | 形状刻度 0/4/8/12/16/28/full | 顶层容器（窗口、对话框、浮层）8px，页内控件 4px | 窗口 10pt 左右，控件 5–6pt | 自定 |
| 层级表达 | 材质（模糊）+ 层叠 | 色调高度（tonal elevation，表面越高颜色越带主色） | 材质 Mica（窗口底）/ Acrylic（浮层） | 材质 vibrancy | 阴影 / 描边 |
| 导航 | 底部 Tab Bar（≤5 项）+ 左上返回 + 右滑返回 | 底部导航栏 / Navigation Rail / 系统返回 | 左侧 NavigationView / 顶部 Tab | 侧边栏 + 工具栏 + 菜单栏 | 顶栏 / 侧栏 |
| 主操作位置 | 导航栏右侧文字按钮 | FAB（悬浮按钮） | 命令栏 | 工具栏右侧；对话框主按钮在右 | 视情况 |
| 对话框按钮顺序 | 取消在左，确认在右 | 确认在右 | 主按钮在左（Windows 惯例：确定 / 取消） | 主按钮在最右 | 跟随目标用户的主力系统 |
| 光标 | — | — | 按钮默认箭头光标，不用手形 | 同左，原生按钮不用手形光标 | 链接和按钮用 pointer |

出处：[Apple HIG](https://developer.apple.com/design/human-interface-guidelines)、[Material 3](https://m3.material.io/)、[Fluent 2](https://fluent2.microsoft.design/)、[Windows 圆角规范](https://learn.microsoft.com/windows/apps/design/style/rounded-corner)、[Windows 材质 Mica](https://learn.microsoft.com/windows/apps/design/style/mica)、[Windows 触控目标](https://learn.microsoft.com/windows/apps/design/input/guidelines-for-targeting)。（Windows 触控目标的数值以官方页面为准，本表是摘要。）

**桌面 Tauri 应用的专项清单（最容易被做成「网页套壳」）：**
- [ ] 标题栏：macOS 的红绿灯按钮在左，Windows 的窗口按钮在右；自绘标题栏要留出拖拽区（`data-tauri-drag-region`）和系统按钮区。
- [ ] 界面外壳（按钮、侧栏、标签）禁止文本选中（`user-select: none`），内容区可以选中；按钮不显示手形光标。
- [ ] 所有高频操作都有快捷键（macOS 用 ⌘，Windows 用 Ctrl），并在菜单和 tooltip 中显示；有原生菜单栏（macOS）和右键菜单。
- [ ] 密度高于移动端：行高 28–36px，控件高度 24–32px。
- [ ] 跟随系统强调色和深浅色；窗口最小尺寸要定义，并测试在最小尺寸下的布局。
- [ ] 失去焦点的窗口：选中态改为灰色（macOS 惯例）。

**iOS / React Native 专项：**
- [ ] 使用系统手势（右滑返回、下拉刷新、长按菜单）；不用 Web 式 hover。
- [ ] 键盘弹出时输入框不被遮挡；输入框类型正确（邮箱、数字键盘）。
- [ ] 触感反馈（haptics）只用在确认、切换、出错这些关键时刻。
- [ ] 图标优先用 SF Symbols（iOS）风格，粗细与旁边的文字字重对应。
- [ ] RN 跨端时，导航和弹层用各平台的原生样式（iOS 用 sheet，Android 用 bottom sheet + 系统返回）。

### 1.11 文案与微文案

- [ ] 【硬】用用户的话，不用系统和工程术语（Nielsen 第 2 条「系统与现实世界匹配」）。错误信息里不出现错误码原文，错误码放在「详情」里。
- [ ] 【硬】按钮写具体动作（「删除项目」「发送邀请」），不写「确定 / 是」；同一动作在整个流程里用同一个词（按钮写「发布」→ 提示写「已发布」）。（Anthropic frontend-design skill；[Apple HIG Writing](https://developer.apple.com/design/human-interface-guidelines/writing)）
- [ ] 【硬】错误信息三要素：发生了什么、为什么（可选）、下一步怎么做；不道歉，不责怪用户。
- [ ] 【建议】英文标题用 sentence case（产品界面）；中文不说「您好！欢迎使用」之类的空话，空状态写下一步行动。
- [ ] 【建议】数字与金额格式全局统一（小数位数统一用 0 位或 2 位），日期使用相对时间加悬停显示绝对时间。
- [ ] 【建议】确认弹窗标题直接写后果（「删除后无法恢复」），不写「确定吗？」。
- [ ] 【建议】营销腔一律删掉：「赋能」「一站式」「无缝」「Unlock the power of」「Seamless」。

### 1.12 两个通用框架（评审时用来对照）

**Nielsen 10 条可用性启发式**（[来源](https://www.nngroup.com/articles/ten-usability-heuristics/)）：系统状态可见、匹配现实世界、用户控制与自由、一致性与标准、预防错误、识别优于回忆、灵活高效、美观简约、帮助用户识别和恢复错误、帮助与文档。严重度用 0–4 打分（[Severity Ratings](https://www.nngroup.com/articles/how-to-rate-the-severity-of-usability-problems/)）：0 不是问题，1 外观问题，2 小问题，3 大问题（必须修），4 灾难（发布前必须修）。

**Laws of UX 中最常用的几条**（[lawsofux.com](https://lawsofux.com/)）：
- Fitts 定律：常用、重要的目标要大、要近。
- Hick 定律：选项越多决策越慢，所以要分步或给默认值。
- Jakob 定律：用户习惯其他产品的做法，所以不要在通用交互上标新立异。
- 美即好用效应：好看的界面会被认为更好用，但这也会掩盖可用性问题，所以评审时不能只看截图。
- 峰终定律：着重设计流程的高峰时刻和结束时刻（完成页、成功反馈）。
- Tesler 定律：复杂度守恒，系统多承担一点，用户就少承担一点。
- Doherty 阈值：响应时间控制在 400ms 以内。
- 米勒定律常被误用：「7±2」不是菜单项上限的依据。

---


### 2.3 区分「客观问题」和「风格偏好」的判定规则（给 AI 设计师用）

按顺序判断：

1. **违反第一节【硬】条目、WCAG、平台规范或 Nielsen 启发式** → 客观问题，必须修，并标严重度 0–4。
2. **不违反硬规范，但能用目标或证据说明它妨碍了任务**（例如主操作不显眼导致首屏转化路径变长；用户测试或截图模糊测试能证明）→ 有依据的设计问题，应该修，并写明依据。
3. **违反项目约定（第 4 层）或用户品味（第 3 层）的明文条目** → 必须修，并注明是哪一条。
4. **只能说「我觉得 X 更好看」，拿不出目标或证据** → 风格偏好。可以作为备选方向记下，但不推翻当前方案；如果用户认可，再沉淀进用户的品味清单或项目约定。

**交付时的自评模板**（每次出稿附上）：
- 本稿目标：用户是谁、要完成什么任务、成功的样子。
- 选用风格档及理由（引用风格库条目和项目约定）。
- 客观自查：第一节清单的未通过项，以及已修复项。
- 取舍说明：哪些地方是风格判断，为什么这样选，对照的业界产品是哪个。
- 想要的反馈类型：方向、细节，还是只做可用性检查。

---


## 四、AI 生成界面的「AI 味」问题与对策

### 4.1 成因

Anthropic 官方的说法是：模型「默认选择安全的选项」，会收敛到「分布上最常见」的输出，也就是所谓的「AI slop」美学（[Cookbook: Prompting for frontend aesthetics](https://platform.claude.com/cookbook/coding-prompting-for-frontend-aesthetics)、[Improving frontend design through Skills](https://claude.com/blog/improving-frontend-design-through-skills)）。训练数据里大量是 Tailwind 默认值加 shadcn 模板加落地页模板。坊间常举的例子是 Tailwind UI 早年默认用 `bg-indigo-500`，这被认为是「AI 紫」的来源之一（[Why Your AI Keeps Building the Same Purple Gradient Website](https://prg.sh/ramblings/Why-Your-AI-Keeps-Building-the-Same-Purple-Gradient-Website)）。

### 4.2 典型症状（评审时逐条查）

**配色**
- 白底配紫色或靛蓝渐变；渐变文字标题。
- 颜色分布平均、没有主导色（Cookbook 原话：「timid, evenly-distributed palettes」）。
- 第二轮套路（来自新版 frontend-design skill 明确列出的）：奶油米色底配赤陶色点缀；近黑底配一个荧光绿或朱红点缀；用 #0B0B0B、#111 之类的近黑当黑色。

**字体**
- Inter、Roboto、Arial、Open Sans、Lato、系统字体一把梭。换字体之后，又收敛到 Space Grotesk。
- 标题中只把一个词斜体、加粗或变色。
- 全大写的小标签、标题上方的「眉标」（eyebrow）、不必要的 label。

**版式与组件**
- 「SaaS 卡片套件」：一模一样的圆角卡片、全站一个圆角值、同一种柔和灰阴影、装饰性渐变光晕。
- Hero 区居中大标题、两个按钮、下面三列特性卡（每张卡都是一个圆角方块图标加标题加两行字）、再接一排 logo 墙和价格三栏。
- 模板化装饰：中点分隔的元信息串（「5 分钟 · 设计 · 2025」）、「单词 — 片段」式标签、等宽字体的数据标签、在每个链接和按钮后面加 →。
- 无意义的序号（01 / 02 / 03），内容并非顺序关系。
- 所有东西居中对齐。

**图标与插画**
- 用 emoji 当图标（🚀 ✨ 📊）；图标尺寸和线宽与文字不匹配。
- 所有图标都放在一个带浅色底的圆角方块里。
- 玻璃拟态和模糊光斑背景随处可见。

**动效**
- 每个区块都淡入上滑；每张卡片都有 hover 放大。

**内容**
- 假数据和假社会证明（「10k+ 用户信赖」「99.9% 可用」）。
- 营销空话（「Unlock」「Seamless」「Elevate」「赋能」）。
- 只有成功态，没有加载、空、错误态。

### 4.3 对策（官方做法 + 可执行规则）

1. **先计划再动手**（frontend-design skill）：写下题材、受众、主要任务，然后给出一份简短计划，包含 4–6 个具名 hex 的色板、字体、版式和原则。对照上面的症状清单检查计划里有没有默认选择，有就替换，然后再写代码。
2. **点名禁用默认值**（Cookbook）：在系统提示或项目约定里显式写出「不用 Inter/Roboto/Arial、不用白底紫渐变、不用千篇一律的卡片网格」。官方博客的说法是，仅告诉模型避开 Inter 和 Roboto，效果就会立刻改善。
3. **按维度给具体指令**（Cookbook 四个维度：字体、配色、动效、空间构图）。字体要走极端对比，字重用 100/200 对 800/900（不用 400 对 600），尺寸跳 3 倍以上（不用 1.5 倍）；配色用一个主导色加尖锐点缀；动效只做一次编排好的入场；背景要营造氛围（多层渐变、几何纹理），不要默认纯色。
4. **大胆只花在一处**（skill 原话：「Spend your boldness in one place」）：一个签名元素，其余全部克制。
5. **结构元素必须承载信息**：分隔线、编号、标签、边框都要回答「它告诉用户什么」，答不上来就删掉。
6. **引用灵感而不是复制**：让模型参考 IDE 主题、某种文化美学或具体的业界产品，并用主题约束（例如「Solarpunk」）锁定方向，避免每次生成都回到均值。
7. **多版本加截图自评**：做完截图，对照症状清单和同类最好产品自评再修改（这就是团队流程里「每次出 2~3 版并排」的原因）。
8. **质量底线不能丢**：响应式、可见焦点、减弱动态效果、对比度达标（skill 明确列出）。
9. **图标用成套图标库**（Lucide / Phosphor / SF Symbols / Material Symbols / Fluent Icons），统一尺寸和线宽；禁止用 emoji 当功能图标。
10. **风格档要落成文件**：项目的风格档（见第五节）写死字体、色板和圆角，模型每次都从文件读，而不是临场「发挥」。

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

