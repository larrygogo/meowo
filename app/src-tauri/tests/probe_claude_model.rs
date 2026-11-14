//! 真机走查：claude 会话里切模型，切完 CLI 用的确实是新模型；回合进行中选的模型，回合结束后发出去照样生效。
//!
//! 走的是对话页那条通路：插件声明的 `model_select_command`（`/model <别名>`）写进终端、回车，按
//! `model_select_reply` 的确认行判成败。「CLI 侧确认」看两处：屏幕上的确认行，和切换之后那一回合
//! 落在会话记录（transcript）里的 `message.model`。
//!
//! **`/model` 会把选择存成新会话的默认模型**（插件声明 `model_select_saves_default`，本探针顺带验证）。
//! 所以：开跑前读出配置里的 `model` 并记下；走完用同一条命令切回原值；成败都在最后核对配置文件，
//! 命令没能切回时直接把那一个键改回去，并在输出里说明。别的键不动。
//!
//! 调三次模型（两句算术、一次数数），启动时用 `--model haiku`（只管本次启动，不写配置）。
//!
//! ```text
//! cargo test -p meowo-app --test probe_claude_model -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

mod common;
#[path = "common/tui.rs"]
mod tui;
use tui::{pause, Tui};

const AGENT: &str = "claude";

/// claude 的配置目录：多账号会话里 `CLAUDE_CONFIG_DIR` 指向账号目录，否则是 `~/.claude`。
fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap())
                .join(".claude")
        })
}

fn default_model(settings: &Path) -> Option<String> {
    let text = std::fs::read_to_string(settings).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("model")?.as_str().map(str::to_string)
}

/// 兜底：命令没能切回时，只把 settings.json 里的 `model` 这一个键改回原值（按文本改，别的键、
/// 顺序、缩进都不动）。
fn restore_by_file(settings: &Path, original: Option<&str>) {
    let text = std::fs::read_to_string(settings).unwrap();
    let key = regex::Regex::new(r#"(?m)^(\s*)"model"\s*:\s*"[^"]*"(,?)[ \t]*\r?\n"#).unwrap();
    let restored = match original {
        Some(model) => {
            assert!(key.is_match(&text), "settings.json 里找不到 model 键，无法按文本改回");
            key.replace(&text, |caps: &regex::Captures| {
                format!("{}\"model\": \"{model}\"{}\n", &caps[1], &caps[2])
            })
            .into_owned()
        }
        None => key.replace(&text, "").into_owned(),
    };
    serde_json::from_str::<serde_json::Value>(&restored).expect("改回之后必须还是合法 JSON");
    std::fs::write(settings, restored).unwrap();
}

/// 这次会话的记录里，最后一条 assistant 消息用的模型。
fn transcript_model(config: &Path, cwd: &Path) -> Option<String> {
    let tag = cwd.file_name()?.to_string_lossy().to_string();
    let projects = std::fs::read_dir(config.join("projects")).ok()?;
    let mut last = None;
    for project in projects.flatten() {
        if !project.file_name().to_string_lossy().contains(&tag) {
            continue;
        }
        for file in std::fs::read_dir(project.path()).ok()?.flatten() {
            if file.path().extension().is_some_and(|ext| ext == "jsonl") {
                let text = std::fs::read_to_string(file.path()).unwrap_or_default();
                for line in text.lines() {
                    let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
                        continue;
                    };
                    if entry["type"] == "assistant" {
                        if let Some(model) = entry["message"]["model"].as_str() {
                            // `<synthetic>` 是 CLI 自己合成的提示，不是模型作答。
                            if !model.starts_with('<') {
                                last = Some(model.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    last
}

/// 回答出现了：题目那一行之后，有一行去掉行首标记后正好是答案（同 probe_composer_draft）。
fn answered(screen: &str, question: &str, answer: &str) -> bool {
    let Some(at) = screen.rfind(question) else {
        return false;
    };
    screen[at..].lines().skip(1).any(|line| {
        line.trim()
            .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
            .trim()
            == answer
    })
}

/// 发 `/model <别名>`，等确认行。返回确认行原文；CLI 拒绝时带着它的原话失败。
fn switch_model(tui: &Tui, alias: &str) -> String {
    let plugin = meowo_agent::by_id(AGENT).unwrap();
    let command = plugin.model_select_command().expect("claude 声明了带参切模型的命令");
    let reply = plugin.model_select_reply().expect("claude 声明了切模型的回话");
    // 前端带 i 标志编译这两条，这里同样不分大小写。
    let confirm = regex::Regex::new(&format!("(?i){}", reply.confirm_pattern)).unwrap();
    let reject = regex::Regex::new(&format!("(?im){}", reply.reject_pattern)).unwrap();
    // 只认这条命令之后的确认：上一次切换的确认行还留在屏幕上。
    let before = tui
        .lines()
        .iter()
        .filter(|line| confirm.is_match(line))
        .count();
    // 对话中途切换时 CLI 先弹的二次确认框：按插件声明的规格认，框里要切到的正是这次选的才按确认键
    // （与前端 chat/switchConfirm.ts 同一套判据）。
    let dialog = plugin.switch_confirm().expect("claude 声明了切换确认框");
    let title = regex::Regex::new(&format!("(?i){}", dialog.title_pattern)).unwrap();
    let accept = regex::Regex::new(&format!("(?i){}", dialog.accept_pattern)).unwrap();
    let squash = |text: &str| -> String {
        text.to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    };
    let mut pressed = false;
    tui.submit(&format!("{command} {alias}"));
    let mut line = String::new();
    assert!(
        tui.wait(&format!("{command} {alias} 得到确认"), 30, |tui| {
            let lines = tui.lines();
            if let Some(rejected) = lines.iter().find(|line| reject.is_match(line)) {
                panic!("CLI 拒绝了切换：{}", rejected.trim());
            }
            if !pressed {
                if let Some(at) = lines.iter().rposition(|line| title.is_match(line)) {
                    let target = lines[at + 1..]
                        .iter()
                        .find_map(|line| accept.captures(line).map(|caps| caps[1].to_string()));
                    if target.is_some_and(|target| squash(&target).contains(&squash(alias))) {
                        println!("    CLI 先弹了确认框，认出是这次要切的模型，按确认：");
                        let shown: Vec<String> = lines[at..]
                            .iter()
                            .map(|line| line.trim_end().to_string())
                            .filter(|line| !line.is_empty())
                            .collect();
                        for row in &shown {
                            println!("      |{row}");
                        }
                        if let Ok(out) = std::env::var("PROBE_OUT") {
                            let _ = std::fs::create_dir_all(&out);
                            let _ = std::fs::write(
                                Path::new(&out).join(format!("switch-confirm-{alias}.json")),
                                serde_json::to_string_pretty(&shown).unwrap(),
                            );
                        }
                        pressed = true;
                        tui.send(dialog.accept_input);
                        return false;
                    }
                }
            }
            let confirms: Vec<&String> = lines.iter().filter(|line| confirm.is_match(line)).collect();
            if confirms.len() > before {
                line = confirms.last().unwrap().trim().to_string();
                return true;
            }
            false
        }),
        "没等到确认行"
    );
    println!("    CLI 的确认：{line}");
    line
}

#[test]
#[ignore = "启动真实 claude 并调用模型；会临时改动并复原新会话默认模型；真机走查用"]
fn model_switch_takes_effect_and_the_default_is_put_back() {
    tui::use_bundled_conpty();
    let plugin = meowo_agent::by_id(AGENT).unwrap();
    let version = tui::cli_version(AGENT);
    let spec = plugin.composer(Some(&version)).expect("claude 的输入框规格");
    let config = config_dir();
    let settings = config.join("settings.json");
    let original = default_model(&settings);
    println!("===== claude {version} =====");
    println!("  配置目录：{}", config.display());
    println!("  开跑前的新会话默认模型：{original:?}");
    assert!(plugin.model_select_saves_default(), "插件声明应为：/model 会改默认模型");

    let cwd = tui::temp_cwd("claude-model-probe");
    let mut argv = plugin.launch_argv();
    argv.extend(["--model", "haiku"].map(String::from));
    let mut tui = Tui::spawn(&argv, &cwd, &[]);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        walk(&tui, &spec, &config, &settings, &cwd, original.as_deref())
    }));
    tui.kill();
    // 成败都核对：默认模型必须是开跑前的值。
    pause(500);
    let after = default_model(&settings);
    if after != original {
        println!("  ! 命令没能把默认模型切回（现在是 {after:?}），直接改回配置文件里的 model 键");
        restore_by_file(&settings, original.as_deref());
    }
    let restored = default_model(&settings);
    println!("  收尾核对：新会话默认模型 = {restored:?}（开跑前 {original:?}）");
    for _ in 0..20 {
        if std::fs::remove_dir_all(&cwd).is_ok() || !cwd.exists() {
            break;
        }
        pause(250);
    }
    assert_eq!(restored, original, "默认模型没有复原");
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
    println!("===== claude：通过 =====");
}

fn walk(
    tui: &Tui,
    spec: &meowo_agent::chat_ui::ComposerSpec,
    config: &Path,
    settings: &Path,
    cwd: &Path,
    original: Option<&str>,
) {
    tui::wait_ready(tui, spec);

    // 1. 空闲时切一次：haiku → sonnet。
    println!("  [空闲时切模型]");
    let line = switch_model(tui, "sonnet");
    assert!(line.to_lowercase().contains("sonnet"), "确认行里应该是 Sonnet：{line}");
    println!("    配置里的默认模型此刻是 {:?}（/model 确实会改默认值）", default_model(settings));
    tui.submit("What is 17+25? Reply with just the number.");
    assert!(tui.wait("切换后的回合得到回答（42）", 120, |tui| answered(&tui.text(), "17+25", "42")));
    let mut used = None;
    assert!(tui.wait("会话记录里这一回合用的是 sonnet", 30, |_| {
        used = transcript_model(config, cwd);
        used.as_deref().is_some_and(|model| model.contains("sonnet"))
    }));
    println!("    会话记录里的模型：{}", used.unwrap());

    // 2. 回合进行中选了模型：对话页把它排到回合结束后执行。这里起一个要跑一阵的回合，等它结束、
    //    输入框回到空闲，再把切换命令发出去，确认照样生效。
    println!("  [回合进行中选模型，回合结束后执行]");
    tui.submit("Count from 1 to 80, one number per line, no other text.");
    // 不等「看见它在跑」：sonnet 数到 80 只要一两秒，状态行一闪就过（第一次走查就栽在这儿）。
    // 这里要验的是回合结束之后发出去的切换命令生效，排队本身由前端测试断言。
    println!("    （回合进行中用户在对话页选了 haiku，选择进了队列）");
    let mut quiet = 0;
    assert!(tui.wait("回合结束，输入框认得出、是空的", 180, |tui| {
        let text = tui.text();
        let finished = text
            .lines()
            .any(|line| line.trim().trim_start_matches(|c: char| !c.is_ascii_digit()) == "80")
            && !text.contains("esc to interrupt")
            && tui.composer(spec).is_some_and(|read| read.empty);
        quiet = if finished { quiet + 1 } else { 0 };
        // 连续几次读屏都是结束态才算（状态行在工具边界会短暂消失）。
        quiet >= 6
    }));
    let line = switch_model(tui, "haiku");
    assert!(line.to_lowercase().contains("haiku"), "确认行里应该是 Haiku：{line}");
    tui.submit("What is 310+470? Reply with just the number.");
    assert!(tui.wait("切换后的回合得到回答（780）", 120, |tui| answered(&tui.text(), "310+470", "780")));
    let mut used = None;
    assert!(tui.wait("会话记录里这一回合用的是 haiku", 30, |_| {
        used = transcript_model(config, cwd);
        used.as_deref().is_some_and(|model| model.contains("haiku"))
    }));
    println!("    会话记录里的模型：{}", used.unwrap());

    // 3. 把新会话默认模型切回开跑前的值。原来没设的，用 `default` 清掉。
    println!("  [切回原来的默认模型]");
    assert!(tui.wait("回到空闲", 60, |tui| {
        !tui.text().contains("esc to interrupt") && tui.composer(spec).is_some_and(|read| read.empty)
    }));
    pause(1000);
    switch_model(tui, original.unwrap_or("default"));
    let mut now = None;
    assert!(
        tui.wait("配置里的默认模型回到原值", 15, |_| {
            now = default_model(settings);
            now.as_deref() == original
        }),
        "切回之后配置里是 {now:?}，应为 {original:?}"
    );
    std::thread::sleep(Duration::from_millis(300));
}
