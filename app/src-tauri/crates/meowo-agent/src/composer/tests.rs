use super::*;
use crate::chat_ui::{ComposerAltMode, ComposerFidelity, ComposerStash};

fn leak(value: &serde_json::Value) -> &'static str {
    Box::leak(value.as_str().unwrap().to_string().into_boxed_str())
}

/// 样例文件里的规格还原成 ComposerSpec（Kimi 的规格还要求装着 modern 版，本机没装时插件
/// 返回 None，只能用样例里的规格测判定）。
fn spec_from(declared: &serde_json::Value) -> ComposerSpec {
    let list = |value: &serde_json::Value| -> &'static [&'static str] {
        Box::leak(
            value
                .as_array()
                .unwrap()
                .iter()
                .map(leak)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
    };
    let optional = |value: &serde_json::Value| value.as_str().map(|_| leak(value));
    let modes: Vec<ComposerAltMode> = declared["alt_modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mode| ComposerAltMode {
            id: leak(&mode["id"]),
            prompt_pattern: optional(&mode["prompt_pattern"]),
            below_pattern: optional(&mode["below_pattern"]),
            exit_input: leak(&mode["exit_input"]),
        })
        .collect();
    ComposerSpec {
        prompt_footer: declared["prompt_footer"].as_bool().unwrap(),
        input_start_pattern: leak(&declared["input_start_pattern"]),
        top_pattern: leak(&declared["top_pattern"]),
        bottom_pattern: leak(&declared["bottom_pattern"]),
        row_pattern: leak(&declared["row_pattern"]),
        prompt_prefix: leak(&declared["prompt_prefix"]),
        tail_rows: declared["tail_rows"].as_u64().unwrap() as usize,
        tail_pattern: optional(&declared["tail_pattern"]),
        empty_pattern: leak(&declared["empty_pattern"]),
        send_requires_recognition: declared["send_requires_recognition"].as_bool().unwrap(),
        stash: declared["stash"].as_object().map(|_| ComposerStash {
            input: leak(&declared["stash"]["input"]),
            stashed_marker: leak(&declared["stash"]["stashed_marker"]),
        }),
        clear_inputs: list(&declared["clear_inputs"]),
        clear_rounds: declared["clear_rounds"].as_u64().unwrap() as u8,
        clear_while_running: declared["clear_while_running"].as_bool().unwrap(),
        alt_modes: Box::leak(modes.into_boxed_slice()),
        fidelity: declared["fidelity"].as_object().map(|_| {
            let rule = &declared["fidelity"];
            ComposerFidelity {
                paste_pattern: leak(&rule["paste_pattern"]),
                overflow_pattern: optional(&rule["overflow_pattern"]),
                overflow_rows: rule["overflow_rows"].as_u64().map(|rows| rows as usize),
                wrap_edge: match rule["wrap_edge"].as_str().unwrap() {
                    "border" => ComposerWrapEdge::Border,
                    "terminal" => ComposerWrapEdge::Terminal,
                    other => panic!("没见过的 wrap_edge：{other}"),
                },
                wrap_inset: rule["wrap_inset"].as_u64().unwrap() as usize,
            }
        }),
    }
}

/// 宿主与前端的输入框判定对同一批实拍帧必须给出同样的结论（做空 N2）。帧与规格放在前端
/// 测试也读的 app/src/test/fixtures/composer-frames.json；规格先与插件声明逐字核对，免得
/// 样例里的规格过期。
#[test]
fn reads_agree_with_shared_frames() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../src/test/fixtures/composer-frames.json"
    ))
    .unwrap();
    let mut checked = 0;
    let mut drafts = 0;
    let mut providers = std::collections::BTreeSet::new();
    for frame in fixture["frames"].as_array().unwrap() {
        let provider = frame["provider"].as_str().unwrap();
        let declared = &fixture["specs"][provider];
        let plugin = crate::by_id(provider).unwrap();
        if let Some(spec) = plugin.composer(frame["version"].as_str()) {
            assert_eq!(
                &serde_json::to_value(spec).unwrap(),
                declared,
                "{provider} 样例里的规格与插件声明不一致"
            );
        }
        let spec = spec_from(declared);
        let lines: Vec<String> = frame["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line.as_str().unwrap().to_string())
            .collect();
        let trimmed: Vec<String> = lines
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect();
        let cursor = frame.get("cursor").map(|cursor| Cursor {
            row: cursor["row"].as_u64().unwrap() as usize,
            col: cursor["col"].as_u64().unwrap() as usize,
            visible: cursor["visible"].as_bool().unwrap(),
            dim: cursor["dim"].as_bool().unwrap(),
        });
        let expect = frame["expect"].as_str().unwrap();
        let cols = frame["cols"].as_u64().map(|cols| cols as usize);
        // rawOnly：这一帧去掉行尾空格后本来就认不出（Gemini 草稿里的空行是一串空格），只按原样判。
        let raw_only = frame["rawOnly"].as_bool().unwrap_or(false);
        let screens: &[&Vec<String>] = if raw_only { &[&lines] } else { &[&lines, &trimmed] };
        for screen in screens {
            let got = read_with_cols(screen, cursor, cols, &spec);
            let verdict = match &got {
                None => "unknown".to_string(),
                Some(read) => match read.alt_mode {
                    Some(id) => format!("alt:{id}"),
                    None if read.empty => "empty".to_string(),
                    None => "occupied".to_string(),
                },
            };
            assert_eq!(verdict, expect, "{}", frame["name"]);
            if let (Some(text), Some(read)) = (frame.get("text").and_then(|t| t.as_str()), &got) {
                assert_eq!(read.text, text, "{}", frame["name"]);
            }
            // 草稿保真：读全的原文逐字相等，读不全的原因一致（与前端 composer.test.ts 同一批帧）。
            if let Some(want) = frame.get("draft") {
                let want = if want["complete"].as_bool().unwrap() {
                    Draft::Complete(want["text"].as_str().unwrap().to_string())
                } else {
                    Draft::Incomplete(match want["why"].as_str().unwrap() {
                        "paste" => Incomplete::Paste,
                        "overflow" => Incomplete::Overflow,
                        "wrap" => Incomplete::Wrap,
                        "unverified" => Incomplete::Unverified,
                        other => panic!("没见过的原因：{other}"),
                    })
                };
                assert_eq!(
                    got.as_ref().and_then(|read| read.draft.clone()),
                    Some(want),
                    "{}",
                    frame["name"]
                );
                drafts += 1;
            }
        }
        providers.insert(provider.to_string());
        checked += 1;
    }
    assert!(checked >= 25);
    // 五家 × 十来种草稿形态的保真帧都在（每帧按原样与去行尾空格各判一次）。
    assert!(drafts >= 100, "草稿保真的帧只核对了 {drafts} 次");
    // 五家都要有实拍帧：规格只有一份，哪一家没有样例就等于没测。
    assert_eq!(
        providers.into_iter().collect::<Vec<_>>(),
        ["claude", "codex", "gemini", "kimi", "opencode"]
    );
}

/// 占位与同文草稿只差在光标：同一帧文字，光标停在输入起点是占位，停在文末是草稿，
/// 没有光标证据时退回只看文字。
#[test]
fn cursor_separates_placeholder_from_identical_draft() {
    let codex = crate::by_id("codex")
        .unwrap()
        .composer(Some("0.167.2"))
        .unwrap();
    let screen: Vec<String> = [
        "› Ask Codex to do anything",
        "",
        "  GPT-5.6 high · ~/repo",
    ]
    .iter()
    .map(|line| line.to_string())
    .collect();
    let at = |col, dim| {
        Some(Cursor {
            row: 0,
            col,
            visible: true,
            dim,
        })
    };
    assert!(read(&screen, at(2, true), &codex).unwrap().empty);
    assert!(!read(&screen, at(26, false), &codex).unwrap().empty);
    assert!(read(&screen, None, &codex).unwrap().empty);
    // 光标隐藏：不算证据，退回只看文字。
    let hidden = Some(Cursor {
        row: 0,
        col: 26,
        visible: false,
        dim: false,
    });
    assert!(read(&screen, hidden, &codex).unwrap().empty);
}

/// 架构守卫：认终端输入框的规格每家只有一份（走查输入根因 1：三份规格各接各的功能）。
/// - 旧的三份类型名、字段名不得再出现；
/// - 每个插件至多一个 `fn composer(`、至多构造一次 ComposerSpec，别处不构造；
/// - 认框（读 top / bottom / row 这几条正则）只在 composer.rs。
#[test]
fn only_one_composer_spec_per_plugin() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    let mut stack = vec![root.join("src"), root.join("../../src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    assert!(files.len() > 50, "没扫到源码");
    // 拼出来写，免得这份清单自己命中自己。
    let banned: Vec<String> = [
        ["Draft", "Stash"],
        ["Undo", "Composer"],
        ["Composer", "Guard"],
        ["Undo", "InputClear"],
        ["draft", "_stash"],
        ["composer", "_guard"],
        ["composer", "_border"],
        ["row", "_prefix"],
        ["bottom", "_prefix"],
    ]
    .iter()
    .map(|parts| parts.concat())
    .collect();
    let mut plugins_declaring = 0;
    for file in &files {
        let source = std::fs::read_to_string(file).unwrap();
        let name = file.display().to_string().replace('\\', "/");
        for word in &banned {
            assert!(
                !source.contains(word.as_str()),
                "{name} 里又出现了旧的输入框规格 `{word}`"
            );
        }
        let declares = source.matches("fn composer(").count();
        let builds = source.matches("ComposerSpec {").count();
        let is_plugin = name.contains("/plugins/") && name.ends_with("/mod.rs");
        let is_definition = name.ends_with("/chat_ui.rs") || name.contains("/src/composer");
        if is_plugin {
            assert!(
                declares <= 1 && builds <= 1,
                "{name} 声明了不止一份输入框规格"
            );
            plugins_declaring += declares;
        } else if name.ends_with("/registry.rs") {
            assert_eq!((declares, builds), (1, 0), "{name} 只放默认实现");
        } else if !is_definition {
            assert_eq!(
                (declares, builds),
                (0, 0),
                "{name} 不该自己声明输入框规格"
            );
        }
        if !name.contains("/src/composer") {
            for field in ["top_pattern", "bottom_pattern", "row_pattern"] {
                assert!(
                    !source.contains(&format!(".{field}")),
                    "{name} 自己在认输入框（读 {field}），应调用 composer::read"
                );
            }
        }
    }
    assert_eq!(plugins_declaring, 5, "五家各声明一份");
}
