//! Lead 工作流的 hook：`meowo-reporter lead <session|claim|stop>`。
//!
//! 为什么在 reporter 里而不是脚本：Lead 工作流随 Meowo 分发给所有用户，用户机器上不一定有
//! Python（初版是 ~/.claude/lead/hook.py）。行为与那版逐条对齐，场景测试也一一移植（见文末）。
//!
//! 与 reporter 主路径的边界：主路径「绝不阻塞 Agent」，lead 子命令是用户在设置里显式打开的
//! 独立 hook 条目，**只有它**会输出改变 CLI 行为的内容（注入上下文 / 拦住停下）。任何异常
//! 一律放行（不输出、退出码 0），宁可漏拦，不可误拦。
//!
//! 三种模式：
//! - session（SessionStart，含压缩后 source=compact）：注入当前任务的规格与进度。
//! - claim（PostToolUse）：哪个会话**写了**任务的 progress.md，任务就归哪个会话。
//! - stop（Stop）：任务归属会话在实施阶段、**本轮推进过任务**、验收没过完时拦住它。

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 一轮内最多连续拦截次数；用户每发一条新消息就清零。
const MAX_BLOCKS: u64 = 5;
/// 用户明确叫停的说法。只收明确说法："停车场""就这样做吧"这类不能误判成叫停（实测踩过）。
const STOP_WORDS: &[&str] = &[
    "停下", "停一下", "先停", "暂停", "停止", "停掉", "先别做", "别做了", "不用做了",
    "什么都不要做", "别的都不要做", "不要做别的", "到此为止", "结束吧",
];
const STOP_WORDS_EN: &[&str] = &["stop", "pause"];
const TRANSCRIPT_TAIL: u64 = 4 * 1024 * 1024;
const WAIT_POLL: std::time::Duration = std::time::Duration::from_secs(2);
/// 关卡最多等 7 天；hook 的 timeout 由安装时设得比这更长。
const WAIT_MAX: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// hook 的输出。只有 wait 模式会用到 stderr 与退出码 2（asyncRewake：退出码 2 唤醒模型，
/// stderr 作为唤醒消息）；其余模式恒为退出码 0。
#[derive(Debug, Default, PartialEq)]
pub struct Output {
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit: i32,
}

/// 入口。`registry` 是 session_id → 任务目录的登记表（默认在 Meowo 数据目录旁）。
pub fn run(mode: &str, raw: &str, registry: &Path) -> Output {
    // hook 绝不能因为我们的 bug 把错误抛给 agent：panic 一律当作放行。
    std::panic::catch_unwind(|| run_inner(mode, raw, registry)).unwrap_or_default()
}

fn run_inner(mode: &str, raw: &str, registry: &Path) -> Output {
    if mode == "claim" && !raw.contains("progress.md") {
        return Output::default(); // 快速路径：绝大多数工具调用直接返回
    }
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(raw) else {
        return Output::default();
    };
    let data = Value::Object(data);
    let stdout = match mode {
        "session" => session(&data, registry),
        "claim" => {
            claim(&data, registry);
            None
        }
        "stop" => stop(&data, registry),
        "wait" => return wait(&data, registry, WAIT_POLL, WAIT_MAX),
        _ => None,
    };
    Output { stdout, ..Output::default() }
}

// ---------------- 文件与解析 ----------------

fn read(p: &Path) -> String {
    match std::fs::read(p) {
        Ok(bytes) => {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            s.strip_prefix('\u{feff}').map(str::to_owned).unwrap_or(s) // Windows PowerShell 5 会写 BOM
        }
        Err(_) => String::new(),
    }
}

fn truncate(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        return s.to_owned();
    }
    let head: String = s.chars().take(limit).collect();
    format!("{head}\n…(已截断，完整内容见文件)")
}

fn tail(s: &str, limit: usize) -> String {
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(limit)).collect()
}

fn load_map(p: &Path) -> serde_json::Map<String, Value> {
    match serde_json::from_str::<Value>(&read(p)) {
        Ok(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn slug_ok(slug: &str) -> bool {
    !slug.is_empty() && slug != "." && slug != ".." && slug.chars().all(|c| is_word(c) || c == '.' || c == '-')
}

/// 统一成 `/` 分隔的文本，方便匹配 Windows 路径（JSON 里的 `\\` 与裸 `\` 都换掉）。
fn norm(v: &Value) -> String {
    v.to_string().replace("\\\\", "/").replace('\\', "/")
}

/// git 根目录；不在 git 里就是 cwd 本身。绝不越过项目边界向上找（初版踩过：主目录下的任务
/// 注入到了所有非 git 目录）。
fn project_root(cwd: &Path) -> PathBuf {
    let start = cwd.canonicalize().map(strip_verbatim).unwrap_or_else(|_| cwd.to_path_buf());
    let mut d = Some(start.as_path());
    while let Some(dir) = d {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        d = dir.parent();
    }
    start
}

/// Windows 上 canonicalize 会带 `\\?\` 前缀（真机实测登记表里出现过），登记表和界面里都不该出现它。
pub(crate) fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

fn find_task(cwd: Option<&str>) -> Option<PathBuf> {
    let lead = project_root(Path::new(cwd?)).join(".lead");
    let active = read(&lead.join("active"));
    let slug = active.trim();
    if !slug_ok(slug) {
        return None;
    }
    let task = lead.join(slug);
    task.is_dir().then_some(task)
}

fn task_of_session(sid: &str, registry: &Path) -> Option<PathBuf> {
    let t = load_map(registry).get(sid)?.as_str()?.to_owned();
    let task = PathBuf::from(t);
    (task.is_dir() && read(&task.join(".owner")).trim() == sid).then_some(task)
}

/// `phase: implement` 一行；容忍大小写、markdown 加粗等前缀。
pub fn phase_of(task: &Path) -> String {
    for line in read(&task.join("progress.md")).lines() {
        let rest = line.trim_start_matches(|c: char| !is_word(c));
        // 按字节取前 5 个，必须走 get：中文行（如「# 进度」）直接切会落在字符中间而 panic。
        if !rest.get(..5).is_some_and(|w| w.eq_ignore_ascii_case("phase")) {
            continue;
        }
        let after = rest[5..].trim_start_matches(|c: char| !is_word(c) && c != ':');
        let Some(after) = after.strip_prefix(':') else { continue };
        let word: String = after.trim_start().chars().take_while(|c| is_word(*c)).collect();
        if !word.is_empty() {
            return word.to_lowercase();
        }
    }
    "unknown".into()
}

fn acceptance(task: &Path) -> Option<Vec<Value>> {
    let text = read(&task.join("acceptance.json"));
    let text = if text.trim().is_empty() { "[]".to_owned() } else { text };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Array(items)) => Some(items.into_iter().filter(Value::is_object).collect()),
        _ => None,
    }
}

/// 只有 `passes: true` 算通过；字符串 "false"、缺字段都算未通过。
fn failing(items: &[Value]) -> Vec<&Value> {
    items.iter().filter(|i| i.get("passes") != Some(&Value::Bool(true))).collect()
}

fn item_line(i: &Value) -> String {
    let id = i.get("id").map(|v| v.to_string()).unwrap_or_default();
    let check = i.get("check").and_then(Value::as_str).unwrap_or("");
    format!("#{id} {check}")
}

/// 命令文本里有没有写入特征：重定向（排除 `2>/dev/null`、`2>&1`、`>$null`）、`sed -i`、`tee`、
/// PowerShell 写文件、`mv`/`cp`。
pub fn shell_writes(cmd: &str) -> bool {
    let b: Vec<char> = cmd.chars().collect();
    for (i, &c) in b.iter().enumerate() {
        if c != '>' {
            continue;
        }
        let prev = if i > 0 { b[i - 1] } else { ' ' };
        if prev.is_ascii_digit() || prev == '&' || prev == '>' {
            continue;
        }
        let mut j = i + 1;
        if j < b.len() && b[j] == '>' {
            j += 1;
        }
        let rest: String = b[j..].iter().collect::<String>().trim_start().to_owned();
        if rest.starts_with("/dev/null") || rest.starts_with('&') || rest.starts_with("$null") {
            continue;
        }
        return true;
    }
    let words: Vec<&str> = cmd.split(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&').collect();
    let has_word = |w: &str| words.contains(&w);
    (cmd.contains("sed -i") || has_word("tee") || has_word("mv") || has_word("cp"))
        || ["Set-Content", "Add-Content", "Out-File"].iter().any(|k| cmd.contains(k))
}

fn is_write(name: &str, input: &Value) -> bool {
    match name {
        "Write" | "Edit" | "MultiEdit" => true,
        "Bash" | "PowerShell" => input.get("command").and_then(Value::as_str).is_some_and(shell_writes),
        _ => false,
    }
}

/// 在规范化文本里找 `[<路径>/].lead/<slug>/progress.md`，返回整段路径。
/// 相对路径（Bash 里 `echo x >> .lead/a/progress.md`）也要认：`.lead` 前是 `/` 或词元边界。
fn find_progress_path(text: &str) -> Option<String> {
    let needle = ".lead/";
    let boundary = |c: char| c.is_whitespace() || "\"'<>|=:".contains(c);
    let mut from = 0;
    while let Some(off) = text[from..].find(needle) {
        let at = from + off;
        from = at + needle.len();
        let prev = text[..at].chars().next_back();
        if !prev.is_none_or(|c| c == '/' || boundary(c)) {
            continue;
        }
        let after = &text[at + needle.len()..];
        let slug: String = after.chars().take_while(|c| is_word(*c) || *c == '.' || *c == '-').collect();
        if !(slug_ok(&slug) && after[slug.len()..].starts_with("/progress.md")) {
            continue;
        }
        let before = &text[..at];
        // 在引号里（之前出现了奇数个同种引号）：路径从那个左引号之后开始，允许含空格。
        let quoted = ['"', '\''].iter().find_map(|q| {
            (before.matches(*q).count() % 2 == 1).then(|| before.rfind(*q).map(|i| i + 1)).flatten()
        });
        let start = quoted.unwrap_or_else(|| {
            let s = before.char_indices().rev().find(|(_, c)| boundary(*c)).map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
            // Windows 盘符 `C:/…` 的冒号不是边界
            let drive = s >= 2 && before[..s].ends_with(':') && before[..s - 1].chars().next_back().is_some_and(|c| c.is_ascii_alphabetic());
            if drive { s - 2 } else { s }
        });
        return Some(text[start..at + needle.len() + slug.len() + "/progress.md".len()].to_owned());
    }
    None
}

fn contains_stop_word(text: &str) -> bool {
    if STOP_WORDS.iter().any(|w| text.contains(w)) {
        return true;
    }
    let lower = text.to_lowercase();
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| STOP_WORDS_EN.contains(&w))
}

// ---------------- session ----------------

fn session(data: &Value, registry: &Path) -> Option<String> {
    let sid = data.get("session_id").and_then(Value::as_str).unwrap_or("");
    let task = find_task(data.get("cwd").and_then(Value::as_str)).or_else(|| task_of_session(sid, registry))?;
    let phase = phase_of(&task);
    if phase == "done" {
        return None;
    }
    let lead = task.parent()?;
    let items = acceptance(&task).unwrap_or_default();
    let fail: Vec<String> = failing(&items).iter().map(|i| format!("  {}", item_line(i))).collect();
    let mut parts = vec![
        format!(
            "[lead] 这个项目有进行中的任务：{}（phase: {phase}）。如果用户这次说的事与它有关，就按 lead skill 的流程继续推进，不要等用户说“继续”；无关就正常处理用户的请求。",
            task.display()
        ),
        format!("## SPEC.md\n{}", truncate(&read(&task.join("SPEC.md")), 4000)),
        format!(
            "## 未通过的验收项（{}/{}）\n{}",
            fail.len(),
            items.len(),
            if fail.is_empty() { "  无".to_owned() } else { fail.join("\n") }
        ),
        format!("## progress.md（最近部分）\n{}", tail(&read(&task.join("progress.md")), 2500)),
    ];
    // 只有关卡的归属会话才拿到「按答复继续」：同项目新开的无关会话不能去执行别人的单向门决定
    // （做空审查第二轮 M1；第一轮只修了 wait）。
    let owner = task_of_session(sid, registry).is_some_and(|t| t == task);
    if let Some(g) = read_gate(&task).filter(|g| g.status == "answered" && owner) {
        parts.push(answer_message(&g));
    }
    for extra in ["product.md", "standards.md"] {
        let t = read(&lead.join(extra));
        if !t.is_empty() {
            parts.push(format!("## .lead/{extra}\n{}", truncate(&t, 3000)));
        }
    }
    Some(
        json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": parts.join("\n\n")}})
            .to_string(),
    )
}

// ---------------- claim ----------------

/// 只有写入任务的 progress.md 才算认领；从被写的路径反推任务目录，不看工作目录
/// （会话在主目录启动、在 worktree 里工作时都能认领）。
fn claim(data: &Value, registry: &Path) {
    let (Some(sid), Some(name)) = (
        data.get("session_id").and_then(Value::as_str),
        data.get("tool_name").and_then(Value::as_str),
    ) else {
        return;
    };
    let input = data.get("tool_input").cloned().unwrap_or(Value::Null);
    if !is_write(name, &input) {
        return;
    }
    // 编辑类工具直接用 file_path（路径可能带空格，文本搜索会截断）；命令类才在命令文本里找。
    let path = match name {
        "Write" | "Edit" | "MultiEdit" => input
            .get("file_path")
            .and_then(Value::as_str)
            .map(|p| p.replace('\\', "/"))
            .filter(|p| p.ends_with("/progress.md")),
        _ => input.get("command").and_then(Value::as_str).and_then(|c| find_progress_path(&c.replace('\\', "/"))),
    };
    let Some(path) = path else { return };
    let mut p = PathBuf::from(&path);
    if !p.is_absolute() {
        if let Some(cwd) = data.get("cwd").and_then(Value::as_str) {
            p = Path::new(cwd).join(p);
        }
    }
    let Some(task) = p.parent().and_then(|t| t.canonicalize().ok()).map(strip_verbatim) else { return };
    if !task.is_dir() || task.parent().and_then(Path::file_name).is_none_or(|n| n != ".lead") {
        return;
    }
    let _ = std::fs::write(task.join(".owner"), sid);
    let key = task.to_string_lossy().into_owned();
    let mut reg: BTreeMap<String, Value> = load_map(registry)
        .into_iter()
        .filter(|(_, v)| v.as_str().is_some_and(|s| s != key && Path::new(s).is_dir()))
        .collect();
    reg.insert(sid.to_owned(), Value::String(key)); // 一个任务只归一个会话
    // 先写临时文件再 rename：直接 write 会先截断，并发认领时别的会话可能读到空表再写回，
    // 把其他会话的映射冲掉（做空审查第二轮 L1）。
    let tmp = registry.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_string(&reg).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, registry);
    }
}

// ---------------- stop ----------------

/// 返回（用户最近一条真正的输入，这之后 agent 发起的工具调用）。
fn this_turn(transcript: Option<&str>) -> (String, Vec<Value>) {
    use std::io::{Read, Seek, SeekFrom};
    let Some(path) = transcript else { return (String::new(), vec![]) };
    let Ok(mut f) = std::fs::File::open(path) else { return (String::new(), vec![]) };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_TAIL)));
    let mut bytes = Vec::new();
    let _ = f.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let mut calls = Vec::new();
    for line in text.lines().rev() {
        let Ok(o) = serde_json::from_str::<Value>(line) else { continue };
        let content = o.pointer("/message/content");
        let kind = o.get("type").and_then(Value::as_str);
        if kind == Some("assistant") {
            if let Some(Value::Array(arr)) = content {
                calls.extend(arr.iter().filter(|x| x.get("type").and_then(Value::as_str) == Some("tool_use")).cloned());
            }
            continue;
        }
        if kind != Some("user") || o.get("isMeta").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let text = match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(arr)) => {
                if arr.iter().any(|x| x.get("type").and_then(Value::as_str) == Some("tool_result")) {
                    continue;
                }
                arr.iter()
                    .filter(|x| x.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|x| x.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            _ => continue,
        };
        if !text.trim().is_empty() && !text.starts_with('<') {
            return (text, calls);
        }
    }
    (String::new(), calls)
}

fn wrote_task(calls: &[Value], task: &Path) -> bool {
    // 不带前导 `/`：Bash 里常用相对路径 `.lead/<slug>/…`（真机实测漏拦过一次）。slug 带日期前缀，
    // 误撞别的目录的概率可以忽略。
    let key = format!(".lead/{}/", task.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
    calls.iter().any(|c| {
        let input = c.get("input").cloned().unwrap_or(Value::Null);
        norm(&input).contains(&key) && c.get("name").and_then(Value::as_str).is_some_and(|n| is_write(n, &input))
    })
}

fn stop(data: &Value, registry: &Path) -> Option<String> {
    let sid = data.get("session_id").and_then(Value::as_str)?;
    let task = task_of_session(sid, registry)?; // 按会话找任务：不依赖工作目录
    if phase_of(&task) != "implement" {
        return None;
    }
    if read_gate(&task).is_some_and(|g| g.status != "consumed") {
        return None; // 在等 leader 拍板，停下是对的
    }
    let items = acceptance(&task)?;
    let fail = failing(&items);
    let state_f = task.join(".stop-state.json");
    let mut state = load_map(&state_f);
    if fail.is_empty() {
        if state.remove(sid).is_some() {
            let _ = std::fs::write(&state_f, Value::Object(state).to_string());
        }
        return None;
    }
    let (user_text, calls) = this_turn(data.get("transcript_path").and_then(Value::as_str));
    if contains_stop_word(&user_text) {
        return None; // 用户明确叫停
    }
    if !wrote_task(&calls, &task) {
        return None; // 这一轮没在推进任务（用户插话、提问），不拦
    }
    // stop_hook_active=true：同一轮被拦后再次停下；否则是新的一轮，重新计数。
    let same_turn = data.get("stop_hook_active").and_then(Value::as_bool) == Some(true);
    let prev = if same_turn { state.get(sid).and_then(|s| s.get("blocks")).and_then(Value::as_u64) } else { None };
    let blocks = prev.unwrap_or(0);
    if blocks >= MAX_BLOCKS {
        return None;
    }
    let blocks = blocks + 1;
    state.insert(sid.to_owned(), json!({"blocks": blocks}));
    let _ = std::fs::write(&state_f, Value::Object(state).to_string());
    let slug = task.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let list: Vec<String> = fail.iter().take(8).map(|i| item_line(i)).collect();
    let reason = format!(
        "[lead] 任务 {slug} 还在实施阶段，有 {} 条验收未通过（本轮第 {blocks}/{MAX_BLOCKS} 次拦截）：\n{}\n继续推进下一条，并实际验证。确实卡住了：在 progress.md 写明原因，把 phase 改为 blocked，再找用户。需要用户做决定（单向门）：把 phase 改为 design，再提问。",
        fail.len(),
        list.join("\n")
    );
    Some(json!({"decision": "block", "reason": reason}).to_string())
}

// ---------------- 关卡（gate.json）与快照 ----------------

use meowo_protocol::ipc::{LeadCheckDto, LeadGateAnswerDto, LeadGateDto, LeadGateOptionDto, LeadTaskDto};

/// 读 gate.json；文件不存在或格式不合法返回 None。
pub fn read_gate(task: &Path) -> Option<LeadGateDto> {
    let v: Value = serde_json::from_str(&read(&task.join("gate.json"))).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    let (id, kind, status) = (s("id"), s("type"), s("status"));
    if id.is_empty()
        || !["A", "B", "C"].contains(&kind.as_str())
        || !["pending", "answered", "consumed"].contains(&status.as_str())
    {
        return None;
    }
    let options = v
        .get("options")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|o| serde_json::from_value::<LeadGateOptionDto>(o.clone()).ok()).collect())
        .unwrap_or_default();
    let answer = v.get("answer").and_then(|a| serde_json::from_value::<LeadGateAnswerDto>(a.clone()).ok());
    Some(LeadGateDto { id, kind, title: s("title"), summary: s("summary"), options, status, answer })
}

/// 唤醒 / 注入给 agent 的答复文本。内容以 gate.json 为准，这句话重复送达也无害。
fn answer_message(g: &LeadGateDto) -> String {
    let (choice, note) = g.answer.as_ref().map(|a| (a.choice.as_str(), a.note.as_str())).unwrap_or(("", ""));
    let note = if note.trim().is_empty() { String::new() } else { format!("，意见：{note}") };
    format!(
        "[lead] leader 已在 Meowo 里回答关卡 {}（{}）：选择 {choice}{note}。读 gate.json 确认，把答复记进 decisions.md，把 gate.json 的 status 改成 consumed，然后按答复继续。",
        g.kind, g.title
    )
}

/// leader 作答：核对 gate id 与状态后原子写入 answered。同一答案重复提交视为成功
/// （手机断线重发）；关卡已换、已被消费或已用别的答案作答则报错（`模块/代码` 结构化 reason）。
pub fn answer_gate(task: &Path, gate_id: &str, answer: &LeadGateAnswerDto) -> Result<(), String> {
    let path = task.join("gate.json");
    let mut v: Value = serde_json::from_str(&read(&path)).map_err(|_| "lead/gate_missing".to_owned())?;
    let g = read_gate(task).ok_or("lead/gate_invalid")?;
    if g.id != gate_id {
        return Err("lead/gate_changed".into());
    }
    match g.status.as_str() {
        "pending" => {}
        "answered" if g.answer.as_ref() == Some(answer) => return Ok(()),
        _ => return Err("lead/gate_closed".into()),
    }
    // 选项必须是这个关卡的：A 只有 confirm/revise，C 只有 accept/reject，B 是 agent 写的方案 id
    // （做空审查第二轮 L5）。
    let valid = match g.kind.as_str() {
        "A" => ["confirm", "revise"].contains(&answer.choice.as_str()),
        "C" => ["accept", "reject"].contains(&answer.choice.as_str()),
        _ => g.options.iter().any(|o| o.id == answer.choice),
    };
    if !valid {
        return Err("lead/gate_bad_choice".into());
    }
    v["status"] = json!("answered");
    v["answer"] = serde_json::to_value(answer).map_err(|e| e.to_string())?;
    let tmp = task.join("gate.json.tmp");
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// 会话 cwd 所在项目的任务快照；没有任务返回 None。只读 `.lead` 下约定的文件。
pub fn snapshot(cwd: &str) -> Option<LeadTaskDto> {
    snapshot_for(cwd, None)
}

/// 任务归属的会话（`.owner` 里的 cc_session_id）；还没认领为空串。
fn owner_of(task: &Path) -> String {
    read(&task.join(".owner")).trim().to_owned()
}

/// 这个任务给 session 看吗：没认领的谁都看得到；认领了只给认领它的会话。session 为 None = 不过滤。
fn visible_to(task: &Path, session: Option<&str>) -> bool {
    let owner = owner_of(task);
    match session {
        Some(sid) => owner.is_empty() || owner == sid,
        None => true,
    }
}

/// 同 snapshot，但只给任务归属的会话看（用户反馈 2026-11-09：同一项目的另一个会话里也冒出了别的会话的
/// Lead 入口和关卡卡片）。没认领的任务（还在访谈、写规格）照旧给项目里所有会话看。
pub fn snapshot_for(cwd: &str, session: Option<&str>) -> Option<LeadTaskDto> {
    let task = find_task(Some(cwd)).filter(|task| visible_to(task, session))?;
    let checks = acceptance(&task)
        .unwrap_or_default()
        .iter()
        .map(|i| LeadCheckDto {
            id: i.get("id").map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default(),
            check: i.get("check").and_then(Value::as_str).unwrap_or("").to_owned(),
            passes: i.get("passes") == Some(&Value::Bool(true)),
        })
        .collect();
    Some(LeadTaskDto {
        slug: task.file_name()?.to_string_lossy().into_owned(),
        phase: phase_of(&task),
        spec: truncate(&read(&task.join("SPEC.md")), 12000),
        checks,
        decisions: tail(&read(&task.join("decisions.md")), 6000),
        progress: tail(&read(&task.join("progress.md")), 6000),
        gate: read_gate(&task).filter(|g| g.status != "consumed").or_else(|| {
            project_gates(cwd)
                .into_iter()
                .filter(|(t, _)| visible_to(t, session))
                .map(|(_, g)| g)
                .find(|g| g.status == "pending")
        }),
    })
}

/// 会话 cwd 所在项目的当前任务目录（`.lead/active` 指向的那个）。
pub fn task_dir(cwd: &str) -> Option<PathBuf> {
    find_task(Some(cwd))
}

/// 项目里**所有**任务目录下的关卡（不只 active）：新开一个任务会改写 active，但旧任务挂着的
/// 关卡还在等 leader，不能因此从卡片、看板、防休眠里消失（做空审查 M4）。
pub fn project_gates(cwd: &str) -> Vec<(PathBuf, LeadGateDto)> {
    let lead = project_root(Path::new(cwd)).join(".lead");
    let Ok(entries) = std::fs::read_dir(&lead) else { return vec![] };
    let mut out: Vec<(PathBuf, LeadGateDto)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| slug_ok(&n.to_string_lossy())))
        .filter_map(|t| read_gate(&t).map(|g| (t, g)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 按关卡 id 找它所在的任务目录（作答用）。
pub fn task_of_gate(cwd: &str, gate_id: &str) -> Option<PathBuf> {
    project_gates(cwd).into_iter().find(|(_, g)| g.id == gate_id).map(|(t, _)| t)
}

// ---------------- wait（asyncRewake 后台 hook） ----------------

/// 后台等待关卡答复（Stop / SessionStart 上的 asyncRewake hook）。
/// - 关卡已作答：立刻以退出码 2 结束，stderr 是答复 → 模型被唤醒（会话恢复后也走这里）。
/// - 关卡挂起：每 `poll` 看一次，直到作答（退出码 2）、被消费/撤掉/换了关卡（0）、
///   claude 进程没了（0）或超过 `max`（0）。
/// - 只唤醒关卡的归属会话：做空审查复现过，按工作目录兜底时，同项目里任何会话停下都会被唤醒，
///   无关会话会去执行别人任务的单向门决定。
/// - 等待者「后来者接管」：`.waiter` 记最新等待者的 `pid:关卡id`，旧等待者看到被换掉就自己退出。
///   不能靠「父进程还活着」判断：Windows 上 hook 的父进程是 bash 存根而不是 claude，claude 退出后
///   旧等待者会一直活着，挡住恢复后的新会话（做空审查复现过）。
pub fn wait(data: &Value, registry: &Path, poll: std::time::Duration, max: std::time::Duration) -> Output {
    let sid = data.get("session_id").and_then(Value::as_str).unwrap_or("");
    let Some(task) = task_of_session(sid, registry) else { return Output::default() };
    let Some(first) = read_gate(&task) else { return Output::default() };
    // 每个关卡的答复只唤醒一次（`.delivered` 记关卡 id）。真机实测踩过：agent 醒来后没把关卡标成
    // consumed，下一次 Stop 的 wait 又看到 answered，再唤醒一次——agent 若一直不消费就成了死循环。
    // 例外是 SessionStart：那是新起的进程（例如崩溃后恢复），之前的唤醒可能没被处理完，再送一次。
    let delivered = task.join(".delivered");
    let on_start = data.get("hook_event_name").and_then(Value::as_str) == Some("SessionStart");
    let wake = |g: &LeadGateDto| {
        let _ = std::fs::write(&delivered, &g.id);
        Output { stderr: Some(answer_message(g)), exit: 2, ..Output::default() }
    };
    match first.status.as_str() {
        "answered" if on_start || read(&delivered).trim() != first.id => return wake(&first),
        "pending" => {}
        _ => return Output::default(),
    }
    let me = std::process::id();
    let waiter = task.join(".waiter");
    let mine = format!("{me}:{}", first.id);
    let _ = std::fs::write(&waiter, &mine); // 接管：之前的等待者下一轮看到不是自己就退出
    let start = std::time::Instant::now();
    let mut misses = 0;
    let out = loop {
        std::thread::sleep(poll);
        if read(&waiter).trim() != mine || start.elapsed() > max {
            break Output::default(); // 被新等待者接管，或超时
        }
        match read_gate(&task) {
            Some(g) if g.id == first.id && g.status == "answered" => break wake(&g),
            Some(g) if g.id == first.id && g.status == "pending" => misses = 0,
            Some(_) => break Output::default(), // 换了关卡或已消费
            // 读失败（作答时的 rename、杀毒软件占用）偶发一次不算数，连续多次才当作关卡被撤掉
            None => {
                misses += 1;
                if misses >= 5 {
                    break Output::default();
                }
            }
        }
    };
    if read(&waiter).trim() == mine {
        let _ = std::fs::remove_file(&waiter);
    }
    out
}

#[cfg(test)]
mod tests;
