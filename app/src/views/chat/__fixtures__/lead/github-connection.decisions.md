# 决定

## D1 用 OAuth App + 设备码（leader 2026-11-18：「2. 注册」）

与 gh 同一路线：scope `repo read:org`，一次授权覆盖所有仓库含私有仓库。GitHub App 权限更细，但要用户逐个账号/组织安装，门槛高，不选。设备码不需要 Client Secret，Client ID 可公开打包。

## D2 在 GhExec 这一层替换（agent 决定）

forge 所有 gh 调用经同一个函数类型，生产入口唯一。内置实现对同样的参数给出与 gh 同形的输出，上层逻辑与现有测试基本不动，风险最小。不内置 gh 二进制（安装包增大约 15MB 以上，且登录仍绑 gh 的配置）。

## D3 默认「自动」

老用户已装好 gh 的不受影响；登录过 Meowo 的优先用 Meowo 的令牌。

## D4 OAuth App 已注册（2026-11-18，leader：「你来帮我注册」，leader 自己在面板浏览器登录 GitHub 后由 agent 填表提交）

- larrygogo 名下 OAuth App「Meowo」（id 4031134），Homepage 与 Redirect URI 均为 https://github.com/larrygogo/meowo。
- Client ID：xxxxxxxxxxxxxxxxxxxx（公开，可打包）。
- 开启 Device Flow；关闭「Expire user access tokens」：过期令牌的续期需要 client secret，桌面端不能内置，开着会让用户每 8 小时重新登录。与 gh 一致用不过期令牌，用户可在 GitHub 设置里随时撤销。
- 未生成 client secret。

## D5 G7 写操作授权（2026-11-18，leader：「按你说的做」）

授权在 larrygogo/meowo 上用临时分支验证写路径：建两个临时分支，开草稿 PR（临时分支之间，不碰 main），评论一条，合并进临时 base 分支，最后关 PR、删两个分支。用本机 gh 的令牌驱动 Meowo 内置执行器，令牌不落盘不打印。

## D6 leader 验收（2026-11-18）

关卡 C-20261118-github-connection-1（publication-1），构建 0afbd838 本地安装包：leader 选择 accept，无附言。签署范围为快照中唯一未通过的 leader 项 G8。
核对：本机 ~/.meowo 下没有 github.json，Windows 凭据管理器里没有 Meowo 的 GitHub 条目，说明本机还没有用 Meowo 登录成功过。G7（用正式 Client ID 的设备码登录）因此仍缺证据，不因本次 accept 视为通过。

## D7 关卡 D 答复（2026-11-18）

D-20261118-github-login-1（publication-2）：leader 选择 approve，无附言。核对：本机仍没有 ~/.meowo/github.json，凭据管理器里也没有 Meowo 条目——approve 是同意这一步，不等于已经登录成功。G7 仍缺登录证据，保持未通过；不再重复发关卡，等 leader 实际登录后（或给出报错）再补。
