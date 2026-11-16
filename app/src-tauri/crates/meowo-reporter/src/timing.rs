//! hook 的时间预算：自己保证不超时，慢了留下记录。
//!
//! 各家 CLI 给普通 hook 的时限多为 5 秒，超时后 Claude Code 会在对话里打出红字
//! 「hook timed out」，这次上报也一并作废（2026-11-16 实拍：Meowo 刚升级重启时，一条
//! UserPromptSubmit 超过 5 秒）。reporter 平时 0.2 到 0.6 秒，慢下来通常是在等别的连接
//! 释放数据库写锁，或者给大仓库拍回合快照。
//!
//! 所以普通事件挂一个看门狗：到 [`BUDGET`] 还没做完，就记一行日志、以 0 退出。少记这一笔
//! 和超时被丢弃的结果一样，但用户不会再看到红字。会阻塞等人作答的事件（GUI 审批、提问代答）
//! 本来就要等几分钟，不挂看门狗。
//!
//! 慢了要留痕：超过 [`SLOW`] 或被看门狗结束，都往 `~/.meowo/logs/reporter.log` 写一行，
//! 记下事件、provider、耗时和当时卡在哪一步。上一次超时查不出原因，就是因为没有这行记录。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// 普通事件的总时限。低于各家常见的 5 秒；SQLite 的 busy_timeout 是 3 秒，留出一次
/// 完整的锁等待再加收尾的余量。Codex 的 SessionEnd/Interrupt 声明 3 秒，看门狗管不到，
/// 那两个事件只做收尾写库，不拍快照。
pub const BUDGET: Duration = Duration::from_secs(4);
/// 实际用的时限：`MEOWO_HOOK_BUDGET_MS` 可以改短（测试靠它触发看门狗，排查时也可临时调），
/// 只接受比 [`BUDGET`] 短的值，不能借它拖过 CLI 的时限。
pub fn budget() -> Duration {
    std::env::var("MEOWO_HOOK_BUDGET_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .filter(|value| *value < BUDGET)
        .unwrap_or(BUDGET)
}

/// 超过这个时长就记一行慢日志（不打断）。
pub const SLOW: Duration = Duration::from_secs(2);
/// 日志超过这个大小就挪成 `reporter.1.log`（覆盖上一份）。
const MAX_LOG_BYTES: u64 = 256 * 1024;

/// 当前卡在哪一步。看门狗触发时据此写日志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    Start = 0,
    OpenDb = 1,
    Dispatch = 2,
    Checkpoint = 3,
    Broker = 4,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Start => "start",
            Phase::OpenDb => "open_db",
            Phase::Dispatch => "dispatch",
            Phase::Checkpoint => "checkpoint",
            Phase::Broker => "broker",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Phase::OpenDb,
            2 => Phase::Dispatch,
            3 => Phase::Checkpoint,
            4 => Phase::Broker,
            _ => Phase::Start,
        }
    }
}

static PHASE: AtomicU8 = AtomicU8::new(Phase::Start as u8);

pub fn set_phase(phase: Phase) {
    PHASE.store(phase as u8, Ordering::Relaxed);
}

pub fn phase() -> Phase {
    Phase::from_u8(PHASE.load(Ordering::Relaxed))
}

/// 进入一步，离开（guard 析构）时恢复上一步。用于嵌在别的步骤里的快照。
pub fn enter(phase: Phase) -> PhaseGuard {
    let previous = PHASE.swap(phase as u8, Ordering::Relaxed);
    PhaseGuard(previous)
}

pub struct PhaseGuard(u8);

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        PHASE.store(self.0, Ordering::Relaxed);
    }
}

/// 起看门狗：`budget` 内没结束就记日志并以 0 退出整个进程。
///
/// 进程在写库途中退出是安全的：SQLite 的 WAL 事务要么整笔提交、要么整笔不算。
/// 拍快照的 git 子进程只写临时 index 和隐藏 ref，被丢下也不碰用户的工作区和暂存区。
pub fn arm_watchdog(budget: Duration, started: Instant, event: String, provider: String) {
    let _ = std::thread::Builder::new()
        .name("hook-watchdog".into())
        .spawn(move || {
            std::thread::sleep(budget.saturating_sub(started.elapsed()));
            log_slow(&event, &provider, started.elapsed(), phase(), true);
            std::process::exit(0);
        });
}

/// 写一行慢日志。任何失败都吞掉：日志不能反过来拖慢或打断 hook。
pub fn log_slow(event: &str, provider: &str, elapsed: Duration, phase: Phase, aborted: bool) {
    let Some(dir) = log_dir() else {
        return;
    };
    let line = format_line(SystemTime::now(), event, provider, elapsed, phase, aborted);
    let _ = append(&dir, &line);
}

fn log_dir() -> Option<PathBuf> {
    Some(crate::db_path()?.parent()?.join("logs"))
}

pub fn format_line(
    at: SystemTime,
    event: &str,
    provider: &str,
    elapsed: Duration,
    phase: Phase,
    aborted: bool,
) -> String {
    let outcome = if aborted {
        "到时限提前结束"
    } else {
        "慢"
    };
    format!(
        "{} reporter {outcome}：{event} · provider={provider} · {}ms · 停在 {}\n",
        utc_stamp(at),
        elapsed.as_millis(),
        phase.name()
    )
}

fn append(dir: &Path, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("reporter.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, dir.join("reporter.1.log"));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

/// `2026-11-04T07:12:03Z`，与 app 的 meowo.log 同格式（同一算法，见 app 的 diaglog.rs）。
fn utc_stamp(at: SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_names_event_phase_and_outcome() {
        let at = std::time::UNIX_EPOCH + Duration::from_secs(1_794_808_080);
        let line = format_line(
            at,
            "UserPromptSubmit",
            "claude",
            Duration::from_millis(4012),
            Phase::Checkpoint,
            true,
        );
        assert_eq!(
            line,
            "2026-11-16T05:48:00Z reporter 到时限提前结束：UserPromptSubmit · provider=claude · 4012ms · 停在 checkpoint\n"
        );
    }

    #[test]
    fn log_rotates_when_too_big() {
        let dir = std::env::temp_dir().join(format!("meowo-timing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("reporter.log"),
            vec![b'x'; (MAX_LOG_BYTES + 1) as usize],
        )
        .unwrap();
        append(&dir, "new\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("reporter.log")).unwrap(),
            "new\n"
        );
        assert!(dir.join("reporter.1.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
