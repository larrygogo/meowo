//! OpenCode 的账号能力：**只读本地登录态，不查用量**。
//!
//! # opencode 没有「账号」，只有「一组已连接的 provider」
//!
//! 这是它与另四家的根本不同，也决定了这里能做什么、不能做什么（结论均来自 1.17.20 源码
//! `packages/opencode/src/auth/index.ts`）：
//!
//! - `auth.json` 就是一张 `{providerID: 凭据}` 的表，凭据按 `type` 三选一：
//!   `oauth` / `api` / `wellknown`。
//! - **同一个 provider 只能有一份凭据**——`set()` 是 `{...data, [id]: info}`，后写覆盖先写
//!   （官方测试直接断言了这点）。所以「两个 Anthropic 账号」在 auth 层无从表达。
//! - **没有「当前活跃账号」**。auth.json 里没有任何 active/current 字段；用哪份凭据由你选的
//!   **model** 隐式决定（选 `anthropic/claude-*` 就取 anthropic 那条）。
//!
//! 所以「登录 opencode」这句话本身就不成立，UI 上能诚实表达的只有**连了哪几家 provider**——
//! 这恰好等价于 `opencode auth list` 的输出，我们给不出比 CLI 更多的信息。切换账号之类的 UI
//! 不要做：那个概念在 provider 层不存在。
//!
//! # 登录态的判据不止 auth.json
//!
//! opencode 有两条旁路，只读文件会把这些用户**误判成「未登录」**（而他们明明能正常用）：
//!
//! - `OPENCODE_AUTH_CONTENT`：整个 auth.json 的内容可由这个环境变量提供，命中时 opencode
//!   **根本不读文件**。
//! - provider 自己的环境变量（`ANTHROPIC_API_KEY` 等）——`opencode auth list` 自己就为此单列了
//!   一段 `Environment`。这条**尚未覆盖**，见 `has_env_provider_key` 上的说明。
//!
//! 凭据路径走 [`CredentialSource::HomeFile`](crate::auth::CredentialSource)：auth.json 在 opencode
//! 的**数据**目录（`~/.local/share/opencode`），而我们的 `data_dir` 是它的**配置**目录
//! （`~/.config/opencode`，插件得落在那儿）——两者不是一回事。
//!
//! **用量不支持**：opencode 只是个壳，额度归背后的 provider 管，它自己没有配额端点。

use serde_json::Value;

use crate::account::{Account, AccountCap, ProviderUsage, USAGE_UNSUPPORTED};
use crate::ports::Ports;
use crate::variant::Installation;

/// 整份 auth.json 可由它提供；命中时 opencode 根本不读文件（源码 `Auth.all()` 的第一分支）。
const AUTH_CONTENT_VAR: &str = "OPENCODE_AUTH_CONTENT";

pub static ACCOUNT: OpencodeAccount = OpencodeAccount;

pub struct OpencodeAccount;

impl AccountCap for OpencodeAccount {
    fn account(&self, inst: &Installation, _ports: &Ports) -> Option<Account> {
        // 环境变量整体覆盖文件——与 opencode 自己的取值顺序一致（它命中这条时压根不读文件）。
        //
        // 注意它是**进程级**的：多账号（profile）下每个 profile 有自己的 auth.json，而这个变量
        // 一旦设了就盖住所有 profile。这与 opencode 自身的行为一致（它也只认这一份），如实反映即可。
        let text = match std::env::var(AUTH_CONTENT_VAR) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => std::fs::read_to_string(inst.credentials_path()?).ok()?,
        };

        let label = connected_providers(&text)?;
        Some(Account {
            // auth.json 里只有 provider 凭据，没有任何身份信息（`opencode auth list` 同样只给
            // provider 名 + 类型，给不出邮箱）。
            email: None,
            display_name: None,
            organization: None,
            plan: None,
            // 「连了哪几家 provider」是这张卡片上唯一有信息量的东西。
            login_label: Some(label),
        })
    }

    /// 额度归 opencode 背后的 provider 管，它自己没有配额端点。
    fn fetch_usage(&self, _inst: &Installation, _ports: &Ports) -> Result<ProviderUsage, String> {
        Err(USAGE_UNSUPPORTED.to_string())
    }

    fn usage_supported(&self, _inst: &Installation, _ports: &Ports) -> bool {
        false
    }
}

/// auth.json 的文本 → 「连了哪几家 provider」的展示串（如 `anthropic (oauth), openai (api)`）。
/// 一家都没连（`{}`、空文本、坏 JSON）→ None，即「未登录」。
///
/// 带上 `type` 是有意的：`oauth` 与 `api` 在续期行为上完全不同（前者会过期刷新，后者是长期 key），
/// 而这是 `opencode auth list` 唯一比「provider 名」多给的信息。
///
/// 键**未必是 provider id**：`wellknown` 类型的键是一个 URL（源码 `auth.set(url, …)`）。原样展示
/// 即可——那正是用户在 `auth list` 里看到的东西。
fn connected_providers(text: &str) -> Option<String> {
    let root: Value = serde_json::from_str(text).ok()?;
    // `opencode auth logout` 会把它清成 `{}`——空对象是「登出了」，不是「登录了但没信息」。
    let map = root.as_object().filter(|m| !m.is_empty())?;

    let label = map
        .iter()
        .map(|(id, cred)| match cred.get("type").and_then(Value::as_str) {
            Some(t) => format!("{id} ({t})"),
            // schema 漂了也不至于把「已连接」判成「没连接」——退回只显示名字。
            None => id.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_broken_auth_json_is_not_logged_in() {
        // logout 之后就是这个形状——不能当成「已登录」。
        assert_eq!(connected_providers("{}"), None);
        assert_eq!(connected_providers(""), None);
        assert_eq!(connected_providers("not json"), None);
        // 顶层不是对象 → 不认。
        assert_eq!(connected_providers("[]"), None);
    }

    /// 「多账号」在 opencode 其实是「多 provider」——同一 provider 只能有一份凭据（后写覆盖），
    /// 也没有「活跃账号」。所以卡片上诚实的表达就是把连上的几家都列出来。
    #[test]
    fn connected_providers_are_listed_with_their_credential_type() {
        assert_eq!(
            connected_providers(r#"{"anthropic":{"type":"oauth"}}"#).as_deref(),
            Some("anthropic (oauth)")
        );

        // 连了多家就都列出来（serde_json 的 Map 默认按键排序，故次序稳定）。
        let label =
            connected_providers(r#"{"openai":{"type":"api"},"anthropic":{"type":"oauth"}}"#).unwrap();
        assert_eq!(label, "anthropic (oauth), openai (api)");

        // 键未必是 provider id：wellknown 的键是 URL，原样展示。
        assert_eq!(
            connected_providers(r#"{"https://x.dev":{"type":"wellknown"}}"#).as_deref(),
            Some("https://x.dev (wellknown)")
        );

        // type 缺失（schema 漂移）→ 退回只显示名字，绝不因此判成「未连接」。
        assert_eq!(
            connected_providers(r#"{"anthropic":{}}"#).as_deref(),
            Some("anthropic")
        );
    }

    /// 环境变量整体覆盖文件——与 opencode 自己的取值顺序一致。只读文件的话，这类用户
    /// （容器 / CI / 用 env 注入凭据的）会被误判成「未登录」，而他们明明能正常用。
    #[test]
    fn auth_content_env_var_overrides_the_file() {
        let _env = crate::env_guard();
        std::env::set_var(AUTH_CONTENT_VAR, r#"{"anthropic":{"type":"oauth"}}"#);
        let ports = Ports { http: &crate::ports::test_doubles::NoHttp, keychain: &crate::ports::NoKeychain };
        // 本机 auth.json 大概率不存在，但环境变量提供了内容 → 仍算已连接。
        let inst = crate::by_id("opencode").unwrap().resolve().expect("总能推出默认落点");
        let acc = ACCOUNT
            .account(&inst, &ports)
            .expect("环境变量提供了凭据 → 应算已登录");
        assert_eq!(acc.login_label.as_deref(), Some("anthropic (oauth)"));

        // 空串不算数（设了但没值）。
        std::env::set_var(AUTH_CONTENT_VAR, "");
        // 此时回退读文件；本机没有该文件就是 None，有则不为空——两种都合法，只要不 panic。
        let _ = ACCOUNT.account(&inst, &ports);
    }

    /// 额度归背后的 provider 管——如实回 UNSUPPORTED。NoHttp 会在任何请求上 panic，
    /// 据此同时断言这条路径**不联网**。
    #[test]
    fn usage_is_declared_unsupported_without_network() {
        use crate::ports::test_doubles::NoHttp;
        let ports = Ports { http: &NoHttp, keychain: &crate::ports::NoKeychain };
        let inst = crate::by_id("opencode").unwrap().resolve().expect("总能推出默认落点");
        assert!(!ACCOUNT.usage_supported(&inst, &ports));
        assert_eq!(
            ACCOUNT.fetch_usage(&inst, &ports).unwrap_err(),
            USAGE_UNSUPPORTED
        );
    }
}
