use cc_store::{PendingReview, SessionStatus, Store, StoreError};
use crate::hook::HookEvent;

use std::path::Path;

/// 把一个 hook 事件落到库。未知/缺字段一律降级为「无操作」，绝不报错冒泡。
/// `provider` 为 agent 提供方（claude/kimi…）：仅在 SessionStart 标记到会话上，并决定标题解析路径。
pub fn dispatch(store: &Store, ev: &HookEvent, now_ms: i64, provider: &str) -> Result<(), StoreError> {
    match ev.hook_event_name.as_str() {
        "SessionStart" => {
            let Some(cwd) = ev.cwd.as_deref() else { return Ok(()) };
            if ev.session_id.is_empty() { return Ok(()); }
            let sid = create_session(store, ev, cwd, provider, now_ms)?;
            apply_title(store, ev, sid, now_ms, provider)?;
        }
        "UserPromptSubmit" => {
            if let Some(sid) = lookup_or_create(store, ev, provider, now_ms)? {
                store.clear_pending_review(sid)?;
                if let Some(prompt) = ev.prompt_text() {
                    store.on_user_prompt(sid, &prompt, now_ms)?;
                    store.set_last_user_text(sid, &prompt)?;
                }
                // 给已注册（含压缩漏掉 SessionStart）的会话补抓 PID；每用户回合一次，开销可忽略。
                if let Some(p) = crate::proc::owner_pid() {
                    store.set_session_pid(sid, p as i64, now_ms)?;
                }
                apply_title(store, ev, sid, now_ms, provider)?;
                write_tab_token(ev, provider);
            }
        }
        "PostToolUse" => {
            if let Some(sid) = lookup_or_create(store, ev, provider, now_ms)? {
                store.clear_pending_review(sid)?;
                match ev.tool_name.as_deref() {
                    Some("TodoWrite") => {
                        store.sync_todos(sid, &ev.todo_items(), now_ms)?;
                    }
                    Some("Bash") => {
                        if let Some(cmd) = ev.bash_command() {
                            store.set_current_activity(sid, &format!("› {cmd}"), now_ms)?;
                        }
                    }
                    _ => { store.touch_session(sid, now_ms)?; }
                }
            }
        }
        "Stop" => {
            if let Some(sid) = lookup_or_create(store, ev, provider, now_ms)? {
                store.clear_pending_review(sid)?;
                store.set_session_status(sid, SessionStatus::Waiting, now_ms)?;
                // 最近 AI 正文 + 模型由 agent 决定来源：claude 用 Stop hook 携带的正文（模型走 statusline）；
                // kimi 的 Stop hook 不带，读会话 wire.jsonl 一次出正文 + 模型。
                let out = crate::agent::for_provider(provider).stop_outputs(ev);
                if let Some(msg) = out.last_ai {
                    store.set_last_ai_text(sid, &msg)?;
                }
                if let Some(model) = out.model {
                    store.set_session_context(&ev.session_id, None, None, Some(&model), now_ms)?;
                }
                apply_title(store, ev, sid, now_ms, provider)?;
                write_tab_token(ev, provider);
            }
        }
        "SessionEnd" => {
            if let Some(sid) = lookup_session(store, ev)? {
                store.clear_pending_review(sid)?;
                store.end_session(sid, now_ms)?;
            }
        }
        "PermissionRequest" => {
            if let Some(sid) = lookup_session(store, ev)? {
                let kind = match ev.tool_name.as_deref() {
                    Some("ExitPlanMode") => PendingReview::Plan,
                    Some("AskUserQuestion") => PendingReview::Question,
                    _ => PendingReview::Approval,
                };
                store.set_pending_review(sid, kind, now_ms)?;
            }
        }
        "PreToolUse" => {
            if let Some(sid) = lookup_session(store, ev)? {
                let kind = match ev.tool_name.as_deref() {
                    Some("AskUserQuestion") => Some(PendingReview::Question),
                    Some("ExitPlanMode") => Some(PendingReview::Plan),
                    _ => None, // 安装侧已用 matcher 限定;这里再兜一层防御
                };
                if let Some(kind) = kind {
                    store.set_pending_review(sid, kind, now_ms)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn apply_title(store: &Store, ev: &HookEvent, sid: i64, now_ms: i64, provider: &str) -> Result<(), StoreError> {
    // 是否由 transcript 解析标题由 agent 决定：claude 是；kimi 否（不给 transcript_path 且 JSONL 格式不同，
    // 标题靠 UserPromptSubmit 的首条 prompt 命名，与 Claude 的占位回退同款）。
    if !crate::agent::for_provider(provider).resolves_transcript_title() {
        return Ok(());
    }
    // cwd 优先用事件携带的，否则回退到 SessionStart 时存进库的 cwd。
    let cwd_owned: Option<String> = match ev.cwd.clone() {
        Some(c) => Some(c),
        None => store.session_cwd(sid).ok().flatten(),
    };
    if let Some(title) = crate::transcript::resolve_title(
        ev.transcript_path.as_deref(),
        cwd_owned.as_deref(),
        &ev.session_id,
    ) {
        store.set_session_title(sid, &title, now_ms)?;
    }
    Ok(())
}

/// 仅当该 provider 需由 cc-reporter 补 token 时(kimi)，把 `<cwd 末段目录名> ·<sid8>` 写进本标签的 WT
/// 标题——sid8=session_id 末 8 位、全局唯一，cc-app 据此精确切到该标签（解决同窗口同目录两会话标签
/// 同名分不清）。cc-reporter 是 hook 子进程、继承本会话的 ConPTY，写 CONOUT$ 只影响自己这个标签。
/// 非 Windows / 非 WT(CONOUT$ 打不开) 静默 no-op。
fn write_tab_token(ev: &HookEvent, provider: &str) {
    if !crate::agent::for_provider(provider).writes_tab_token() {
        return;
    }
    let sid8 = crate::tabtitle::short_sid(&ev.session_id);
    if sid8.is_empty() {
        return;
    }
    let base = ev
        .cwd
        .as_deref()
        .map(|c| c.trim_end_matches(['/', '\\']))
        .and_then(|c| Path::new(c).file_name().and_then(|s| s.to_str()))
        .unwrap_or("session");
    crate::tabtitle::set_tab_title(&format!("{base} ·{sid8}"));
}

/// 建会话（项目 upsert + 会话 + provider + cwd + 抓 PID），返回 sid。SessionStart 与懒创建共用。
fn create_session(store: &Store, ev: &HookEvent, cwd: &str, provider: &str, now_ms: i64) -> Result<i64, StoreError> {
    let (root, name) = project_root_and_name(cwd);
    let pid = store.upsert_project_by_root(&root, &name, now_ms)?;
    let (sid, _) = store.start_session(pid, &ev.session_id, now_ms)?;
    if provider != "claude" {
        store.set_session_provider(sid, provider)?;
    }
    store.set_session_cwd(sid, cwd, now_ms)?;
    if let Some(p) = crate::proc::owner_pid() {
        store.set_session_pid(sid, p as i64, now_ms)?;
    }
    Ok(sid)
}

fn lookup_session(store: &Store, ev: &HookEvent) -> Result<Option<i64>, StoreError> {
    if ev.session_id.is_empty() {
        return Ok(None);
    }
    store.find_session_id_pub(&ev.session_id)
}

/// 查会话；查不到且事件带 cwd 时就地懒创建——让「hooks 中途装上 / SessionStart 漏掉（压缩等）」
/// 的会话在下一条带 cwd 的活动事件（UserPromptSubmit/PostToolUse/Stop）上也能补建上板，不必重开。
fn lookup_or_create(store: &Store, ev: &HookEvent, provider: &str, now_ms: i64) -> Result<Option<i64>, StoreError> {
    if ev.session_id.is_empty() {
        return Ok(None);
    }
    if let Some(sid) = store.find_session_id_pub(&ev.session_id)? {
        // 会话曾被误清成 ended（如 kimi 的 pid 一度不被 app 认作存活而被 reap），但仍有活动事件到来
        // → 统一自愈复活（清 ended_at、置 running），不再只在 UserPromptSubmit 一条路径上修。
        store.revive_if_ended(sid, now_ms)?;
        return Ok(Some(sid));
    }
    match ev.cwd.as_deref() {
        Some(cwd) => Ok(Some(create_session(store, ev, cwd, provider, now_ms)?)),
        None => Ok(None),
    }
}

/// cwd 的 git 根（向上找 .git）作为项目 root；无 git 则用 cwd 本身。
/// name 优先取 git remote origin 的 owner/repo，否则用末段目录名。
pub(crate) fn project_root_and_name(cwd: &str) -> (String, String) {
    let root = find_git_root(cwd).unwrap_or_else(|| cwd.to_string());
    let name = owner_repo_from_git(&root).unwrap_or_else(|| {
        Path::new(&root)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&root)
            .to_string()
    });
    (root, name)
}

fn find_git_root(start: &str) -> Option<String> {
    let mut dir = Path::new(start);
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        dir = dir.parent()?;
    }
}

/// 从 URL 提取 owner/repo：去掉结尾 .git，把 ':' 当 '/'，按 '/' 切分去空，取最后两段。
/// 支持 https://github.com/o/r.git 与 git@github.com:o/r.git。
fn owner_repo_from_url(url: &str) -> Option<String> {
    let normalized = url.trim().trim_end_matches(".git").replace(':', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        let n = parts.len();
        Some(format!("{}/{}", parts[n - 2], parts[n - 1]))
    } else {
        None
    }
}

/// 读 <root>/.git/config，取 [remote "origin"] 的 url，解析 owner/repo。
fn owner_repo_from_git(root: &str) -> Option<String> {
    let cfg = Path::new(root).join(".git").join("config");
    let content = std::fs::read_to_string(cfg).ok()?;
    let mut in_origin = false;
    for line in content.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_origin = l == r#"[remote "origin"]"#;
            continue;
        }
        if in_origin {
            if let Some(rest) = l.strip_prefix("url") {
                if let Some(eq) = rest.find('=') {
                    let url = rest[eq + 1..].trim();
                    return owner_repo_from_url(url);
                }
            }
        }
    }
    None
}

/// 供测试调用的公开包装。
#[doc(hidden)]
pub fn owner_repo_from_url_pub(url: &str) -> Option<String> {
    owner_repo_from_url(url)
}
