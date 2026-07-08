# macOS 菜单栏「红绿灯」状态图标

## 背景与动机

macOS 菜单栏当前的状态图标由 `app/src-tauri/src/macos/menubar.rs` 的 `render_status_rgba`
把预渲染字形图集（`icons/menubar-glyphs.bin`）拼成一张 **单色模板图**：
`↻N`（`arrow.triangle.2.circlepath` + 运行数）后跟 `✋M`（`hand.raised.fill` + 待交互数）。
用户反馈这套符号在菜单栏小尺寸下观感差（发虚、符号不好认），希望改成
**类似托盘「红绿灯」的彩色状态点 + 数字**（绘制形状，不用 emoji）。

## 目标

- 菜单栏图标改为 **彩色实心圆点 + 数字**，颜色沿用应用自身状态色语言：
  - 运行中：橙 `#d97757`
  - 待交互：黄 `#fbbf24`
- 计数信息保留（一个橙点跟运行数、一个黄点跟待交互数）。
- 全空闲（运行 0、待交互 0）时 **保持现状**：回落单色三柱 app logo（模板图，随明暗自动反色）。
- 视觉档位取 mockup 的「A 实心点中号」。

## 非目标（YAGNI）

- 不改 Windows 托盘（Windows 仍是静态 logo + 悬浮提示文案，无红绿灯）。
- 不新增第三种状态色（在线绿仅用于将来可能的空闲态，本次空闲回落 logo，不画绿点）。
- 不改右键菜单、不改点击行为、不改计数来源（`tray_running` / `tray_waiting` 照旧）。

## 视觉规格（对应 mockup 变体 A）

以 24pt 逻辑高度为基准、按 2x 渲染（Retina 清晰）：

| 元素 | 规格 |
|---|---|
| 圆点直径 | GLYPH_H=36 画布下约满高（`DOT_D=32`，`DOT_W=34`）；真机目视调定 |
| 点 → 其数字 间隔 | `SYM_NUM_GAP=5` |
| 数字之间 | `DIGIT_GAP=1` |
| 运行组 → 待交互组 间隔 | `PAIR_GAP=16` |
| 数字字体 | 复用图集内 SF 字形，定高 36 |
| 计数上限 | 最多两位；超过 99 显示 `99+`（`+` 为运行时用前景色绘制的合成字形） |

- 顺序：运行组在前，待交互组在后。
- 只有一种状态非零时只画那一组；两者都为零时回落 logo。
- 点用各自固定颜色；数字与 `+` 用「菜单栏前景色」（见明暗自适应）。

## 明暗自适应（关键技术点）

彩色图标 **不能用模板模式**（`set_icon_with_as_template(_, false)`），因此 macOS 不再自动反色，
数字颜色必须自己跟随 **系统菜单栏明暗**：暗栏白字、亮栏黑字。

- 判定信号用 **系统外观**（`NSApp.effectiveAppearance` best-match `.darkAqua` / `.aqua`），
  而非应用内主题设置——菜单栏文字跟随系统 Dark/Light，与 app 的深浅主题无关。
- 监听系统主题切换（`AppleInterfaceThemeChangedNotification`，DistributedNotificationCenter）
  在切换时用「上次计数」重渲染。
- 点颜色不随明暗变化（橙/黄在暗亮栏都可辨）。
- 空闲态回落的 logo 仍走模板模式（`as_template = true`），由系统自动反色，无需特殊处理。

## 实现要点（altitude：函数级，落在 menubar.rs）

1. **拆纯函数**：保留/微调 `status_seq(running, waiting)` 的「组序列」纯逻辑，便于单测；
   把「符号字形」替换为「彩色圆点」这一表现层差异下沉到渲染函数。
2. **渲染函数**改造 `render_status_rgba(running, waiting, dark: bool) -> Option<(rgba, w, h)>`：
   - 圆点：Rust 直接光栅化填充圆（RGB = 点色，A = 圆形抗锯齿掩码）。
   - 数字：复用图集里现有的 `0-9` alpha 字形（`GLYPH_ATLAS` 前 10 个），
     RGB 取前景色（白/黑随 `dark`），A 取字形 alpha。
   - `↻`/`✋` 两个符号字形（下标 10/11）**不再使用**（图集文件保留不动，避免重生成）。
3. **更新入口** `update_tray_status`：
   - 非空闲 → 传彩色图，`as_template = false`。
   - 空闲 → 现状 logo，`as_template = true`。
   - 需要拿到当前系统 `dark` 布尔；并把「上次计数」存起来（供主题切换观察者重渲染）。
4. **主题观察者**：注册一次系统外观变更通知，回调里用存下来的计数重渲染。
   （app 已在多处用 objc/私有 API，新增此观察者与既有风格一致。）

## 测试

- **单测**（纯逻辑，跨平台）：`status_seq` 的组序列（只运行 / 只待交互 / 两者 / 全零→空）。
- **像素级健全性**（可选、macOS 下）：`render_status_rgba` 输出的宽高、非零 alpha 覆盖，
  以及 `dark=true/false` 下数字像素 RGB 分别偏白/偏黑。
- **真机目视**：暗/亮菜单栏各看一次；切换系统 Dark/Light 观察数字反色；
  计数 0→有→0 观察彩色点与 logo 的切换。

## 风险 / 待确认

- **数字明暗自适应的时机**：若观察者注册失败或漏事件，最坏情况是切换系统主题后数字颜色短暂不匹配
  （下次计数变化即自愈）。可接受。
- **非 Retina 屏**（如本机）圆点直径取整后的观感：mockup 已在实际尺寸下确认可辨。
