//! 认领：从一次写入类工具调用里认出它写的是哪个任务目录。
//!
//! 为什么不能只认字面的 `.lead/<slug>`：agent 写任务文件的命令常常先 `cd` 进 `.lead`，任务名只出现在
//! 变量赋值里（`D=<slug> && cat > $D/SPEC.md`、PowerShell 的 `$D='<slug>'`、`Join-Path $L <slug>`），
//! 命令里根本没有 `.lead/<slug>` 这串。认领不上，`.owner` 为空，项目里（含 worktree）所有会话的 Lead
//! 面板都显示这个任务（用户实拍 2026-11-09、2026-11-18 两次）。
//!
//! 认法按把握从高到低，前一档认出就不看后一档：
//! 1. 字面的 `[<路径>/].lead/<slug>`（原有规则，写哪个就归哪个，可以从别的会话手里接过任务）。
//! 2. 命令里出现了某个 `.lead` 目录（cd 目标、引号里的路径、`Join-Path <根> .lead`、会话 cwd 就在 `.lead`
//!    里），同时文本里有个词元正好是这个 `.lead` 下真实存在的任务目录名。已归别的会话的任务，只有在本次
//!    调用期间确实被写过才转手，免得一句 `cat <slug>/SPEC.md > 别处` 就把别人的任务抢走。
//! 3. 兜底：命令提到了 `.lead` 却认不出任务名（`$D` 在之前的命令里赋的值），看这些 `.lead` 下哪个**无主**
//!    任务在本次调用期间被写过（hook 的时间窗：工具耗时 `duration_ms` 加一点余量）。恰好一个才认；
//!    两个以上（别的会话同时也在写）就谁都不认——拿不准宁可不认。

use super::{find_task_refs, is_write, lead_dirs, owner_of, slug_ok, strip_verbatim, windows_path};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 时间窗在工具耗时之外再放宽的余量：hook 进程启动、文件系统时间戳精度。
const SLACK: Duration = Duration::from_secs(2);
/// 载荷没带 `duration_ms`（老版本 CLI、其他 Provider 的正规化载荷）时的时间窗。
const DEFAULT_WINDOW: Duration = Duration::from_secs(5);

/// 一次工具调用要看的文本：编辑类工具看 file_path，命令类看命令文本。
pub(crate) fn call_text<'a>(name: &str, input: &'a Value) -> Option<&'a str> {
    match name {
        "Write" | "Edit" | "MultiEdit" => input.get("file_path").and_then(Value::as_str),
        _ => input.get("command").and_then(Value::as_str),
    }
}

/// PostToolUse 载荷对应的任务目录（认领目标）；认不出或拿不准返回 None。
pub(crate) fn claim_target(data: &Value) -> Option<PathBuf> {
    let sid = data.get("session_id").and_then(Value::as_str)?;
    let name = data.get("tool_name").and_then(Value::as_str)?;
    let input = data.get("tool_input")?;
    if !is_write(name, input) {
        return None;
    }
    let text = call_text(name, input)?;
    let cwd = data.get("cwd").and_then(Value::as_str);
    let took = data
        .get("duration_ms")
        .and_then(Value::as_u64)
        .map(Duration::from_millis);
    let since = SystemTime::now()
        .checked_sub(took.unwrap_or(DEFAULT_WINDOW) + SLACK)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let refs = Refs::parse(text, cwd);
    if let Some(task) = refs.literal.into_iter().next() {
        return Some(task);
    }
    // 第 2 档：已归本会话或无主的直接算；归别人的要本次确实写过。
    let mine = |t: &PathBuf| {
        let owner = owner_of(t);
        owner.is_empty() || owner == sid
    };
    let named: Vec<PathBuf> = refs
        .named
        .into_iter()
        .filter(|t| mine(t) || touched_since(t, since))
        .collect();
    if let Some(task) = pick(named, since) {
        return Some(task);
    }
    // 第 3 档：只在无主任务里找，且必须恰好一个在时间窗内被写过。
    let mut fresh = refs
        .lead_dirs
        .iter()
        .flat_map(|lead| tasks_in(lead))
        .filter(|t| owner_of(t).is_empty() && touched_since(t, since));
    let first = fresh.next()?;
    fresh.next().is_none().then_some(first)
}

/// 一次调用写到的任务里，属于 `among` 的那些（Stop 时按 transcript 补认领、判断本轮是否推进过任务用）。
/// 没有时间窗可用：只认字面路径和第 2 档的任务名。
pub(crate) fn touches(name: &str, input: &Value, cwd: Option<&str>) -> Vec<PathBuf> {
    if !is_write(name, input) {
        return Vec::new();
    }
    let Some(text) = call_text(name, input) else {
        return Vec::new();
    };
    let refs = Refs::parse(text, cwd);
    let mut out = refs.literal;
    for t in refs.named {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// 多个候选时：只留本次调用期间写过的；仍不止一个就不认。
fn pick(mut tasks: Vec<PathBuf>, since: SystemTime) -> Option<PathBuf> {
    if tasks.len() > 1 {
        tasks.retain(|t| touched_since(t, since));
    }
    (tasks.len() == 1).then(|| tasks.remove(0))
}

/// `.lead` 下的任务目录（目录名过滤与 own_task / project_gates 一致）。
pub(crate) fn tasks_in(lead: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(lead) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(slug_ok))
        .collect();
    out.sort();
    out
}

/// 任务目录里的内容文件（不含 `.owner`、`.stop-state.json` 这类 hook 自己写的点文件）或子目录
/// 在 `since` 之后改过。不看任务目录本身的 mtime：别的会话认领时创建 `.owner` 也会刷新它。
fn touched_since(task: &Path, since: SystemTime) -> bool {
    let Ok(entries) = std::fs::read_dir(task) else {
        return false;
    };
    entries.flatten().any(|e| {
        let hidden = e.file_name().to_string_lossy().starts_with('.');
        !hidden
            && e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= since)
    })
}

/// 从文本里解析出的任务目录引用。
struct Refs {
    /// 字面 `.lead/<slug>` 路径指向的任务（按出现顺序）。
    literal: Vec<PathBuf>,
    /// 第 2 档：提到的 `.lead` 目录下、名字出现在文本词元里的任务。
    named: Vec<PathBuf>,
    /// 文本或 cwd 牵涉到的 `.lead` 目录（第 3 档兜底只在这些目录里找）。
    lead_dirs: Vec<PathBuf>,
}

impl Refs {
    fn parse(text: &str, cwd: Option<&str>) -> Self {
        let text = text.replace('\\', "/");
        let cwd_path = cwd.map(Path::new);
        // 文本和 cwd 都不牵涉 `.lead`：绝大多数调用走这里，不碰文件系统
        if !text.contains(".lead") && !cwd.is_some_and(|c| c.contains(".lead")) {
            return Refs {
                literal: Vec::new(),
                named: Vec::new(),
                lead_dirs: Vec::new(),
            };
        }
        let literal: Vec<PathBuf> = find_task_refs(&text)
            .into_iter()
            .filter_map(|r| resolve(&r, cwd_path))
            .filter(|p| is_task(p))
            .collect();
        let mut leads: Vec<PathBuf> = Vec::new();
        let mut push = |p: PathBuf| {
            if !leads.contains(&p) {
                leads.push(p);
            }
        };
        for r in lead_refs(&text) {
            if let Some(p) = resolve(&r, cwd_path).filter(|p| is_lead(p)) {
                push(p);
            }
            // 单独一段 `.lead`（`Join-Path "<根>" '.lead'`）：根目录在文本里别的绝对路径上
            if r == ".lead" {
                for base in absolute_words(&text) {
                    if let Some(p) = resolve(&format!("{base}/.lead"), None).filter(|p| is_lead(p))
                    {
                        push(p);
                    }
                }
            }
        }
        // 会话 cwd 就在某个 `.lead` 里（上一条命令 cd 进去后 cwd 跟着变）：那个 `.lead` 和 cwd 所在的任务
        let mut cwd_task = None;
        if let Some(cwd) = cwd_path {
            let mut dir = Some(cwd);
            while let Some(d) = dir {
                if d.parent().is_some_and(is_lead) && is_task(d) {
                    cwd_task = resolve(&d.to_string_lossy(), None);
                }
                if is_lead(d) {
                    if let Some(p) = resolve(&d.to_string_lossy(), None) {
                        push(p);
                    }
                }
                dir = d.parent();
            }
            // 文本提到了 `.lead` 却没解析出目录（`Join-Path $root '.lead'`）：退回 cwd 所在项目的 `.lead`
            if text.contains(".lead") {
                for lead in lead_dirs(cwd).into_iter().filter(|p| p.is_dir()) {
                    if let Some(p) = resolve(&lead.to_string_lossy(), None) {
                        push(p);
                    }
                }
            }
        }
        let words: Vec<&str> = text
            .split(|c: char| !(super::is_word(c) || c == '.' || c == '-'))
            .filter(|w| slug_ok(w) && *w != ".lead")
            .collect();
        let mut named: Vec<PathBuf> = Vec::new();
        for lead in &leads {
            for w in &words {
                let t = lead.join(w);
                if is_task(&t) && !named.contains(&t) {
                    named.push(t);
                }
            }
        }
        if let Some(t) = cwd_task {
            if !named.contains(&t) {
                named.push(t);
            }
        }
        Refs {
            literal,
            named,
            lead_dirs: leads,
        }
    }
}

fn is_lead(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n == ".lead") && p.is_dir()
}

fn is_task(p: &Path) -> bool {
    p.parent().is_some_and(is_lead)
        && p.file_name().and_then(|n| n.to_str()).is_some_and(slug_ok)
        && p.is_dir()
}

/// 路径文本 → 规范化的绝对路径（Git Bash 盘符、相对 cwd）；不存在返回 None。
fn resolve(r: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let mut p = PathBuf::from(windows_path(r));
    if !p.is_absolute() {
        p = cwd?.join(p);
    }
    p.canonicalize().ok().map(strip_verbatim)
}

/// 文本（已统一成 `/`）里所有指向 `.lead` 目录本身的路径：`.lead` 后面是结尾、`/` 加非任务名字符
/// （如 `.lead/$D`）或边界。路径起点的找法与 find_task_refs 一致（引号内允许空格、盘符冒号不算边界）。
fn lead_refs(text: &str) -> Vec<String> {
    let needle = ".lead";
    let boundary = |c: char| c.is_whitespace() || "\"'<>|=:;&(),{}".contains(c);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = text[from..].find(needle) {
        let at = from + off;
        from = at + needle.len();
        let prev = text[..at].chars().next_back();
        if !prev.is_none_or(|c| c == '/' || boundary(c)) {
            continue;
        }
        let after = &text[at + needle.len()..];
        let ok = match after.chars().next() {
            None => true,
            Some('/') => !after[1..]
                .chars()
                .next()
                .is_some_and(|c| super::is_word(c) || c == '.' || c == '-'),
            Some(c) => boundary(c),
        };
        if !ok {
            continue;
        }
        let before = &text[..at];
        let quoted = ['"', '\''].iter().find_map(|q| {
            (before.matches(*q).count() % 2 == 1)
                .then(|| before.rfind(*q).map(|i| i + 1))
                .flatten()
        });
        let start = quoted.unwrap_or_else(|| {
            let s = before
                .char_indices()
                .rev()
                .find(|(_, c)| boundary(*c))
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            let drive = s >= 2
                && before[..s].ends_with(':')
                && before[..s - 1]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphabetic());
            if drive {
                s - 2
            } else {
                s
            }
        });
        out.push(text[start..at + needle.len()].to_owned());
    }
    out
}

/// 文本里像绝对路径的词（`C:/…`、`/c/…`、`/home/…`），去掉两端引号与结尾的 `/`。
fn absolute_words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || "\"'<>|=;&(),{}".contains(c))
        .map(|w| w.trim_end_matches('/'))
        .filter(|w| {
            let b = w.as_bytes();
            (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/')
                || (b.len() >= 2 && b[0] == b'/' && b[1] != b'/')
        })
        .map(str::to_owned)
        .collect()
}
