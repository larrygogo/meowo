//! 跨平台纯逻辑：供 macOS 终端跳转使用，但不依赖 macOS API，便于在任意平台单测。
#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKind {
    Terminal,
    ITerm2,
    Other,
}

/// 把 `ps -o tty=` 的输出规范化成 `/dev/ttysNNN`；无控制终端返回 None。
pub fn normalize_tty(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t == "??" || t == "?" {
        return None;
    }
    if let Some(rest) = t.strip_prefix("/dev/") {
        return if rest.is_empty() { None } else { Some(format!("/dev/{rest}")) };
    }
    if let Some(num) = t.strip_prefix("ttys") {
        return Some(format!("/dev/ttys{num}"));
    }
    if let Some(num) = t.strip_prefix('s') {
        // ps 偶尔返回 's003'
        return Some(format!("/dev/ttys{num}"));
    }
    Some(format!("/dev/{t}"))
}

/// 进程名按「从 claude 自身向祖先」顺序传入，返回最近的已知终端宿主。
pub fn detect_term_kind(ancestor_names_root_first: &[String]) -> TermKind {
    for name in ancestor_names_root_first {
        let n = name.to_ascii_lowercase();
        if n.contains("iterm") {
            return TermKind::ITerm2;
        }
        if n == "terminal" || n.contains("terminal.app") {
            return TermKind::Terminal;
        }
    }
    TermKind::Other
}

/// 设置里的字符串 → 打开未连接会话用的终端宿主：含 "iterm" → iTerm2，其余（含 "terminal"/未知/空）→ Terminal。
pub fn resume_kind_from_setting(s: &str) -> TermKind {
    if s.to_ascii_lowercase().contains("iterm") {
        TermKind::ITerm2
    } else {
        TermKind::Terminal
    }
}

/// 返回按 tty 定位并置前的 AppleScript（tty 通过 osascript argv 传入）。未知宿主返回 None。
pub fn focus_script(kind: TermKind) -> Option<&'static str> {
    match kind {
        TermKind::Terminal => Some(
            r#"on run argv
  set targetTTY to item 1 of argv
  tell application "Terminal"
    repeat with w in windows
      repeat with t in tabs of w
        if (tty of t) is targetTTY then
          set selected of t to true
          set frontmost of w to true
          activate
          return "FOUND"
        end if
      end repeat
    end repeat
  end tell
  return "NOT_FOUND"
end run"#,
        ),
        TermKind::ITerm2 => Some(
            r#"on run argv
  set targetTTY to item 1 of argv
  tell application "iTerm2"
    repeat with w in windows
      repeat with t in tabs of w
        repeat with s in sessions of t
          if (tty of s) is targetTTY then
            select w
            select t
            select s
            activate
            return "FOUND"
          end if
        end repeat
      end repeat
    end repeat
  end tell
  return "NOT_FOUND"
end run"#,
        ),
        TermKind::Other => None,
    }
}

/// 返回新开终端执行 `cd <cwd> && claude --resume <id>` 的 AppleScript（cwd/id 通过 argv 传入，用 quoted form 防注入）。
pub fn resume_script(kind: TermKind) -> &'static str {
    match kind {
        TermKind::ITerm2 => {
            r#"on run argv
  set targetDir to item 1 of argv
  set sessionId to item 2 of argv
  set theCmd to "cd " & quoted form of targetDir & " && claude --resume " & quoted form of sessionId
  tell application "iTerm2"
    activate
    set newWindow to (create window with default profile)
    tell current session of newWindow to write text theCmd
  end tell
end run"#
        }
        // Terminal 与 Other(回退到 Terminal) 共用
        _ => {
            r#"on run argv
  set targetDir to item 1 of argv
  set sessionId to item 2 of argv
  set theCmd to "cd " & quoted form of targetDir & " && claude --resume " & quoted form of sessionId
  tell application "Terminal"
    activate
    do script theCmd
  end tell
end run"#
        }
    }
}

/// 无 cwd 时的恢复脚本（仅 `claude --resume <id>`，不 cd），镜像 Windows 在 cwd 缺失时不带 -d 的行为。
pub fn resume_script_cwdless(kind: TermKind) -> &'static str {
    match kind {
        TermKind::ITerm2 => {
            r#"on run argv
  set sessionId to item 1 of argv
  set theCmd to "claude --resume " & quoted form of sessionId
  tell application "iTerm2"
    activate
    set newWindow to (create window with default profile)
    tell current session of newWindow to write text theCmd
  end tell
end run"#
        }
        _ => {
            r#"on run argv
  set sessionId to item 1 of argv
  set theCmd to "claude --resume " & quoted form of sessionId
  tell application "Terminal"
    activate
    do script theCmd
  end tell
end run"#
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tty_variants() {
        assert_eq!(normalize_tty("ttys003"), Some("/dev/ttys003".into()));
        assert_eq!(normalize_tty("s003"), Some("/dev/ttys003".into()));
        assert_eq!(normalize_tty("/dev/ttys012"), Some("/dev/ttys012".into()));
        assert_eq!(normalize_tty("  ttys004  "), Some("/dev/ttys004".into()));
        assert_eq!(normalize_tty("??"), None);
        assert_eq!(normalize_tty(""), None);
    }

    #[test]
    fn detect_term_kind_picks_nearest_known_host() {
        let names = vec![
            "claude".to_string(),
            "zsh".to_string(),
            "login".to_string(),
            "iTerm2".to_string(),
            "launchd".to_string(),
        ];
        assert_eq!(detect_term_kind(&names), TermKind::ITerm2);

        let names2 = vec!["claude".into(), "zsh".into(), "Terminal".into()];
        assert_eq!(detect_term_kind(&names2), TermKind::Terminal);

        let names3 = vec!["claude".into(), "zsh".into(), "WezTerm".into()];
        assert_eq!(detect_term_kind(&names3), TermKind::Other);
    }

    #[test]
    fn resume_kind_from_setting_maps() {
        assert_eq!(resume_kind_from_setting("iterm"), TermKind::ITerm2);
        assert_eq!(resume_kind_from_setting("iTerm2"), TermKind::ITerm2);
        assert_eq!(resume_kind_from_setting("terminal"), TermKind::Terminal);
        assert_eq!(resume_kind_from_setting(""), TermKind::Terminal); // 缺省/未知 → Terminal
        assert_eq!(resume_kind_from_setting("wezterm"), TermKind::Terminal);
    }

    #[test]
    fn focus_script_present_for_known_hosts_only() {
        assert!(focus_script(TermKind::Terminal).unwrap().contains("tty of t"));
        assert!(focus_script(TermKind::ITerm2).unwrap().contains("tty of s"));
        assert!(focus_script(TermKind::Other).is_none());
    }

    #[test]
    fn resume_script_uses_argv_and_quoted_form() {
        let s = resume_script(TermKind::Terminal);
        assert!(s.contains("on run argv"));
        assert!(s.contains("quoted form of"));
        assert!(s.contains("claude --resume"));
        let c = resume_script_cwdless(TermKind::Terminal);
        assert!(c.contains("on run argv"));
        assert!(c.contains("claude --resume"));
        assert!(!c.contains("cd "));
    }
}
