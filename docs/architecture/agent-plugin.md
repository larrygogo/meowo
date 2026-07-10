# Agent 插件架构

> meowo 提供能力,agent 声明自己用哪些。加一个 agent = 新增 `plugins/<id>/` 一个目录 + 注册表一行。

## 为什么要改

现状下,「一个 agent 是什么」这件事被切成五份,分住四个 crate:

| 碎片 | 位置 | 管什么 |
|---|---|---|
| `ProviderKey` 枚举 | `meowo-store/models.rs` | 身份 |
| `AgentPlugin` | `meowo-agent/plugins/*.rs` | 变体 / 路径 / hooks 规格 / 鉴权 |
| `Agent` | `meowo-reporter/agent.rs` | 进程名 / resume / stop 正文 / context / rename / 安装脚本 |
| `ProviderSetup` | `app/setup/mod.rs` | 接线副作用(amend / after_write) |
| `ProviderAccount` | `app/account/mod.rs` | 账号 / 用量 |
| `PROVIDERS` 表 | `app/src/providers.tsx` 等 | 展示名 / 图标 / 品牌色 |

四个 trait **每个都自带 `key()` 和自己的 `ALL` 注册表**,再靠三个 "enum↔registry" 绊线单测把碎片钉在一起。
测试能钉住,恰恰说明它们本该是一个东西 —— 这不是抽象,是同一个抽象被复制了四遍。

具体病灶:

- `ProviderKey::from_str` 未知降级为 `Claude`。历史 DB 里任何未知 provider 的会话会被**静默改写**成 claude。
- `lib.rs` 的 `install_for` / `repair_provider_hooks` 是两处硬 `match`,靠编译错误提醒补 arm。
- `About.tsx` 的 `provider === "claude"` 特判 + `styles.css` 的 `--cc-claude`:claude 的品牌色被写进了框架。
- `setup::Amend` / `AfterWrite` 已经是 per-agent 的函数指针,却住在 app 的 setup 模块而非插件里。

## 三条原则

1. **身份是字符串,不是枚举。** 唯一身份类型是 `meowo_agent::AgentId`。`meowo-store` 不认识任何具体
   agent,provider 列存原样字符串;未知 id 原样保留,绝不降级。加 agent 不再需要动 DB 层。

2. **能力是槽位,不是方法。** 一个 `AgentPlugin` 通过能力查询暴露自己支持什么,不支持的返回 `None`,
   由框架降级 —— 而不是让每个 agent 实现十几个方法、其中一半返回 `false`。

3. **IO 是注入的端口,不是直接依赖。** 插件层保持纯声明 + 纯函数。要联网 / 读 keychain / 原子写盘的
   能力,通过宿主注入的端口完成。于是账号、接线这些「重」能力也住得进插件,而插件 crate 依然不依赖
   tauri、不依赖 reqwest。

## 目标形态

```
meowo-agent/
  src/
    api/                  能力 trait + 端口 trait + 数据类型。零 IO 依赖
      caps.rs             AgentPlugin + 各能力 trait
      ports.rs            HttpPort / FsPort / KeychainPort
    plugins/
      claude/
        mod.rs            身份 + 变体表 + 能力装配
        account.rs        impl AccountCap —— 只用注入的 HttpPort
        setup.rs          impl WiringCap —— amend(statusLine)
        telemetry.rs      impl TelemetryCap —— transcript 标题解析
      kimi/ ...
      codex/ ...
    registry.rs           static ALL —— 加 agent 只在此补一行
```

`meowo-store` / `meowo-reporter` / `meowo-app` 都只消费同一张注册表。app 负责在启动时注入端口实现
(reqwest / keychain / 原子写),reporter 注入一套最小实现。

### 能力槽位

```rust
pub trait AgentPlugin: Sync {
    // ── 必填:身份 ──
    fn id(&self) -> AgentId;
    fn descriptor(&self) -> &'static Descriptor;   // 展示名 / 品牌色 / 图标 SVG
    fn variants(&self) -> &'static [Variant];      // 变体表(路径/hooks 规格/鉴权/启动)

    // ── 选填:能力。不支持返回 None,框架降级 ──
    fn launch(&self)    -> Option<&dyn LaunchCap>    { None }  // resume/launch argv、安装脚本
    fn wiring(&self)    -> Option<&dyn WiringCap>    { None }  // amend / after_write
    fn telemetry(&self) -> Option<&dyn TelemetryCap> { None }  // stop 正文 / context / transcript / rename
    fn terminal(&self)  -> Option<&dyn TerminalCap>  { None }  // 进程名 / 标签标题 / tab token
    fn account(&self)   -> Option<&dyn AccountCap>   { None }  // 账号 / 用量
}
```

对比现状:`writes_tab_token()` 返回 `false`、`transcript()` 返回 `None`、`usage_supported()` 返回
`false` 这些「我没有这个能力」的表达,现在统一成能力查询返回 `None`。codex 不支持 rename,就不实现
`TelemetryCap::write_rename`;kimi 不需要 amend,就不提供 `WiringCap`。

### 端口

```rust
pub trait HttpPort: Sync {
    fn get_json(&self, url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, PortError>;
    fn post_form(&self, url: &str, body: &str) -> Result<serde_json::Value, PortError>;
}
pub trait FsPort: Sync {
    fn write_atomic(&self, path: &Path, text: &str) -> Result<(), PortError>;
    fn backup_once(&self, path: &Path);
}
pub trait KeychainPort: Sync {
    fn read(&self, service: &str, account: &str) -> Option<String>;
    fn write(&self, service: &str, account: &str, secret: &str) -> Result<(), PortError>;
}
```

端口以 `&dyn` 传入能力方法,不做全局单例 —— 测试可注入假实现,插件层的单测不再需要真网络。

### 前端零 agent 知识

后端暴露 `list_agents() -> Vec<AgentDescriptor>`:

```jsonc
{
  "id": "claude",
  "display_name": "Claude Code",
  "brand_color": "#d97757",
  "icon_svg": "<path d=\"…\"/>",
  "capabilities": { "usage": true, "rename": true, "resume": true }
}
```

据此删除:`providers.tsx` 的 `PROVIDERS` 表、`api.ts` 的 `ProviderKey` 联合类型、`About.tsx` 的
`=== "claude"` 特判、`styles.css` 的 `--cc-claude`(改为从描述符注入 CSS 变量)、`i18n` 的
`agentClaudeCode` / `agentKimiCode` / `agentCodex` 三条(产品名不翻译)。

`noAgents` 之类硬列三家名字的文案,改为按 `list_agents()` 动态拼接。

## 迁移阶段

每一阶段独立编译、独立测试通过、可独立发布。

| 阶段 | 内容 | 验收 |
|---|---|---|
| 1 | **身份收敛**:删 `ProviderKey`,全仓改用 `AgentId`;store 的 provider 列退化为字符串;修掉未知降级 claude 的 bug | 三个 enum↔registry 绊线单测中的两个变得无意义并删除 |
| 2 | **注册表合一**:`Agent`(reporter)/ `ProviderSetup` / `ProviderAccount` 三个 trait 折进 `AgentPlugin` 的能力槽;`lib.rs` 两处硬 match 消失 | 全仓只剩一张 `ALL` 注册表 |
| 3 | **端口注入**:定义 `api/ports.rs`;account 的联网逻辑与 setup 的 amend/after_write 搬进 `plugins/<id>/` | app 的 `account/` 与 `setup/` 只剩端口实现与编排 |
| 4 | **前端描述符**:`list_agents()` 下发;删前端三张表与 claude 特判 | 加 agent 前端零改动 |

## 验收:加一个 gemini 要动哪些文件

改完之后应当只有两处:

1. 新增 `crates/meowo-agent/src/plugins/gemini/`(`mod.rs` 必需;`account.rs` / `setup.rs` /
   `telemetry.rs` 按需)
2. `crates/meowo-agent/src/registry.rs` 补一行 `&GEMINI`

前端、store、reporter、app 均零改动。这是本次重构的唯一验收标准 —— 也应当有一个测试来守住它:
注册表里每个插件都能被 `list_agents()` 完整描述,不需要任何调用方知道它的名字。
