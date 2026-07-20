//! 对话页能力：由**安装实况**组装，而不是一张写死的表。
//!
//! 「agent 的能力由装的是什么决定」在工程上分三层，可动态化程度不同：
//!
//! 1. **真正可发现的**：CLI 版本（宿主探测 `--version`）、用户自定义斜杠命令（文件系统：
//!    claude 的 `commands/`、codex 的 `prompts/`、gemini 的 `commands/*.toml`、opencode 的
//!    `command/`，含项目级目录）。本模块负责这层——装了新命令，下一次询问就反映。
//! 2. **版本相关但查询不到的**：内置命令表、`/model` 是否接受内联参数。五家 CLI 都没有
//!    「自述能力」的接口（没有任何 `list-commands`），这份知识只能由插件整理；但**选表**由
//!    安装实况驱动——变体粒度走 `Variant`，未来真出现版本分叉时 [`ChatUiContext::version`]
//!    已经在场，插件加分支即可、不必改接口。
//! 3. **本质是 meowo 的知识**（resume 参数、接线格式、安装地址）：CLI 永远不会提供，留在插件层。
//!
//! 与 transcript 同理：本模块只**读**文件系统，不写盘、不联网、不 spawn（探测版本的子进程
//! 由宿主负责，经 [`ChatUiContext`] 传入）。

use crate::registry::ModelPreset;
use serde::Serialize;
use std::path::Path;

/// 多家 CLI 共用的启动目录信任标题。即使某个 provider 当前版本尚无此功能，继承这些足够
/// 具体的句子也不会把普通欢迎页误判为交互提示，并能自然覆盖未来新增的同类安全检查。
pub const COMMON_STARTUP_ATTENTION_MARKERS: &[&str] = &[
    // Claude Code、Gemini CLI
    "do you trust the files in this folder",
    // Codex CLI
    "do you trust the contents of this directory",
];

/// 组装对话页能力时的输入，由宿主（meowo-app）构造。
#[derive(Debug, Default, Clone, Copy)]
pub struct ChatUiContext<'a> {
    /// 会话工作目录——发现**项目级**自定义命令用。None = 只发现用户级。
    pub cwd: Option<&'a Path>,
    /// 宿主探测到的已装 CLI 版本（`--version` 首行，探测失败为 None）。
    /// 当前没有按版本分叉的表；它在场是为了真出现版本差异时插件能分支，而不必改接口。
    pub version: Option<&'a str>,
    /// Agent 自己的会话 id（不是 Meowo DB id）。有它时插件可从该会话的 runtime 元数据
    /// 发现真实能力，例如 Claude transcript 的 `skill_listing`。
    pub session_id: Option<&'a str>,
}

/// 一条斜杠命令来自哪里。前端可据此区分展示（内置/用户级/项目级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SlashSource {
    Builtin,
    User,
    Project,
}

/// 一条斜杠命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SlashCommand {
    /// 含前导 `/`；子目录命令含命名空间（`/frontend:component`）。
    pub name: String,
    /// 自定义命令从文件头取（md 的 frontmatter `description:` / toml 的顶层 `description`）；
    /// 内置命令为 None——描述文案是翻译资产，前端 i18n 按名取。
    pub description: Option<String>,
    pub source: SlashSource,
}

impl SlashCommand {
    fn builtin(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            source: SlashSource::Builtin,
        }
    }

    /// Agent 在当前会话启动时自述的命令/skill。归为 builtin 是因为它不是用户在
    /// commands 目录创建的文件；description 直接采用 Agent 自己给出的说明。
    pub(crate) fn runtime(name: String, description: Option<String>) -> Self {
        Self {
            name,
            description,
            source: SlashSource::Builtin,
        }
    }
}

/// 一次模式切换需要写入 PTY 的输入。斜杠命令要提交回车，快捷键则只写原始字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModeInput {
    pub data: &'static str,
    pub submit: bool,
}

/// 可直接跳转到的模式值。一个选项允许由多次输入组成，例如 Kimi 回到 manual 需要同时
/// 关闭 `/yolo` 与 `/auto`，不能让前端理解 provider 私有语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModeOption {
    pub value: &'static str,
    pub inputs: &'static [ModeInput],
}

/// ChatWindow 中一个独立的模式维度。`cycle_input` 与 `options` 可任选其一或同时存在；
/// 只读维度无需声明 control，仍可由 transcript 状态展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModeControl {
    pub dimension: &'static str,
    pub cycle_input: Option<&'static str>,
    pub options: &'static [ModeOption],
}

/// 对话页能力总装的结果，原样下发前端。
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct ChatUi {
    pub slash_commands: Vec<SlashCommand>,
    pub model_presets: Vec<ModelPreset>,
    /// Provider 声明的模式交互能力。模式的真实当前值由 transcript 增量独立提供。
    pub mode_controls: Vec<ModeControl>,
    /// 框架默认值与 provider 补充的、启动/恢复期间必须由用户在终端里处理的提示文本片段。
    /// 宿主据此不能把
    /// “首屏已有输出”误当成 composer 已就绪，更不能把待发送消息写进交互式选择器。
    pub startup_attention_markers: Vec<&'static str>,
    /// 当前会话支持 runtime 命令自发现、但权威清单尚未写入 transcript。前端据此继续随
    /// transcript 增量重试；一旦为 false 就停止探测，避免稳态轮询反复扫描文件。
    pub runtime_commands_pending: bool,
    /// 探测到的 CLI 版本，原样回传（展示/排障）。
    pub version: Option<String>,
}

/// 自定义斜杠命令的发现规格——**这才是「命令由 agent 提供」字面成立的那部分**：
/// 用户在 agent 自己的目录里放了什么，补全就出什么。
#[derive(Debug, Clone, Copy)]
pub struct CustomCommandSpec {
    /// 用户级命令目录，相对该 agent 的**数据目录**（多账号时即 profile 的数据目录，天然隔离）。
    pub user_dir: Option<&'static str>,
    /// 项目级命令目录，相对会话 cwd。None = 该 agent 无项目级命令。
    pub project_dir: Option<&'static str>,
    /// 命令文件扩展名（不含点）。`md` 的描述取 frontmatter，`toml` 取顶层 `description` 键。
    pub ext: &'static str,
    /// 子目录的命名空间连接符（claude/gemini 都是 `:`：`commands/git/commit.*` → `/git:commit`）。
    /// None = **只收顶层文件**——用于嵌套语义未经验证的 agent，宁可少收也不编造名字。
    pub namespace_sep: Option<&'static str>,
}

/// 单个目录树的扫描上限。命令目录本该只有几十个文件；真被指到一棵大树（比如 cwd 误配到 home）
/// 时截断保护，宁可补全不满也不把 UI 查询变成全盘遍历。
const MAX_FILES: usize = 200;
const MAX_DEPTH: usize = 3;

impl CustomCommandSpec {
    /// 扫出全部自定义命令：用户级（agent 数据目录下）+ 项目级（cwd 下）。
    /// 目录不存在/不可读一律静默跳过——「没配过自定义命令」是常态，不是错误。
    pub fn discover(&self, data_dir: &Path, cwd: Option<&Path>) -> Vec<SlashCommand> {
        let mut out = Vec::new();
        if let Some(rel) = self.user_dir {
            self.scan(&crate::join_rel(data_dir, rel), SlashSource::User, &mut out);
        }
        if let (Some(rel), Some(cwd)) = (self.project_dir, cwd) {
            self.scan(&crate::join_rel(cwd, rel), SlashSource::Project, &mut out);
        }
        out
    }

    fn scan(&self, root: &Path, source: SlashSource, out: &mut Vec<SlashCommand>) {
        let mut budget = MAX_FILES;
        self.scan_dir(root, source, &mut Vec::new(), &mut budget, out);
    }

    fn scan_dir(
        &self,
        dir: &Path,
        source: SlashSource,
        prefix: &mut Vec<String>,
        budget: &mut usize,
        out: &mut Vec<SlashCommand>,
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        // 排序保证同一目录内容产出稳定的命令顺序（read_dir 的顺序是平台细节）。
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if *budget == 0 {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                if self.namespace_sep.is_some() && prefix.len() + 1 < MAX_DEPTH {
                    prefix.push(entry.file_name().to_string_lossy().into_owned());
                    self.scan_dir(&path, source, prefix, budget, out);
                    prefix.pop();
                }
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if path.extension().and_then(|e| e.to_str()) != Some(self.ext) {
                continue;
            }
            *budget -= 1;
            let name = match (self.namespace_sep, prefix.is_empty()) {
                (Some(sep), false) => format!("/{}{sep}{stem}", prefix.join(sep)),
                _ => format!("/{stem}"),
            };
            out.push(SlashCommand {
                name,
                description: read_description(&path, self.ext),
                source,
            });
        }
    }
}

/// 从命令文件头里取描述，纯 best-effort：取不到就 None，前端只是少一行说明文字。
fn read_description(path: &Path, ext: &str) -> Option<String> {
    // 描述在文件头部；整读上限防住把大文件塞进命令目录的意外。
    let text = read_head(path, 64 * 1024)?;
    let desc = match ext {
        "toml" => text
            .parse::<toml_edit::DocumentMut>()
            .ok()?
            .get("description")?
            .as_str()?
            .to_string(),
        // md：YAML frontmatter（首行 `---` 到下一个 `---`）里的 `description:`。
        _ => {
            let mut lines = text.lines();
            if lines.next()?.trim() != "---" {
                return None;
            }
            lines
                .take_while(|l| l.trim() != "---")
                .find_map(|l| l.strip_prefix("description:"))?
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        }
    };
    let desc = desc.trim();
    (!desc.is_empty()).then(|| desc.to_string())
}

fn read_head(path: &Path, max: u64) -> Option<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(max)
        .read_to_string(&mut buf)
        .ok()?;
    Some(buf)
}

/// 内置表 ∪ 发现的自定义命令：同名时**自定义覆盖内置**（与 CLI 自身的遮蔽语义一致，
/// 且自定义带着从文件里读出的描述），最后按名排序供补全稳定展示。
pub(crate) fn merge_commands(
    builtins: &[&str],
    custom: Vec<SlashCommand>,
) -> Vec<SlashCommand> {
    let mut out: Vec<SlashCommand> = builtins
        .iter()
        .filter(|b| !custom.iter().any(|c| c.name == **b))
        .map(|b| SlashCommand::builtin(b))
        .collect();
    out.extend(custom);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-chatui-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const MD_SPEC: CustomCommandSpec = CustomCommandSpec {
        user_dir: Some("commands"),
        project_dir: Some(".claude/commands"),
        ext: "md",
        namespace_sep: Some(":"),
    };

    #[test]
    fn discovers_user_and_project_commands_with_namespaces_and_descriptions() {
        let root = temp_root("md");
        let user = root.join("data").join("commands");
        fs::create_dir_all(user.join("git")).unwrap();
        fs::write(
            user.join("deploy.md"),
            "---\ndescription: \"部署到测试环境\"\n---\n正文",
        )
        .unwrap();
        // 子目录 → 命名空间；无 frontmatter → 无描述。
        fs::write(user.join("git").join("commit.md"), "正文而已").unwrap();
        // 扩展名不符的不收（README、临时文件混进目录是常态）。
        fs::write(user.join("README.txt"), "x").unwrap();
        let proj = root.join("repo").join(".claude").join("commands");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("review.md"), "---\ndescription: 走查清单\n---").unwrap();

        let got = MD_SPEC.discover(&root.join("data"), Some(&root.join("repo")));
        assert_eq!(
            got,
            vec![
                SlashCommand {
                    name: "/deploy".into(),
                    description: Some("部署到测试环境".into()),
                    source: SlashSource::User,
                },
                SlashCommand {
                    name: "/git:commit".into(),
                    description: None,
                    source: SlashSource::User,
                },
                SlashCommand {
                    name: "/review".into(),
                    description: Some("走查清单".into()),
                    source: SlashSource::Project,
                },
            ]
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn toml_description_and_flat_only_when_no_namespace_sep() {
        let root = temp_root("toml");
        let user = root.join("commands");
        fs::create_dir_all(user.join("nested")).unwrap();
        fs::write(
            user.join("commit.toml"),
            "description = \"生成提交信息\"\nprompt = \"...\"",
        )
        .unwrap();
        fs::write(user.join("nested").join("x.toml"), "prompt = \"...\"").unwrap();

        // namespace_sep = None：嵌套语义未验证的 agent 只收顶层，宁可少收也不编造名字。
        let flat = CustomCommandSpec {
            user_dir: Some("commands"),
            project_dir: None,
            ext: "toml",
            namespace_sep: None,
        };
        let got = flat.discover(&root, None);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "/commit");
        assert_eq!(got[0].description.as_deref(), Some("生成提交信息"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_dirs_are_silently_empty() {
        let root = temp_root("none");
        assert!(MD_SPEC.discover(&root.join("nope"), None).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_prefers_custom_over_builtin_and_sorts() {
        let custom = vec![SlashCommand {
            name: "/review".into(),
            description: Some("自定义走查".into()),
            source: SlashSource::Project,
        }];
        let merged = merge_commands(&["/review", "/clear"], custom);
        assert_eq!(
            merged.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["/clear", "/review"]
        );
        // 同名时自定义覆盖内置——与 CLI 自身的遮蔽语义一致。
        assert_eq!(merged[1].source, SlashSource::Project);
        assert_eq!(merged[1].description.as_deref(), Some("自定义走查"));
    }
}
