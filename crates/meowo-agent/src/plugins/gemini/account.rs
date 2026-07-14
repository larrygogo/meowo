//! Gemini CLI 的账号能力：**只读本地登录态，不查用量**。
//!
//! Gemini 有**两条**能落地的登录路径（枚举值取自 0.50 bundle：`oauth-personal` / `gemini-api-key`
//! / `vertex-ai`），认哪一条都算已登录：
//!
//! | 路径 | 凭据在哪 | 我们怎么判 |
//! |---|---|---|
//! | Sign in with Google（`oauth-personal`） | `~/.gemini/oauth_creds.json` | **只判存在**，绝不解析、绝不打印 |
//! | Use Gemini API Key（`gemini-api-key`） | 环境变量 `GEMINI_API_KEY` / `GOOGLE_API_KEY`，或 `~/.gemini/.env` | 只判**有没有**，绝不读值 |
//!
//! 只认 OAuth 那条是不够的——而且恰恰是**行不通**的那条：Google 已停掉个人版的
//! Gemini Code Assist（实测报错：*This client is no longer supported for Gemini Code Assist for
//! individuals*，让人迁去 Antigravity）。个人用户现在基本只能走 API Key，若只认
//! `oauth_creds.json`，他们会永远显示「未登录」。
//!
//! 邮箱只有 OAuth 那条才有（`google_accounts.json`），且其 schema **未经实测**，故提取是防御性的：
//! 取不到就不显示邮箱，**绝不因此把「已登录」误判成「未登录」**。
//!
//! **用量不支持**：Gemini 没有公开的配额查询端点，`usage_supported` 返回 false——卡片会显示
//! 「当前登录方式不支持用量查询」，而不是摆一个永远转圈的刷新按钮。

use std::path::Path;

use serde_json::Value;

use crate::account::{Account, AccountCap, ProviderUsage, USAGE_UNSUPPORTED};
use crate::ports::Ports;
use crate::variant::Installation;

pub static ACCOUNT: GeminiAccount = GeminiAccount;

pub struct GeminiAccount;

/// 账号信息文件，相对 data_dir（与 `oauth_creds.json` 同处 `~/.gemini`）。
const ACCOUNTS_REL: &str = "google_accounts.json";

/// Gemini 认的 API key 环境变量（bundle 里两个都读）。
const API_KEY_VARS: [&str; 2] = ["GEMINI_API_KEY", "GOOGLE_API_KEY"];

impl AccountCap for GeminiAccount {
    fn account(&self, inst: &Installation, _ports: &Ports) -> Option<Account> {
        // 先看**用户选的是哪种认证方式**（`settings.json` 的 `security.auth.selectedType`）。
        //
        // 这一步不是锦上添花：残留的 `oauth_creds.json` 会说谎。Google 停掉个人版 Code Assist 之后，
        // 一次失败的「Sign in with Google」照样会把 token 落盘（OAuth 换到了，只是后端拒绝发牌），
        // 于是「凭据文件在」不再等于「能用」。用户改配 API Key 后 selectedType 会变过来，
        // 判据也就跟着对了——而只看文件的话，他会一直被显示成「已登录 Google」。
        match selected_auth_type(&inst.data_dir).as_deref() {
            Some(AUTH_API_KEY) => return api_key_account(&inst.data_dir),
            Some(AUTH_OAUTH) => return oauth_account(inst),
            // vertex-ai 或本版本不认识的取值：不猜，走下面的「任一命中即可」。
            _ => {}
        }

        // 没有 settings.json / 没记认证方式（老版本、或还没走完首次引导）→ 两条路都试。
        oauth_account(inst).or_else(|| api_key_account(&inst.data_dir))
    }

    /// Gemini 没有配额查询 API——如实回 UNSUPPORTED，前端据此显示专门的文案。
    fn fetch_usage(&self, _inst: &Installation, _ports: &Ports) -> Result<ProviderUsage, String> {
        Err(USAGE_UNSUPPORTED.to_string())
    }

    fn usage_supported(&self, _inst: &Installation, _ports: &Ports) -> bool {
        false
    }
}

/// `settings.json` 里 `security.auth.selectedType` 的两个取值（第三个是 `vertex-ai`，我们不特判）。
const AUTH_OAUTH: &str = "oauth-personal";
const AUTH_API_KEY: &str = "gemini-api-key";

/// 用户选的认证方式。实测 `~/.gemini/settings.json`：`{"security":{"auth":{"selectedType":"oauth-personal"}}}`。
fn selected_auth_type(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join("settings.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("security")?
        .get("auth")?
        .get("selectedType")?
        .as_str()
        .map(str::to_string)
}

/// OAuth（Sign in with Google）：凭据文件在即已登录。内容一概不碰——判断「登录没登录」不需要读 token。
fn oauth_account(inst: &Installation) -> Option<Account> {
    if !inst.credentials_path().is_some_and(|p| p.is_file()) {
        return None;
    }
    let email = std::fs::read_to_string(crate::join_rel(&inst.data_dir, ACCOUNTS_REL))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| find_email(&v));
    Some(Account {
        email,
        display_name: None,
        organization: None,
        plan: None,
        // 拿不到邮箱时，卡片至少得说清登录方式，而不是一片空白。
        login_label: Some("Google".to_string()),
    })
}

/// API Key。个人用户现在基本只剩这条路（Google 已停掉个人版的 Code Assist OAuth）。
fn api_key_account(data_dir: &Path) -> Option<Account> {
    has_api_key(data_dir).then(|| Account {
        email: None, // API key 不携带身份信息。
        display_name: None,
        organization: None,
        plan: None,
        login_label: Some("API Key".to_string()),
    })
}

/// 有没有配好 API key。**只判有无，绝不读取值**——登录态不需要知道 key 是什么。
///
/// 两个来源，任一命中即可（gemini 自己两处都读）：
/// - 进程环境变量 `GEMINI_API_KEY` / `GOOGLE_API_KEY`。注意这读的是 **meowo-app 自己的**环境：
///   用户若只在某个终端里 `export`，meowo 是看不见的——所以 `.env` 那条不是可有可无的补充。
/// - `~/.gemini/.env`（gemini 支持把 key 放这儿，bundle 里有这条路径）。
fn has_api_key(data_dir: &Path) -> bool {
    if API_KEY_VARS
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
    {
        return true;
    }
    let Ok(text) = std::fs::read_to_string(data_dir.join(".env")) else {
        return false;
    };
    text.lines().any(env_line_sets_api_key)
}

/// 一行 `.env` 是否给某个 API key 变量赋了**非空**值。容忍 `export K=v`、空格、引号。
fn env_line_sets_api_key(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };
    if !API_KEY_VARS.contains(&key.trim()) {
        return false;
    }
    // `GEMINI_API_KEY=` / `GEMINI_API_KEY=""` 都是「没配」。
    !value.trim().trim_matches(['"', '\'']).is_empty()
}

/// 在任意形状的 JSON 里捞出第一个像邮箱的字符串。
///
/// 刻意**不**硬编码字段路径：`google_accounts.json` 的 schema 没有实测过，写死一条
/// `v["active"]["email"]` 之类的路径，一旦猜错就是静默取不到值。递归找带 `@` 的字符串则对
/// 形状不敏感——`{"active":"a@b.c"}`、`{"active":{"email":"a@b.c"}}`、数组套娃都能命中。
fn find_email(v: &Value) -> Option<String> {
    match v {
        // 排除 "@scope/pkg" 这类以 @ 开头的非邮箱串。
        Value::String(s) if s.contains('@') && !s.starts_with('@') => Some(s.clone()),
        Value::Object(m) => m.values().find_map(find_email),
        Value::Array(a) => a.iter().find_map(find_email),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// email 提取对 schema 不敏感——这正是它存在的理由（真实形状未经实测）。
    #[test]
    fn finds_email_regardless_of_shape() {
        assert_eq!(
            find_email(&json!({"active": "a@b.c"})).as_deref(),
            Some("a@b.c")
        );
        assert_eq!(
            find_email(&json!({"active": {"email": "x@y.z", "id": 1}})).as_deref(),
            Some("x@y.z")
        );
        assert_eq!(
            find_email(&json!({"accounts": [{"email": "p@q.r"}]})).as_deref(),
            Some("p@q.r")
        );
    }

    /// 找不到邮箱不是错误——登录态由凭据文件决定，邮箱只是锦上添花。
    #[test]
    fn missing_email_is_not_an_error() {
        assert_eq!(find_email(&json!({})), None);
        assert_eq!(find_email(&json!({"id": 42, "ok": true})), None);
        // 不能把 "@scope/pkg" 这种当成邮箱。
        assert_eq!(find_email(&json!({"pkg": "@google/gemini-cli"})), None);
    }

    /// API Key 登录必须被认出来。
    ///
    /// 这不是个边角情形，而是**现在的主路径**：Google 已停掉个人版的 Gemini Code Assist
    /// （"Sign in with Google" 会直接报 *This client is no longer supported*），个人用户只剩
    /// API Key 可走。只认 `oauth_creds.json` 的话，他们会永远显示「未登录」，而登录按钮点了
    /// 也没用——因为他们本来就已经配好了。
    #[test]
    fn env_line_recognizes_api_key_assignments() {
        assert!(env_line_sets_api_key("GEMINI_API_KEY=abc123"));
        assert!(env_line_sets_api_key("GOOGLE_API_KEY=abc123"));
        // 常见的书写变体。
        assert!(env_line_sets_api_key("export GEMINI_API_KEY=abc123"));
        assert!(env_line_sets_api_key("  GEMINI_API_KEY = \"abc123\"  "));
        assert!(env_line_sets_api_key("GEMINI_API_KEY='abc123'"));

        // 空值 = 没配（比「没有这一行」更常见：用户删了 key 但留着行）。
        assert!(!env_line_sets_api_key("GEMINI_API_KEY="));
        assert!(!env_line_sets_api_key("GEMINI_API_KEY=\"\""));
        assert!(!env_line_sets_api_key("GEMINI_API_KEY=   "));
        // 注释掉的不算。
        assert!(!env_line_sets_api_key("# GEMINI_API_KEY=abc123"));
        // 别的变量不算。
        assert!(!env_line_sets_api_key("OPENAI_API_KEY=abc123"));
        assert!(!env_line_sets_api_key("GEMINI_API_KEY_BACKUP=abc123"));
        assert!(!env_line_sets_api_key("random text"));
    }

    /// 认证方式取自 `settings.json`——**残留的 oauth_creds.json 会说谎**。
    ///
    /// Google 停掉个人版 Code Assist 之后，一次失败的「Sign in with Google」照样会把 token 落盘
    /// （OAuth 换到了，只是后端拒绝发牌）。用户改配 API Key 后，`selectedType` 会变过来；若只看
    /// 「哪个凭据文件存在」，他会一直被显示成「已登录 Google」，而 gemini 根本跑不起来。
    #[test]
    fn selected_auth_type_is_read_from_settings() {
        let dir = std::env::temp_dir().join(format!("meowo-gemini-auth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 没有 settings.json → 不猜。
        assert_eq!(selected_auth_type(&dir), None);

        // 实测的真实形状。
        std::fs::write(
            dir.join("settings.json"),
            r#"{"security":{"auth":{"selectedType":"oauth-personal"}},"hooks":{}}"#,
        )
        .unwrap();
        assert_eq!(selected_auth_type(&dir).as_deref(), Some(AUTH_OAUTH));

        std::fs::write(
            dir.join("settings.json"),
            r#"{"security":{"auth":{"selectedType":"gemini-api-key"}}}"#,
        )
        .unwrap();
        assert_eq!(selected_auth_type(&dir).as_deref(), Some(AUTH_API_KEY));

        // 形状不对 → None（走回退，不 panic）。
        std::fs::write(dir.join("settings.json"), r#"{"hooks":{}}"#).unwrap();
        assert_eq!(selected_auth_type(&dir), None);
        std::fs::write(dir.join("settings.json"), "not json").unwrap();
        assert_eq!(selected_auth_type(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `.env` 里配了 key → 已登录，哪怕根本没有 oauth_creds.json。
    #[test]
    fn api_key_in_dotenv_counts_as_logged_in() {
        let _env = crate::env_guard();
        for k in API_KEY_VARS {
            std::env::remove_var(k);
        }
        let dir = std::env::temp_dir().join(format!("meowo-gemini-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 没 .env、没环境变量 → 没有 API key。
        assert!(!has_api_key(&dir));

        // 写了 key → 认出来（注意这里没有任何 oauth_creds.json）。
        std::fs::write(dir.join(".env"), "# 我的 key\nGEMINI_API_KEY=abc123\n").unwrap();
        assert!(has_api_key(&dir));

        // 只留一个空赋值 → 仍算没配。
        std::fs::write(dir.join(".env"), "GEMINI_API_KEY=\n").unwrap();
        assert!(!has_api_key(&dir));

        // 环境变量兜底（用户设在系统环境里）。
        std::env::set_var("GEMINI_API_KEY", "from-env");
        assert!(has_api_key(&dir));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Gemini 无配额端点：如实回 UNSUPPORTED，别让前端摆一个永远转圈的刷新按钮。
    /// NoHttp 会在任何请求上 panic——据此同时断言这条路径**不联网**。
    #[test]
    fn usage_is_declared_unsupported_without_network() {
        use crate::ports::test_doubles::NoHttp;
        let ports = Ports {
            http: &NoHttp,
            keychain: &crate::ports::NoKeychain,
        };
        let inst = crate::by_id("gemini")
            .unwrap()
            .resolve()
            .expect("总能推出默认落点");
        assert!(!ACCOUNT.usage_supported(&inst, &ports));
        assert_eq!(
            ACCOUNT.fetch_usage(&inst, &ports).unwrap_err(),
            USAGE_UNSUPPORTED
        );
    }
}
