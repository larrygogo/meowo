//! 原生 TUI 会话的关卡等待器：`meowo-reporter lead wait --provider <kimi|gemini> --session <id>`。
//!
//! 为什么要它（任务 .lead/20261113-lead-tui）：kimi/gemini 的 hook 都不能自己开新回合，但两家的
//! TUI 都会在「后台命令结束」时于空闲处自动开一轮（kimi 后台 Bash 完成 → loop.notify；gemini
//! 后台 shell 完成 → inject + modelSteering 空闲自动提交）。所以关卡挂起时让 TUI 自己在后台跑这个
//! 等待器：答复落盘它就把答复打到 stdout 并退出，CLI 收到「后台任务完成」后接着做。不往 PTY 打字
//! （覆盖草稿、误答审批），也不走 ACP（leader：Provider 会把 ACP 流量另算）。
//!
//! 与 Claude 的 `lead wait`（asyncRewake hook，stdin 读 hook 载荷、退出码 2 唤醒）不同：这里没有
//! hook 载荷，stdin 可能是 TUI 的伪终端、永远读不到 EOF，所以身份全走参数，且绝不读 stdin。
//!
//! 退出条件（全部以 0 退出，stdout 是给模型看的一句话）：
//! - 关卡被作答 → 打印答复；
//! - 关卡被撤掉 / 换成别的 / 已被消费 → 说明无需动作；
//! - 同一任务有新等待器接手（`.native-waiter.json` 记的不是自己）→ 说明无需动作；
//! - 宿主 CLI 进程没了（TUI 退出、终端被关）→ 静默退出，不留孤儿（kimi 退出时不杀后台任务，实测）。

use crate::lead::{self, Output};
use meowo_protocol::ipc::LeadGateDto;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 等待器登记：宿主据此判断「会话里确实有人在等」，hook 据此判断要不要（再）启动等待器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaiterRecord {
    pub pid: u32,
    /// 进程启动时刻（秒），防 pid 被系统回收复用后误判「还在等」。
    pub started: u64,
    /// 在等的关卡；空串 = 已启动、还在等下一个挂起关卡写出来（kimi 先启动等待器再写关卡）。
    pub gate: String,
    pub session: String,
    pub provider: String,
    /// 托管它的 CLI 进程；找不到时为 None（仍按关卡状态退出，只是少了孤儿保护）。
    pub cli_pid: Option<u32>,
    pub cli_started: Option<u64>,
}

pub const WAITER_FILE: &str = ".native-waiter.json";
const POLL: Duration = Duration::from_secs(1);
/// 与 Claude 的 wait 同上限：关卡最多等 7 天。
const MAX: Duration = Duration::from_secs(7 * 24 * 3600);
/// 先启动、后写关卡时，最多等关卡出现这么久（模型被拦下后通常几秒内重写 gate.json）。
const AWAIT_GATE: Duration = Duration::from_secs(10 * 60);

pub fn waiter_path(task: &Path) -> PathBuf {
    task.join(WAITER_FILE)
}

pub fn read_record(task: &Path) -> Option<WaiterRecord> {
    let bytes = std::fs::read(waiter_path(task)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 进程启动时刻（秒）；进程不存在返回 None。
pub fn process_started(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let target = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        false,
        ProcessRefreshKind::new(),
    );
    sys.process(target).map(|p| p.start_time())
}

fn same_process(pid: u32, started: u64) -> bool {
    // 启动时刻按秒取整，两次读数偶有 1 秒出入；pid 复用的进程启动时刻相差远大于此。
    process_started(pid).is_some_and(|now| now.abs_diff(started) <= 1)
}

/// 这个关卡此刻有没有活着的等待器（含「在等下一个关卡」的那种）。
pub fn live_waiter(task: &Path, gate_id: &str) -> Option<WaiterRecord> {
    read_record(task)
        .filter(|r| (r.gate == gate_id || r.gate.is_empty()) && same_process(r.pid, r.started))
}

/// 任务里有没有活着的等待器，不论在等哪个关卡。
pub fn any_live_waiter(task: &Path) -> bool {
    read_record(task).is_some_and(|r| same_process(r.pid, r.started))
}

/// 关卡是否需要（再）启动等待器：任务归本会话、关卡挂起、没有活着的等待器。
/// 已作答的关卡不在这里：那种情况 hook 直接把答复交给模型（见 lead_provider）。
pub fn needs_waiter(task: &Path, session: &str) -> Option<LeadGateDto> {
    if session.is_empty() || std::fs::read_to_string(task.join(".owner")).ok()?.trim() != session {
        return None;
    }
    let gate = lead::read_gate(task)?;
    (gate.status == "pending" && live_waiter(task, &gate.id).is_none()).then_some(gate)
}

/// 已作答、本会话还没被送达过的关卡（`.delivered` 记关卡 id，与 Claude 的 wait 共用）。
pub fn undelivered_answer(task: &Path, session: &str) -> Option<LeadGateDto> {
    if session.is_empty() || std::fs::read_to_string(task.join(".owner")).ok()?.trim() != session {
        return None;
    }
    let gate = lead::read_gate(task)?;
    let delivered = std::fs::read_to_string(task.join(".delivered")).unwrap_or_default();
    (gate.status == "answered" && delivered.trim() != gate.id).then_some(gate)
}

pub fn mark_delivered(task: &Path, gate: &LeadGateDto) {
    let _ = std::fs::write(task.join(".delivered"), &gate.id);
}

/// 等待命令。session 只含 `[A-Za-z0-9_-]`（hook 层已校验），reporter 路径来自本进程，
/// 命令里没有任何用户可控文本——关卡 id 也不放进去，等待器启动时自己读当前关卡。
/// 前缀与 Lead 安装写入的放行规则同源（[`meowo_agent::lead_install::waiter_command_prefix`]）。
/// shell 方言由插件声明（[`meowo_agent::lead_install::waiter_shell`]）；没有会话内等待器机制的
/// provider 返回 None，调用方不提示启动等待器。
pub fn command(provider: &str, reporter: &str, session: &str) -> Option<String> {
    let shell = meowo_agent::lead_install::waiter_shell(provider)?;
    Some(format!(
        "{} --session {session}",
        meowo_agent::lead_install::waiter_command_prefix(shell, provider, reporter)
    ))
}

/// 本进程的路径（hook 与等待器是同一个 reporter）。
pub fn reporter_path() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe = lead::strip_verbatim(exe);
    Some(exe.to_string_lossy().into_owned())
}

/// 命令行入口：`lead wait --provider P --session S`。没有 `--session` 时返回 None（交给 Claude 的 wait）。
pub fn run_from_args(args: &[String], registry: &Path) -> Option<Output> {
    let flag = |name: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].clone())
    };
    let session = flag("--session")?;
    let provider = flag("--provider").unwrap_or_default();
    Some(
        std::panic::catch_unwind(|| {
            wait(&provider, &session, registry, POLL, MAX, || {
                crate::proc::owner_pid(&provider)
            })
        })
        .unwrap_or_default(),
    )
}

fn say(text: impl Into<String>) -> Output {
    Output {
        stdout: Some(text.into()),
        ..Output::default()
    }
}

/// 阻塞等待本会话任务上的关卡。`find_cli` 抽出来是为了单测不依赖真实进程树。
pub fn wait(
    provider: &str,
    session: &str,
    registry: &Path,
    poll: Duration,
    max: Duration,
    find_cli: impl FnOnce() -> Option<u32>,
) -> Output {
    let valid = !session.is_empty()
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    // 只服务声明了会话内等待器机制的 provider（插件描述，不按身份硬编码，做空 M7）。
    if !valid || meowo_agent::lead_install::waiter_shell(provider).is_none() {
        return say("[lead] 等待器参数无效，已退出。");
    }
    let Some(task) = lead::task_of_session(session, registry) else {
        return say("[lead] 没有找到本会话认领的 Lead 任务，等待器已退出，无需处理。");
    };
    let initial = lead::read_gate(&task);
    if let Some(gate) = initial.as_ref().filter(|g| g.status == "answered") {
        mark_delivered(&task, gate);
        return say(lead::answer_message(&task, gate));
    }
    // 关卡还没写（或只剩上一轮已消费的旧关卡）：先登记、等下一个挂起关卡出现再绑定。
    let stale = initial
        .as_ref()
        .filter(|g| g.status != "pending")
        .map(|g| g.id.clone());
    let me = std::process::id();
    let cli_pid = find_cli();
    let mut record = WaiterRecord {
        pid: me,
        started: process_started(me).unwrap_or_default(),
        gate: initial
            .as_ref()
            .filter(|g| g.status == "pending")
            .map(|g| g.id.clone())
            .unwrap_or_default(),
        session: session.into(),
        provider: provider.into(),
        cli_started: cli_pid.and_then(process_started),
        cli_pid,
    };
    let path = waiter_path(&task);
    let publish = |record: &WaiterRecord| {
        serde_json::to_string(record)
            .ok()
            .is_some_and(|text| meowo_agent::fsutil::write_atomic(&path, &text).is_ok())
    };
    // 后来者接管：同一任务重复启动时只留最新的那个，旧的看到记录不是自己就退出。
    if !publish(&record) {
        return say("[lead] 等待器无法登记，已退出。在会话里发一句话即可继续。");
    }
    let mine = |r: &WaiterRecord| r.pid == record.pid && r.started == record.started;
    let start = std::time::Instant::now();
    let mut misses = 0;
    let out = loop {
        std::thread::sleep(poll);
        if !read_record(&task).as_ref().is_some_and(mine) {
            break say("[lead] 另一个等待器接手了这个关卡，本条无需处理，直接结束本轮。");
        }
        if let (Some(pid), Some(started)) = (record.cli_pid, record.cli_started) {
            if !same_process(pid, started) {
                break Output::default(); // TUI 没了，没有人会读这条输出
            }
        }
        if start.elapsed() > max {
            break say("[lead] 等待关卡答复超时，已退出。");
        }
        let gate = lead::read_gate(&task);
        if record.gate.is_empty() {
            match gate {
                Some(g) if g.status == "pending" && stale.as_deref() != Some(g.id.as_str()) => {
                    record.gate = g.id;
                    if !publish(&record) {
                        break say("[lead] 等待器无法登记，已退出。在会话里发一句话即可继续。");
                    }
                }
                _ if start.elapsed() > AWAIT_GATE.min(max) => {
                    break say("[lead] 没等到新的关卡，等待器已退出，无需处理。");
                }
                _ => {}
            }
            continue;
        }
        match gate {
            Some(g) if g.id == record.gate && g.status == "answered" => {
                mark_delivered(&task, &g);
                break say(lead::answer_message(&task, &g));
            }
            Some(g) if g.id == record.gate && g.status == "pending" => misses = 0,
            Some(_) => break say("[lead] 关卡已更换或已处理，本条无需处理。"),
            // 作答时的 rename、杀毒软件占用会偶发读失败，连续多次才当作关卡被撤掉。
            None => {
                misses += 1;
                if misses >= 5 {
                    break say("[lead] 关卡已撤掉，本条无需处理。");
                }
            }
        }
    };
    if read_record(&task).as_ref().is_some_and(mine) {
        let _ = std::fs::remove_file(&path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fx {
        root: PathBuf,
        task: PathBuf,
        registry: PathBuf,
    }
    impl Fx {
        fn new(status: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "meowo-native-waiter-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let task = root.join(".lead/t1");
            std::fs::create_dir_all(&task).unwrap();
            let task = lead::strip_verbatim(task.canonicalize().unwrap());
            std::fs::write(task.join(".owner"), "session_abc").unwrap();
            std::fs::write(
                task.join("gate.json"),
                json!({"id":"Q-1","type":"Q","title":"t","status":status,"options":[{"id":"go","label":"Go"}]}).to_string(),
            )
            .unwrap();
            let registry = root.join("lead-sessions.json");
            std::fs::write(
                &registry,
                json!({"session_abc": task.to_string_lossy()}).to_string(),
            )
            .unwrap();
            Self {
                root,
                task,
                registry,
            }
        }
        fn answer(&self) {
            lead::answer_gate(
                &self.task,
                "Q-1",
                &meowo_protocol::ipc::LeadGateAnswerDto {
                    choice: "go".into(),
                    note: "ok".into(),
                },
            )
            .unwrap();
        }
    }
    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    const P: Duration = Duration::from_millis(20);
    const M: Duration = Duration::from_secs(20);

    #[test]
    fn answer_arriving_later_is_printed_and_marks_delivered() {
        let fx = Fx::new("pending");
        let (task, registry) = (fx.task.clone(), fx.registry.clone());
        let handle =
            std::thread::spawn(move || wait("kimi", "session_abc", &registry, P, M, || None));
        // 等待器登记后才算在等：宿主据此显示「会自动继续」。
        for _ in 0..200 {
            if live_waiter(&task, "Q-1").is_some() {
                break;
            }
            std::thread::sleep(P);
        }
        assert!(live_waiter(&fx.task, "Q-1").is_some());
        assert!(needs_waiter(&fx.task, "session_abc").is_none());
        fx.answer();
        let out = handle.join().unwrap();
        assert_eq!(out.exit, 0);
        assert!(out
            .stdout
            .unwrap()
            .contains("leader 已在 Meowo 里回答关卡 Q"));
        assert_eq!(
            std::fs::read_to_string(fx.task.join(".delivered")).unwrap(),
            "Q-1"
        );
        assert!(!waiter_path(&fx.task).exists(), "退出后要清掉登记");
        assert!(undelivered_answer(&fx.task, "session_abc").is_none());
    }

    #[test]
    fn already_answered_returns_immediately_without_registering() {
        let fx = Fx::new("pending");
        fx.answer();
        assert!(undelivered_answer(&fx.task, "session_abc").is_some());
        let out = wait("gemini", "session_abc", &fx.registry, P, M, || None);
        assert!(out.stdout.unwrap().contains("选择 go"));
        assert!(!waiter_path(&fx.task).exists());
    }

    #[test]
    fn foreign_session_or_bad_args_never_wait() {
        let fx = Fx::new("pending");
        let out = wait("kimi", "session_other", &fx.registry, P, M, || None);
        assert!(out.stdout.unwrap().contains("没有找到"));
        let out = wait("kimi", "a b;rm", &fx.registry, P, M, || None);
        assert!(out.stdout.unwrap().contains("参数无效"));
        // 没声明会话内等待器机制的 provider 不等（codex/opencode 走宿主服务投递）。
        for provider in ["codex", "opencode", "claude", ""] {
            let out = wait(provider, "session_abc", &fx.registry, P, M, || None);
            assert!(out.stdout.unwrap().contains("参数无效"), "{provider}");
            assert!(command(provider, "r", "s").is_none(), "{provider}");
        }
        assert!(needs_waiter(&fx.task, "session_other").is_none());
        assert!(needs_waiter(&fx.task, "session_abc").is_some());
    }

    #[test]
    fn newer_waiter_takes_over_and_gate_change_releases() {
        let fx = Fx::new("pending");
        let (task, registry) = (fx.task.clone(), fx.registry.clone());
        let handle =
            std::thread::spawn(move || wait("kimi", "session_abc", &registry, P, M, || None));
        for _ in 0..200 {
            if read_record(&task).is_some() {
                break;
            }
            std::thread::sleep(P);
        }
        // 另一个等待器接手（同一测试进程 pid 相同，改 started 模拟另一条记录）。
        let mut other = read_record(&fx.task).unwrap();
        other.started += 100;
        std::fs::write(
            waiter_path(&fx.task),
            serde_json::to_string(&other).unwrap(),
        )
        .unwrap();
        let out = handle.join().unwrap();
        assert!(out.stdout.unwrap().contains("接手"));
        assert!(waiter_path(&fx.task).exists(), "不能删掉接手者的登记");

        // 关卡被换掉：等待器自己退出。
        std::fs::remove_file(waiter_path(&fx.task)).unwrap();
        let (task, registry) = (fx.task.clone(), fx.registry.clone());
        let handle =
            std::thread::spawn(move || wait("kimi", "session_abc", &registry, P, M, || None));
        for _ in 0..200 {
            if read_record(&task).is_some() {
                break;
            }
            std::thread::sleep(P);
        }
        std::fs::write(
            fx.task.join("gate.json"),
            json!({"id":"Q-2","type":"Q","title":"t","status":"pending"}).to_string(),
        )
        .unwrap();
        assert!(handle.join().unwrap().stdout.unwrap().contains("已更换"));
    }

    /// kimi 先启动等待器、后写关卡：上一轮已消费的旧关卡不算，等新挂起关卡出现再绑定。
    #[test]
    fn waiter_started_before_the_gate_binds_to_the_next_pending_gate() {
        let fx = Fx::new("consumed");
        let (task, registry) = (fx.task.clone(), fx.registry.clone());
        let handle =
            std::thread::spawn(move || wait("kimi", "session_abc", &registry, P, M, || None));
        for _ in 0..200 {
            if read_record(&task).is_some() {
                break;
            }
            std::thread::sleep(P);
        }
        assert_eq!(read_record(&fx.task).unwrap().gate, "");
        assert!(
            live_waiter(&fx.task, "anything").is_some(),
            "等下一个关卡时对任何关卡都算在等"
        );
        std::fs::write(
            fx.task.join("gate.json"),
            json!({"id":"Q-2","type":"Q","title":"t","status":"pending","options":[{"id":"go","label":"Go"}]}).to_string(),
        )
        .unwrap();
        for _ in 0..200 {
            if read_record(&fx.task).is_some_and(|r| r.gate == "Q-2") {
                break;
            }
            std::thread::sleep(P);
        }
        lead::answer_gate(
            &fx.task,
            "Q-2",
            &meowo_protocol::ipc::LeadGateAnswerDto {
                choice: "go".into(),
                note: String::new(),
            },
        )
        .unwrap();
        assert!(handle.join().unwrap().stdout.unwrap().contains("关卡 Q"));
    }

    /// TUI 退出后等待器不能变孤儿：宿主 CLI 进程没了就静默退出。
    #[test]
    fn exits_silently_when_the_cli_process_is_gone() {
        let fx = Fx::new("pending");
        let mut child = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .args(if cfg!(windows) {
                ["/C", "ping -n 3 127.0.0.1 >NUL"]
            } else {
                ["-c", "sleep 2"]
            })
            .spawn()
            .unwrap();
        let pid = child.id();
        let registry = fx.registry.clone();
        let handle = std::thread::spawn(move || {
            wait("kimi", "session_abc", &registry, P, M, move || Some(pid))
        });
        child.wait().unwrap();
        let out = handle.join().unwrap();
        assert_eq!(out, Output::default());
        assert!(!waiter_path(&fx.task).exists());
    }

    #[test]
    fn commands_have_no_user_text_and_quote_per_shell() {
        let kimi = command(
            "kimi",
            r"C:\Program Files\Meowo\meowo-reporter.exe",
            "session_abc",
        )
        .unwrap();
        assert_eq!(kimi, "\"C:/Program Files/Meowo/meowo-reporter.exe\" lead wait --provider kimi --session session_abc");
        let gemini = command("gemini", r"C:\Users\o'neil\meowo-reporter.exe", "abc-1").unwrap();
        if cfg!(windows) {
            assert_eq!(
                gemini,
                r"& 'C:\Users\o''neil\meowo-reporter.exe' lead wait --provider gemini --session abc-1"
            );
        }
    }
}
