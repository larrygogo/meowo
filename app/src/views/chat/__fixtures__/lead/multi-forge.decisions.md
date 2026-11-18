# 决定

## D1 范围与做法（leader 2026-11-18：「按你的建议」）

- 平台：GitHub（.com + 企业版）、GitLab（.com + 自管）、Gitea/Forgejo（含 Codeberg）、Bitbucket Cloud；Azure DevOps 等不做。
- 登录：github.com、gitlab.com 用设备码（gitlab.com 需 leader 注册应用）；其余用个人访问令牌，存系统凭据库。
- 设置改为「代码托管账号」，按远端主机自动选账号。
- 架构：抽统一的 PR/MR 接口，不再以 gh 命令行形状为中心；GitHub 现有两条实现迁到接口后面。

## D2 追加 Gitee 与 gitlab.com 应用注册（2026-11-18，leader：「好的，还要加一下 gitee」）

- 平台加 Gitee（gitee.com，API v5，个人访问令牌）；Gitee 企业版/私有部署本轮不做。
- 同意在 gitlab.com 注册 Meowo OAuth 应用（leader 在面板浏览器登录，agent 代填）。
- 同意：#196 CI 通过后合并；删除 meowo-github-auth worktree 与 feat/github-connection 分支。

## D3 取消本机 gh（2026-11-18，leader：「取消gh」）

- Meowo 不再调用本机 gh 命令行；github.com 与其他平台一样只走账号（设备码或令牌）+ 内置实现。
- 去掉 github.com 的四种连接方式；没账号时引导去设置添加，不报错、不发请求。
- 保留解析对话里 agent 执行 gh 命令的输出来关联会话 PR（不调用 gh）。
- 不提供「从 gh 导入」。

## D4 不注册 gitlab.com 应用（2026-11-18，leader：「取消gitlab」）

- gitlab.com 不做设备码登录，与自管 GitLab 一样只用个人访问令牌；删除 GitLab 设备码、刷新令牌与 MEOWO_GITLAB_CLIENT_ID 相关代码。GitLab 平台支持保留。
- 设备码登录只剩 github.com。

## D5 设置分区「代码托管」（2026-11-18，leader：「代码托管账号改名叫代码托管，把git工具和git平台都放在这里，另外，不要单独放一个关闭，还有把github登录和其他平台都放在一起」）

- 分区名「代码托管」；内含「Git」组（git 程序路径与版本、全局提交身份）与「平台账号」组。
- github.com 与其他平台同一列表、同一添加流程；不设任何「关闭」开关（取代 D3 里「关闭」的保留）。

## D6 验收通过（2026-11-18，leader 在 Meowo 关卡 C-20261118-multi-forge-1 / publication-1 选择 accept，无附言）

- 签署范围：冻结的 9 项（M1–M9），交付版本为本地集成构建 032d0505（#197 1315887c），安装包 SHA-256 1cd790e8…b5d9。
- 不含合并授权：#197、#198 合并仍需 leader 单独同意。
