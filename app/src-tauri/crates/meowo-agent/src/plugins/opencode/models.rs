//! OpenCode 的离线清单与新建页默认值（会话没起、没有 HTTP 服务时用）。
//!
//! 有服务时清单来自 `GET /config/providers`、`GET /agent`（宿主 lead_native/opencode_http）；
//! 没服务时 `opencode models --verbose` 与 `opencode agent list` 会自己做配置合并（各起一个进程
//! 约 1~2 秒，research-codex-opencode §1/§7），输出格式 2026-11-14 隔离 XDG 目录实测 1.19.5。

use serde_json::Value;
use std::path::Path;

/// opencode 自己用、不让用户选的 primary agent（`GET /agent` 里 hidden=true，oc1 ev.json）。
/// `agent list` 不带 hidden 字段，只能按名字排除。
const HIDDEN_PRIMARY: &[&str] = &["compaction", "summary", "title"];
/// 插件静态声明了的两档（带统一说法），动态清单里不再重复。
const DECLARED: &[&str] = &["build", "plan"];

/// `opencode models [provider] --verbose`：每个模型一行 `provider/id`，后面跟一段缩进的 JSON
/// （含 `name`、`variants`）。只取行首不缩进、形如 `a/b` 的那些行开段。
pub(super) fn parse_models_verbose(output: &str) -> Vec<crate::ModelCatalogEntry> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None;
    let flush = |current: &mut Option<(String, String)>, out: &mut Vec<crate::ModelCatalogEntry>| {
        let Some((id, body)) = current.take() else {
            return;
        };
        let json: Option<Value> = serde_json::from_str(&body).ok();
        let label = json
            .as_ref()
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map_or_else(|| id.clone(), str::to_string);
        let mut entry = crate::ModelCatalogEntry::new(id.clone(), label);
        entry.group = id.split_once('/').map(|(provider, _)| provider.to_string());
        entry.efforts = json
            .as_ref()
            .and_then(|v| v.get("variants"))
            .and_then(Value::as_object)
            .map(|variants| variants.keys().cloned().collect())
            .unwrap_or_default();
        out.push(entry);
    };
    for line in output.lines() {
        let head = !line.starts_with(char::is_whitespace)
            && !line.starts_with('{')
            && !line.starts_with('}')
            && line.contains('/')
            && !line.contains(' ');
        if head {
            flush(&mut current, &mut out);
            current = Some((line.trim().to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    flush(&mut current, &mut out);
    out
}

/// `opencode agent list`：每个 agent 一行 `name (primary|subagent|all)`，后面是缩进的权限规则。
/// 只要用户能直接选的 primary / all，去掉内部用的与已静态声明的。
pub(super) fn parse_agent_list(output: &str) -> Vec<crate::DynamicModeChoice> {
    output
        .lines()
        .filter(|line| !line.starts_with(char::is_whitespace))
        .filter_map(|line| {
            let (name, mode) = line.trim().split_once(' ')?;
            let mode = mode.strip_prefix('(')?.strip_suffix(')')?;
            matches!(mode, "primary" | "all").then_some(name)
        })
        .filter(|name| !HIDDEN_PRIMARY.contains(name) && !DECLARED.contains(name))
        .filter(|name| {
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
        .map(|name| crate::DynamicModeChoice {
            option: "agent".into(),
            id: name.to_string(),
            label: name.to_string(),
        })
        .collect()
}

fn read_config(dir: &Path) -> Option<Value> {
    ["opencode.jsonc", "opencode.json"].iter().find_map(|name| {
        let text = std::fs::read_to_string(dir.join(name)).ok()?;
        serde_json::from_str(&crate::settings::strip_json_comments(&text)).ok()
    })
}

/// 新建页默认值：用户级 `~/.config/opencode/opencode.json(c)`，项目目录的同名文件覆盖它。
/// `model` 是 `provider/model`，`default_agent` 是「会话没选 agent 时用的 primary agent」
/// （官方文档 config#default-agent）。
pub(super) fn launch_defaults(config_dir: &Path, cwd: Option<&Path>) -> crate::LaunchDefaults {
    let layers: Vec<Value> = [Some(config_dir), cwd]
        .into_iter()
        .flatten()
        .filter_map(read_config)
        .collect();
    let pick = |key: &str| {
        layers
            .iter()
            .rev()
            .find_map(|layer| layer.get(key).and_then(Value::as_str))
            .map(str::to_string)
    };
    let mut out = crate::LaunchDefaults {
        model: pick("model"),
        ..Default::default()
    };
    if let Some(agent) = pick("default_agent") {
        out.choices.insert("agent".into(), agent);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1.19.5 `opencode models opencode --verbose` 原文节选（隔离 XDG 目录实测）。
    #[test]
    fn verbose_models_read_name_and_variants() {
        let output = r#"opencode/beam-free
{
  "id": "beam-free",
  "providerID": "opencode",
  "name": "Beam Free",
  "variants": {
    "low": {
      "reasoningEffort": "low"
    },
    "max": {
      "reasoningEffort": "max"
    }
  }
}
opencode/big-pickle
{
  "id": "big-pickle",
  "name": "Big Pickle",
  "variants": {}
}
"#;
        let list = parse_models_verbose(output);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "opencode/beam-free");
        assert_eq!(list[0].label, "Beam Free");
        assert_eq!(list[0].group.as_deref(), Some("opencode"));
        assert_eq!(list[0].efforts, vec!["low", "max"]);
        assert!(list[1].efforts.is_empty());
    }

    /// 1.19.5 `opencode agent list` 的行首形态（规则 JSON 缩进在下面）。
    #[test]
    fn agent_list_keeps_user_primary_agents_only() {
        let output = "build (primary)\n  [\n  ]\ncompaction (primary)\n]\nexplore (subagent)\n]\nplan (primary)\n]\nreviewer (primary)\n  [\n]\nwriter (all)\n]\n";
        let ids: Vec<String> = parse_agent_list(output).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["reviewer", "writer"]);
    }
}
