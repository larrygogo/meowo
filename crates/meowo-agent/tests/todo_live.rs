//! 用**本机真实 kimi 会话**验证待办快照的读取。数据不存在时自动跳过。
//!
//! 存在的理由：待办的解析依赖 kimi 的落盘格式（工具名 `TodoList`、字段 `title`、
//! 状态词 `done`），那是外部约定、随版本变。合成用例只能证明「按我以为的格式解析是对的」，
//! 证明不了「格式还是我以为的那个」。

use meowo_agent::plugins::kimi::telemetry::parse_todos;
use std::path::PathBuf;

fn kimi_main_wires() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Ok(workdirs) = std::fs::read_dir(home.join(".kimi-code/sessions")) else {
        return out;
    };
    for workdir in workdirs.flatten() {
        let Ok(sessions) = std::fs::read_dir(workdir.path()) else {
            continue;
        };
        for session in sessions.flatten() {
            let wire = session.path().join("agents/main/wire.jsonl");
            if wire.is_file() {
                out.push(wire);
            }
        }
    }
    out
}

#[test]
fn reads_real_kimi_todo_snapshots_with_done_status() {
    let wires = kimi_main_wires();
    if wires.is_empty() {
        eprintln!("跳过：本机没有 kimi 会话数据");
        return;
    }
    let mut sessions_with_todos = 0;
    let mut statuses = std::collections::BTreeMap::new();
    for wire in &wires {
        let Ok(text) = std::fs::read_to_string(wire) else {
            continue;
        };
        let Some(todos) = parse_todos(&text) else {
            continue;
        };
        sessions_with_todos += 1;
        for todo in &todos {
            *statuses.entry(todo.status.clone()).or_insert(0usize) += 1;
            assert!(
                !todo.content.trim().is_empty(),
                "待办内容不该为空：{todo:?}"
            );
        }
    }
    eprintln!("有待办的会话 {sessions_with_todos}/{}，状态分布 {statuses:?}", wires.len());
    if sessions_with_todos == 0 {
        eprintln!("跳过断言：本机 kimi 会话里没有待办记录");
        return;
    }
    // 关键回归：kimi 用 `done` 表示已完成。若某天它改词，这里会先红——而不是等到界面上
    // 「待办永远勾不上」才被发现。状态词→枚举的映射由 meowo-store 侧的单测覆盖
    //（本 crate 刻意不依赖 store，插件层不反向依赖 DB 层）。
    let known = ["pending", "in_progress", "done"];
    for status in statuses.keys() {
        assert!(
            known.contains(&status.as_str()),
            "出现未知状态词 {status:?}；需同步 TodoStatus::from_str 的别名表，否则会被静默降级成待办"
        );
    }
}
