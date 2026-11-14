//! Gemini 的模型清单与新建页默认值。
//!
//! 清单是 CLI 内置常量再按认证方式过滤，不读配置也不联网列举（0.68 bundle chunk-RCB6BI5H.js
//! 280306-280374 常量表、interactiveCli 17041-17300 的 `/model` 对话框，research-kimi-gemini §3.1）。
//! `gemini models list` 跳过了认证初始化、结果不全，不能用。

use serde_json::Value;
use std::path::Path;

/// api-key / vertex / gateway 认证下 `/model` 手动页的 7 项（`e3-04-model-manual.screen.txt` 实拍）
/// 加 Auto。这几种认证方式的预览、3.1 Pro、最新 flash 权限都直接为真，离线就能确定。
const FULL: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("gemini-3.1-pro-preview", "gemini-3.1-pro-preview"),
    ("gemini-3-flash-preview", "gemini-3-flash-preview"),
    ("gemini-2.5-pro", "gemini-2.5-pro"),
    ("gemini-3.8-flash", "gemini-3.8-flash"),
    ("gemini-3.5-flash-lite", "gemini-3.5-flash-lite"),
    ("gemma-4-31b-it", "gemma-4-31b-it"),
    ("gemma-4-26b-a4b-it", "gemma-4-26b-a4b-it"),
];

/// 个人 Google 账号（oauth）：预览与 Pro 权限看服务端配额和实验开关，离线算不准。只给对话框基础
/// 三项（2.5-pro、当前 flash、当前 flash-lite）加 Auto，并整份标「以服务端为准」。
const CONSERVATIVE: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("gemini-2.5-pro", "gemini-2.5-pro"),
    ("gemini-3.5-flash", "gemini-3.5-flash"),
    ("gemini-3.1-flash-lite", "gemini-3.1-flash-lite"),
];

/// 认证方式能离线确定完整清单的那几种（G:384006-384095）。
fn offline_certain(auth: Option<&str>) -> bool {
    matches!(
        auth,
        Some("gemini-api-key" | "vertex-ai" | "gateway" | "gemini-api" | "use_gemini" | "vertex")
    )
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&crate::settings::strip_json_comments(&text)).ok()
}

/// 用户级与项目级 settings.json，项目覆盖用户（包内 docs/reference/configuration.md 9-17 的合并顺序，
/// 系统级文件不读——Meowo 不碰那里）。
fn settings(data_dir: &Path, cwd: Option<&Path>) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(user) = read_json(&data_dir.join("settings.json")) {
        out.push(user);
    }
    if let Some(project) = cwd.and_then(|cwd| read_json(&cwd.join(".gemini").join("settings.json")))
    {
        out.push(project);
    }
    out
}

fn last_str<'a>(layers: &'a [Value], pointer: &str) -> Option<&'a str> {
    layers
        .iter()
        .rev()
        .find_map(|layer| layer.pointer(pointer).and_then(Value::as_str))
}

pub(super) fn catalog(data_dir: &Path, cwd: Option<&Path>) -> Vec<crate::ModelCatalogEntry> {
    let layers = settings(data_dir, cwd);
    // 环境变量里有 GEMINI_API_KEY 时 CLI 也走 api-key（未在 settings 里选时的回退），按同口径认。
    let auth = last_str(&layers, "/security/auth/selectedType")
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("GEMINI_API_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .map(|_| "gemini-api-key".to_string())
        });
    let certain = offline_certain(auth.as_deref());
    let table = if certain { FULL } else { CONSERVATIVE };
    let configured = last_str(&layers, "/model/name");
    let mut out: Vec<crate::ModelCatalogEntry> = table
        .iter()
        .map(|(id, label)| {
            let mut entry = crate::ModelCatalogEntry::new(*id, *label);
            entry.uncertain = !certain && *id != "auto";
            entry.is_default = configured.map_or(*id == "auto", |name| name == *id);
            entry
        })
        .collect();
    // settings 里配了表外的模型（`model.name`）：CLI 照样用它，清单并进去，否则当前模型对不上号。
    if let Some(name) = configured.filter(|name| !table.iter().any(|(id, _)| id == name)) {
        let mut entry = crate::ModelCatalogEntry::new(name, name);
        entry.is_default = true;
        entry.uncertain = !certain;
        out.push(entry);
    }
    out
}

/// 新建页默认值：`model.name`（缺省 auto）与 `general.defaultApprovalMode`（只收 default、
/// auto_edit、plan；写成 yolo 被 CLI 静默忽略，G:chunk-VFM6AETN.js 34024）。
pub(super) fn launch_defaults(data_dir: &Path, cwd: Option<&Path>) -> crate::LaunchDefaults {
    let layers = settings(data_dir, cwd);
    let mut out = crate::LaunchDefaults {
        model: last_str(&layers, "/model/name").map(str::to_owned),
        ..Default::default()
    };
    let plan_enabled = layers
        .iter()
        .rev()
        .find_map(|layer| {
            layer
                .pointer("/general/plan/enabled")
                .and_then(Value::as_bool)
        })
        .unwrap_or(true);
    let approval = match last_str(&layers, "/general/defaultApprovalMode") {
        Some("auto_edit") => Some("autoEdit"),
        Some("plan") if plan_enabled => Some("plan"),
        Some("default") | Some("plan") => Some("default"),
        _ => None,
    };
    if let Some(approval) = approval {
        out.choices.insert("approval".into(), approval.into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("meowo-gemini-models-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn home(tag: &str, settings: &str) -> Dir {
        let dir = Dir::new(tag);
        std::fs::write(dir.path().join("settings.json"), settings).unwrap();
        dir
    }

    #[test]
    fn api_key_auth_lists_the_full_offline_table() {
        let dir = home(
            "apikey",
            r#"{"security":{"auth":{"selectedType":"gemini-api-key"}}}"#,
        );
        let list = catalog(dir.path(), None);
        let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), 8);
        assert!(ids.contains(&"gemini-3.8-flash") && ids.contains(&"gemma-4-31b-it"));
        assert!(list.iter().all(|e| !e.uncertain));
        assert!(list.iter().find(|e| e.id == "auto").unwrap().is_default);
    }

    #[test]
    fn oauth_gets_the_conservative_list_marked_uncertain() {
        let dir = home(
            "oauth",
            r#"{"security":{"auth":{"selectedType":"oauth-personal"}},"model":{"name":"gemini-2.5-pro"}}"#,
        );
        let list = catalog(dir.path(), None);
        assert!(list
            .iter()
            .any(|e| e.id == "gemini-2.5-pro" && e.is_default));
        assert!(list.iter().filter(|e| e.id != "auto").all(|e| e.uncertain));
        assert!(!list.iter().any(|e| e.id == "gemini-3.1-pro-preview"));
    }

    #[test]
    fn defaults_merge_project_over_user_and_ignore_yolo() {
        let dir = home(
            "user",
            r#"{"model":{"name":"gemini-2.5-pro"},"general":{"defaultApprovalMode":"auto_edit"}}"#,
        );
        let project = Dir::new("project");
        std::fs::create_dir_all(project.path().join(".gemini")).unwrap();
        std::fs::write(
            project.path().join(".gemini").join("settings.json"),
            r#"{"general":{"defaultApprovalMode":"plan"}}"#,
        )
        .unwrap();
        let merged = launch_defaults(dir.path(), Some(project.path()));
        assert_eq!(merged.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(
            merged.choices.get("approval").map(String::as_str),
            Some("plan")
        );
        let yolo = home("yolo", r#"{"general":{"defaultApprovalMode":"yolo"}}"#);
        assert!(launch_defaults(yolo.path(), None).choices.is_empty());
    }
}
