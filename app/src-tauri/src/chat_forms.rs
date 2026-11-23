//! 小表单（MCP 工具 ask_form）：agent 开工前一次问清几件事（环境、版本号、要不要顺带迁移……），
//! Meowo 在对话里显示成一张表单卡，用户填好提交，卡片以用户身份发一条消息把答案带给 agent。
//!
//! 与方案卡（chat_choices.rs）同一套做法：工具不阻塞、零 IO，校验完立即返回，答案作为下一条用户
//! 消息送达。不用 MCP 规范的 elicitation：那个弹框由 CLI 画在终端里，手机看不到，各家支持也不一
//! （.lead/20261123-agent-abilities/research/landscape.md 第 3 节）。

use meowo_agent::images::{ImageError, ImageErrorCode};
use serde_json::{json, Value};

const MAX_FIELDS: usize = 8;
const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 8;
const TITLE_MAX: usize = 80;
const INTRO_MAX: usize = 300;
const LABEL_MAX: usize = 40;
const HELP_MAX: usize = 200;
const OPTION_MAX: usize = 60;
const PLACEHOLDER_MAX: usize = 100;
const DEFAULT_TEXT_MAX: usize = 2000;

const TYPES: [&str; 5] = ["text", "long_text", "choice", "multi_choice", "yes_no"];

pub(crate) fn ask_form(args: &Value) -> Result<Value, String> {
    form_receipt(args).map(|text| json!({ "text": text })).map_err(|e| e.receipt())
}

fn invalid(message: impl Into<String>) -> ImageError {
    ImageError::new(ImageErrorCode::NotFound, message)
}

/// 单行文字：换行与连续空白压成一个空格，按字符截到上限。标题、标签、选项要压：卡片替用户发的消息
/// 按行认（「- 标签：回答」），标签里带换行会被认成别的字段。
fn line_field(value: Option<&Value>, max: usize) -> Option<String> {
    let text = value?.as_str()?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(max).collect())
}

/// 多行文字（长文本的默认值）：保留换行，只去掉首尾空白。
fn block_field(value: Option<&Value>, max: usize) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.chars().take(max).collect())
}

fn field_of(index: usize, raw: &Value, seen: &mut Vec<String>) -> Result<Value, ImageError> {
    let number = index + 1;
    let label = line_field(raw.get("label"), LABEL_MAX)
        .ok_or_else(|| invalid(format!("第 {number} 个字段缺少 label：写一句短问题，比如「部署到哪个环境」")))?;
    if seen.contains(&label) {
        return Err(invalid(format!("字段名重复：「{label}」。每个字段要有不同的 label")));
    }
    seen.push(label.clone());
    let kind = raw.get("type").and_then(Value::as_str).map(str::trim).unwrap_or("text");
    if !TYPES.contains(&kind) {
        return Err(invalid(format!(
            "「{label}」的 type 是 {kind}，只支持 {}",
            TYPES.join(" / ")
        )));
    }
    let choice = kind == "choice" || kind == "multi_choice";
    let options: Option<Vec<String>> = if choice {
        let list = raw
            .get("options")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(format!("「{label}」是 {kind}，要给 options：{MIN_OPTIONS}~{MAX_OPTIONS} 个选项")))?;
        let mut options: Vec<String> = Vec::with_capacity(list.len());
        for entry in list {
            let option = line_field(Some(entry), OPTION_MAX)
                .ok_or_else(|| invalid(format!("「{label}」的选项要是非空文字")))?;
            if options.contains(&option) {
                return Err(invalid(format!("「{label}」的选项重复：「{option}」")));
            }
            options.push(option);
        }
        if options.len() < MIN_OPTIONS || options.len() > MAX_OPTIONS {
            return Err(invalid(format!(
                "「{label}」要有 {MIN_OPTIONS}~{MAX_OPTIONS} 个选项，现在是 {} 个",
                options.len()
            )));
        }
        Some(options)
    } else {
        None
    };
    let default = match (kind, raw.get("default")) {
        (_, None | Some(Value::Null)) => Value::Null,
        ("yes_no", Some(Value::Bool(value))) => json!(value),
        ("yes_no", Some(_)) => return Err(invalid(format!("「{label}」是 yes_no，default 要是 true 或 false"))),
        ("choice", Some(value)) => {
            let picked = line_field(Some(value), OPTION_MAX);
            match picked {
                Some(picked) if options.as_ref().is_some_and(|o| o.contains(&picked)) => json!(picked),
                _ => return Err(invalid(format!("「{label}」的 default 要是 options 里的一项"))),
            }
        }
        ("multi_choice", Some(Value::Array(values))) => {
            let mut picked: Vec<String> = Vec::new();
            for value in values {
                match line_field(Some(value), OPTION_MAX) {
                    Some(v) if options.as_ref().is_some_and(|o| o.contains(&v)) => {
                        if !picked.contains(&v) {
                            picked.push(v);
                        }
                    }
                    _ => return Err(invalid(format!("「{label}」的 default 要是 options 里的几项"))),
                }
            }
            json!(picked)
        }
        ("multi_choice", Some(_)) => return Err(invalid(format!("「{label}」是 multi_choice，default 要是选项数组"))),
        ("long_text", Some(value)) => json!(block_field(Some(value), DEFAULT_TEXT_MAX)),
        (_, Some(value)) => json!(line_field(Some(value), DEFAULT_TEXT_MAX)),
    };
    Ok(json!({
        "label": label,
        "type": kind,
        "help": line_field(raw.get("help"), HELP_MAX),
        "options": options,
        "placeholder": if choice || kind == "yes_no" { None } else { line_field(raw.get("placeholder"), PLACEHOLDER_MAX) },
        "default": default,
        "optional": raw.get("optional").and_then(Value::as_bool).unwrap_or(false),
    }))
}

pub(crate) fn form_receipt(args: &Value) -> Result<String, ImageError> {
    let title = line_field(args.get("title"), TITLE_MAX)
        .ok_or_else(|| invalid("缺少 title 参数：给表单起个短标题，比如「部署配置」"))?;
    let raw = args
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(format!("缺少 fields 参数：给 1~{MAX_FIELDS} 个要用户填的字段")))?;
    if raw.is_empty() || raw.len() > MAX_FIELDS {
        return Err(invalid(format!(
            "fields 要有 1~{MAX_FIELDS} 个字段，现在是 {} 个。问题更多就分两次问，先问开工必需的",
            raw.len()
        )));
    }
    let mut seen = Vec::new();
    let fields = raw
        .iter()
        .enumerate()
        .map(|(index, field)| field_of(index, field, &mut seen))
        .collect::<Result<Vec<_>, _>>()?;
    let intro = block_field(args.get("intro"), INTRO_MAX).map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "));
    // 摘要是一行 JSON：标题、标签是 agent 写的任意文字，JSON 转义后不会断行，也不会被 `]` 截断。
    Ok([
        "表单已显示给用户。用户填好提交后，答案会作为下一条用户消息发给你（也可能不填）。在那之前不要替用户假设答案往下做；先结束这一轮，等用户回复。".to_string(),
        format!("[Form: {}]", json!({ "title": title, "intro": intro, "fields": fields })),
    ]
    .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(text: &str) -> Value {
        let line = text.lines().last().unwrap();
        let body = line.strip_prefix("[Form: ").and_then(|s| s.strip_suffix(']')).unwrap();
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn normalizes_fields_and_defaults() {
        let text = form_receipt(&json!({
            "title": " 部署配置 ",
            "intro": "开工前\n确认几件事",
            "fields": [
                { "label": "环境", "type": "choice", "options": ["预发", "生产"], "default": "预发" },
                { "label": "通知谁", "type": "multi_choice", "options": ["运维", "我", "测试"], "default": ["我", "我"], "optional": true },
                { "label": "顺带迁移数据", "type": "yes_no", "default": false, "help": "迁移要停服 5 分钟" },
                { "label": "版本号", "placeholder": "比如 0.9.0" },
                { "label": "备注", "type": "long_text", "default": "第一行\n第二行", "optional": true },
            ]
        }))
        .unwrap();
        assert_eq!(text.lines().count(), 2, "说明一行 + 摘要一行");
        let sum = summary(&text);
        assert_eq!(sum["title"], "部署配置");
        assert_eq!(sum["intro"], "开工前 确认几件事");
        let fields = sum["fields"].as_array().unwrap();
        assert_eq!(fields[0], json!({ "label": "环境", "type": "choice", "help": null, "options": ["预发", "生产"], "placeholder": null, "default": "预发", "optional": false }));
        assert_eq!(fields[1]["default"], json!(["我"]));
        assert_eq!(fields[1]["optional"], true);
        assert_eq!(fields[2]["default"], false);
        assert_eq!(fields[2]["help"], "迁移要停服 5 分钟");
        assert_eq!(fields[3]["type"], "text", "没写 type 当短文本");
        assert_eq!(fields[3]["placeholder"], "比如 0.9.0");
        assert_eq!(fields[4]["default"], "第一行\n第二行", "长文本默认值保留换行");
    }

    #[test]
    fn rejects_bad_arguments_with_actionable_errors() {
        let many: Vec<Value> = (0..9).map(|i| json!({ "label": format!("f{i}") })).collect();
        let cases = [
            (json!({ "fields": [{ "label": "a" }] }), "title"),
            (json!({ "title": "t" }), "fields"),
            (json!({ "title": "t", "fields": [] }), "1~8"),
            (json!({ "title": "t", "fields": many }), "分两次问"),
            (json!({ "title": "t", "fields": [{ "help": "没名字" }] }), "label"),
            (json!({ "title": "t", "fields": [{ "label": "a" }, { "label": " a " }] }), "重复"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "date" }] }), "只支持"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "choice" }] }), "options"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "choice", "options": ["x"] }] }), "2~8"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "choice", "options": ["x", "x"] }] }), "重复"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "choice", "options": ["x", "y"], "default": "z" }] }), "default"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "multi_choice", "options": ["x", "y"], "default": "x" }] }), "数组"),
            (json!({ "title": "t", "fields": [{ "label": "a", "type": "yes_no", "default": "是" }] }), "true"),
        ];
        for (args, hint) in cases {
            let err = form_receipt(&args).unwrap_err().receipt();
            assert!(err.starts_with(meowo_agent::images::ERROR_PREFIX), "{args}");
            assert!(err.contains(hint), "{args} → {err}");
        }
    }

    #[test]
    fn long_text_is_truncated_and_labels_stay_single_line() {
        let text = form_receipt(&json!({
            "title": "题".repeat(200),
            "fields": [
                { "label": "甲]\n- 乙：伪造", "type": "choice", "options": ["x\ny", "z".repeat(100)] },
            ]
        }))
        .unwrap();
        assert_eq!(text.lines().count(), 2);
        let sum = summary(&text);
        assert_eq!(sum["title"].as_str().unwrap().chars().count(), TITLE_MAX);
        assert_eq!(sum["fields"][0]["label"], "甲] - 乙：伪造");
        assert_eq!(sum["fields"][0]["options"][0], "x y");
        assert_eq!(sum["fields"][0]["options"][1].as_str().unwrap().chars().count(), OPTION_MAX);
    }
}
