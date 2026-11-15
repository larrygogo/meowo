//! 真机取证：终端输入框里的草稿，各种形态在屏幕上长什么样，能不能从屏幕原样读回来。
//!
//! 背景（做空①）：对话页发送前把终端里的草稿清掉、搬进对话页。大段粘贴被 CLI 折成占位、长行被
//! 框宽折行、内容超出输入框可视高度时，屏幕上的字不等于草稿原文，清掉就是丢内容。这个探针逐家
//! 逐种形态把草稿打进（或粘贴进）真实 CLI 的输入框，存下整屏、光标、终端仿真层的折行标记，再用
//! 生产规则（插件声明的输入框规格加 `meowo_agent::composer::read_with_size`）读一遍，打印被判成了
//! 什么（读全 / 读不全及原因），读全的再核对与原文是否逐字相等。
//!
//! **只打字和粘贴，从不提交**（全程不发回车），不调模型、不花额度。每种形态之间用规格里的清空键
//! 清掉（认出输入框里有字才按），清不掉就停。
//!
//! ```text
//! PROBE_AGENT=kimi PROBE_OUT=<目录> cargo test -p meowo-app --test probe_draft_fidelity -- --ignored --nocapture
//! ```
//! `PROBE_SUITE=wrap PROBE_COLS=80 PROBE_ROWS=24`：换终端尺寸跑折行相关的形态（见 [`wrap_forms`]）。
//! 形态的长度按这一尺寸下**实测**的一行格数生成（先打一串 `x` 量出来），所以「刚好填满」「差一格」
//! 在每个宽度下都卡在真实边界上。样本写到 `draft-<家>-<列>x<行>.json`。
//!
//! `PROBE_FORMS=short,wrap_words` 只跑其中几种；`PROBE_ARGS` 追加启动参数；`PROBE_EDITOR=<按键>`
//! 另外取证「外部编辑器」这条路能不能拿到输入框全文（把 EDITOR 指到一个只做复制的脚本，按这个键，
//! 看脚本收到的文件是不是原文；按键支持 `\x07` 这样的转义）。

use std::path::Path;
use std::time::{Duration, Instant};

mod common;
#[path = "common/tui.rs"]
mod tui;
use meowo_agent::composer::Draft;
use tui::{pause, Tui};

/// 一种草稿形态：怎么进输入框（逐字打 / 一次粘贴）与原文。
struct Form {
    id: &'static str,
    paste: bool,
    text: String,
}

/// 恰好 `len` 个字符的一行英文：按词排、词间单个空格，末尾用 `z` 补齐（首尾都不是空格）。
fn words(len: usize) -> String {
    const WORDS: [&str; 12] = [
        "keep", "the", "draft", "when", "sending", "from", "chat", "page", "and", "verify",
        "every", "provider",
    ];
    let mut out = String::new();
    for word in WORDS.iter().cycle() {
        if out.len() + 1 + word.len() > len {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    while out.len() < len {
        out.push('z');
    }
    out.truncate(len);
    out
}

fn solid(len: usize) -> String {
    "abcdefghij0123456789".chars().cycle().take(len).collect()
}

/// 折行相关的形态。`width` 是这一尺寸下实测的一行最多能放的格数（见 [`measure_width`]）。
///
/// 为什么按实测宽度生成：判定最容易错的地方就在边界上（刚好填满、差一格、折行处是空格），固定长度
/// 的文本换一个终端宽度就落不到边界上了。`near_*` 三种是同一幅屏幕的两种来历：行尾留 3 格后真换行，
/// 与行尾 3 个空格后被折行，屏幕上若长得一样，判定就必须都判读不全。
fn wrap_forms(width: usize, rows: u16) -> Vec<Form> {
    let typed = |id, text: String| Form { id, paste: false, text };
    let cjk = |len: usize| -> String { "终端里的草稿要原样搬进对话页".chars().cycle().take(len).collect() };
    let mixed: String = "修复 bug：终端 draft 要原样 keep，不丢 a single 字符 "
        .chars()
        .cycle()
        .take(width + width / 4)
        .collect();
    let tall: Vec<String> = (1..=rows as usize + 5).map(|n| format!("row {n:02} of the tall draft")).collect();
    vec![
        typed("short", "meowo short draft".to_string()),
        typed("multi_short", "line one
    indented four

  two spaces after blank
last line".to_string()),
        typed("wrap_words", words(2 * width + width / 3)),
        typed("wrap_solid", solid(2 * width + width / 3)),
        typed("wrap_wide", cjk(width)),
        typed("wrap_mixed", mixed.trim_end().to_string()),
        typed("wrap_url", format!("curl -fsSL https://example.com/{} | sh -s -- --yes", solid(width))),
        typed("fill_exact", solid(width)),
        typed("fill_minus1", solid(width - 1)),
        typed("fill_plus1", solid(width + 1)),
        typed("fill_words", words(width)),
        typed("multi_long", format!("first line
{}
last line", words(width + width / 2))),
        typed("multi_full", format!("{}
next line", solid(width))),
        typed("near_newline", format!("{}
ab cd", words(width - 3))),
        typed("near_spaces3", format!("{}   ab cd", words(width - 3))),
        typed("near_spaces5", format!("{}     ab cd", words(width - 6))),
        typed("over_spaces6", format!("{}      ab cd", words(width - 3))),
        typed("tall", tall.join("
")),
    ]
}

/// 这一尺寸下一行最多能放多少格：打一串比终端还宽的 `x`，数首行画出了几个。量完清掉。
fn measure_width(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec, cols: u16) -> usize {
    for _ in 0..cols as usize + 20 {
        tui.send("x");
        pause(12);
    }
    tui.wait("量宽度的 x 进了输入框", 20, |tui| tui.text().contains("xxxxxxxxxx"));
    settle(tui);
    let lines = tui.lines();
    let first = lines
        .iter()
        .find(|line| line.contains("xxxxxxxxxx"))
        .expect("屏幕上没有量宽度的那一行");
    let width = first.chars().filter(|c| *c == 'x').count();
    println!("  实测一行最多 {width} 格（终端 {cols} 列）：|{}|", first.trim_end());
    assert!(clear(tui, spec), "量宽度的草稿清不掉，停");
    pause(2500);
    width
}

fn forms() -> Vec<Form> {
    let typed = |id, text: &str| Form { id, paste: false, text: text.to_string() };
    let pasted = |id, text: String| Form { id, paste: true, text };
    let words = "fix: keep the draft when sending from the chat page and do not ask the user to clear it in the terminal, then verify every provider keeps the same behaviour across desktop and mobile without losing a single character of what was typed";
    let tall: Vec<String> = (1..=45).map(|n| format!("row {n:02} of the tall draft")).collect();
    let big_line = words.repeat(23)[..5120].to_string();
    let big_lines: Vec<String> = (1..=60).map(|n| format!("pasted line {n:02} with some words in it")).collect();
    vec![
        typed("short", "meowo short draft"),
        typed("wrap_words", words),
        typed("wrap_solid", &"abcdefghij0123456789".repeat(8)),
        typed("multi", "line one\n    indented four\n\n  two spaces after blank\nlast line"),
        typed("trailing", "ends with three spaces   "),
        typed("wide", "中文草稿测试，全角标点。emoji 😀 结束"),
        typed("wide_wrap", &"终端里的草稿要原样搬进对话页".repeat(8)),
        typed("tall", &tall.join("\n")),
        pasted("paste_small", "pasted alpha\n  pasted beta indented\npasted gamma".to_string()),
        // 逐字打的 emoji 经 ConPTY 的按键通道到不了 Codex（输入框里根本没有它），宽字符另用粘贴取证。
        pasted("paste_wide", "中文草稿测试，全角标点。emoji 😀 结束".to_string()),
        pasted("paste_tab", "tab line\n\tone tab indent\nend".to_string()),
        pasted("paste_big_line", big_line),
        pasted("paste_big_lines", big_lines.join("\n")),
    ]
}

/// 屏幕不再变化（连续 900ms 相同）。
fn settle(tui: &Tui) {
    let started = Instant::now();
    let mut last = tui.lines();
    let mut same_since = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        pause(150);
        let now = tui.lines();
        if now != last {
            last = now;
            same_since = Instant::now();
        } else if same_since.elapsed() > Duration::from_millis(900) {
            return;
        }
    }
}

/// 用规格里的清空键清掉输入框（与前端 clearComposerDraft 同序：认出有字才按，每轮复核）。
fn clear(tui: &Tui, spec: &meowo_agent::chat_ui::ComposerSpec) -> bool {
    let empty = |tui: &Tui| tui.composer(spec).is_some_and(|read| read.empty);
    let rounds = if spec.clear_rounds > 1 { 120 } else { 2 };
    for _ in 0..rounds {
        if empty(tui) {
            return true;
        }
        // 生产规则是「认不出就不按」。探针这里不拦：草稿是探针自己刚打进去的，认不出正是要取证的
        // 形态之一（超出可视高度时框线上多出滚动标记），清掉它才能接着测下一种。
        for (index, input) in spec.clear_inputs.iter().enumerate() {
            if index > 0 {
                pause(if spec.clear_rounds > 1 { 60 } else { 300 });
                if empty(tui) {
                    return true;
                }
            }
            tui.send(input);
        }
        pause(if spec.clear_rounds > 1 { 250 } else { 800 });
    }
    empty(tui)
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).unwrap() as char);
            }
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[test]
#[ignore = "启动真实 CLI（只在输入框里打字与粘贴，不提交、不调模型）；真机取证用"]
fn draft_forms_on_screen() {
    tui::use_bundled_conpty();
    let agent = std::env::var("PROBE_AGENT").expect("PROBE_AGENT");
    let plugin = meowo_agent::by_id(&agent).unwrap();
    let version = tui::cli_version(&agent);
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("{agent} {version} 没有输入框规格"));
    let out = std::env::var("PROBE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out).unwrap();
    let cwd = tui::temp_cwd("draft-fidelity");
    let mut argv = plugin.launch_argv();
    if let Ok(extra) = std::env::var("PROBE_ARGS") {
        argv.extend(extra.split_whitespace().map(String::from));
    }
    // 外部编辑器取证：EDITOR 指到一个只把文件复制出来的脚本（CLI 读回的还是原文件，草稿不变）。
    let editor_key = std::env::var("PROBE_EDITOR").ok().map(|raw| unescape(&raw));
    let editor_copy = out.join(format!("editor-{agent}.txt"));
    let _ = std::fs::remove_file(&editor_copy);
    let mut env: Vec<(&str, String)> = Vec::new();
    if editor_key.is_some() {
        let script = cwd.join("meowo-probe-editor.cmd");
        std::fs::write(
            &script,
            format!("@echo off\r\ncopy /y \"%~1\" \"{}\" >nul\r\n", editor_copy.display()),
        )
        .unwrap();
        env.push(("EDITOR", script.display().to_string()));
        env.push(("VISUAL", script.display().to_string()));
    }
    println!("===== {agent} {version} =====");
    let mut tui = Tui::spawn(&argv, &cwd, &env);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&agent, &version, &spec, &tui, &out, editor_key.as_deref(), &editor_copy)
    }));
    tui.kill();
    for _ in 0..20 {
        if std::fs::remove_dir_all(&cwd).is_ok() || !cwd.exists() {
            break;
        }
        pause(250);
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

type Snapshot = (Vec<String>, (u16, u16, bool, bool), Vec<u16>, (u16, u16));

/// 同一把锁里取整屏、光标（行、列、可见、暗色）、折行标记行、终端尺寸。
fn snapshot(tui: &Tui) -> Snapshot {
    tui.with_screen(|s| {
        let (rows, cols) = s.size();
        let lines: Vec<String> = s.rows(0, cols).collect();
        let (row, col) = s.cursor_position();
        let wrapped: Vec<u16> = (0..rows).filter(|r| s.row_wrapped(*r)).collect();
        let dim = s
            .cell(row, col)
            .is_some_and(|cell| cell.dim() && !cell.contents().trim().is_empty());
        (lines, (row, col, !s.hide_cursor(), dim), wrapped, (rows, cols))
    })
}

fn walk(
    agent: &str,
    version: &str,
    spec: &meowo_agent::chat_ui::ComposerSpec,
    tui: &Tui,
    out: &Path,
    editor_key: Option<&str>,
    editor_copy: &Path,
) {
    let plugin = meowo_agent::by_id(agent).unwrap();
    // 在确认屏幕已是输入框之前不发任何草稿按键（wait_ready 只在认出的启动询问上按键）。
    tui::wait_ready(tui, spec);
    let (alt_screen, bracketed) = tui.with_screen(|s| (s.alternate_screen(), s.bracketed_paste()));
    println!("  备用屏：{alt_screen}，bracketed paste：{bracketed}");
    let only: Option<Vec<String>> = std::env::var("PROBE_FORMS")
        .ok()
        .map(|list| list.split(',').map(str::to_string).collect());
    let newline = plugin.newline_input();
    let mut samples = Vec::new();
    let (rows, cols) = tui::size();
    let suite = std::env::var("PROBE_SUITE").unwrap_or_default();
    let (all, file, tag, width) = if suite == "wrap" {
        // 空输入框在这一尺寸下的样子也存一帧：窄窗里占位文字自己会折行（OpenCode 60 列实拍折成两行，
        // 当时的规则认不出，整条发送链路按「认不出输入框」拦下）。
        let (lines, cursor, _, size) = snapshot(tui);
        samples.push(serde_json::json!({
            "name": format!("{agent}-{version}-{cols}x{rows}-form-empty"),
            "provider": agent, "version": version, "form": "empty", "how": "none", "original": "",
            "lines": lines,
            "cursor": { "row": cursor.0, "col": cursor.1, "visible": cursor.2, "dim": cursor.3 },
            "rows": size.0, "cols": size.1, "verdict": "空",
        }));
        let width = measure_width(tui, spec, cols);
        (
            wrap_forms(width, rows),
            format!("draft-{agent}-{cols}x{rows}.json"),
            format!("{cols}x{rows}-form"),
            Some(width),
        )
    } else {
        (forms(), format!("draft-{agent}.json"), "form".to_string(), None)
    };
    for form in all {
        if only.as_ref().is_some_and(|list| !list.iter().any(|id| id == form.id)) {
            continue;
        }
        if form.paste && !bracketed {
            println!("  - {}：这家没开 bracketed paste，跳过", form.id);
            continue;
        }
        if !form.paste && form.text.contains('\n') && newline.is_none() {
            println!("  - {}：这家没有声明换行键，跳过", form.id);
            continue;
        }
        assert!(
            tui.composer(spec).is_some_and(|read| read.empty),
            "{}：开始前输入框必须认得出、是空的",
            form.id
        );
        if form.paste {
            tui.send(&format!("\x1b[200~{}\x1b[201~", form.text));
        } else {
            // 逐字打、字间留空：一口气写进去会被一些 TUI 当成粘贴（Codex 的 paste burst）。
            for c in form.text.chars() {
                if c == '\n' {
                    pause(120);
                    tui.send(newline.unwrap());
                    pause(120);
                } else {
                    tui.send(&c.to_string());
                    pause(12);
                }
            }
        }
        // 大段粘贴要过一会儿才进输入框（Codex 5KB 约 2 秒）：先等到输入框不再是空的，再等画面停下来。
        tui.wait("草稿进了输入框", 20, |tui| !tui.composer(spec).is_some_and(|read| read.empty));
        settle(tui);
        let (lines, cursor, wrapped, size) = snapshot(tui);
        let read = tui.composer(spec);
        // 生产判定：读全了才给原文（Draft::Complete），读不全给原因。
        let draft = read.as_ref().and_then(|r| r.draft.clone());
        let (read_text, verdict) = match &draft {
            Some(Draft::Complete(text)) => (Some(text.clone()), "读全".to_string()),
            Some(Draft::Incomplete(why)) => (None, format!("读不全（{}）", why.as_str())),
            None => (None, "认不出输入框".to_string()),
        };
        let same = read_text.as_deref() == Some(form.text.as_str());
        println!(
            "  ── {}（{}，{} 字 {} 行）判定：{}{}；折行标记行 {:?}；光标 {:?}",
            form.id,
            if form.paste { "粘贴" } else { "逐字" },
            form.text.chars().count(),
            form.text.lines().count(),
            verdict,
            match (&read_text, same) {
                (Some(_), true) => "，与原文逐字相等",
                (Some(_), false) => "，与原文不相等",
                (None, _) => "",
            },
            wrapped,
            cursor
        );
        if let Some(read) = &read {
            for line in lines.iter().take(read.bottom_row + 1).skip(read.top_row) {
                println!("    |{}", line.trim_end());
            }
            if let (Some(text), false) = (&read_text, same) {
                println!("    读回：{text:?}");
            }
        } else {
            println!("    认不出输入框");
            tui.dump();
        }
        let mut sample = serde_json::json!({
            "name": format!("{agent}-{version}-{tag}-{}", form.id),
            "provider": agent,
            "version": version,
            "form": form.id,
            "how": if form.paste { "paste" } else { "typed" },
            "original": form.text,
            // 原样存，不去行尾空格：生产读到的帧就是这样（Gemini 草稿里的空行是一串空格，去掉就认不出框）。
            "lines": lines,
            "cursor": { "row": cursor.0, "col": cursor.1, "visible": cursor.2, "dim": cursor.3 },
            "rows": size.0,
            "cols": size.1,
            "wrappedRows": wrapped,
            // 这一尺寸下实测的一行最多能放的格数（只有 wrap 形态量）。
            "width": width,
            "verdict": verdict,
            "readText": read_text,
        });
        // 外部编辑器：草稿还在输入框里时按键，看脚本拿到的文件是不是原文。
        if let Some(key) = editor_key {
            let _ = std::fs::remove_file(editor_copy);
            for part in key.split('+') {
                tui.send(part);
                pause(300);
            }
            let got = tui.wait("外部编辑器脚本收到了文件", 10, |_| editor_copy.exists());
            pause(800);
            let content = std::fs::read_to_string(editor_copy).ok();
            let exact = content.as_deref().map(|c| c.replace("\r\n", "\n"));
            let equal = exact.as_deref().map(|c| c.trim_end_matches('\n')) == Some(form.text.as_str());
            println!(
                "    外部编辑器：{}",
                if !got { "没有调起".to_string() } else if equal { "拿到全文，逐字相等".to_string() } else { format!("拿到的不是原文：{:?}", exact.map(|c| c.chars().take(120).collect::<String>())) }
            );
            sample["editor"] = serde_json::json!({ "called": got, "equal": equal });
            settle(tui);
        }
        samples.push(sample);
        // 每种形态取完就落盘：后面某一种清不掉而中止时，前面的样本还在。
        std::fs::write(
            out.join(&file),
            serde_json::to_string_pretty(&samples).unwrap(),
        )
        .unwrap();
        assert!(clear(tui, spec), "{}：清不掉草稿，停", form.id);
        // kimi / gemini 清空后挂「再按一次退出」的提示，等它过去再继续。
        pause(2500);
    }
    println!("===== {agent}：样本写到 {} =====", out.join(&file).display());
}
