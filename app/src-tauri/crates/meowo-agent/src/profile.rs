//! 多账号（profile）：**一个 profile = 一个私有目录 + 启动该 agent 时注入的一组环境变量**。
//!
//! # 为什么是「目录隔离」而不是「轮换凭据」
//!
//! 直觉上更简单的做法是把选中账号的凭据写进 agent 真实的凭据位置（切换 = 换文件内容）。它有一个
//! **没法绕开**的冲突：agent 会用 refresh_token 换新 token 并**写回**凭据文件。你切到账号 B 之后，
//! 一个还在跑的账号 A 的会话刷新了 token，就会把 A 的凭据写回去——直接覆盖掉你刚切过去的 B。
//! 这不是理论风险，是 OAuth agent 的常规行为。
//!
//! 目录隔离没有这个问题：每个 profile 各写各的凭据与配置，谁也覆盖不了谁。Agent 自己通常也会
//! 把会话历史放进该目录；Meowo 对**取证确认过会话文件协议**的 provider（claude / codex / kimi，
//! 见各插件的 `CROSS_ACCOUNT` 与那里的实测记录）会在恢复前只同步这一个 session 的会话数据，
//! 使用户能用当前账号继续旧会话，同时绝不复制凭据。opencode 尚无可靠协议（会话存储没取证过），
//! 仍保持各 profile 独立。
//!
//! # 默认 profile 不注入任何东西
//!
//! 「默认账号」就是 agent 自己的目录（`~/.claude`），**不注入环境变量**。于是现有用户零感知：
//! 不建新 profile，一切与从前一模一样。
//!
//! # 各家的隔离变量（全部实测）
//!
//! | agent | 变量 | 备注 |
//! |---|---|---|
//! | claude | `CLAUDE_CONFIG_DIR` | 一个变量搞定 |
//! | codex | `CODEX_HOME` | 同上 |
//! | kimi | `KIMI_CODE_HOME` | 同上。**不是** `KIMI_SHARE_DIR`——那在 kimi-code 里是「旧版 kimi-cli 的迁移来源」，设了它等于没隔离（实测出处见 `plugins/kimi` 的 `PROFILE`） |
//! | opencode | `OPENCODE_CONFIG_DIR` + `XDG_DATA_HOME` | **要两个**，见下 |
//! | gemini | —— | **不支持**：`GEMINI_DIR` 实测无效（设了照样读 `~/.gemini`） |
//!
//! opencode 需要两个变量，是因为它把**配置**与**数据**分了家：插件读配置目录
//! （`~/.config/opencode`），凭据却写数据目录（`~/.local/share/opencode/auth.json`）。只设一个的话，
//! 另一半仍然共用——账号根本没隔离开，而这种「看起来隔离了、其实没有」是最坏的一种失败。

use std::path::{Path, PathBuf};

/// **跨账号会话迁移**规格。声明它 = 该 agent 的会话可以搬到当前活跃账号下继续：
/// 恢复前把 session 级数据同步过去，因此恢复用的是**活跃账号**而非会话原先所属账号。
///
/// 不声明（默认）= 不支持，恢复沿用会话记录的原账号。这不是「还没做」而是安全默认：
/// 迁移要求该 agent 的会话数据可整体复制且换目录后仍能被它自己认出，未取证就搬运
/// 只会造出一个 agent 读不了的半份副本。
///
/// 字段描述的是「会话级数据散落在数据根的哪些地方」，宿主据此复制，不必知道任何
/// 具体 agent 的目录名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrossAccountSession {
    /// 正文文件在数据根**之下隔了几级**（＝从正文上溯几次 `parent()` 回到数据根）。
    ///
    /// 宿主只按这个层数还原数据根，再把正文**按它相对数据根的原路径**放到目标账号下——
    /// 目录名、日期分层、文件名前缀一概不必让宿主知道。实测出处见各插件的声明：
    ///
    /// | agent | 正文路径 | 层数 |
    /// |---|---|---|
    /// | claude | `projects/<项目>/<id>.jsonl` | 3 |
    /// | codex | `sessions/<年>/<月>/<日>/rollout-<时刻>-<id>.jsonl` | 5 |
    /// | kimi | `sessions/<wd_工作区>/<session-id>/agents/main/wire.jsonl` | 6 |
    ///
    /// 写错的后果是数据根落在中间目录上：`session_buckets` 从错误位置复制，且「源与目标
    /// 同根」的短路判断永不成立，同账号内每次恢复都全量重拷一遍。
    pub transcript_depth: usize,
    /// 「整棵都属于这个会话」的目录在正文的**第几级祖先**上；`0` = 没有这种目录，
    /// 只搬正文文件本身（claude/codex）。
    ///
    /// kimi 是 `3`：正文 `…/<session-id>/agents/main/wire.jsonl` 只是会话目录里的一个文件，
    /// 旁边还有 blobs、侧车等，单搬正文会造出一个它自己读不全的半份副本。必须 <
    /// [`transcript_depth`](Self::transcript_depth)——否则「会话目录」会跑到数据根之外，
    /// 那等于把整个账号搬过去。
    pub session_dir_up: usize,
    /// 其余按 session id 分目录保存的数据桶（相对数据根）：`<桶>/<session-id>/…`。
    pub session_buckets: &'static [&'static str],
    /// 子 agent 数据是否放在「与正文同名的同级目录」（`<项目>/<session-id>/`）。
    pub subagents_beside_transcript: bool,
}

/// 某 agent 的 profile 隔离规格。声明式，加/改 agent 只动 `plugins/`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileSpec {
    /// 启动该 agent 时注入的环境变量：`(变量名, 相对 profile 根的子路径)`。
    /// 空子路径 = profile 根本身。
    ///
    /// **首条必须指向承载 hooks 的那个目录**（即 [`data_rel`](Self::data_rel) 所指），
    /// 下方有绊线测试盯着这条约定。
    pub envs: &'static [(&'static str, &'static str)],
    /// 承载 hooks 的目录（＝该 agent 的 `data_dir`），相对 profile 根。
    pub data_rel: &'static str,
    /// 凭据文件，相对 profile 根。
    ///
    /// 刻意**不复用** [`crate::auth::CredentialSource`]：那个描述的是「默认安装」下凭据在哪
    /// （opencode 的是相对 home 的 `~/.local/share/opencode/auth.json`），而 profile 模式下整个
    /// 数据目录都被搬走了，那条路径不再成立。两者描述的是不同世界，硬要合并只会拼出错误的路径。
    pub creds_rel: &'static str,
}

impl ProfileSpec {
    /// 该 profile 要注入给 agent 进程的环境变量（绝对路径）。
    pub fn env_for(&self, root: &Path) -> Vec<(String, String)> {
        self.envs
            .iter()
            .map(|(key, rel)| {
                (
                    (*key).to_string(),
                    crate::join_rel(root, rel).to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    /// 该 profile 需要**预先建出**的目录——每个环境变量各指一处（opencode 是两处）。
    ///
    /// 得先于 agent 的第一次启动建好：接线要往里写 hooks，而 hooks 必须在会话开始前就位。
    pub fn dirs(&self, root: &Path) -> Vec<PathBuf> {
        self.envs
            .iter()
            .map(|(_, rel)| crate::join_rel(root, rel))
            .collect()
    }

    /// 该 profile 的数据目录（hooks 落在这里）。
    pub fn data_dir(&self, root: &Path) -> PathBuf {
        crate::join_rel(root, self.data_rel)
    }

    /// 该 profile 的凭据文件。
    pub fn credentials(&self, root: &Path) -> PathBuf {
        crate::join_rel(root, self.creds_rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::all;

    fn root() -> PathBuf {
        PathBuf::from("/p/root")
    }

    /// 约定：`envs` 的**首条**必须指向 `data_rel`。接线走 data_dir，而 agent 进程按 envs 找目录——
    /// 两者一旦指向不同的地方，hooks 会被写进一个 agent 根本不读的目录里：接线「成功」了，
    /// 会话却永远不上板，且没有任何报错。
    #[test]
    fn first_env_must_point_at_the_data_dir() {
        for p in all() {
            let Some(spec) = p.profile() else { continue };
            let (_, first_rel) = spec.envs.first().unwrap_or_else(|| {
                panic!(
                    "{} 声明了 profile 却没有任何环境变量——那就隔离不了任何东西",
                    p.id()
                )
            });
            assert_eq!(
                *first_rel,
                spec.data_rel,
                "{} 的首个环境变量没指向 data_rel：hooks 会被写进 agent 不读的目录",
                p.id()
            );
        }
    }

    /// 约定：`envs` 的首条变量名必须**等于**该 agent 首个变体的 `DataDirSpec.env`。
    ///
    /// 两处说的是同一件事的两侧：profile 用它把 agent 的数据根搬进 profile 目录，检测侧
    /// （reporter 作为 agent 的 hook 子进程继承这套环境）用它反解出数据根。名字一旦对不上，
    /// 就会出现「注入的变量 agent 根本不读」——账号看起来切了，实际所有 profile 共用同一个
    /// 默认目录，凭据互相覆盖、会话历史混在一起，而且全程零报错。
    ///
    /// kimi 正是这么翻车的：注入的 `KIMI_SHARE_DIR` 在 kimi-code 0.40.1 里是「旧版 kimi-cli 的
    /// 迁移来源」，数据根另有其人（`KIMI_CODE_HOME`）。见 `plugins/kimi` 的 `PROFILE` 注释。
    ///
    /// profile 恒取 `variants().first()`（见 `AgentPlugin::installation_for_profile`），故只钉首个变体。
    #[test]
    fn profile_env_must_be_the_variant_data_dir_env() {
        for p in all() {
            let Some(spec) = p.profile() else { continue };
            let variant = p.variants().first().expect("声明了 profile 必有变体");
            let (first_key, _) = spec.envs.first().expect("声明了 profile 必有变量");
            assert_eq!(
                Some(*first_key),
                variant.data_dir.env,
                "{} 注入的数据目录变量与检测侧读的不是同一个——账号会静默共用同一个目录",
                p.id()
            );
        }
    }

    /// 会话目录必须仍在数据根**之下**（`session_dir_up < transcript_depth`）。
    ///
    /// 写反了就是把「会话目录」指到数据根本身甚至它的上层——搬迁会把整个账号（含凭据）
    /// 拷进另一个账号，两个账号当场合并。这条不能靠人眼盯，故设绊线。
    #[test]
    fn session_dir_must_stay_below_the_data_root() {
        for p in all() {
            let Some(spec) = p.cross_account_session() else {
                continue;
            };
            assert!(
                spec.transcript_depth > 0,
                "{} 的 transcript_depth 为 0：数据根会落在正文文件自己身上",
                p.id()
            );
            assert!(
                spec.session_dir_up < spec.transcript_depth,
                "{} 的会话目录跑到了数据根之上——搬迁会把整个账号连凭据一起拷过去",
                p.id()
            );
        }
    }

    /// 跨账号迁移的覆盖面**钉成矩阵**：加一家就得连同它的实测记录一起加进来。
    /// 这不是「都该支持」，恰恰相反——没取证就不许声明（见本模块顶部的安全默认）。
    #[test]
    fn cross_account_migration_matches_the_declared_matrix() {
        let with: std::collections::BTreeSet<&str> = all()
            .iter()
            .filter(|p| p.cross_account_session().is_some())
            .map(|p| p.id().as_str())
            .collect();
        assert_eq!(
            with,
            ["claude", "codex", "kimi"]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            "跨账号会话迁移的覆盖面变了——新加的那家取证记录写进插件注释了吗？"
        );
    }

    /// 凭据必须落在 profile 根**底下**——跑到外面去就等于没隔离（几个 profile 共用同一份凭据）。
    #[test]
    fn credentials_stay_inside_the_profile_root() {
        for p in all() {
            let Some(spec) = p.profile() else { continue };
            let creds = spec.credentials(&root());
            assert!(
                creds.starts_with(root()),
                "{} 的凭据跑到了 profile 根之外（{}）——那几个账号会共用同一份凭据",
                p.id(),
                creds.display()
            );
            assert!(
                !spec.creds_rel.is_empty(),
                "{} 的 creds_rel 为空：凭据路径会退化成 profile 根目录本身",
                p.id()
            );
        }
    }

    /// opencode 必须隔离**两个**目录。只设 `OPENCODE_CONFIG_DIR` 的话，凭据（在数据目录里）仍然
    /// 共用——「看起来隔离了、其实没有」是这里最坏的失败模式，故单独钉死。
    #[test]
    fn opencode_isolates_both_config_and_data() {
        let spec = crate::by_id("opencode")
            .and_then(|p| p.profile())
            .expect("opencode 支持多账号");
        let keys: Vec<&str> = spec.envs.iter().map(|(k, _)| *k).collect();
        assert!(
            keys.contains(&"OPENCODE_CONFIG_DIR"),
            "配置目录（插件）没隔离"
        );
        assert!(
            keys.contains(&"XDG_DATA_HOME"),
            "数据目录（凭据）没隔离——账号看起来切了，其实共用同一份 auth.json"
        );

        // 两者必须指向不同的子目录。
        let env = spec.env_for(&root());
        let cfg = &env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG_DIR")
            .unwrap()
            .1;
        let data = &env.iter().find(|(k, _)| k == "XDG_DATA_HOME").unwrap().1;
        assert_ne!(cfg, data);

        // 凭据落在 XDG_DATA_HOME/opencode/auth.json —— opencode 自己就是这么拼的。
        assert_eq!(
            spec.credentials(&root()),
            crate::join_rel(&root(), "data/opencode/auth.json")
        );
    }

    /// gemini 不支持多账号——`GEMINI_DIR` 实测无效（设了它，gemini 照样读 `~/.gemini`）。
    /// 谎称支持的后果是：切了账号，两个 profile 却仍在共用同一份凭据。
    #[test]
    fn gemini_declares_no_profile_support() {
        assert!(
            crate::by_id("gemini").unwrap().profile().is_none(),
            "gemini 的数据目录不可被环境变量覆盖，不能谎称支持多账号"
        );
    }

    #[test]
    fn env_for_resolves_absolute_paths_under_the_root() {
        let spec = crate::by_id("claude").and_then(|p| p.profile()).unwrap();
        let env = spec.env_for(&root());
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        // 空子路径 → profile 根本身。
        assert_eq!(PathBuf::from(&env[0].1), root());
        assert_eq!(spec.data_dir(&root()), root());
    }
}
