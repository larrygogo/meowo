//! 有会话在跑时防止电脑休眠（对照 Kimi Code 设置里的「防止休眠」：电脑休眠后，智能体会暂停执行任务）。
//!
//! Meowo 托管的是一跑就是几十分钟的 agent；人走开、电脑按电源计划睡过去，回来看到的是跑到一半停住的
//! 任务。这里只拦**系统休眠**、不拦息屏（屏幕照常按计划熄灭，耗电小），并且只在「运行中」有会话时拦——
//! 等你回答 / 审批的会话不算（agent 自己也停着），全部停下立刻交还系统的休眠策略。
//!
//! 「有没有会话在跑」与看板「运行中」角标同一口径（session_query::live_counts_now）。巡检 30s 一次：
//! 系统休眠计时以分钟计，晚 30s 拿到 / 放开无关紧要，而每次采样要扫进程表，不该更勤。

use std::time::Duration;

const POLL: Duration = Duration::from_secs(30);

pub(crate) fn spawn_keep_awake(app: tauri::AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("keep-awake".into())
        .spawn(move || {
            let mut awake = KeepAwake::default();
            loop {
                let wanted = crate::settings::load_settings().keep_awake_while_running
                    && crate::session_query::live_counts_now(&app)
                        .map(|counts| counts.running > 0)
                        // 统计失败（库暂不可用等）按「没在跑」处理：宁可放开让系统睡，也不无限期拦着。
                        .unwrap_or(false);
                awake.set(wanted);
                std::thread::sleep(POLL);
            }
        });
    if let Err(error) = spawned {
        eprintln!("防止休眠巡检线程启动失败: {error}");
    }
}

#[derive(Default)]
struct KeepAwake {
    held: bool,
    /// macOS：持有期间挂着一个 `caffeinate -i`，放开时结束它。
    #[cfg(target_os = "macos")]
    child: Option<std::process::Child>,
}

impl KeepAwake {
    fn set(&mut self, wanted: bool) {
        if wanted == self.held {
            return;
        }
        self.held = wanted;
        // Windows：ES_CONTINUOUS 状态挂在**调用线程**上、线程退出即失效，所以必须由这条常驻的巡检
        // 线程自己设、自己清（进程退出时系统也会一并收回）。只给 ES_SYSTEM_REQUIRED：不拦息屏。
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Power::{
                SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
            };
            let flags = if wanted {
                ES_CONTINUOUS | ES_SYSTEM_REQUIRED
            } else {
                ES_CONTINUOUS
            };
            // SAFETY: 纯 Win32 调用，无指针参数。返回 0 表示失败，失败就是没拦住，系统照常休眠，无副作用。
            unsafe {
                SetThreadExecutionState(flags);
            }
        }
        // macOS（未经本机编译，依赖 CI）：系统自带的 caffeinate，-i 只阻止空闲休眠（不拦息屏），
        // -w 跟着本进程，Meowo 退出或崩溃时它自己结束，不会留下一个永远拦着休眠的孤儿。
        #[cfg(target_os = "macos")]
        {
            if wanted {
                self.child = std::process::Command::new("caffeinate")
                    .arg("-i")
                    .arg("-w")
                    .arg(std::process::id().to_string())
                    .spawn()
                    .ok();
            } else if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
