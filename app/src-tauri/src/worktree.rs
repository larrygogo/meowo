//! 已合并 worktree 的清理：判断会话所在 worktree 能不能删，以及安全地删掉它。
//!
//! 起因（2026-11-09 用户实拍）：并行开任务攒下 67 个 worktree，PR 合并后没人回头删；手工清理时
//! 踩到 junction 删不掉留空壳、残留预览服务占着目录、子模块提交没推三个坑。规格见用户确认的
//! 「只在条件全满足时可删，不提供强制删除」：判定宁可保守，也绝不误删。
//!
//! 判定条件（全部满足才 eligible）：
//! - 会话目录在一个 linked worktree 里（主检出永远不删）
//! - 没有别的未归档会话也在这个 worktree 里
//! - `gh` 查到当前分支的 PR 是 MERGED——用 PR 状态而不是提交祖先关系，squash 合并后
//!   分支提交不在主干历史里，祖先判断会全部误判成「没合并」（这次清理里有 30+ 个这样的分支）
//! - 子模块干净，且子模块当前提交在其远端存在
//! - 工作区没有未提交改动和未跟踪文件（被忽略的文件不算，比如 node_modules、target）
//! - 本地分支没有远端没有的提交：等于或落后于 PR 的 head 提交，或被某个远端分支包含
//!
//! 删除顺序：再判定一次（状态可能变了）→ 查占用进程（有就停，等用户确认）→ 改名试探
//! （改不动说明还有句柄，停手，一个文件都不动）→ 只拆链接不进入 → `git worktree remove`
//! → 清残留 → prune → 删本地分支 → 确认目录真的没了。远端分支不动。

use meowo_protocol::ipc::{
    CleanableWorktreeDto, WorktreeProcessDto, WorktreeRemovalDto, WorktreeStatusDto,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::State;

// ═══ 判定 ═══

/// `gh pr view` 的结论。gh 没装、没登录、不是 GitHub 仓库都归到 Unavailable：
/// 判断不了有没有合并，就不能删。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrLookup {
    Unavailable,
    NoPr,
    Found {
        number: u32,
        /// `OPEN` / `MERGED` / `CLOSED`
        state: String,
        head_oid: String,
    },
}

/// 子模块状况，按严重程度取最坏的一个。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubmoduleState {
    Clean,
    Dirty,
    /// 子模块检出的提交在它的远端找不到：删了就丢了。
    Unpushed,
}

/// 判定用到的全部事实。从磁盘和 git 收集（[`gather_facts`]），判定本身是纯函数（[`evaluate`]）。
#[derive(Debug, Clone)]
pub(crate) struct Facts {
    pub(crate) linked_worktree: bool,
    pub(crate) branch: Option<String>,
    pub(crate) other_sessions: u32,
    pub(crate) pr: PrLookup,
    pub(crate) submodules: SubmoduleState,
    pub(crate) dirty: u32,
    pub(crate) untracked: u32,
    pub(crate) unpushed: bool,
    pub(crate) locked: bool,
}

/// 不可清理的原因码 + 附带数量。顺序即优先级：先说最根本的那个原因。
pub(crate) fn evaluate(f: &Facts) -> Option<(&'static str, Option<u32>)> {
    if !f.linked_worktree {
        return Some(("not_worktree", None));
    }
    if f.branch.is_none() {
        return Some(("detached", None));
    }
    if f.locked {
        return Some(("locked", None));
    }
    if f.other_sessions > 0 {
        return Some(("shared_session", Some(f.other_sessions)));
    }
    match &f.pr {
        PrLookup::Unavailable => return Some(("gh_unavailable", None)),
        PrLookup::NoPr => return Some(("no_pr", None)),
        PrLookup::Found { state, .. } => match state.as_str() {
            "MERGED" => {}
            "CLOSED" => return Some(("pr_closed", None)),
            _ => return Some(("pr_open", None)),
        },
    }
    match f.submodules {
        SubmoduleState::Unpushed => return Some(("submodule_unpushed", None)),
        SubmoduleState::Dirty => return Some(("submodule_dirty", None)),
        SubmoduleState::Clean => {}
    }
    if f.dirty > 0 {
        return Some(("dirty", Some(f.dirty)));
    }
    if f.untracked > 0 {
        return Some(("untracked", Some(f.untracked)));
    }
    if f.unpushed {
        return Some(("unpushed", None));
    }
    None
}

// ═══ git / gh ═══

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    // Windows：GUI 进程 spawn 控制台子进程会闪现 conhost 黑窗，压掉（同 git.rs）。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = command("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 真实的 `gh pr view`。不走 git.rs 的 60s 缓存：删除前要的是此刻的状态。
pub(crate) fn gh_pr_lookup(dir: &Path) -> PrLookup {
    let out = match command("gh")
        .current_dir(dir)
        .args(["pr", "view", "--json", "number,state,headRefOid"])
        .output()
    {
        Ok(out) => out,
        Err(_) => return PrLookup::Unavailable,
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        return if err.contains("no pull requests found") {
            PrLookup::NoPr
        } else {
            PrLookup::Unavailable
        };
    }
    parse_pr_view(&String::from_utf8_lossy(&out.stdout)).unwrap_or(PrLookup::Unavailable)
}

fn parse_pr_view(json: &str) -> Option<PrLookup> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(PrLookup::Found {
        number: u32::try_from(v.get("number")?.as_u64()?).ok()?,
        state: v.get("state")?.as_str()?.to_string(),
        head_oid: v.get("headRefOid")?.as_str()?.to_string(),
    })
}

/// worktree 根目录与主仓库根目录。不在 git 里返回 None。
struct Layout {
    top: PathBuf,
    main: PathBuf,
    linked: bool,
}

fn layout(dir: &Path) -> Option<Layout> {
    let top = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?);
    let git_dir = abs_from(dir, &git(dir, &["rev-parse", "--git-dir"])?);
    let common = abs_from(dir, &git(dir, &["rev-parse", "--git-common-dir"])?);
    let linked = norm(&git_dir) != norm(&common);
    // 主仓库根 = 公共 .git 目录的上一级（bare 仓库不在本功能范围内，linked 判断照样成立）。
    let main = common
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| top.clone());
    Some(Layout { top, main, linked })
}

fn abs_from(base: &Path, p: &str) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

/// 路径比较口径：正斜杠、去尾斜杠；Windows 不分大小写。dunce 去掉 `\\?\` 前缀。
pub(crate) fn norm(p: &Path) -> String {
    let resolved = dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = resolved
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if cfg!(target_os = "windows") {
        s.to_lowercase()
    } else {
        s
    }
}

/// `child` 是否就是 `root` 或在它里面（两者都已 [`norm`] 过）。
pub(crate) fn is_under(child: &str, root: &str) -> bool {
    child == root || child.starts_with(&format!("{root}/"))
}

fn is_locked(main: &Path, top: &Path) -> bool {
    let Some(list) = git(main, &["worktree", "list", "--porcelain"]) else {
        return false;
    };
    let want = norm(top);
    let mut current: Option<String> = None;
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            current = Some(norm(Path::new(p)));
        } else if (line == "locked" || line.starts_with("locked "))
            && current.as_deref() == Some(want.as_str())
        {
            return true;
        }
    }
    false
}

fn submodule_state(top: &Path) -> SubmoduleState {
    let Some(list) = git(top, &["submodule", "status", "--recursive"]) else {
        return SubmoduleState::Clean;
    };
    let mut worst = SubmoduleState::Clean;
    for line in list.lines() {
        let Some(flag) = line.chars().next() else {
            continue;
        };
        let mut parts = line[flag.len_utf8()..].split_whitespace();
        let (Some(sha), Some(path)) = (parts.next(), parts.next()) else {
            continue;
        };
        if flag == '-' {
            continue; // 没初始化的子模块里什么都没有
        }
        let sub = top.join(path);
        if flag == '+' || flag == 'U' {
            // 检出的提交和父仓库记录的不一样：它要是只在本地，删了就没了。
            let on_remote = git(&sub, &["branch", "-r", "--contains", sha])
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if !on_remote {
                return SubmoduleState::Unpushed;
            }
            worst = SubmoduleState::Dirty;
        }
        if git(&sub, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()) {
            worst = SubmoduleState::Dirty;
        }
    }
    worst
}

/// 未提交改动数 / 未跟踪文件数。子模块单独看（[`submodule_state`]），这里忽略。
fn worktree_changes(top: &Path) -> (u32, u32) {
    let Some(list) = git(top, &["status", "--porcelain", "--ignore-submodules=all"]) else {
        return (u32::MAX, 0); // 读不到状态就当脏的：宁可不删
    };
    let mut dirty = 0;
    let mut untracked = 0;
    for line in list.lines().filter(|l| !l.is_empty()) {
        if line.starts_with("??") {
            untracked += 1;
        } else {
            dirty += 1;
        }
    }
    (dirty, untracked)
}

/// 本地分支上有没有「哪儿都找不到」的提交。PR 的 head 提交是合并时远端拿到的最后一个
/// 版本：本地 HEAD 等于它或是它的祖先，说明本地没有多出来的东西。合并后远端分支常被
/// 自动删掉，所以不能只看 upstream。
fn has_unpushed(top: &Path, pr: &PrLookup) -> bool {
    let Some(head) = git(top, &["rev-parse", "HEAD"]) else {
        return true;
    };
    if let PrLookup::Found { head_oid, .. } = pr {
        if head.eq_ignore_ascii_case(head_oid)
            || git_ok(top, &["merge-base", "--is-ancestor", "HEAD", head_oid])
        {
            return false;
        }
    }
    !git(top, &["branch", "-r", "--contains", "HEAD"]).is_some_and(|s| !s.is_empty())
}

fn gather_facts(lay: &Layout, other_sessions: u32, pr_lookup: &dyn Fn(&Path) -> PrLookup) -> Facts {
    let top = &lay.top;
    let branch = git(top, &["symbolic-ref", "--short", "-q", "HEAD"]).filter(|b| !b.is_empty());
    let mut facts = Facts {
        linked_worktree: lay.linked,
        branch,
        other_sessions,
        pr: PrLookup::Unavailable,
        submodules: SubmoduleState::Clean,
        dirty: 0,
        untracked: 0,
        unpushed: false,
        locked: false,
    };
    // 前面的条件不满足就不往下查：gh 要联网，没必要对主检出也问一遍。
    // 注意不能拿 evaluate 判断——此时 pr 还是默认的 Unavailable，会直接短路掉 gh 查询。
    if !facts.linked_worktree || facts.branch.is_none() || facts.other_sessions > 0 {
        return facts;
    }
    facts.locked = is_locked(&lay.main, top);
    facts.pr = pr_lookup(top);
    if evaluate(&facts).is_some() {
        return facts;
    }
    facts.submodules = submodule_state(top);
    (facts.dirty, facts.untracked) = worktree_changes(top);
    facts.unpushed = has_unpushed(top, &facts.pr);
    facts
}

/// 一个目录的完整结论。`other_sessions`：除这条会话外，还有几个未归档会话在这个 worktree 里——
/// 要先知道 worktree 根目录才能数，所以传一个回调。
pub(crate) fn status_for_dir(
    dir: &Path,
    other_sessions: &dyn Fn(&str) -> u32,
    pr_lookup: &dyn Fn(&Path) -> PrLookup,
) -> WorktreeStatusDto {
    let refuse = |path: Option<String>, reason: &str| WorktreeStatusDto {
        path,
        branch: None,
        pr_number: None,
        eligible: false,
        reason: Some(reason.to_string()),
        count: None,
    };
    if !dir.is_dir() {
        return refuse(Some(dir.to_string_lossy().into_owned()), "dir_missing");
    }
    let Some(lay) = layout(dir) else {
        return refuse(Some(dir.to_string_lossy().into_owned()), "not_repo");
    };
    let facts = gather_facts(&lay, other_sessions(&norm(&lay.top)), pr_lookup);
    let verdict = evaluate(&facts);
    WorktreeStatusDto {
        path: Some(dunce::simplified(&lay.top).to_string_lossy().into_owned()),
        branch: facts.branch.clone(),
        pr_number: match facts.pr {
            PrLookup::Found { number, .. } => Some(number),
            _ => None,
        },
        eligible: verdict.is_none(),
        reason: verdict.map(|(r, _)| r.to_string()),
        count: verdict.and_then(|(_, c)| c),
    }
}

// ═══ 占用进程 ═══

/// 当前目录或可执行文件落在 `root` 里的进程（不含本进程）。
///
/// 不按命令行匹配（2026-11-09 真机探针实测）：命令行里「提到」这个路径的进程并不一定在用它——
/// 探针自己的 shell 就因为环境变量里写着路径被列成了占用者，用户在确认框里看到一串 bash.exe，
/// 很可能顺手把无关的终端结束掉。真正占着目录的进程，当前目录就在里面；漏掉的还有改名试探兜底。
pub(crate) fn processes_using(root: &Path) -> Vec<WorktreeProcessDto> {
    let root = norm(root);
    let me = std::process::id();
    let mut out: Vec<WorktreeProcessDto> = scan_processes()
        .into_iter()
        .filter(|p| p.pid != me)
        .filter(|p| {
            p.paths.iter().any(|path| {
                let n = path.replace('\\', "/").trim_end_matches('/').to_string();
                let n = if cfg!(target_os = "windows") {
                    n.to_lowercase()
                } else {
                    n
                };
                is_under(&n, &root)
            })
        })
        .map(|p| WorktreeProcessDto {
            pid: p.pid,
            name: p.name,
        })
        .collect();
    out.sort_by_key(|p| p.pid);
    out.dedup_by_key(|p| p.pid);
    out
}

struct ProcInfo {
    pid: u32,
    name: String,
    /// 当前目录 + 可执行文件路径。
    paths: Vec<String>,
}

#[cfg(target_os = "windows")]
fn scan_processes() -> Vec<ProcInfo> {
    crate::proc::snapshot_processes()
        .into_iter()
        .filter(|(pid, _)| *pid > 4)
        .map(|(pid, (_, name))| {
            let (cwd, image) = win_peb::read(pid);
            ProcInfo {
                pid,
                name,
                paths: cwd.into_iter().chain(image).collect(),
            }
        })
        .collect()
}

/// macOS：`lsof -d cwd` 拿每个进程的当前目录，`ps` 拿可执行文件路径。
#[cfg(not(target_os = "windows"))]
fn scan_processes() -> Vec<ProcInfo> {
    use std::collections::HashMap;
    let mut cwds: HashMap<u32, String> = HashMap::new();
    if let Ok(out) = command("lsof")
        .args(["-n", "-P", "-d", "cwd", "-F", "pn"])
        .output()
    {
        let mut pid = 0u32;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(p) = line.strip_prefix('p') {
                pid = p.parse().unwrap_or(0);
            } else if let Some(n) = line.strip_prefix('n') {
                cwds.insert(pid, n.to_string());
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(ps) = command("ps")
        .args(["-axww", "-o", "pid=", "-o", "comm="])
        .output()
    {
        for line in String::from_utf8_lossy(&ps.stdout).lines() {
            let line = line.trim_start();
            let Some((pid, comm)) = line.split_once(char::is_whitespace) else {
                continue;
            };
            let Ok(pid) = pid.parse::<u32>() else {
                continue;
            };
            let comm = comm.trim().to_string();
            let name = comm.rsplit('/').next().unwrap_or(&comm).to_string();
            let mut paths = vec![comm];
            paths.extend(cwds.get(&pid).cloned());
            out.push(ProcInfo { pid, name, paths });
        }
    }
    out
}

/// 读别的进程的当前目录：PEB → RTL_USER_PROCESS_PARAMETERS。Windows 没有公开 API
/// 能拿到别的进程的当前目录，而「某个 shell cd 进了这个目录」恰恰是最常见的占用方式。
/// 只处理 64 位进程（偏移按 x64 布局）；32 位进程读出来是垃圾就丢掉——宁可漏报，
/// 漏掉的占用还有改名试探兜底。
#[cfg(target_os = "windows")]
mod win_peb {
    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::{
        IsWow64Process, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_INFORMATION,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    /// PROCESS_BASIC_INFORMATION 的 x64 布局。windows-sys 把它放在 Win32_System_Kernel 特性后面，
    /// 为一个结构体多开一整个特性不值得，按文档布局自己声明。
    #[repr(C)]
    struct BasicInfo {
        exit_status: i32,
        peb_base: usize,
        affinity: usize,
        base_priority: i32,
        unique_pid: usize,
        parent_pid: usize,
    }

    const PEB_PROCESS_PARAMETERS: usize = 0x20;
    const PARAMS_CURRENT_DIRECTORY: usize = 0x38;

    /// (当前目录, 可执行文件路径)
    pub(super) fn read(pid: u32) -> (Option<String>, Option<String>) {
        unsafe {
            let image = {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if h.is_null() {
                    None
                } else {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) != 0;
                    CloseHandle(h);
                    ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
                }
            };
            let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
            if h.is_null() {
                return (None, image);
            }
            let mut wow: i32 = 0;
            if IsWow64Process(h, &mut wow) == 0 || wow != 0 {
                CloseHandle(h);
                return (None, image);
            }
            let mut info: BasicInfo = std::mem::zeroed();
            let mut ret = 0u32;
            let status = NtQueryInformationProcess(
                h,
                ProcessBasicInformation,
                &mut info as *mut _ as *mut core::ffi::c_void,
                std::mem::size_of::<BasicInfo>() as u32,
                &mut ret,
            );
            if status < 0 || info.peb_base == 0 {
                CloseHandle(h);
                return (None, image);
            }
            let peb = info.peb_base;
            let params = read_usize(h, peb + PEB_PROCESS_PARAMETERS);
            let cwd = match params {
                Some(p) if p != 0 => read_unicode_string(h, p + PARAMS_CURRENT_DIRECTORY),
                _ => None,
            };
            CloseHandle(h);
            (cwd, image)
        }
    }

    unsafe fn read_bytes(h: *mut core::ffi::c_void, addr: usize, buf: &mut [u8]) -> bool {
        let mut got = 0usize;
        ReadProcessMemory(
            h,
            addr as *const core::ffi::c_void,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            buf.len(),
            &mut got,
        ) != 0
            && got == buf.len()
    }

    unsafe fn read_usize(h: *mut core::ffi::c_void, addr: usize) -> Option<usize> {
        let mut b = [0u8; 8];
        read_bytes(h, addr, &mut b).then(|| usize::from_le_bytes(b))
    }

    /// UNICODE_STRING：Length(u16) MaximumLength(u16) 填充(4) Buffer(ptr)。
    unsafe fn read_unicode_string(h: *mut core::ffi::c_void, addr: usize) -> Option<String> {
        let mut head = [0u8; 2];
        if !read_bytes(h, addr, &mut head) {
            return None;
        }
        let len = u16::from_le_bytes(head) as usize;
        if len == 0 || len > 65534 || !len.is_multiple_of(2) {
            return None;
        }
        let ptr = read_usize(h, addr + 8)?;
        if ptr == 0 {
            return None;
        }
        let mut raw = vec![0u8; len];
        if !read_bytes(h, ptr, &mut raw) {
            return None;
        }
        let wide: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&wide))
    }
}

// ═══ 删除 ═══

/// 只拆链接、不进入链接目标地删掉 `dir` 里的所有链接（junction / 符号链接）。
/// worktree 里的 `node_modules` 常常是指向主仓库的 junction：跟进去删就把主仓库的依赖删了，
/// 不拆又会让 `git worktree remove` 删到一半失败、留下空壳（这次清理留了 40 个）。
pub(crate) fn unlink_links(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if is_link(&meta) {
            remove_link(&path, &meta)?;
        } else if meta.is_dir() {
            unlink_links(&path)?;
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_link(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(target_os = "windows"))]
fn is_link(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// 目录型链接（junction、目录符号链接）用 RemoveDirectory：只删链接本身，不碰目标。
#[cfg(target_os = "windows")]
fn remove_link(path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    if meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(not(target_os = "windows"))]
fn remove_link(path: &Path, _meta: &std::fs::Metadata) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

/// 不跟进链接的递归删除，用于 `git worktree remove` 之后清残留目录。
fn remove_tree_no_follow(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if is_link(&meta) {
            remove_link(&path, &meta)?;
        } else if meta.is_dir() {
            remove_tree_no_follow(&path)?;
        } else {
            let mut perms = meta.permissions();
            if perms.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&path, perms);
            }
            std::fs::remove_file(&path)?;
        }
    }
    std::fs::remove_dir(dir)
}

/// 改名试探：目录里还有被打开的句柄（某个进程 cd 在里面、开着文件）时，Windows 拒绝改名。
/// 改得动就立刻改回去。这一步在动任何文件之前做，避免删到一半才发现删不动。
fn rename_probe(top: &Path) -> Result<(), String> {
    let name = top
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let probe = top.with_file_name(format!(".{name}.meowo-probe-{}", std::process::id()));
    std::fs::rename(top, &probe).map_err(|_| "worktree/busy".to_string())?;
    std::fs::rename(&probe, top).map_err(|e| format!("worktree/probe_restore: {e}"))
}

/// 原因码 → 结构化错误串。逐条写成字面量：前端 errors.ts 的对账测试要求每个匹配串
/// 在 Rust 源码里有字面出处，拼出来的串对不上账。
fn refusal_error(reason: &str) -> &'static str {
    match reason {
        "not_worktree" => "worktree/not_worktree",
        "not_repo" => "worktree/not_repo",
        "dir_missing" => "worktree/dir_missing",
        "detached" => "worktree/detached",
        "locked" => "worktree/locked",
        "shared_session" => "worktree/shared_session",
        "gh_unavailable" => "worktree/gh_unavailable",
        "no_pr" => "worktree/no_pr",
        "pr_open" => "worktree/pr_open",
        "pr_closed" => "worktree/pr_closed",
        "submodule_unpushed" => "worktree/submodule_unpushed",
        "submodule_dirty" => "worktree/submodule_dirty",
        "dirty" => "worktree/dirty",
        "untracked" => "worktree/untracked",
        "unpushed" => "worktree/unpushed",
        _ => "worktree/not_repo",
    }
}

/// 删除主流程。`kill_processes = false` 且有占用进程时什么都不动，返回占用清单。
pub(crate) fn remove_dir_worktree(
    dir: &Path,
    other_sessions: &dyn Fn(&str) -> u32,
    pr_lookup: &dyn Fn(&Path) -> PrLookup,
    kill_processes: bool,
) -> Result<WorktreeRemovalDto, String> {
    let status = status_for_dir(dir, other_sessions, pr_lookup);
    if !status.eligible {
        return Err(refusal_error(status.reason.as_deref().unwrap_or_default()).into());
    }
    let lay = layout(dir).ok_or("worktree/not_repo")?;
    let top = lay.top.clone();
    let branch = status.branch.clone().ok_or("worktree/detached")?;

    let blockers = processes_using(&top);
    if !blockers.is_empty() {
        if !kill_processes {
            return Ok(WorktreeRemovalDto {
                removed: false,
                blockers,
                branch_deleted: false,
            });
        }
        for p in &blockers {
            crate::proc::kill_pid(p.pid);
        }
        // 进程退出、句柄释放要一点时间。
        for _ in 0..20 {
            if processes_using(&top).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    rename_probe(&top)?;
    unlink_links(&top).map_err(|e| format!("worktree/unlink_failed: {e}"))?;

    let top_s = top.to_string_lossy().into_owned();
    // 单个 --force：允许删掉被忽略的文件和含子模块的 worktree（干不干净上面已经查过）；
    // 不用两个 --force，被 lock 的 worktree 仍然会被拒绝。
    let out = command("git")
        .arg("-C")
        .arg(&lay.main)
        .args(["worktree", "remove", "--force", &top_s])
        .output()
        .map_err(|e| format!("worktree/git_failed: {e}"))?;
    if !out.status.success() && !top.exists() {
        // 目录已经没了但 git 报错：交给下面的 prune 收尾。
    } else if !out.status.success() {
        // git 没删干净（比如长路径）：用不跟进链接的方式清残留，再 prune。
        remove_tree_no_follow(&top).map_err(|e| {
            format!(
                "worktree/remove_failed: {} {e}",
                String::from_utf8_lossy(&out.stderr).trim()
            )
        })?;
    }
    if top.exists() {
        remove_tree_no_follow(&top).map_err(|e| format!("worktree/remove_failed: {e}"))?;
    }
    let _ = git(&lay.main, &["worktree", "prune"]);
    if top.exists() {
        return Err("worktree/still_exists".into());
    }
    // squash 合并后 `-d` 会认为没合并而拒绝；没推的提交上面已经排除，这里用 -D。
    let branch_deleted = git_ok(&lay.main, &["branch", "-D", &branch]);
    Ok(WorktreeRemovalDto {
        removed: true,
        blockers: Vec::new(),
        branch_deleted,
    })
}

// ═══ Tauri 命令 ═══

/// 除 `exclude` 外，cwd 落在 `top` 里的未归档会话数。
fn count_other_sessions(db_path: &Path, exclude: &[i64], top: &str) -> u32 {
    let Ok(store) = super::open_store(db_path) else {
        return u32::MAX;
    };
    let Ok(rows) = store.unarchived_session_cwds() else {
        return u32::MAX;
    };
    rows.iter()
        .filter(|(id, _)| !exclude.contains(id))
        .filter(|(_, cwd)| is_under(&norm(Path::new(cwd)), top))
        .count() as u32
}

fn session_dir(db_path: &Path, session_id: i64) -> Result<PathBuf, String> {
    let store = super::open_store(db_path)?;
    let cwd = store
        .session_cwd(session_id)
        .map_err(|e| e.to_string())?
        .filter(|c| !c.trim().is_empty())
        .ok_or("worktree/no_cwd")?;
    Ok(PathBuf::from(cwd))
}

#[tauri::command]
pub(crate) async fn worktree_status(
    state: State<'_, super::AppState>,
    session_id: i64,
) -> Result<WorktreeStatusDto, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = session_dir(&db_path, session_id)?;
        Ok(status_for_dir(
            &dir,
            &|top| count_other_sessions(&db_path, &[session_id], top),
            &gh_pr_lookup,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除会话所在的 worktree。只收会话 id、不收路径：手机端经远程桥调用时，能删的只有
/// 某个会话自己的 worktree，并且要过同一套判定。
#[tauri::command]
pub(crate) async fn remove_session_worktree(
    app: tauri::AppHandle,
    state: State<'_, super::AppState>,
    session_id: i64,
    kill_processes: bool,
) -> Result<WorktreeRemovalDto, String> {
    let db_path = state.db_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let dir = session_dir(&db_path, session_id)?;
        // 同一个 worktree 下其他已归档会话不算「在用」：它们本来就会跟着一起显示「目录已删除」。
        remove_dir_worktree(
            &dir,
            &|top| count_other_sessions(&db_path, &[session_id], top),
            &gh_pr_lookup,
            kill_processes,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    if matches!(&result, Ok(r) if r.removed) {
        super::watch::emit_board_changed(&app, "remove_worktree");
    }
    result
}

/// 批量清理清单：所有已归档会话所在的 worktree（去重），各附判定结果。gh 要联网，
/// 按 worktree 并发查，最多 4 个一起。
#[tauri::command]
pub(crate) async fn list_cleanable_worktrees(
    state: State<'_, super::AppState>,
) -> Result<Vec<CleanableWorktreeDto>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = super::open_store(&db_path)?;
        let archived = store.archived_session_cwds().map_err(|e| e.to_string())?;
        // 先按 worktree 根目录分组（git rev-parse 很快，不联网）。
        let mut groups: Vec<(PathBuf, String, Vec<i64>)> = Vec::new();
        for (id, cwd) in archived {
            let dir = PathBuf::from(&cwd);
            if !dir.is_dir() {
                continue;
            }
            let Some(lay) = layout(&dir) else { continue };
            if !lay.linked {
                continue;
            }
            let key = norm(&lay.top);
            match groups.iter_mut().find(|(_, k, _)| *k == key) {
                Some(g) => g.2.push(id),
                None => groups.push((lay.top.clone(), key, vec![id])),
            }
        }
        let mut out = Vec::with_capacity(groups.len());
        for chunk in groups.chunks(4) {
            let results: Vec<CleanableWorktreeDto> = std::thread::scope(|s| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|(top, _, ids)| {
                        let db_path = &db_path;
                        s.spawn(move || CleanableWorktreeDto {
                            session_ids: ids.clone(),
                            status: status_for_dir(
                                top,
                                &|t| count_other_sessions(db_path, ids, t),
                                &gh_pr_lookup,
                            ),
                        })
                    })
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok()).collect()
            });
            out.extend(results);
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
