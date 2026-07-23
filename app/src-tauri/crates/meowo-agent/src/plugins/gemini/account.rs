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

use crate::account::{Account, AccountCap, ApiKeyLoginCap, ProviderUsage, USAGE_UNSUPPORTED};
use crate::ports::Ports;
use crate::variant::Installation;

pub static ACCOUNT: GeminiAccount = GeminiAccount;
pub static API_KEY_LOGIN: GeminiApiKeyLogin = GeminiApiKeyLogin;

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

/// API Key 登录：把 key 写到 gemini 自己会读的两个地方。
///
/// OAuth 已死（个人版 Code Assist 被 Google 停掉），而 gemini 没有「输入 key」的登录子命令——
/// key 只认环境变量与 `~/.gemini/.env`。宿主替用户落盘的正是后者，因为它对**所有**终端生效，
/// 不依赖 meowo 注入环境变量。
///
/// 两步缺一不可：
/// - `.env` 写 `GEMINI_API_KEY=<key>`——没有它 CLI 无 key 可用；
/// - `settings.json` 写 `security.auth.selectedType = "gemini-api-key"`——没有它，走过 OAuth 的
///   老用户的 TUI 仍会按旧选择去跑 OAuth，key 配了也不被理会（[`selected_auth_type`] 的判定同理）。
pub struct GeminiApiKeyLogin;

impl ApiKeyLoginCap for GeminiApiKeyLogin {
    fn save_api_key(&self, inst: &Installation, key: &str) -> Result<(), String> {
        let key = key.trim();
        validate_api_key(key)?;
        // 目录可能还不存在（装了 CLI 但从没跑过）。登录是用户的显式意图，替它建目录与 gemini
        // 首次运行的行为一致——这不是 wiring 那种「绝不凭空创建」的场景。
        std::fs::create_dir_all(&inst.data_dir).map_err(|e| format!("创建配置目录失败：{e}"))?;
        write_env_key(&inst.data_dir, Some(key))?;
        set_selected_auth_type(&inst.data_dir)?;
        Ok(())
    }

    fn clear_api_key(&self, inst: &Installation) -> Result<(), String> {
        // 幂等：目录都没有 = 本来就没配。
        if !inst.data_dir.is_dir() {
            return Ok(());
        }
        write_env_key(&inst.data_dir, None)
        // selectedType 留着不动：没有 key 时账号判定已然是「未登录」，而 CLI 下次启动发现无 key
        // 会自己回到认证选择页。反过来擅自删掉它，等于替用户改了一个我们没被要求动的设置。
    }
}

/// key 的形状校验。**不是**为了猜 Google 的 key 格式（AIza… 会变），而是守住两条底线：
/// 非空、且不含空白/控制字符——`.env` 按行解析，一个带换行的“key”能注入任意变量行。
fn validate_api_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("API Key 不能为空".into());
    }
    if key.len() > 512 {
        return Err("API Key 过长".into());
    }
    if !key.chars().all(|c| c.is_ascii_graphic()) {
        return Err("API Key 含有空白或不可见字符".into());
    }
    Ok(())
}

/// 更新 `.env` 里的 key 行：`Some(key)` = 覆盖或追加 `GEMINI_API_KEY`；`None` = 删除
/// `GEMINI_API_KEY` **与** `GOOGLE_API_KEY` 的赋值行（两个都算「已登录」，登出漏一个都退不干净）。
/// 其余行（用户自己放的别家变量、注释）一律原样保留。
///
/// 旧行的判定用「是否赋值」而非 [`env_line_sets_api_key`] 的「是否赋了非空值」：残留的
/// `GEMINI_API_KEY=` 空行也必须清掉，否则会与新行并存，靠 dotenv 的重复键顺序碰运气。
fn write_env_key(data_dir: &Path, key: Option<&str>) -> Result<(), String> {
    let path = data_dir.join(".env");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    // 保存时只清 GEMINI_API_KEY 自己的旧行——GOOGLE_API_KEY 是用户自己配的，且优先级在
    // GEMINI_API_KEY 之后，不动它。登出时两个都得清（任一在场都算「已登录」）。
    let doomed: &[&str] = if key.is_some() {
        &["GEMINI_API_KEY"]
    } else {
        &API_KEY_VARS
    };
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| !env_line_assigns(line, doomed))
        .map(str::to_string)
        .collect();
    if let Some(key) = key {
        lines.push(format!("GEMINI_API_KEY={key}"));
    } else if lines.iter().all(|l| l.trim().is_empty()) {
        // 清完只剩空白 → 整个文件删掉，不留一个空壳。删不掉（不存在）也算成功。
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除 .env 失败：{e}")),
        };
    }
    let mut body = lines.join("\n");
    body.push('\n');
    crate::fsutil::write_atomic_secure(&path, &body).map_err(|e| format!("写入 .env 失败：{e}"))
}

/// 一行 `.env` 是否给 `keys` 中的某个变量**赋值**（值空不空都算）。容忍 `export K=v`、空格；
/// 注释行不算。
fn env_line_assigns(line: &str, keys: &[&str]) -> bool {
    let line = line.trim();
    if line.starts_with('#') {
        return false;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    line.split_once('=')
        .is_some_and(|(key, _)| keys.contains(&key.trim()))
}

/// 把 `settings.json` 的 `security.auth.selectedType` 设为 `gemini-api-key`，其余键原样保留；
/// 文件不存在从 `{}` 建（gemini 本就允许它缺席，hooks 接线也是这么做的）。
fn set_selected_auth_type(data_dir: &Path) -> Result<(), String> {
    let path = data_dir.join("settings.json");
    let mut root: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| "settings.json 不是有效 JSON，不敢覆盖".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(format!("读取 settings.json 失败：{e}")),
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "settings.json 顶层不是对象，不敢覆盖".to_string())?;
    let auth = obj
        .entry("security")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "settings.json 的 security 不是对象".to_string())?
        .entry("auth")
        .or_insert_with(|| serde_json::json!({}));
    auth.as_object_mut()
        .ok_or_else(|| "settings.json 的 security.auth 不是对象".to_string())?
        .insert("selectedType".into(), Value::String(AUTH_API_KEY.into()));
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&path, &body).map_err(|e| format!("写入 settings.json 失败：{e}"))
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

    /// 建一个指向临时 home 的 gemini 实况（`<home>/.gemini` 已建好）。
    fn temp_installation(tag: &str) -> (std::path::PathBuf, Installation) {
        let home = std::env::temp_dir().join(format!("meowo-gemini-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        let inst = super::super::VARIANTS[0]
            .probe(crate::id::GEMINI, &home)
            .expect("数据目录在 → 应命中");
        (home, inst)
    }

    /// 保存 = `.env` 落 key + `settings.json` 落 selectedType，且账号判定当场认出「API Key」。
    /// 这条链就是「API Key 登录」的全部定义——断口任何一环，用户点完保存仍显示未登录。
    #[test]
    fn save_api_key_persists_env_and_selected_auth_type() {
        let _env = crate::env_guard();
        for k in API_KEY_VARS {
            std::env::remove_var(k);
        }
        let (home, inst) = temp_installation("save");
        let dir = inst.data_dir.clone();
        // 已有的 settings 内容必须原样保留——它承载 hooks 与用户配置，覆盖即断线。
        std::fs::write(
            dir.join("settings.json"),
            r#"{"hooks":{"SessionStart":[]},"theme":"dark"}"#,
        )
        .unwrap();
        // 已有的 .env：别家变量与注释保留；残留的空赋值行必须被清掉，不与新行并存。
        std::fs::write(dir.join(".env"), "# mine\nOTHER=1\nGEMINI_API_KEY=\n").unwrap();

        API_KEY_LOGIN
            .save_api_key(&inst, "  AIzaTest123  ")
            .unwrap();

        let env_body = std::fs::read_to_string(dir.join(".env")).unwrap();
        assert!(
            env_body.contains("GEMINI_API_KEY=AIzaTest123"),
            "{env_body}"
        );
        assert!(env_body.contains("# mine") && env_body.contains("OTHER=1"));
        assert_eq!(
            env_body.matches("GEMINI_API_KEY").count(),
            1,
            "旧的空赋值行必须被清掉：{env_body}"
        );

        let settings: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            settings["security"]["auth"]["selectedType"], AUTH_API_KEY,
            "不写 selectedType，走过 OAuth 的 TUI 仍会去跑 OAuth"
        );
        assert_eq!(settings["theme"], "dark", "settings 其余键必须保留");
        assert!(settings["hooks"].is_object(), "hooks 接线必须保留");

        // 端到端：账号判定当场转为「API Key 已登录」。
        assert_eq!(selected_auth_type(&dir).as_deref(), Some(AUTH_API_KEY));
        assert!(has_api_key(&dir));

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 数据目录不存在（装了没跑过）→ 替用户建出来，与 gemini 首次运行的行为一致。
    #[test]
    fn save_api_key_creates_missing_data_dir() {
        let _env = crate::env_guard();
        let home = std::env::temp_dir().join(format!("meowo-gemini-mkdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let inst = super::super::VARIANTS[0].installation_at(
            crate::id::GEMINI,
            home.join(".gemini"),
            Some(&home),
        );

        API_KEY_LOGIN.save_api_key(&inst, "AIzaNew").unwrap();
        assert!(std::fs::read_to_string(home.join(".gemini/.env"))
            .unwrap()
            .contains("GEMINI_API_KEY=AIzaNew"));

        let _ = std::fs::remove_dir_all(&home);
    }

    /// `.env` 按行解析——一个带换行的“key”能注入任意变量行，必须当场拒绝，而不是写进去。
    #[test]
    fn save_api_key_rejects_malformed_keys() {
        let (home, inst) = temp_installation("reject");
        for bad in [
            "",
            "   ",
            "with space",
            "line\nGOOGLE_API_KEY=evil",
            "带中文",
        ] {
            assert!(
                API_KEY_LOGIN.save_api_key(&inst, bad).is_err(),
                "{bad:?} 应被拒绝"
            );
        }
        assert!(!inst.data_dir.join(".env").exists(), "拒绝时不得落盘");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 清除 = 两个 key 变量的行都删（任一在场都算「已登录」），别家行保留；清空则删文件；幂等。
    #[test]
    fn clear_api_key_removes_both_vars_and_is_idempotent() {
        let _env = crate::env_guard();
        for k in API_KEY_VARS {
            std::env::remove_var(k);
        }
        let (home, inst) = temp_installation("clear");
        let dir = inst.data_dir.clone();
        std::fs::write(
            dir.join(".env"),
            "OTHER=1\nGEMINI_API_KEY=a\nexport GOOGLE_API_KEY=b\n",
        )
        .unwrap();

        API_KEY_LOGIN.clear_api_key(&inst).unwrap();
        let body = std::fs::read_to_string(dir.join(".env")).unwrap();
        assert!(body.contains("OTHER=1"));
        assert!(
            !body.contains("API_KEY"),
            "登出漏清任一变量都退不干净：{body}"
        );
        assert!(!has_api_key(&dir));

        // 只剩 key 行的文件：清完不留空壳。
        std::fs::write(dir.join(".env"), "GEMINI_API_KEY=a\n").unwrap();
        API_KEY_LOGIN.clear_api_key(&inst).unwrap();
        assert!(!dir.join(".env").exists());

        // 幂等：没有 .env、甚至没有目录，都返回 Ok。
        API_KEY_LOGIN.clear_api_key(&inst).unwrap();
        let _ = std::fs::remove_dir_all(&home);
        API_KEY_LOGIN.clear_api_key(&inst).unwrap();
    }

    /// settings.json 是坏 JSON 时**拒绝保存**而不是拿 `{}` 覆盖——它还承载 hooks 与用户配置，
    /// 覆盖等于替用户删配置。
    #[test]
    fn save_api_key_refuses_to_clobber_corrupt_settings() {
        let (home, inst) = temp_installation("corrupt");
        std::fs::write(inst.data_dir.join("settings.json"), "not json").unwrap();
        assert!(API_KEY_LOGIN.save_api_key(&inst, "AIzaX").is_err());
        assert_eq!(
            std::fs::read_to_string(inst.data_dir.join("settings.json")).unwrap(),
            "not json",
            "拒绝时原文件必须原样"
        );
        let _ = std::fs::remove_dir_all(&home);
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
