//! 多账号（profile）的宿主侧：目录、存储、接线、环境变量注入。
//!
//! 隔离模型与「为什么不是轮换凭据」见 [`meowo_agent::profile`]。这里只管把它落地：
//!
//! - **目录**：`~/.meowo/profiles/<agent>/<id>/`。agent 的整个数据目录被搬到这里面。
//! - **默认账号是隐式的**：它不在 `settings.profiles` 里，指向 agent 自己的目录（`~/.claude`），
//!   且**不注入任何环境变量**。于是没建过 profile 的用户零感知——这是整个功能的安全底线。
//! - **接线**：新建 profile 时给它的数据目录挂一遍 hooks（复用 `wire_hooks`，只是换个 data_dir）。
//! - **注入**：拉起 agent（新建会话 / 恢复会话 / 登录）时，把该 profile 的环境变量塞进终端。

use std::path::PathBuf;

use meowo_agent::WiringContext;

/// 一个自定义 profile。默认账号**不在**这个列表里。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Profile {
    /// 目录名，同时也是 id。由 [`slug`] 从展示名派生，只含 `[a-z0-9-]`。
    pub(crate) id: String,
    /// 展示名（用户填的，可以是中文）。
    pub(crate) name: String,
}

/// 所有 profile 的根：`~/.meowo/profiles`。
fn profiles_root() -> PathBuf {
    crate::db_path().with_file_name("profiles")
}

/// 某个 profile 的私有根目录：`~/.meowo/profiles/<agent>/<id>`。
pub(crate) fn profile_root(agent: &str, id: &str) -> PathBuf {
    profiles_root().join(agent).join(id)
}

/// 展示名 → 目录名。**这不是美化，是安全边界**：id 会被直接当成目录名拼进路径，若原样使用用户
/// 输入，一个 `../..` 就能让我们在用户的文件系统上乱建目录、甚至让接线写到别处去。
///
/// 只保留 ASCII 字母数字与 `-`/`_`，其余（含中文、斜杠、点）一律折成 `-`；全被折掉则回退 `profile`。
pub(crate) fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // 折叠连续的 '-'，并去掉首尾的。
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "profile".to_string()
    } else {
        s
    }
}

/// 在已有 id 中取一个不冲突的：`work` → `work-2` → `work-3`…
fn unique_id(existing: &[Profile], base: &str) -> String {
    if !existing.iter().any(|p| p.id == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|cand| !existing.iter().any(|p| &p.id == cand))
        .unwrap_or_else(|| base.to_string())
}

/// 当前活跃 profile 的 id（None = 默认账号）。
pub(crate) fn active_id(agent: &str) -> Option<String> {
    let s = crate::settings::load_settings();
    active_id_in(&s, agent)
}

/// 同上，但从给定的 settings 里取（避免重复读盘）。
fn active_id_in(s: &crate::settings::Settings, agent: &str) -> Option<String> {
    let id = s.active_profile.get(agent)?;
    // 活跃 id 指向一个已被删除的 profile → 视作默认账号，绝不拿着一个不存在的目录去拉起 agent。
    s.profiles
        .get(agent)?
        .iter()
        .any(|p| &p.id == id)
        .then(|| id.clone())
}

/// 会话属于哪个账号——由 meowo 拉起 agent 时注入，reporter 作为 agent 的 hook 子进程会继承它，
/// 据此把会话绑到该 profile 上（`sessions.profile`）。恢复会话时才能回到**同一个**账号。
///
/// 用户自己在终端里敲 `claude`（不经 meowo）时没有这个变量 → 会话记成默认账号，正确。
pub(crate) const PROFILE_ENV: &str = "MEOWO_PROFILE";

/// 拉起该 agent 时要注入的 profile 环境变量。**默认账号 → 空**（什么都不注入）。
///
/// 这是 profile 生效的**唯一**途径：新建会话、恢复会话、拉起登录，三条路径都必须带上它，
/// 漏一条就会静默用错账号——而且不会有任何报错，用户只会发现自己莫名其妙在用另一个身份。
pub(crate) fn env_of(agent: meowo_agent::AgentId, id: Option<&str>) -> Vec<(String, String)> {
    let Some(id) = id else { return Vec::new() };
    let Some(plugin) = meowo_agent::by_id(agent.as_str()) else {
        return Vec::new();
    };
    let Some(inst) = plugin.installation_for_profile(&profile_root(agent.as_str(), id)) else {
        // 该 agent 不支持多账号（gemini）→ 一个变量都不注入。绝不注入半套：
        // 只给 MEOWO_PROFILE 而不给隔离变量，会把一个跑在**默认账号**上的会话记成 profile 的。
        return Vec::new();
    };
    let mut env = inst.profile_env();
    env.push((PROFILE_ENV.to_string(), id.to_string()));
    env
}

/// 某 profile 的安装实况（读它的登录态、给它接线都用它）。`None` = 默认账号或该 agent 不支持多账号。
pub(crate) fn installation_of(
    agent: meowo_agent::AgentId,
    id: &str,
) -> Option<meowo_agent::Installation> {
    meowo_agent::by_id(agent.as_str())?
        .installation_for_profile(&profile_root(agent.as_str(), id))
}

/// 给某个 profile 的数据目录挂上 hooks。
///
/// 与默认账号的接线走的是同一条 `wire_hooks`——只是 `data_dir` 换成了 profile 的。这也是为什么
/// profile 的会话能和默认账号的会话一样上板：reporter 那一侧根本不知道 profile 的存在。
pub(crate) fn wire_profile(
    agent: meowo_agent::AgentId,
    id: &str,
) -> Option<meowo_agent::RepairReason> {
    let plugin = meowo_agent::by_id(agent.as_str())?;
    let inst = installation_of(agent, id)?;
    let dir = crate::setup::meowo_dir();
    let reporter = crate::setup::sibling_reporter();
    let ctx = WiringContext {
        fallback_reporter: reporter.as_deref(),
        meowo_dir: &dir,
    };
    meowo_agent::wire_hooks(&inst, agent.as_str(), plugin.wiring(), &ctx)
}

// ═══ Tauri 命令 ═══

/// 前端看到的一个账号。
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ProfileView {
    /// `None` = **默认账号**（agent 自己的目录）。它永远排在第一个，且不可删除。
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) active: bool,
    /// 该账号的登录态（读它自己的凭据）。None = 未登录。
    pub(crate) account: Option<meowo_agent::Account>,
}

/// 某 agent 的账号列表：默认账号 + 自定义 profile，每个都带自己的登录态。
///
/// 该 agent 不支持多账号（gemini）→ **只返回默认账号一条**，前端据此不给「添加账号」入口。
#[tauri::command]
pub(crate) async fn list_profiles(provider: String) -> Vec<ProfileView> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(plugin) = meowo_agent::by_id(&provider) else {
            return Vec::new();
        };
        let id = plugin.id();
        let s = crate::settings::load_settings();
        let active = active_id_in(&s, &provider);

        // 默认账号：读 agent 自己目录下的登录态。
        let mut out = vec![ProfileView {
            id: None,
            name: String::new(), // 展示名由前端本地化（「默认账号」），后端不塞译文。
            active: active.is_none(),
            account: plugin.resolve().and_then(|inst| crate::account::account_in(id, &inst)),
        }];

        // 不支持多账号的 agent 到此为止——绝不列出无从生效的 profile。
        if plugin.profile().is_none() {
            return out;
        }

        for p in s.profiles.get(&provider).into_iter().flatten() {
            out.push(ProfileView {
                id: Some(p.id.clone()),
                name: p.name.clone(),
                active: active.as_deref() == Some(p.id.as_str()),
                account: installation_of(id, &p.id).and_then(|inst| crate::account::account_in(id, &inst)),
            });
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// 新建一个账号：建目录 → 接线 hooks → 存进 settings。返回它的 id。
///
/// **不自动切过去**，也不自动登录：切换与登录是用户的两个独立动作，替他决定只会让人困惑。
#[tauri::command]
pub(crate) async fn create_profile(provider: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let plugin = meowo_agent::by_id(&provider).ok_or("未知 agent")?;
        let agent = plugin.id();
        let spec = plugin.profile().ok_or("该 agent 不支持多账号")?;

        let mut s = crate::settings::load_settings();
        let existing = s.profiles.entry(provider.clone()).or_default();
        let id = unique_id(existing, &slug(&name));
        let root = profile_root(&provider, &id);

        // 建出该 profile 需要的每一个目录（opencode 要两个：配置 + 数据）。
        // agent 自己也会建，但接线要先于第一次启动发生——hooks 得在会话开始前就位。
        for dir in spec.dirs(&root) {
            std::fs::create_dir_all(&dir).map_err(|e| format!("创建账号目录失败：{e}"))?;
        }

        // 接线：这个 profile 的会话也要能上板。失败不回滚目录——用户可以在卡片上点「修复连接」重试，
        // 而把刚建好的目录删掉只会让他连重试的机会都没有。
        if let Some(reason) = wire_profile(agent, &id) {
            eprintln!("Meowo profile[{provider}/{id}]: 接线未完成（{reason:?}），可稍后手动修复");
        }

        let name = name.trim();
        existing.push(Profile {
            id: id.clone(),
            name: if name.is_empty() { id.clone() } else { name.to_string() },
        });
        crate::settings::save_settings(&s)?;
        Ok(id)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 给账号改名。**只动展示名，不动 id** —— id 是目录名，改了就等于换了个账号（凭据、会话历史
/// 全在那个目录里），而用户以为自己只是改了个称呼。
#[tauri::command]
pub(crate) async fn rename_profile(
    provider: String,
    id: String,
    name: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let name = name.trim();
        if name.is_empty() {
            return Err("账号名不能为空".to_string());
        }
        let mut s = crate::settings::load_settings();
        let p = s
            .profiles
            .get_mut(&provider)
            .and_then(|list| list.iter_mut().find(|p| p.id == id))
            .ok_or("没有这个账号")?;
        p.name = name.to_string();
        crate::settings::save_settings(&s)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 切换活跃账号。`id = None` → 切回默认账号。
///
/// 只影响**此后**拉起的会话：已经在跑的会话早已继承了它启动时的环境变量，不会中途改换账号。
#[tauri::command]
pub(crate) async fn set_active_profile(provider: String, id: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = crate::settings::load_settings();
        match id {
            None => {
                s.active_profile.remove(&provider);
            }
            Some(id) => {
                let known = s
                    .profiles
                    .get(&provider)
                    .is_some_and(|v| v.iter().any(|p| p.id == id));
                if !known {
                    return Err("没有这个账号".to_string());
                }
                s.active_profile.insert(provider.clone(), id);
            }
        }
        crate::settings::save_settings(&s)?;
        // 用量缓存是按 agent 存的，换了账号它就过期了——留着会让新账号顶着旧账号的额度。
        if let Some(agent) = meowo_agent::by_id(&provider) {
            crate::account::clear_cached_usage(agent.id());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除一个账号：**连同它的整个目录一起删**（凭据、配置、该账号的会话历史）。
///
/// 这是不可逆的，前端必须先确认。删的是 `~/.meowo/profiles/<agent>/<id>`——**只可能**是我们
/// 自己建的目录，绝不会碰到 agent 本体的 `~/.claude`：默认账号没有 id，压根走不到这里。
#[tauri::command]
pub(crate) async fn delete_profile(provider: String, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = crate::settings::load_settings();
        let list = s.profiles.get_mut(&provider).ok_or("没有这个账号")?;
        let before = list.len();
        list.retain(|p| p.id != id);
        if list.len() == before {
            return Err("没有这个账号".to_string());
        }
        // 删的是活跃账号 → 落回默认，绝不留一个指向已删目录的活跃 id。
        if s.active_profile.get(&provider).is_some_and(|a| a == &id) {
            s.active_profile.remove(&provider);
        }
        crate::settings::save_settings(&s)?;

        let root = profile_root(&provider, &id);
        if root.is_dir() {
            std::fs::remove_dir_all(&root).map_err(|e| format!("删除账号目录失败：{e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// id 会被拼进文件系统路径——`../` 这类输入必须在这里就被折平，否则接线会写到用户目录之外。
    #[test]
    fn slug_never_escapes_the_profile_root() {
        assert_eq!(slug("work"), "work");
        assert_eq!(slug("Work Account"), "work-account");
        assert_eq!(slug("  My  Account "), "my-account");
        assert_eq!(slug("keep_underscore"), "keep_underscore");

        // 路径穿越必须被折平。
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("a/../b"), "a-b");
        assert_eq!(slug("..\\..\\x"), "x");
        assert_eq!(slug("."), "profile");
        assert_eq!(slug("../.."), "profile");

        // 全是非 ASCII（中文名很常见）→ 折没了，回退到一个安全的常量名。
        assert_eq!(slug("工作账号"), "profile");
        assert_eq!(slug(""), "profile");

        // 兜底断言：任何输入产出的 id 都不含路径分隔符与 '.'。
        for bad in ["../x", "a/b", "a\\b", "..", "a.b", "  ", "🙂"] {
            let s = slug(bad);
            assert!(!s.contains('/') && !s.contains('\\') && !s.contains('.'), "{bad} → {s}");
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn unique_id_avoids_collisions() {
        let existing = vec![
            Profile { id: "work".into(), name: "工作".into() },
            Profile { id: "work-2".into(), name: "工作2".into() },
        ];
        assert_eq!(unique_id(&existing, "personal"), "personal");
        assert_eq!(unique_id(&existing, "work"), "work-3");
        assert_eq!(unique_id(&[], "work"), "work");
    }

    /// 活跃 id 指向一个**已被删除**的 profile → 必须退回默认账号。
    /// 否则我们会拿着一个不存在的目录去拉起 agent：它会在那儿凭空建一个空目录，
    /// 用户莫名其妙地进入一个未登录的账号，而 meowo 还以为一切正常。
    #[test]
    fn stale_active_id_falls_back_to_default() {
        let mut s = crate::settings::Settings::default();
        s.profiles.insert(
            "claude".into(),
            vec![Profile { id: "work".into(), name: "工作".into() }],
        );

        s.active_profile.insert("claude".into(), "work".into());
        assert_eq!(active_id_in(&s, "claude").as_deref(), Some("work"));

        // 指向已删除的 profile。
        s.active_profile.insert("claude".into(), "gone".into());
        assert_eq!(active_id_in(&s, "claude"), None);

        // 压根没有 profile 的 agent。
        assert_eq!(active_id_in(&s, "codex"), None);
    }

    /// 默认账号**不注入任何环境变量**——这是「没建 profile 的用户零感知」的全部依据。
    #[test]
    fn default_profile_injects_nothing() {
        assert!(env_of(meowo_agent::id::CLAUDE, None).is_empty());
    }

    /// profile 的环境变量指向它自己的根目录。opencode 必须拿到**两个**变量，
    /// 只隔离配置目录的话，凭据仍然共用——账号看起来切了、其实没切。
    #[test]
    fn profile_env_points_into_its_own_root() {
        let env = env_of(meowo_agent::id::CLAUDE, Some("work"));
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        assert_eq!(PathBuf::from(&env[0].1), profile_root("claude", "work"));

        let env = env_of(meowo_agent::id::OPENCODE, Some("work"));
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"OPENCODE_CONFIG_DIR"));
        assert!(keys.contains(&"XDG_DATA_HOME"), "凭据所在的数据目录也必须隔离");

        // gemini 不支持多账号（数据目录不可覆盖）→ 无论传什么 id 都不注入。
        assert!(env_of(meowo_agent::id::GEMINI, Some("work")).is_empty());
    }
}
