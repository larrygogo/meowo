//! `agy models` 的输出 → 模型目录。
//!
//! 实测 1.5.18：首行 `Fetching available models...`，其后每行 `<id>\t<显示名>`，如
//! `gemini-4-neon-high\tGemini 4 Neon (High)`。effort 编码在 id 里（同一模型 low/medium/high
//! 各一个 id），所以每个 id 单列一项，不再拆 effort。
//!
//! 当前默认：settings.json 的 `model` 按**显示名**保存（settings.md「saved by its label」）。

use std::path::Path;

use crate::ModelCatalogEntry;

pub(super) fn parse_catalog(output: &str, data_dir: Option<&Path>) -> Vec<ModelCatalogEntry> {
    let saved = data_dir.and_then(saved_model);
    output
        .lines()
        .filter_map(|line| {
            let (id, label) = line.split_once('\t')?;
            let (id, label) = (id.trim(), label.trim());
            (!id.is_empty() && !id.contains(char::is_whitespace)).then(|| {
                let mut entry = ModelCatalogEntry::new(id, if label.is_empty() { id } else { label });
                entry.is_default = saved.as_deref().is_some_and(|s| s == label || s == id);
                entry
            })
        })
        .collect()
}

fn saved_model(data_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join("settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("model")?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_separated_lines_and_skips_the_banner() {
        let out = "Fetching available models...\ngemini-4-neon-high\tGemini 4 Neon (High)\ngemini-3.8-flash-low\tGemini 3.8 Flash (Low)\n";
        let catalog = parse_catalog(out, None);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].id, "gemini-4-neon-high");
        assert_eq!(catalog[0].label, "Gemini 4 Neon (High)");
        assert!(!catalog[0].is_default);
    }

    #[test]
    fn saved_label_marks_the_default() {
        let dir = std::env::temp_dir().join(format!("meowo-agy-models-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"model":"Gemini 4 Neon (High)"}"#).unwrap();
        let catalog = parse_catalog("a\tA\ngemini-4-neon-high\tGemini 4 Neon (High)\n", Some(&dir));
        assert!(!catalog[0].is_default);
        assert!(catalog[1].is_default);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
