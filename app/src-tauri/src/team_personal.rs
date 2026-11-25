//! 团队设置子页：读写个人层 `<Meowo 数据目录>/team/config.json`。
//!
//! 界面和文件是同一份数据（leader 2026-11-25 讨论定的方向；Cursor 的教训是界面一套、文件一套，
//! 界面上看不到正在生效的规则）。所以：
//! - 只改界面管的字段（taste、protected、authority），其余字段（blocked_commands、用户自己加的键、
//!   「_说明」）原样保留；
//! - 文件写坏了不覆盖，报原因让用户去修；
//! - 界面读到的版本和保存时磁盘上的版本不一样（用户在编辑器里改过），不覆盖，让界面重新载入；
//! - 用户写的条目一条不藏、一条不删：界面显示的就是文件里的，保存写回的也就是这些（做空 redteam-1 问题 1）；
//! - 按文件原来的键顺序写回：「_说明」键紧挨着它说明的字段，界面保存一次不能把手改的排版打乱（问题 4）。
//!
//! 个人层「Meowo 从不写」（团队规则 R2）指的是升级和刷新不碰它；这里是用户在界面上亲手保存，等同用户自己写。

use meowo_protocol::ipc::{SaveTeamPersonalConfigArgs, TeamPersonalConfigDto};
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// 硬底线：永远「动手前先问」，个人层和契约都解不开（团队规则 R6）。不存进 config.json，界面锁定展示，
/// 角色按 team-staff 技能总是带上。
///
/// 这里只认「就是在说某条底线」的写法：条目以底线的说法开头（模板里的「花钱：开机器、买资源」、手改的
/// 「删除数据（清理测试库）」）。不按 production、secret、发消息这类词在句中出现来认：「读 production 日志排查」
/// 「给客户发消息前先给我看草稿」是用户自己的普通规则，误认成底线会被标成「不生效」（做空 redteam-1 问题 2）。
/// 认出来也只是提示，界面照常能换档、能删，文件里的原文不动。
const FLOOR_PREFIXES: &[&str] = &[
    "上生产", "部署到生产", "发布到生产", "花钱", "改生产配置", "改密钥", "对外发消息", "删除数据", "删数据",
    "ship to production", "deploy to production", "deploy to prod", "spend money", "change production config", "send messages out", "delete data",
];

/// 文件里没写三档时的默认值，和团队模板 contract.json 一致（team-staff 技能「缺某一档就用模板默认值」）。
const DEFAULT_DO_AND_REPORT: &[&str] = &[
    "范围内的代码修改、测试、构建、本地提交",
    "在任务范围内顺手修小问题（列进结尾卡片）",
    "清理已合并的 worktree 和本地分支",
    "可撤回的取舍：布局、配色、交互、文案、默认值、实现方式（列进结尾卡片的「我替你定了的事」）",
];
const DEFAULT_ASK_AT_END: &[&str] = &["推 beta / 部署到测试环境（改动攒着，结尾一起问）", "合并到主分支", "发版"];

const TASTE_TEMPLATE: &str = "# 我的品味清单\n\n团队的设计师、产品经理和开发会读这份清单。写你在意的、和常见做法不一样的地方，一条一句，最好带一个例子。\n团队被你纠正时，team-learn 会往这里追加。\n\n## 产品\n\n## 交互和视觉\n\n## 工程\n";

fn personal_dir() -> Result<PathBuf, String> {
    meowo_reporter::team_personal_dir().ok_or_else(|| "lead/claude_missing".into())
}

/// 文件内容的指纹：界面拿着它来保存，磁盘上的内容变了就对不上。不存在时是空串。
fn token_of(text: Option<&str>) -> String {
    match text {
        None => String::new(),
        Some(t) => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.hash(&mut h);
            format!("{:016x}-{}", h.finish(), t.len())
        }
    }
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("team/config_unreadable：{e}")),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// 这一条是不是在说某条底线：以底线说法开头，且紧跟着的是结尾、标点或空格（「删除数据（清理测试库）」
/// 「花钱：开机器」算；「删除数据库表前先导出备份」「上生产前先跑冒烟」不算，做空 redteam-3 R3-2）。
/// 界面用 DTO 里的 floor_prefixes 做同样的判断（TeamSection 的 floorLike）。
#[cfg(test)]
fn is_floor_like(item: &str) -> bool {
    let low = item.trim().to_lowercase();
    FLOOR_PREFIXES.iter().any(|p| low.strip_prefix(p).is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric())))
}

/// 按原键序读：IndexMap 记住文件里的顺序，写回时已有的键留在原位，新键追加在后面。
fn parse_ordered(text: &str) -> Option<IndexMap<String, Value>> {
    serde_json::from_str::<IndexMap<String, Value>>(text.trim_start_matches('\u{feff}')).ok()
}

/// serde_json 的 Map 不保序；authority 按原文顺序单独读一遍，写回时用它。
fn ordered_authority(text: &str) -> IndexMap<String, Value> {
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(default)]
        authority: Option<IndexMap<String, Value>>,
    }
    serde_json::from_str::<Probe>(text.trim_start_matches('\u{feff}')).ok().and_then(|p| p.authority).unwrap_or_default()
}

pub(crate) fn load_from(dir: &Path) -> Result<TeamPersonalConfigDto, String> {
    let path = dir.join("config.json");
    let text = read_text(&path)?;
    let mut dto = TeamPersonalConfigDto {
        personal_dir: dir.to_string_lossy().into_owned(),
        config_path: path.to_string_lossy().into_owned(),
        exists: text.is_some(),
        error: None,
        error_line: None,
        error_first_line: 0,
        error_lines: Vec::new(),
        authority_defaulted: false,
        token: token_of(text.as_deref()),
        taste: None,
        taste_exists: false,
        protected: Vec::new(),
        do_and_report: Vec::new(),
        ask_at_end: Vec::new(),
        ask_first: Vec::new(),
        floor_prefixes: FLOOR_PREFIXES.iter().map(|p| (*p).to_owned()).collect(),
        blocked_command_count: 0,
    };
    let Some(text) = text else {
        dto.do_and_report = DEFAULT_DO_AND_REPORT.iter().map(|s| (*s).to_owned()).collect();
        dto.ask_at_end = DEFAULT_ASK_AT_END.iter().map(|s| (*s).to_owned()).collect();
        dto.authority_defaulted = true;
        return Ok(dto);
    };
    // 守卫和界面用同一个解析判断「写坏了」：两边说法不能不一样。
    if let Some(problem) = meowo_reporter::team_guard::Config::parse(&text).broken {
        dto.error = Some(problem);
        if let Err(e) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
            let line = e.line().max(1);
            let all: Vec<&str> = text.lines().collect();
            let first = line.saturating_sub(2).max(1);
            let last = (line + 1).min(all.len().max(1));
            dto.error_line = Some(line as u32);
            dto.error_first_line = first as u32;
            dto.error_lines = (first..=last).filter_map(|n| all.get(n - 1)).map(|s| (*s).to_owned()).collect();
        }
        return Ok(dto);
    }
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
    dto.taste = v.get("taste").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    dto.taste_exists = dto.taste.as_deref().is_some_and(|t| Path::new(t).is_file());
    dto.protected = strings(v.get("protected"));
    let auth = v.get("authority").filter(|a| a.is_object());
    dto.authority_defaulted = auth.is_none();
    let tier = |key: &str, default: &[&str]| match auth.and_then(|a| a.get(key)) {
        Some(list) => strings(Some(list)),
        None => default.iter().map(|s| (*s).to_owned()).collect(),
    };
    dto.do_and_report = tier("do_and_report", DEFAULT_DO_AND_REPORT);
    dto.ask_at_end = tier("ask_at_end", DEFAULT_ASK_AT_END);
    // 「动手前先问」里用户写的条目全部原样显示（包括和底线同义的那几条），保存时原样写回。
    dto.ask_first = tier("ask_first", &[]);
    dto.blocked_command_count = v.get("blocked_commands").and_then(Value::as_array).map_or(0, |a| a.len() as u32);
    Ok(dto)
}

pub(crate) fn save_to(dir: &Path, args: &SaveTeamPersonalConfigArgs) -> Result<TeamPersonalConfigDto, String> {
    let path = dir.join("config.json");
    let current = read_text(&path)?;
    if token_of(current.as_deref()) != args.token {
        return Err("team/config_changed".into());
    }
    let mut root = match &current {
        None => {
            let mut m = IndexMap::new();
            m.insert("_说明".to_owned(), json!("团队个人层设置。Meowo 设置页的「团队」子页和这个文件是同一份数据，两边改哪边都行。"));
            m
        }
        Some(text) => {
            // 写坏（含字段类型不对）不覆盖：和守卫、读取同一个判断。
            if meowo_reporter::team_guard::Config::parse(text).broken.is_some() {
                return Err("team/config_broken".into());
            }
            parse_ordered(text).ok_or("team/config_broken")?
        }
    };
    let clean = |items: &[String]| -> Value {
        let mut seen = Vec::new();
        for i in items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if !seen.iter().any(|s: &String| s == i) {
                seen.push(i.to_owned());
            }
        }
        json!(seen)
    };
    match args.taste.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => {
            root.insert("taste".to_owned(), json!(t));
        }
        None => {
            root.shift_remove("taste");
        }
    }
    root.insert("protected".to_owned(), clean(&args.protected));
    let mut authority = current.as_deref().map(ordered_authority).unwrap_or_default();
    authority.insert("do_and_report".to_owned(), clean(&args.do_and_report));
    authority.insert("ask_at_end".to_owned(), clean(&args.ask_at_end));
    authority.insert("ask_first".to_owned(), clean(&args.ask_first));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = to_pretty_ordered(&root, &authority)? + "\n";
    meowo_agent::fsutil::write_atomic(&path, &text).map_err(|e| e.to_string())?;
    load_from(dir)
}

/// serde_json 的对象不保序，所以顶层和 authority 都用 IndexMap 输出：顶层按原键序，authority 放回它原来的位置
/// （文件里没有就追加在最后），authority 里也按原键序。
fn to_pretty_ordered(root: &IndexMap<String, Value>, authority: &IndexMap<String, Value>) -> Result<String, String> {
    #[derive(serde::Serialize)]
    #[serde(untagged)]
    enum Node<'a> {
        Map(&'a IndexMap<String, Value>),
        Plain(&'a Value),
    }
    let mut out: IndexMap<&str, Node> = IndexMap::new();
    for (k, v) in root {
        if k == "authority" {
            out.insert("authority", Node::Map(authority));
        } else {
            out.insert(k.as_str(), Node::Plain(v));
        }
    }
    if !out.contains_key("authority") {
        out.insert("authority", Node::Map(authority));
    }
    serde_json::to_string_pretty(&out).map_err(|e| e.to_string())
}

/// 个人层里新建一份品味清单（已有同名文件就直接用，不覆盖），并把 config.json 的 taste 指过去。
pub(crate) fn create_taste_in(dir: &Path, token: &str) -> Result<TeamPersonalConfigDto, String> {
    let taste = dir.join("taste.md");
    // 先核对 config.json，通过了再写 taste.md：失败时个人层里不留多余的文件（做空 redteam-2 N5）。
    let mut now = load_from(dir)?;
    if now.error.is_some() {
        return Err("team/config_broken".into());
    }
    if now.token != token {
        return Err("team/config_changed".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !taste.exists() {
        meowo_agent::fsutil::write_atomic(&taste, TASTE_TEMPLATE).map_err(|e| e.to_string())?;
    }
    now.taste = Some(taste.to_string_lossy().into_owned());
    save_to(
        dir,
        &SaveTeamPersonalConfigArgs {
            token: now.token.clone(),
            taste: now.taste.clone(),
            protected: now.protected,
            do_and_report: now.do_and_report,
            ask_at_end: now.ask_at_end,
            ask_first: now.ask_first,
        },
    )
}

#[tauri::command]
pub(crate) async fn team_personal_config() -> Result<TeamPersonalConfigDto, String> {
    tauri::async_runtime::spawn_blocking(|| load_from(&personal_dir()?)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn save_team_personal_config(args: SaveTeamPersonalConfigArgs) -> Result<TeamPersonalConfigDto, String> {
    tauri::async_runtime::spawn_blocking(move || save_to(&personal_dir()?, &args)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn create_team_taste(token: String) -> Result<TeamPersonalConfigDto, String> {
    tauri::async_runtime::spawn_blocking(move || create_taste_in(&personal_dir()?, &token)).await.map_err(|e| e.to_string())?
}

/// 打开个人层里的东西：`folder` 在文件管理器里打开个人层，`config` 用默认程序打开 config.json，
/// `taste` 打开品味清单。品味清单的路径来自用户配置，可能指向任何地方：只用默认程序打开文本类文件，
/// 别的（包括可执行文件）只在文件管理器里定位，不运行。
#[tauri::command]
pub(crate) async fn open_team_personal(target: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = personal_dir()?;
        match target.as_str() {
            "folder" => {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                crate::fsutil::open_with_default_app(&dir)
            }
            "config" => {
                let p = dir.join("config.json");
                if p.is_file() { crate::fsutil::open_with_default_app(&p) } else { crate::fsutil::open_with_default_app(&dir) }
            }
            "taste" => {
                let dto = load_from(&dir)?;
                let p = PathBuf::from(dto.taste.ok_or("team/taste_missing")?);
                if !p.is_file() {
                    return Err("team/taste_missing".into());
                }
                let text_like = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt"));
                if text_like { crate::fsutil::open_with_default_app(&p) } else { crate::fsutil::reveal_target(&p) }
            }
            _ => Err("team/open_target_invalid".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
