//! CLI 意外退出的判定与提醒（二期 #7，2026-11-10）。
//!
//! 托管 PTY 退出时 finalize_exit 不管退出码一律把会话写成「已结束」，之后 liveness 轮询跳过
//! 已结束的会话——CLI 崩了（OOM、panic、被杀）和用户自己结束看起来完全一样，没有提醒，
//! 对话页也只说「会话已结束」。这里补上：判定为意外退出就发出错提醒与系统通知，
//! 对话页据快照里的 exit_unexpected 显示「Agent 意外退出（退出码 N）」并附最后几行输出。

use crate::settings::{load_settings, notify_kind_enabled, tr, ui_lang};

/// Windows 上 Ctrl+C 结束控制台进程的退出码（STATUS_CONTROL_C_EXIT）。
const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
/// Unix 上 SIGINT 结束的惯例退出码（128 + 2）。
const SIGINT_EXIT: u32 = 130;

/// 算不算「意外退出」：退出码不为 0，且不是用户结束的（点了「结束会话」、退出 Meowo、
/// 在终端里按 Ctrl+C 退出），也不在就地重启的窗口内（切模型、接管换参数会先退出再起新进程）。
/// 拿不到退出码（收尸超时）不报：宁可漏一次，也不把正常结束说成崩溃。
pub(crate) fn is_unexpected_exit(code: Option<u32>, user_stopped: bool, restarted: bool) -> bool {
    if user_stopped || restarted {
        return false;
    }
    matches!(code, Some(code) if code != 0 && code != STATUS_CONTROL_C_EXIT && code != SIGINT_EXIT)
}

/// 提醒与通知里的那一句：「Agent 意外退出（退出码 N）」。与前端 chat.agentCrashed 同文。
pub(crate) fn crash_label(lang: &str, code: u32) -> String {
    if lang == "en" {
        format!("Agent exited unexpectedly (exit code {code})")
    } else {
        format!("Agent 意外退出（退出码 {code}）")
    }
}

/// 发出错提醒（灵动岛）、手机推送与系统通知。由 finalize_exit 在重启窗口过后的线程里调用，
/// 此刻会话在库里已是「已结束」，liveness 不会再看它，只能在这里发。
pub(crate) fn notify_unexpected_exit(app: &tauri::AppHandle, session_id: i64, code: u32) {
    let settings = load_settings();
    let lang = ui_lang(&settings);
    let store = crate::open_store(&crate::db_path()).ok();
    let title = store
        .as_ref()
        .and_then(|store| store.session_title(session_id).ok().flatten())
        .unwrap_or_default();
    let cwd = store
        .as_ref()
        .and_then(|store| store.session_cwd(session_id).ok().flatten());
    // 项目名取工作目录的末段：与看板卡片的项目名同源（项目按目录建）。
    let project = cwd
        .as_deref()
        .and_then(|cwd| std::path::Path::new(cwd).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let label = crash_label(lang, code);
    crate::watch::emit_island_alert(
        app,
        session_id,
        "error",
        &title,
        &project,
        Some(label.clone()),
    );
    crate::push::push_session(
        &settings,
        crate::push::PushMessage {
            title: format!("{} · {}", project, tr(lang, "notify.error")),
            body: format!("{} · {}", title, label),
            session_id,
            urgent: false,
        },
    );
    if notify_kind_enabled(&settings, "error") {
        // 进程已经没了：pid 给 0，点通知时托管判定落空、聚焦终端也落空，回退打开对话窗
        // 并落在该会话上（show_session_notification 的既有兜底），正好看到意外退出的说明。
        crate::watch::show_session_notification(
            app,
            format!("{} · {}", project, tr(lang, "notify.error")),
            format!("{} · {}", title, label),
            session_id,
            0,
            title.clone(),
            cwd,
            None,
            false,
            crate::watch::NotifyKind::Error,
            lang.to_string(),
            settings.notification_sound,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonzero_exit_without_user_stop_is_unexpected() {
        assert!(is_unexpected_exit(Some(1), false, false));
        assert!(
            is_unexpected_exit(Some(0xC000_0005), false, false),
            "访问冲突崩溃"
        );
    }

    #[test]
    fn clean_exit_user_stop_restart_and_unknown_code_are_not_reported() {
        assert!(!is_unexpected_exit(Some(0), false, false), "正常退出");
        assert!(
            !is_unexpected_exit(Some(1), true, false),
            "用户点了结束会话 / 退出 Meowo"
        );
        assert!(!is_unexpected_exit(Some(1), false, true), "就地重启窗口内");
        assert!(!is_unexpected_exit(None, false, false), "拿不到退出码不报");
        assert!(
            !is_unexpected_exit(Some(STATUS_CONTROL_C_EXIT), false, false),
            "终端里 Ctrl+C"
        );
        assert!(
            !is_unexpected_exit(Some(SIGINT_EXIT), false, false),
            "Unix 上 Ctrl+C"
        );
    }

    #[test]
    fn crash_label_matches_frontend_copy() {
        assert_eq!(crash_label("zh", 3), "Agent 意外退出（退出码 3）");
        assert_eq!(
            crash_label("en", 3),
            "Agent exited unexpectedly (exit code 3)"
        );
    }
}
