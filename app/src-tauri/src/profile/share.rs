//! 让各账号（profile）用上用户在默认目录里装的 skill。
//!
//! 起因（用户报障）：profile 靠 `CLAUDE_CONFIG_DIR` 之类的变量把 agent 的整个数据目录搬走，
//! agent 从此只读 profile 目录下的 `skills/`——用户在 `~/.claude/skills` 里装的 skill
//! （lead、lark-*…）在新账号下全部消失，而且零报错。
//!
//! 做法：给默认目录 `skills/` 下的每个 skill 在 profile 的 `skills/` 里建一个**目录链接**
//! （Windows 用 junction，免管理员权限；其余平台用符号链接）。改一处，所有账号都生效。
//!
//! 为什么逐个链接，而不是把整个 `skills/` 链过去：`skills/synced/` 是 claude.ai 按**账号**
//! 同步下来的 skill，整目录共享会让两个账号的同步内容混在一起。同理跳过：
//! - 以 `.` 开头的条目（agent 自己的系统目录，如 codex 的 `.system`）；
//! - Meowo 托管的 skill（目前只有 Lead）：它由 [`crate::lead`] 给每个账号单独装真实文件，
//!   链过去的话，在 profile 里「卸载」会顺着链接把默认目录里的那份也删掉。
//!
//! 只动自己建的链接：profile 里已有的同名真实目录是用户给这个账号单独装的，不碰；默认目录里
//! 删掉的 skill，对应的链接只拆链接本身，绝不进入目标删除。

use std::path::{Path, PathBuf};

/// claude.ai 按账号同步的 skill 所在目录，各账号各一份，不共享。
const PER_ACCOUNT_DIR: &str = "synced";

/// 把 `home_data/skills` 下的 skill 链进 `profile_data/skills`，并拆掉指向已删 skill 的旧链接。
///
/// 幂等：已就位的链接不重建。默认目录没有 `skills/` 时只做清理。best-effort 由调用方决定，
/// 这里把第一个错误返回，但不会因为某一条失败而放弃其余条目。
pub(crate) fn share_skills(home_data: &Path, profile_data: &Path) -> Result<(), String> {
    let src = home_data.join("skills");
    let dst = profile_data.join("skills");
    let mut first_error: Option<String> = None;

    let wanted = shareable_skills(&src);
    if !wanted.is_empty() {
        std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
    }
    for name in &wanted {
        let link = dst.join(name);
        // symlink_metadata 不跟随链接：死链接也算「已存在」，交给下面的清理判断。
        if std::fs::symlink_metadata(&link).is_ok() {
            continue;
        }
        let target = src.join(name);
        if !safe_for_shell(&link) || !safe_for_shell(&target) {
            // Windows 上建链接要经过 cmd，路径里的 & | ^ % 之类会被 cmd 解释：轻则建错链接，
            // 重则执行 & 后面的命令（做空审查问题 3）。这种 skill 宁可不共享。
            first_error.get_or_insert(format!("{}: 名字含命令行特殊字符，未共享", target.display()));
            continue;
        }
        if let Err(e) = make_dir_link(&link, &target) {
            first_error.get_or_insert(format!("{}: {e}", link.display()));
        }
    }

    // 清理：只拆「指向默认目录 skills/ 下、但已不在共享名单里」的链接。
    if let Ok(entries) = std::fs::read_dir(&dst) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(target) = link_target(&path) else { continue };
            if !path_starts_with(&target, &src) {
                continue; // 用户自己建的、指向别处的链接
            }
            let name = entry.file_name();
            if wanted.iter().any(|w| w.as_os_str() == name) && target.is_dir() {
                continue;
            }
            if let Err(e) = remove_dir_link(&path) {
                first_error.get_or_insert(format!("{}: {e}", path.display()));
            }
        }
    }

    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 默认目录里该共享的 skill 名（目录，跟随链接判断——用户的 skill 本身常是指向别处的 junction）。
fn shareable_skills(src: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(src) else {
        return Vec::new();
    };
    let mut names: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| PathBuf::from(e.file_name()))
        .filter(|name| {
            let s = name.to_string_lossy();
            !s.starts_with('.') && s != PER_ACCOUNT_DIR
        })
        .filter(|name| !crate::lead::is_managed_skill_dir(&src.join(name)))
        .collect();
    names.sort();
    names
}

/// 路径前缀判断。Windows 文件系统不分大小写，junction 里记下的路径大小写可能和
/// 现算的默认目录不同，按字面比较会让这条链接永远认不出是自己建的、永远清不掉。
fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    #[cfg(windows)]
    {
        let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
        lower(path).starts_with(lower(prefix))
    }
    #[cfg(not(windows))]
    {
        path.starts_with(prefix)
    }
}

/// 路径能否原样交给 cmd（Windows 建 junction 用）。unix 上直接调系统接口，不经 shell。
fn safe_for_shell(path: &Path) -> bool {
    if cfg!(not(windows)) {
        return true;
    }
    !path
        .to_string_lossy()
        .chars()
        .any(|c| matches!(c, '&' | '|' | '<' | '>' | '^' | '%' | '!' | '"' | '(' | ')'))
}

/// `path` 是链接（junction / 符号链接）时返回它的目标，去掉 Windows 的 `\\?\` 前缀以便比较。
fn link_target(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let target = std::fs::read_link(path).ok()?;
    let s = target.to_string_lossy();
    Some(match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => target,
    })
}

#[cfg(windows)]
fn make_dir_link(link: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // 标准库没有建 junction 的 API；mklink /J 不需要管理员权限或开发者模式（目录符号链接要）。
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(unix)]
fn make_dir_link(link: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
}

/// 只删链接本身。Windows 上目录型链接要用 RemoveDirectory（`remove_dir`），它不会进入目标；
/// unix 上符号链接是文件，`remove_file` 同样只删链接。
fn remove_dir_link(link: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        std::fs::remove_dir(link)
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "meowo-share-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            // 先拆链接再删，免得跟进链接删到别的临时目录（同 worktree 删除的教训）。
            for sub in ["profile/skills"] {
                if let Ok(es) = std::fs::read_dir(self.0.join(sub)) {
                    for e in es.flatten() {
                        if link_target(&e.path()).is_some() {
                            let _ = remove_dir_link(&e.path());
                        }
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn skill(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir.join(name)).unwrap();
        std::fs::write(dir.join(name).join("SKILL.md"), body).unwrap();
    }

    fn is_link(p: &Path) -> bool {
        link_target(p).is_some()
    }

    #[test]
    fn links_user_skills_and_skips_per_account_and_managed() {
        let t = Tmp::new("link");
        let home = t.0.join("home");
        let prof = t.0.join("profile");
        let hs = home.join("skills");
        skill(&hs, "mine", "用户的 skill");
        skill(&hs, "synced", "claude.ai 同步的，按账号");
        skill(&hs, ".system", "agent 自己的");
        skill(&hs, "lead", &format!("托管\n{}", crate::lead::MANAGED_MARK));
        skill(&hs, "both", "默认目录里的版本");
        // profile 自己单独装的同名 skill：不能被换成链接。
        skill(&prof.join("skills"), "both", "这个账号自己的版本");

        share_skills(&home, &prof).unwrap();

        let ps = prof.join("skills");
        assert!(is_link(&ps.join("mine")));
        assert_eq!(
            std::fs::read_to_string(ps.join("mine/SKILL.md")).unwrap(),
            "用户的 skill"
        );
        assert!(!ps.join("synced").exists(), "按账号同步的 skill 不共享");
        assert!(!ps.join(".system").exists());
        assert!(!ps.join("lead").exists(), "托管的 Lead 由 lead 模块单独装");
        assert!(!is_link(&ps.join("both")));
        assert_eq!(
            std::fs::read_to_string(ps.join("both/SKILL.md")).unwrap(),
            "这个账号自己的版本"
        );
    }

    #[test]
    fn prunes_links_to_removed_skills_without_touching_targets() {
        let t = Tmp::new("prune");
        let home = t.0.join("home");
        let prof = t.0.join("profile");
        let hs = home.join("skills");
        skill(&hs, "keep", "k");
        skill(&hs, "gone", "g");
        share_skills(&home, &prof).unwrap();
        // 幂等：再跑一次不报错、不重建。
        share_skills(&home, &prof).unwrap();

        // 用户把 gone 从默认目录挪走（不是删掉——挪到别处，好确认链接清理不会跟进去删）。
        let moved = t.0.join("moved-gone");
        std::fs::rename(hs.join("gone"), &moved).unwrap();
        share_skills(&home, &prof).unwrap();

        let ps = prof.join("skills");
        assert!(std::fs::symlink_metadata(ps.join("gone")).is_err(), "死链接应被拆掉");
        assert!(is_link(&ps.join("keep")));
        assert!(moved.join("SKILL.md").is_file());
    }

    /// 名字里有 cmd 特殊字符的 skill 不共享，也不能让 & 后面的东西被当成命令执行；
    /// 其余 skill 照常共享。
    #[cfg(windows)]
    #[test]
    fn skips_names_cmd_would_interpret() {
        let t = Tmp::new("shell");
        let home = t.0.join("home");
        let prof = t.0.join("profile");
        let hs = home.join("skills");
        // cmd 的工作目录是测试进程的当前目录：命令被执行的话，这里会多出一个目录。
        let marker = std::env::current_dir().unwrap().join(format!("meowo-pwned-{}", std::process::id()));
        let evil = format!("x&mkdir {}", marker.file_name().unwrap().to_string_lossy());
        skill(&hs, &evil, "e");
        skill(&hs, "caret^x", "c");
        skill(&hs, "ok", "o");

        assert!(share_skills(&home, &prof).is_err(), "跳过的条目要报出来");
        let ps = prof.join("skills");
        let executed = marker.exists();
        let _ = std::fs::remove_dir(&marker);
        assert!(!executed, "& 后面的命令不能被执行");
        assert!(std::fs::symlink_metadata(ps.join(&evil)).is_err());
        assert!(std::fs::symlink_metadata(ps.join("caretx")).is_err());
        assert!(is_link(&ps.join("ok")));
    }

    #[test]
    fn missing_home_skills_is_not_an_error() {
        let t = Tmp::new("empty");
        share_skills(&t.0.join("home"), &t.0.join("profile")).unwrap();
        assert!(!t.0.join("profile/skills").exists(), "没东西可共享时不凭空建目录");
    }
}
