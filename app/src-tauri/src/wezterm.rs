//! WezTerm 终端集成(仅 Windows):探测、gui socket 发现、cli list/spawn/activate 封装。
//!
//! 关键约束(均已实测,勿"简化"):
//! - 一切 `wezterm cli` 必须带 --no-auto-start,否则 GUI 未运行时会拉起 mux server 并阻塞;
//! - 必须显式设 WEZTERM_UNIX_SOCKET 指向 gui-sock-<pid>,CLI 自动发现不可靠;
//! - cli list 无 pid 字段,pane 匹配只能靠 title(token/任务标题)与 cwd(file:/// URL)。

/// wezterm.exe 是否在 PATH(官方安装器/winget/scoop 均会加)。进程内缓存,同 wt_available。
pub(crate) fn available() -> bool {
    use std::sync::OnceLock;
    static ON_PATH: OnceLock<bool> = OnceLock::new();
    *ON_PATH.get_or_init(|| {
        std::env::var_os("PATH").is_some_and(|p| crate::path_has_exe(&p, "wezterm.exe"))
    })
}

/// `wezterm cli list --format json` 里本模块关心的字段。cwd 保持 file:/// URL 原样,
/// 比较时经 file_url_to_path 归一化。
#[derive(Debug, PartialEq, serde::Deserialize)]
pub(crate) struct PaneInfo {
    pub pane_id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cwd: String,
}

/// 解析 cli list 输出。解析失败返回空(调用方退化为窗口级聚焦)。
fn parse_panes(json: &str) -> Vec<PaneInfo> {
    serde_json::from_str(json).unwrap_or_default()
}

/// percent 解码(%20 → 空格,UTF-8 字节层解码兼容中文路径);非法序列原样保留。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(h) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|hx| u8::from_str_radix(hx, 16).ok())
            {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// file:///C:/Users/x/ → C:\Users\x。非 file URL / 空路径返回 None。
fn file_url_to_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let decoded = percent_decode(rest.trim_start_matches('/'));
    let path = decoded.replace('/', "\\");
    let path = path.trim_end_matches('\\');
    (!path.is_empty()).then(|| path.to_string())
}

/// pane 的 cwd(file URL)与会话 cwd(Windows 路径)是否同一目录:统一反斜杠、去尾斜杠、
/// ASCII 大小写不敏感(NTFS 语义,非 ASCII 按原样比较,够用)。
fn cwd_matches(pane_cwd_url: &str, session_cwd: &str) -> bool {
    let Some(p) = file_url_to_path(pane_cwd_url) else { return false };
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
    norm(&p) == norm(session_cwd)
}

/// 从 panes 里选出唯一最佳匹配的 pane_id。打分:token 命中 title=4 >
/// cwd+title 双命中=3 > cwd 命中=2 > title 命中=1。最高分不唯一或全 0 → None(不猜,
/// 调用方退窗口级,语义对齐 WT 的「同窗口多同名标签不猜」)。
fn match_pane(
    panes: &[PaneInfo],
    want_title: &str,
    token: Option<&str>,
    cwd: Option<&str>,
) -> Option<u64> {
    let score = |p: &PaneInfo| -> u8 {
        if token.is_some_and(|t| !t.is_empty() && p.title.contains(t)) {
            return 4;
        }
        let cwd_hit = cwd.is_some_and(|c| cwd_matches(&p.cwd, c));
        let title_hit = crate::tab_match_score(&p.title, want_title) > 0;
        match (cwd_hit, title_hit) {
            (true, true) => 3,
            (true, false) => 2,
            (false, true) => 1,
            (false, false) => 0,
        }
    };
    let scored: Vec<(u8, u64)> = panes.iter().map(|p| (score(p), p.pane_id)).collect();
    let max = scored.iter().map(|(s, _)| *s).max().filter(|&m| m > 0)?;
    let mut top = scored.iter().filter(|(s, _)| *s == max).map(|(_, id)| *id);
    match (top.next(), top.next()) {
        (Some(one), None) => Some(one),
        _ => None, // 并列:不猜,退窗口级
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 实测样本的字段子集(多余字段应被 serde 忽略)
    const LIST_JSON: &str = r#"[
      {"window_id":0,"tab_id":0,"pane_id":0,"workspace":"default",
       "title":"cc-TOKEN-abc12345","cwd":"file:///C:/Users/larry/","is_active":true},
      {"window_id":0,"tab_id":1,"pane_id":1,
       "title":"cmd.exe","cwd":"file:///C:/Users/larry/Desktop/workspace/"}
    ]"#;

    #[test]
    fn parse_panes_extracts_fields_and_ignores_unknown() {
        let panes = parse_panes(LIST_JSON);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pane_id, 0);
        assert_eq!(panes[0].title, "cc-TOKEN-abc12345");
        assert_eq!(panes[1].cwd, "file:///C:/Users/larry/Desktop/workspace/");
    }

    #[test]
    fn parse_panes_bad_json_gives_empty() {
        assert!(parse_panes("not json").is_empty());
        assert!(parse_panes("{}").is_empty());
    }

    #[test]
    fn percent_decode_handles_space_utf8_and_invalid() {
        assert_eq!(percent_decode("a%20b"), "a b");
        // 中文「工」= E5 B7 A5(UTF-8 percent 编码逐字节)
        assert_eq!(percent_decode("%E5%B7%A5"), "工");
        assert_eq!(percent_decode("50%zz"), "50%zz"); // 非法序列原样保留
        assert_eq!(percent_decode("tail%2"), "tail%2"); // 结尾截断原样保留
    }

    #[test]
    fn file_url_to_path_converts_and_rejects() {
        assert_eq!(
            file_url_to_path("file:///C:/Users/larry/Desktop/workspace/").as_deref(),
            Some(r"C:\Users\larry\Desktop\workspace")
        );
        assert_eq!(
            file_url_to_path("file:///C:/a%20b/").as_deref(),
            Some(r"C:\a b")
        );
        assert_eq!(file_url_to_path("https://example.com/x"), None);
        assert_eq!(file_url_to_path("file:///"), None);
    }

    #[test]
    fn cwd_matches_normalizes_slash_trailing_case() {
        let url = "file:///C:/Users/larry/Desktop/workspace/";
        assert!(cwd_matches(url, r"C:\Users\larry\Desktop\workspace"));
        assert!(cwd_matches(url, r"c:\users\larry\desktop\workspace\"));
        assert!(!cwd_matches(url, r"C:\Users\larry"));
        assert!(!cwd_matches("not-a-url", r"C:\Users\larry"));
    }

    fn pane(id: u64, title: &str, cwd: &str) -> PaneInfo {
        PaneInfo { pane_id: id, title: title.into(), cwd: cwd.into() }
    }

    #[test]
    fn match_pane_token_beats_everything() {
        let panes = vec![
            pane(0, "✳ 修复登录", "file:///C:/proj/"),
            pane(1, "kimi abc12345", "file:///C:/other/"),
        ];
        // pane 0 同时命中 title+cwd(3 分),但 token 命中 pane 1(4 分)胜出
        assert_eq!(
            match_pane(&panes, "修复登录", Some("abc12345"), Some(r"C:\proj")),
            Some(1)
        );
    }

    #[test]
    fn match_pane_cwd_unique_hit() {
        let panes = vec![
            pane(0, "pwsh.exe", "file:///C:/proj-a/"),
            pane(1, "pwsh.exe", "file:///C:/proj-b/"),
        ];
        assert_eq!(match_pane(&panes, "", None, Some(r"C:\proj-b")), Some(1));
    }

    #[test]
    fn match_pane_ambiguous_returns_none() {
        // 同目录两个 pane、无 token、标题相同 → 并列,不猜
        let panes = vec![
            pane(0, "pwsh.exe", "file:///C:/proj/"),
            pane(1, "pwsh.exe", "file:///C:/proj/"),
        ];
        assert_eq!(match_pane(&panes, "", None, Some(r"C:\proj")), None);
    }

    #[test]
    fn match_pane_title_disambiguates_same_cwd() {
        // 同目录,但任务标题只命中其一(cwd+title=3 分 vs cwd=2 分)
        let panes = vec![
            pane(0, "✳ 修复登录", "file:///C:/proj/"),
            pane(1, "pwsh.exe", "file:///C:/proj/"),
        ];
        assert_eq!(match_pane(&panes, "修复登录", None, Some(r"C:\proj")), Some(0));
    }

    #[test]
    fn match_pane_no_signal_returns_none() {
        let panes = vec![pane(0, "pwsh.exe", "file:///C:/x/")];
        assert_eq!(match_pane(&panes, "", None, None), None);
        assert_eq!(match_pane(&panes, "不存在的标题", None, Some(r"C:\y")), None);
    }
}
