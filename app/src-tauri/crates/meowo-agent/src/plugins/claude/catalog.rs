//! Claude Code 内嵌的模型目录：`/model` 菜单各家族「当前是哪一版」的权威来源。
//!
//! 为什么要读可执行文件：claude 的模型清单不在任何配置文件里（`~/.claude.json` 只缓存服务端
//! 额外下发的选项），而是打包进程序的 `model-catalog.json`——`latest_per_family` 给出每个家族的
//! 最新模型 id，`models[]` 里每项带 `display_name`。插件里写死的标签必随 CLI 改版过时（实拍：
//! 写着 Opus 5，CLI 早已是 Opus 5.5），从这里读就跟着用户装的那一版走。
//!
//! 形制（2.1.314 包内，压缩后的对象字面量，不是 JSON）：
//! `latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5-5",…}`，
//! `{id:"claude-opus-5-5",family:"opus",display_name:"Opus 5.5",…}`。认不出就返回空表，
//! 前端照旧用插件内置标签——宁可旧，不可错。
//!
//! 可执行文件约 300MB：分块扫描定位、只读附近窗口解析，结果按 (路径, 大小, 修改时间) 缓存，
//! CLI 升级换了文件才重读。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

const ANCHOR: &[u8] = b"latest_per_family:{";
/// 目录字面量里 models[] 在 latest_per_family 之前约 20KB（2.1.314 实测），留足余量。
const WINDOW_BEFORE: u64 = 512 * 1024;
const WINDOW_AFTER: u64 = 4 * 1024;
const CHUNK: usize = 8 * 1024 * 1024;

type CacheKey = (PathBuf, u64, Option<SystemTime>);
static CACHE: Mutex<Option<(CacheKey, Vec<crate::ModelCatalogEntry>)>> = Mutex::new(None);

/// 读取（带缓存）某个 claude 可执行文件里的家族目录：`[{id: "opus", label: "Opus 5.5"}, …]`。
pub(super) fn family_catalog(exe: &Path) -> Vec<crate::ModelCatalogEntry> {
    let Ok(meta) = std::fs::metadata(exe) else {
        return Vec::new();
    };
    let key: CacheKey = (exe.to_path_buf(), meta.len(), meta.modified().ok());
    if let Ok(cache) = CACHE.lock() {
        if let Some((cached_key, entries)) = cache.as_ref() {
            if *cached_key == key {
                return entries.clone();
            }
        }
    }
    let entries = scan(exe).map(|window| parse(&window)).unwrap_or_default();
    if let Ok(mut cache) = CACHE.lock() {
        *cache = Some((key, entries.clone()));
    }
    entries
}

/// 分块找锚点，返回锚点附近的窗口字节。
fn scan(exe: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(exe).ok()?;
    let finder = memchr::memmem::Finder::new(ANCHOR);
    let mut buf = vec![0u8; CHUNK + ANCHOR.len()];
    let mut carry = 0usize;
    let mut base = 0u64;
    let anchor_at = loop {
        let read = file.read(&mut buf[carry..]).ok()?;
        if read == 0 {
            return None;
        }
        let filled = carry + read;
        if let Some(pos) = finder.find(&buf[..filled]) {
            break base + pos as u64;
        }
        // 保留尾部 len-1 字节，跨块的锚点不漏。
        let keep = (ANCHOR.len() - 1).min(filled);
        buf.copy_within(filled - keep..filled, 0);
        base += (filled - keep) as u64;
        carry = keep;
    };
    let start = anchor_at.saturating_sub(WINDOW_BEFORE);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut window = Vec::new();
    file.take(anchor_at - start + WINDOW_AFTER)
        .read_to_end(&mut window)
        .ok()?;
    Some(window)
}

/// 从窗口字节里解析出各家族的最新模型显示名。
fn parse(window: &[u8]) -> Vec<crate::ModelCatalogEntry> {
    let Some(at) = memchr::memmem::rfind(window, ANCHOR) else {
        return Vec::new();
    };
    let body = &window[at + ANCHOR.len()..];
    let Some(end) = memchr::memchr(b'}', body) else {
        return Vec::new();
    };
    let Ok(pairs) = std::str::from_utf8(&body[..end]) else {
        return Vec::new();
    };
    pairs
        .split(',')
        .filter_map(|pair| {
            let (family, id) = pair.split_once(':')?;
            let family = family.trim().trim_matches('"');
            let id = id.trim().trim_matches('"');
            if family.is_empty() || id.is_empty() {
                return None;
            }
            let label = display_name(&window[..at], id)?;
            Some(crate::ModelCatalogEntry::new(family, label))
        })
        .collect()
}

/// models[] 里 `{id:"<id>",family:"…",display_name:"<名>"` 的显示名（取离锚点最近的那条）。
fn display_name(models: &[u8], id: &str) -> Option<String> {
    let needle = format!("{{id:\"{id}\",");
    let entry = memchr::memmem::rfind(models, needle.as_bytes())?;
    let rest = &models[entry..models.len().min(entry + 512)];
    let key = b"display_name:\"";
    let from = memchr::memmem::find(rest, key)? + key.len();
    let len = memchr::memchr(b'"', &rest[from..])?;
    let name = std::str::from_utf8(&rest[from..from + len]).ok()?;
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 样本取自 2.1.314 包内目录字面量（字段截短）。
    const SAMPLE: &[u8] = br#"models:[{id:"claude-opus-5",family:"opus",display_name:"Opus 5",provider_ids:{}},{id:"claude-opus-5-5",family:"opus",display_name:"Opus 5.5",provider_ids:{}},{id:"claude-fable-5-1",family:"fable",display_name:"Fable 5.1",x:1},{id:"claude-haiku-5-5",family:"haiku",display_name:"Haiku 5.5"}],defaults:{},best:"fable",latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5-5",haiku:"claude-haiku-5-5",sonnet:"claude-sonnet-5-5"},alias_migration:{}"#;

    #[test]
    fn parses_latest_display_name_per_family() {
        let entries = parse(SAMPLE);
        let labels: Vec<(&str, &str)> = entries.iter().map(|e| (e.id.as_str(), e.label.as_str())).collect();
        // sonnet 在 models[] 里找不到条目：宁缺勿猜，不给。
        assert_eq!(labels, vec![("fable", "Fable 5.1"), ("opus", "Opus 5.5"), ("haiku", "Haiku 5.5")]);
    }

    /// 真机探针（默认跳过）：读本机装的 claude 可执行，打印各家族显示名与耗时。
    /// `cargo test -p meowo-agent probe_installed_claude_catalog -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn probe_installed_claude_catalog() {
        let exe = crate::installation(crate::id::CLAUDE)
            .and_then(|inst| inst.launch)
            .and_then(|argv| argv.into_iter().next())
            .expect("未检测到 claude");
        let started = std::time::Instant::now();
        let entries = family_catalog(Path::new(&exe));
        println!("{exe}: {entries:?} in {:?}", started.elapsed());
        let started = std::time::Instant::now();
        let _ = family_catalog(Path::new(&exe));
        println!("cached in {:?}", started.elapsed());
        assert!(!entries.is_empty());
    }

    #[test]
    fn scan_finds_anchor_across_chunk_boundary() {
        let dir = std::env::temp_dir().join(format!("meowo-claude-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("claude.exe");
        // 锚点恰好跨过第一块的边界。
        let mut bytes = vec![b'x'; CHUNK - 5];
        bytes.extend_from_slice(SAMPLE);
        std::fs::write(&exe, &bytes).unwrap();
        let entries = family_catalog(&exe);
        assert!(entries.iter().any(|e| e.id == "opus" && e.label == "Opus 5.5"));
        assert!(family_catalog(&dir.join("missing.exe")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
