//! 真机走查：kimi 提问表单的自定义答案（`Other`）。让 kimi 用 AskUserQuestion 问一道单选题，
//! 按对话页的做法选中 `Other`、等屏幕进入等输入、打一个答案、回车存下，再在确认页提交，然后断言
//! 模型收到的正是打进去的那个答案（让它把答案转成大写回出来，屏幕上出现大写的那串）。
//!
//! 为什么要真机走：这一项要现场打字，流程是「数字键选中、打字、回车」。表单里数字键就是选项
//! （`1` 选第一项并跳到下一题），没进入编辑状态时打进去的字会被当成按键。所以插件只在走通之后才给
//! `Other` 声明打字流程（`kimi:question` 的选项规则，角色 custom_answer）。
//!
//! 按键与就绪标志取自插件声明：`Other` 的数字键来自屏幕上它那一行的编号，就绪标志与提交键来自
//! 选项规则的 `typed`。规则里还没有 `typed` 时（首次取证）用环境变量 `PROBE_READY`、`PROBE_SUBMIT`。
//!
//! ```text
//! PROBE_OUT=<目录> cargo test -p meowo-app --test probe_kimi_other -- --ignored --nocapture
//! ```
//! 调两次模型（发起提问一次，收到答案后回一句）。

use meowo_agent::chat_ui::OptionRole;
use std::path::Path;

mod common;
#[path = "common/tui.rs"]
mod tui;
use tui::{pause, Tui};

const AGENT: &str = "kimi";
const PROMPT: &str = "Use the AskUserQuestion tool right now to ask me exactly one single-choice question, \
\"Pick a color\", with the two options Red and Blue. After I answer, reply with only my answer in upper case.";
const ANSWER: &str = "teal-7391";
const ANSWER_UPPER: &str = "TEAL-7391";

fn compile(pattern: &str) -> regex::Regex {
    regex::Regex::new(&format!("(?im){pattern}")).unwrap_or_else(|error| panic!("规则编不过：{error}"))
}

fn form_on_screen(screen: &str) -> bool {
    meowo_agent::by_id(AGENT)
        .unwrap()
        .attention_patterns()
        .iter()
        .filter(|rule| rule.id == "kimi:question")
        .any(|rule| rule.patterns.iter().any(|pattern| compile(pattern).is_match(screen)))
}

/// 屏幕上 `[n] <文字>` 的选项行里，被声明为自定义答案的那一项的编号。
fn custom_answer_digit(screen: &str) -> Option<String> {
    let rule = meowo_agent::by_id(AGENT)
        .unwrap()
        .attention_patterns()
        .iter()
        .find(|rule| rule.id == "kimi:question")?
        .options
        .iter()
        .find(|option| option.role == OptionRole::CustomAnswer)?;
    let label = regex::Regex::new(&format!("(?i){}", rule.pattern)).unwrap();
    let line = regex::Regex::new(r"^\s*(?:→\s*)?\[(\d+)\]\s+(.+?)\s*$").unwrap();
    screen.lines().rev().find_map(|row| {
        let hit = line.captures(row)?;
        label.is_match(hit[2].trim()).then(|| hit[1].to_string())
    })
}

#[test]
#[ignore = "启动真实 CLI 并调用模型；真机走查用"]
fn custom_answer_is_typed_saved_and_reaches_the_model() {
    tui::use_bundled_conpty();
    let plugin = meowo_agent::by_id(AGENT).unwrap();
    let version = tui::cli_version(AGENT);
    let spec = plugin
        .composer(Some(&version))
        .unwrap_or_else(|| panic!("{AGENT} {version} 没有输入框规格"));
    let declared = plugin
        .attention_patterns()
        .iter()
        .find(|rule| rule.id == "kimi:question")
        .and_then(|rule| rule.options.iter().find(|o| o.role == OptionRole::CustomAnswer))
        .and_then(|option| option.typed);
    let ready = std::env::var("PROBE_READY")
        .ok()
        .or_else(|| declared.map(|typed| typed.ready_pattern.to_string()))
        .expect("规则里还没有 typed，用 PROBE_READY 指定就绪标志");
    let submit = std::env::var("PROBE_SUBMIT")
        .ok()
        .map(|raw| raw.replace("\\r", "\r"))
        .or_else(|| declared.map(|typed| typed.submit_input.to_string()))
        .unwrap_or_else(|| "\r".to_string());
    let cwd = tui::temp_cwd("kimi-other-probe");
    println!("===== {AGENT} {version} =====");
    let mut tui = Tui::spawn(&plugin.launch_argv(), &cwd, &[]);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&version, &compile(&ready), &submit, &spec, &tui)
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

fn walk(
    version: &str,
    ready: &regex::Regex,
    submit: &str,
    spec: &meowo_agent::chat_ui::ComposerSpec,
    tui: &Tui,
) {
    tui::wait_ready(tui, spec);
    tui.submit(PROMPT);
    // 等表单出现并稳住（连续两次读屏都认得出、且认得出自定义答案项）。
    let mut seen = false;
    assert!(tui.wait("提问表单出现", 150, |tui| {
        let text = tui.text();
        let hit = form_on_screen(&text) && custom_answer_digit(&text).is_some();
        let stable = hit && seen;
        seen = hit;
        stable
    }));
    pause(600);
    tui.dump();
    let form_screen = tui.trimmed();
    let digit = custom_answer_digit(&tui.text()).expect("表单里认不出自定义答案项");
    println!("  自定义答案项的数字键：{digit}");

    // 第一步：按它的数字键进入编辑。认出就绪标志才打字。
    tui.send(&digit);
    let entered = tui.wait("进入编辑自定义答案的状态", 8, |tui| ready.is_match(&tui.text()));
    tui.dump();
    assert!(entered, "按了 {digit} 之后屏幕上没出现等输入的标志，不打字");
    let ready_screen = tui.trimmed();

    // 第二步：打答案，回车存下。
    pause(400);
    tui.send(ANSWER);
    pause(800);
    tui.dump();
    assert!(tui.text().contains(ANSWER), "打进去的答案没有出现在屏幕上");
    let typed_screen = tui.trimmed();
    tui.send(submit);

    // 第三步：存下之后表单可能直接交卷，也可能停在确认页（`[1] Submit` / `[2] Cancel`）。
    // 确认页由对话页的提问卡照常接手（数字键 1 提交，既有规则）；这里同样认出来再按。
    pause(1200);
    tui.dump();
    let after_save = tui.trimmed();
    let review = regex::Regex::new(r"(?m)^\s*(?:→\s*)?\[1\]\s+Submit\s*$").unwrap();
    let mut confirm_pressed = false;
    assert!(tui.wait("模型拿到答案并回话（屏幕上出现大写的答案）", 150, |tui| {
        let text = tui.text();
        if !confirm_pressed && form_on_screen(&text) && review.is_match(&text) {
            confirm_pressed = true;
            tui.send("1");
        }
        text.contains(ANSWER_UPPER)
    }));
    assert!(tui.wait("回合结束，输入框回到空闲", 120, |tui| {
        tui.composer(spec).is_some_and(|read| read.empty)
    }));
    pause(1200);
    tui.dump();
    println!("  确认页：{}", if confirm_pressed { "出现过，按 1 提交" } else { "没有出现，存下即交卷" });

    let sample = serde_json::json!({
        "agent": AGENT,
        "version": version,
        "prompt": PROMPT,
        "answer": ANSWER,
        "openInput": digit,
        "readyPattern": ready.as_str().trim_start_matches("(?im)"),
        "submitInput": submit,
        "reviewPage": confirm_pressed,
        "screen": form_screen,
        "readyScreen": ready_screen,
        "typedScreen": typed_screen,
        "afterSaveScreen": after_save,
        "finalScreen": tui.trimmed(),
    });
    let out = std::env::var("PROBE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out).unwrap();
    let file: std::path::PathBuf = Path::new(&out).join("kimi-other.json");
    std::fs::write(&file, serde_json::to_string_pretty(&sample).unwrap()).unwrap();
    println!("  样本写到 {}", file.display());
    println!("===== {AGENT}：通过 =====");
}
