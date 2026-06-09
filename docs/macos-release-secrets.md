# macOS 发布所需 GitHub Secrets

在仓库 Settings → Secrets and variables → Actions 添加：

## 代码签名（Developer ID Application 证书）
- `APPLE_CERTIFICATE`：Developer ID Application 证书导出的 .p12，base64 编码（`base64 -i cert.p12`）
- `APPLE_CERTIFICATE_PASSWORD`：导出 .p12 时设置的密码
- `APPLE_SIGNING_IDENTITY`：如 `Developer ID Application: Your Name (TEAMID)`（`security find-identity -v -p codesigning`）
- `KEYCHAIN_PASSWORD`：任意强随机串（CI 临时钥匙串用）

## 公证（App Store Connect API key，推荐此方案）
- `APPLE_API_ISSUER`：Issuer ID
- `APPLE_API_KEY`：Key ID
- `APPLE_API_KEY_PATH`：.p8 私钥在 runner 上的路径（需在 workflow 里先把 secret 内容写成文件，再把该路径传给此变量）

> 备选公证方式（Apple ID）：`APPLE_ID` / `APPLE_PASSWORD`(App 专用密码) / `APPLE_TEAM_ID`。

## 自动更新（已存在，复用）
- `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

## 注意
- 证书必须是 **Developer ID Application**（站外分发），不是 Apple Development / Mac App Distribution。
- `hardenedRuntime` 已在 tauri.conf 开启（公证必需）。
- 证书 base64 用 `base64 -i cert.p12`，整段（不带多余换行）塞进 `APPLE_CERTIFICATE`。
