//! 仓库里的 worktree 配置：`.meowo/worktree.json`，没有时只读兼容 Cursor / Conductor 的配置。
//!
//! 格式见定案设计一节：
//! ```json
//! { "version": 1, "copy": [".env*", "!.env.example"], "setup": "bun install" 或
//!   { "windows": "...", "unix": "...", "default": "..." }, "archive": 同 setup,
//!   "timeoutSec": 900, "env": { "KEY": "v" } }
//! ```
//! 不用 deny_unknown_fields：以后加字段，旧版 Meowo 读新配置照样能复制和跑 setup。

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// 当前支持的配置版本。配置写了更高的版本时只复制文件、不跑脚本（脚本语义可能变了）。
pub(crate) const SUPPORTED_VERSION: u32 = 1;
pub(crate) const DEFAULT_TIMEOUT_SEC: u64 = 900;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum Script {
    One(String),
    /// Cursor 的写法：一行一条命令的数组。
    Lines(Vec<String>),
    Platform {
        #[serde(default)]
        windows: Option<Box<Script>>,
        #[serde(default)]
        unix: Option<Box<Script>>,
        #[serde(default)]
        default: Option<Box<Script>>,
    },
}

impl Script {
    /// 按平台取出要跑的脚本文本。空串当没配。
    pub(crate) fn resolve(&self, windows: bool) -> Option<String> {
        let text = match self {
            Script::One(s) => s.clone(),
            Script::Lines(lines) => lines.join("\n"),
            Script::Platform {
                windows: w,
                unix: u,
                default: d,
            } => {
                let pick = if windows { w } else { u };
                return pick
                    .as_ref()
                    .and_then(|s| s.resolve(windows))
                    .or_else(|| d.as_ref().and_then(|s| s.resolve(windows)));
            }
        };
        let trimmed = text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MeowoFile {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    copy: Option<Vec<String>>,
    #[serde(default)]
    setup: Option<Script>,
    #[serde(default)]
    archive: Option<Script>,
    #[serde(default)]
    timeout_sec: Option<u64>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Cursor：`.cursor/worktrees.json`（setup-worktree-windows / -unix / setup-worktree）。
#[derive(Debug, Default, Deserialize)]
struct CursorFile {
    #[serde(default, rename = "setup-worktree-windows")]
    windows: Option<Script>,
    #[serde(default, rename = "setup-worktree-unix")]
    unix: Option<Script>,
    #[serde(default, rename = "setup-worktree")]
    default: Option<Script>,
}

/// Conductor：`conductor.json` 的 scripts.setup / scripts.archive。
#[derive(Debug, Default, Deserialize)]
struct ConductorFile {
    #[serde(default)]
    scripts: ConductorScripts,
}
#[derive(Debug, Default, Deserialize)]
struct ConductorScripts {
    #[serde(default)]
    setup: Option<String>,
    #[serde(default)]
    archive: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Meowo,
    Cursor,
    Conductor,
}

impl Source {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Source::Meowo => "meowo",
            Source::Cursor => "cursor",
            Source::Conductor => "conductor",
        }
    }
}

/// 读出来的配置。`raw` 是配置文件原文，信任按它的 hash 记——改了一个字就要重新确认。
#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) source: Option<Source>,
    pub(crate) raw: String,
    pub(crate) version: u32,
    pub(crate) copy: Vec<String>,
    pub(crate) setup: Option<Script>,
    #[allow(dead_code)] // 归档时跑 archive 脚本属于 forge 车道（合并后清理），这里先读出来
    pub(crate) archive: Option<Script>,
    pub(crate) timeout_sec: u64,
    pub(crate) env: BTreeMap<String, String>,
    /// 配置文件存在但读不懂：只复制默认的 `.env*`，setup 标失败并把原因写进日志。
    pub(crate) invalid: Option<String>,
}

impl Config {
    fn empty() -> Self {
        Config {
            source: None,
            raw: String::new(),
            version: SUPPORTED_VERSION,
            copy: default_copy(),
            setup: None,
            archive: None,
            timeout_sec: DEFAULT_TIMEOUT_SEC,
            env: BTreeMap::new(),
            invalid: None,
        }
    }

    pub(crate) fn version_too_high(&self) -> bool {
        self.version > SUPPORTED_VERSION
    }

    pub(crate) fn hash(&self) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.raw.as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// 没有配置时默认只复制根目录的 `.env*`：这是绝大多数项目「新 worktree 跑不起来」的那几个文件。
fn default_copy() -> Vec<String> {
    vec!["/.env*".to_string()]
}

/// 读仓库根目录 `root` 里的配置。优先级：meowo > cursor > conductor，只读一份，只读不写。
pub(crate) fn load(root: &Path) -> Config {
    let mut config = load_primary(root);
    // `.worktreeinclude`（Claude Code 桌面版 / 社区约定）存在时并入 copy：一行一个 gitignore 式模式。
    if let Ok(text) = std::fs::read_to_string(root.join(".worktreeinclude")) {
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                config.copy.push(line.to_string());
            }
        }
    }
    config
}

fn load_primary(root: &Path) -> Config {
    let meowo = root.join(".meowo").join("worktree.json");
    if let Ok(raw) = std::fs::read_to_string(&meowo) {
        let mut config = Config {
            source: Some(Source::Meowo),
            raw: raw.clone(),
            ..Config::empty()
        };
        match serde_json::from_str::<MeowoFile>(strip_bom(&raw)) {
            Ok(file) => {
                config.version = file.version.unwrap_or(SUPPORTED_VERSION);
                if let Some(copy) = file.copy {
                    config.copy = copy;
                }
                config.setup = file.setup;
                config.archive = file.archive;
                config.timeout_sec = file
                    .timeout_sec
                    .filter(|s| *s > 0)
                    .unwrap_or(DEFAULT_TIMEOUT_SEC);
                config.env = file.env;
            }
            Err(e) => config.invalid = Some(e.to_string()),
        }
        return config;
    }
    let cursor = root.join(".cursor").join("worktrees.json");
    if let Ok(raw) = std::fs::read_to_string(&cursor) {
        let mut config = Config {
            source: Some(Source::Cursor),
            raw: raw.clone(),
            ..Config::empty()
        };
        match serde_json::from_str::<CursorFile>(strip_bom(&raw)) {
            Ok(file) => {
                config.setup = Some(Script::Platform {
                    windows: file.windows.map(Box::new),
                    unix: file.unix.map(Box::new),
                    default: file.default.map(Box::new),
                });
            }
            Err(e) => config.invalid = Some(e.to_string()),
        }
        return config;
    }
    let conductor = root.join("conductor.json");
    if let Ok(raw) = std::fs::read_to_string(&conductor) {
        let mut config = Config {
            source: Some(Source::Conductor),
            raw: raw.clone(),
            ..Config::empty()
        };
        match serde_json::from_str::<ConductorFile>(strip_bom(&raw)) {
            Ok(file) => {
                config.setup = file.scripts.setup.map(Script::One);
                config.archive = file.scripts.archive.map(Script::One);
            }
            Err(e) => config.invalid = Some(e.to_string()),
        }
        return config;
    }
    Config::empty()
}

/// Windows 记事本存的 UTF-8 带 BOM，serde_json 不认。
fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}
