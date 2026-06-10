# macOS 发布所需 GitHub Secrets

在仓库 Settings → Secrets and variables → Actions 添加：

## 代码签名（Developer ID Application 证书）
- `APPLE_CERTIFICATE`：Developer ID Application 证书导出的 .p12，base64 编码（`base64 -i cert.p12`）
- `APPLE_CERTIFICATE_PASSWORD`：导出 .p12 时设置的密码
- `APPLE_SIGNING_IDENTITY`：如 `Developer ID Application: Your Name (TEAMID)`（`security find-identity -v -p codesigning`）
- `KEYCHAIN_PASSWORD`：任意强随机串（CI 临时钥匙串用）

## 公证（Apple ID + App 专用密码，当前 workflow 采用）
- `APPLE_ID`：Apple 开发者账号邮箱
- `APPLE_PASSWORD`：App 专用密码（appleid.apple.com → 登录与安全 → App 专用密码 生成）
- `APPLE_TEAM_ID`：开发者 Team ID（如 `JPHJRUCS88`，可从签名证书 `security find-identity` 的括号里看）

> tauri-action 直接支持这套,无需把 .p8 写到 runner 文件。
> 备选公证方式（App Store Connect API key）：`APPLE_API_ISSUER` / `APPLE_API_KEY` / `APPLE_API_KEY_PATH`，但需在 workflow 里加一步把 .p8 secret 落盘后再传路径。

## 自动更新（已存在，复用）
- `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

## 注意
- 证书必须是 **Developer ID Application**（站外分发），不是 Apple Development / Mac App Distribution。
- `hardenedRuntime` 已在 tauri.conf 开启（公证必需）。
- 证书 base64 用 `base64 -i cert.p12`，整段（不带多余换行）塞进 `APPLE_CERTIFICATE`。
