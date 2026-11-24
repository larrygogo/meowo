//! Lead 任务目录的位置：`<数据根>/projects/<项目键>/.lead/`。
//!
//! 用户 2026-11-24 要求任务文件不放项目里、放进 Meowo 数据目录（任务 20261124-lead-store-in-meowo）。
//! 实查时 4 个仓库的 `.lead` 没被忽略、出现在 git 状态里，ledger-app 已把 29 个任务文件提交进仓库，
//! worktree 里建的任务会随 worktree 一起被删。
//!
//! 保留名为 `.lead` 的父目录：认领、redteam 写入拦截、Codex apply_patch、opencode 插件都按
//! 「父目录叫 `.lead`」识别任务文件，换位置不换判据。reporter（hook 进程）、宿主、各家启动参数
//! 都从这里取路径，三方算出同一个目录。

use std::path::{Path, PathBuf};

/// git 根目录；不在 git 里就是 cwd 本身。绝不越过项目边界向上找（初版踩过：主目录下的任务
/// 注入到了所有非 git 目录）。
pub fn project_root(cwd: &Path) -> PathBuf {
    let start = cwd
        .canonicalize()
        .map(strip_verbatim)
        .unwrap_or_else(|_| cwd.to_path_buf());
    let mut d = Some(start.as_path());
    while let Some(dir) = d {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        d = dir.parent();
    }
    start
}

/// worktree 对应的主检出：`<wt>/.git` 是个文件，内容 `gitdir: <主检出>/.git/worktrees/<名>`，那个目录里的
/// `commondir` 指回 `<主检出>/.git`。不是 worktree、或任何一步读不到，返回 None。
pub fn main_checkout(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if !dot_git.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = text.trim().strip_prefix("gitdir:")?.trim().to_owned();
    let gitdir = root.join(gitdir); // 相对路径相对 worktree 根，绝对路径 join 后不变
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = gitdir.join(common.trim()).canonicalize().map(strip_verbatim).ok()?;
    let main = common.parent()?.to_path_buf();
    (main != root).then_some(main)
}

/// 项目的身份：worktree 归到主检出，任务记录才不会随 worktree 删除而丢。
pub fn project_identity(cwd: &Path) -> PathBuf {
    let root = project_root(cwd);
    main_checkout(&root).unwrap_or(root)
}

/// 项目键：沿用 Claude Code `~/.claude/projects` 的编码（非 ASCII 字母数字一律换成 `-`），用户在
/// 数据目录里一眼认得出是哪个项目；再接 8 位哈希，因为这种编码会把「AI账号管理」和「AI其它管理」
/// 编成同一个名字。Windows 路径不分大小写，哈希前统一小写。
pub fn project_key(identity: &Path) -> String {
    let text = identity.to_string_lossy().replace('\\', "/");
    let readable: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let canonical = if cfg!(windows) { text.to_lowercase() } else { text };
    format!("{readable}-{:08x}", fnv1a(canonical.as_bytes()))
}

/// FNV-1a 32 位：目录名要跨版本稳定，标准库的哈希器不保证这一点。
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// 数据根下放各项目 Lead 目录的地方。
pub fn projects_root() -> Option<PathBuf> {
    let root = crate::meowo_data_root()?.join("projects");
    // MEOWO_DB 用 `/` 写时，拼出来的路径 `/` 与 `\` 混用（实测 `…/data\projects\…`）。这个路径要交给 agent
    // 原样使用、也写进放错位置的提示，Windows 上统一成 `\`。
    if cfg!(windows) {
        return Some(PathBuf::from(root.to_string_lossy().replace('/', "\\")));
    }
    Some(root)
}

/// 该工作目录所属项目的 Lead 目录（不保证已存在）。解析不到 home 时为 None。
pub fn lead_home(cwd: &Path) -> Option<PathBuf> {
    lead_home_in(&projects_root()?, cwd)
}

/// 同 [`lead_home`]，数据根由调用方给（测试与宿主用，避免改进程环境变量）。
pub fn lead_home_in(projects: &Path, cwd: &Path) -> Option<PathBuf> {
    let identity = project_identity(cwd);
    Some(projects.join(project_key(&identity)).join(".lead"))
}

/// 建好项目的 Lead 目录，并在项目目录里写 `project.json` 记下原始路径（用户与诊断反查用）。
pub fn ensure_lead_home(cwd: &Path) -> Option<PathBuf> {
    ensure_lead_home_in(&projects_root()?, cwd)
}

pub fn ensure_lead_home_in(projects: &Path, cwd: &Path) -> Option<PathBuf> {
    let home = lead_home_in(projects, cwd)?;
    std::fs::create_dir_all(&home).ok()?;
    let marker = home.parent()?.join("project.json");
    if !marker.exists() {
        let identity = project_identity(cwd);
        let body = serde_json::json!({ "root": identity.to_string_lossy() });
        let _ = std::fs::write(&marker, body.to_string());
    }
    Some(home)
}

/// 怎么让 CLI 能写本项目的 Lead 目录（它在工作目录之外）。不放行时 Claude 每写一次任务文件弹一次审批，
/// Codex 的 workspace-write 沙箱直接拦（hook 进程继承沙箱，连 `.owner` 都写不进），Gemini 的文件工具拒绝
/// 工作区外的路径（调研 explore-1，任务 20261124-lead-store-in-meowo）。只放行这一个目录，不放行整个数据根。
#[derive(Debug, Clone, PartialEq)]
pub enum LeadDirGrant {
    /// 追加到启动命令的参数。
    Args(Vec<String>),
    /// 深合并进内联配置环境变量（opencode 的 `OPENCODE_CONFIG_CONTENT`）的 JSON 片段。
    InlineConfig(serde_json::Value),
}

/// Windows 上 canonicalize 会带 `\\?\` 前缀（真机实测登记表里出现过），登记表和界面里都不该出现它。
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-lead-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        strip_verbatim(dir.canonicalize().unwrap())
    }

    #[test]
    fn key_is_readable_and_disambiguates_lossy_names() {
        let a = project_key(Path::new(r"C:\w\AI账号管理"));
        let b = project_key(Path::new(r"C:\w\AI其它管理"));
        assert!(a.starts_with("C--w-AI"), "{a}");
        assert_ne!(a, b);
        assert_eq!(a, project_key(Path::new(r"C:\w\AI账号管理")));
    }

    #[cfg(windows)]
    #[test]
    fn key_ignores_case_and_separators_on_windows() {
        assert_eq!(
            project_key(Path::new(r"C:\Users\Me\proj")).rsplit('-').next(),
            project_key(Path::new("c:/users/me/proj")).rsplit('-').next()
        );
    }

    #[test]
    fn worktree_maps_to_main_checkout() {
        let base = temp("wt");
        let main = base.join("main");
        let wt = base.join("wt");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        std::fs::write(
            wt.join(".git"),
            format!("gitdir: {}", main.join(".git/worktrees/wt").display()),
        )
        .unwrap();
        std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..").unwrap();
        let projects = base.join("data/projects");
        let from_main = lead_home_in(&projects, &main).unwrap();
        assert_eq!(lead_home_in(&projects, &wt.join("sub")).unwrap(), from_main);
        assert!(from_main.starts_with(&projects));
        assert_eq!(from_main.file_name().unwrap(), ".lead");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn non_git_dir_uses_cwd_and_ensure_writes_marker() {
        let base = temp("plain");
        let cwd = base.join("notes");
        std::fs::create_dir_all(&cwd).unwrap();
        let projects = base.join("data/projects");
        let home = ensure_lead_home_in(&projects, &cwd).unwrap();
        assert!(home.is_dir());
        let marker: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(home.parent().unwrap().join("project.json")).unwrap()).unwrap();
        assert_eq!(marker["root"].as_str().map(PathBuf::from), Some(cwd));
        let _ = std::fs::remove_dir_all(base);
    }
}
