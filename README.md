<div align="center">
  <img src="docs/images/logo.png" width="104" alt="Meowo logo" />
  <h1>Meowo / 喵呜</h1>
  <p><b>多开 AI 编程的桌面工作台。Claude Code、Codex、Kimi、Gemini CLI、OpenCode 的会话，都在一个地方跑。</b></p>
  <p>
    <a href="https://github.com/larrygogo/meowo/actions/workflows/ci.yml"><img src="https://github.com/larrygogo/meowo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="https://github.com/larrygogo/meowo/releases/latest"><img src="https://img.shields.io/github/v/release/larrygogo/meowo?label=release&color=d97757" alt="Release" /></a>
    <a href="https://github.com/larrygogo/meowo/releases"><img src="https://img.shields.io/github/downloads/larrygogo/meowo/total?color=4ec9a5" alt="Downloads" /></a>
    <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS-555" alt="Platform: Windows | macOS" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  </p>
  <p><b>中文</b> · <a href="README.en.md">English</a> · <a href="https://meowo.io">官网 meowo.io</a></p>
  <p>新建会话、看对话、批审批、查改动、开终端和浏览器，都在 Meowo 的对话窗里完成。<br/>屏幕顶部的灵动岛告诉你谁在跑、谁在等你；人不在电脑前，手机上也能接着推进。</p>
  <img src="docs/images/island.webp" alt="Meowo 灵动岛：收起时显示额度和每个会话的状态，会话要你批准时弹出提醒，点开看会话列表" width="760" />
</div>

## 下载

到 **[meowo.io](https://meowo.io)** 下载，页面会按你的系统给出对应的安装包。也可以直接去 [GitHub Releases](https://github.com/larrygogo/meowo/releases/latest) 下载：

| 平台 | 安装包 | 说明 |
|------|--------|------|
| **Windows 10 / 11** | `Meowo_x.y.z_x64-installer.exe` | 双击安装；没有 WebView2 的环境会自动换成经典安装界面 |
| **macOS 14+** | Apple 芯片选 `_aarch64.dmg`，Intel 选 `_x64.dmg` | 已签名公证。不确定芯片：苹果菜单 → 关于本机 |

不用提前装 AI CLI：打开 Meowo 后，在设置里就能一键安装、登录，所需的 hooks 也会自动接好。应用内可以检查并安装更新。

> **Windows 提示「Windows 已保护你的电脑」**：Windows 安装包暂时没有代码签名，点「更多信息」→「仍要运行」即可。
> 安装包由 GitHub Actions 从公开源码构建（见 [release.yml](.github/workflows/release.yml)），自动更新另有 minisign 签名校验。

## 能做什么

### 对话窗：会话在这里跑

<img src="docs/images/panel.png" alt="Meowo 对话窗：左侧是会话侧栏，中间是对话和一张待批准的命令审批卡，右侧面板打开着这次改动的 diff" width="760" />

- **结构化对话**：消息、工具调用、子任务各占一行，展开看完整输入输出；待办清单和进度实时刷新。
- **审批和提问变成按钮**：命令审批卡标出这次操作会碰到哪里（工作区外的路径、删文件、联网），允许 / 拒绝一点即发；Agent 的选择题直接在卡片上作答；终端里的信任确认、模型菜单也识别成按钮。
- **直接开新会话**：在对话窗选好 Agent 和目录，写下第一句话就开聊；可以带图片、截图后框出位置再发。
- **托管终端**：会话跑在 Meowo 托管的终端里，对话和终端是同一个会话的两种视图，随时切过去亲手操作。
- **右侧面板**：改动、文件、终端、浏览器放在同一排页签里。
  - 改动页看 diff，可以对某一行写评论，一键放进输入框发给 Agent。
  - 浏览器页能打开 localhost；claude、codex、kimi 会话可以自己操作这个浏览器，你看得到它的鼠标，随时能接管。
- **标题栏**：显示分支和 Pull Request，一键用编辑器打开项目。

### 灵动岛：状态常驻屏幕顶部

- 收起时显示额度环、每个会话一个头像：在跑的外圈转绿弧，等你的亮黄点。
- 会话要你批准、有提问、出错或跑完时，岛会展开几秒提醒你；点头像直接打开那个会话。
- 点开面板看最要紧的会话和各家额度，也能在这里新建会话。
- Windows 上可以左右拖动位置，或开极简模式只留一道细条；有刘海的 Mac 上岛包住刘海，没刘海时收进菜单栏。

### 手机远程：人不在电脑前也能推进

<img src="docs/images/mobile.png" alt="Meowo 手机远程页：在手机浏览器里看对话、处理命令审批" width="280" align="right" />

- 设置里开启「手机远程」，手机扫码就能用，不用装 App；可以添加到主屏幕。
- 在手机上看对话、发消息、**批审批**、答 Agent 的提问，也能新建和分叉会话、看终端画面和改动。
- 会话需要你时，可以经 [Bark](https://github.com/Finb/Bark) 推送到手机。
- 有会话在跑时电脑不睡；这个开关在手机上也能改。
- 局域网或 Tailscale 下可用，推荐只绑定 Tailscale 网卡。

<br clear="right" />

### 账号、额度与网络

- 每个 Agent 都能保存多个官方账号，一键切换；正在跑的会话也会跟着换。
- 没有官方账号时，可以按模型接入 API 中转。
- 显示 5 小时 / 7 天额度，以及 Opus 等模型单独的周额度，快用完时变红。
- 代理可以设全局默认，也能按 Agent 单独设；支持 HTTP、SOCKS5 和带认证的写法。

### 本地优先

会话数据和设置只存在本机的 `~/.meowo/`（SQLite）。Meowo 没有自己的服务器，不上传会话内容，断网也照常工作。你自己在终端里开的会话也会被收录进来。

## 工作原理

每家 CLI 用自己的 hook 机制把事件交给 `meowo-reporter`（OpenCode 没有 hook，改用一份自动生成的桥接插件），数据写进同一份本地数据库：

```
 AI CLI 会话（claude / codex / kimi / gemini / opencode）
   │  触发 hooks；claude 另有 statusline 提供 Context 用量
   ▼
 meowo-reporter（一次性命令行进程，写完就退出）
   ▼
 ~/.meowo/board.db（SQLite，WAL）
   ▲  文件监听，变了就刷新
 meowo-app（Tauri 桌面应用）
   ├─ 托管会话的终端（PTY），对话窗和终端看的是同一个会话
   └─ 手机远程开启时，内嵌一个只放行白名单接口的 HTTP 服务
```

启动时，Meowo 会为检测到的 CLI 自动接好 hooks：先备份，再原子写入，不破坏你已有的配置。想手动接入或排查问题，见官网的[手动接入说明](https://meowo.io/docs#manual)。数据和配置文件的位置见[数据与配置](https://meowo.io/docs#data)。

架构细节见 [`docs/architecture/`](docs/architecture/)。

## 从源码构建

需要 [Rust](https://rustup.rs/)（stable）和 [Bun](https://bun.sh/)，以及 [Tauri 前置依赖](https://tauri.app/start/prerequisites/)（Windows：WebView2 + MSVC 构建工具；macOS：Xcode 命令行工具）。

```bash
cd app
bun install
bun run tauri dev     # 开发模式，首次会编译 Rust，稍慢
bun run tauri build   # 打安装包，产物在 app/src-tauri/target/release/bundle/
```

测试与检查：

```bash
cd app && bunx tsc --noEmit && bun run test
cd app/src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test
```

代码结构：前端在 `app/src/`（React + TypeScript），Rust workspace 在 `app/src-tauri/`，下面有 `meowo-protocol`（跨进程契约）、`meowo-store`（SQLite）、`meowo-agent`（各家 Agent 的适配）、`meowo-reporter`（hook 上报）四个 crate；官网在 `site/`。README 的演示图由 `cd app && bun run demo:webp` 生成。

## 为什么叫 Meowo？

名字来自猫叫 **meow**，中文写作「喵呜」。

## License

[MIT](LICENSE) © larrygogo
