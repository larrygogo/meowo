//! 可执行解析。产出的是 **argv 而非单个路径**——codex 的 npm 全局安装必须走 `node <codex.js>`
//! 包装（直接拉原生 codex.exe 不会真正恢复会话：无 rollout、无 hook），单路径模型表达不了。
//!
//! 一律优先绝对路径：Meowo 拉起的终端 PATH 是 app 启动时的旧快照，未必含刚装好的 agent，
//! 且 agent 常是 shim/别名。全不中才回退裸名交给 PATH。

use std::path::{Path, PathBuf};

/// 候选路径的根。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// 用户 home。
    Home,
    /// 该 agent 的数据目录（如 codex 的 standalone 安装落在 `<data>/packages/...`）。
    DataDir,
    /// 环境变量指向的目录（如 npm 全局前缀的 `APPDATA`）。变量缺失/为空 → 该候选跳过。
    Env(&'static str),
}

/// 一个候选安装位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchCandidate {
    /// `<root>/<sub>/<stem>[.exe]` → argv = `[路径]`。
    Exe { root: Root, sub: &'static str },
    /// `node "<root>/<rel>"` → argv = `["node", 路径]`。npm 全局包的 JS 入口。
    NodeScript { root: Root, rel: &'static str },
    /// 可执行在 PATH 上 → argv = `[裸名]`（保持交给 PATH 解析，不固化成绝对路径）。
    OnPath,
}

/// 某变体的可执行查找规则，按优先级排列。
#[derive(Debug, Clone, Copy)]
pub struct LaunchSpec {
    /// 无扩展名的可执行名（Windows 自动补 `.exe`）。也是全不中时的回退裸名。
    pub stem: &'static str,
    pub candidates: &'static [LaunchCandidate],
}

impl LaunchSpec {
    /// 平台化的可执行文件名。
    pub fn file_name(&self) -> String {
        crate::exe_file_name(self.stem)
    }

    /// 逐候选找真实存在的可执行，返回启动 argv；全不中返回 None（调用方回退 `[stem]`）。
    pub fn probe(&self, data_dir: Option<&Path>, home: Option<&Path>) -> Option<Vec<String>> {
        self.candidates.iter().find_map(|c| self.probe_one(c, data_dir, home))
    }

    fn probe_one(&self, cand: &LaunchCandidate, data_dir: Option<&Path>, home: Option<&Path>) -> Option<Vec<String>> {
        match cand {
            LaunchCandidate::Exe { root, sub } => {
                let p = crate::join_rel(&resolve_root(*root, data_dir, home)?, sub).join(self.file_name());
                p.is_file().then(|| vec![path_string(&p)])
            }
            LaunchCandidate::NodeScript { root, rel } => {
                let p = crate::join_rel(&resolve_root(*root, data_dir, home)?, rel);
                p.is_file().then(|| vec!["node".to_string(), path_string(&p)])
            }
            // 纯查文件存在，不 spawn。命中也只回裸名——PATH 上的 agent 常是 shim，
            // 把 shim 的绝对路径固化下来反而可能绕过它做的环境准备。
            LaunchCandidate::OnPath => exe_on_path(&self.file_name()).then(|| vec![self.stem.to_string()]),
        }
    }
}

fn resolve_root(root: Root, data_dir: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    match root {
        Root::Home => home.map(Path::to_path_buf),
        Root::DataDir => data_dir.map(Path::to_path_buf),
        Root::Env(k) => {
            let v = std::env::var(k).ok()?;
            (!v.is_empty()).then(|| PathBuf::from(v))
        }
    }
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// 可执行 `name`（Windows 传含 `.exe` 的名）是否能在 PATH 各目录找到。纯查文件存在，不 spawn。
pub fn exe_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("meowo-launch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn probes_candidates_in_declared_order() {
        let home = tmp("order");
        let data = home.join(".agent");
        std::fs::create_dir_all(&data).unwrap();

        static CANDS: [LaunchCandidate; 2] = [
            LaunchCandidate::Exe { root: Root::DataDir, sub: "bin" },
            LaunchCandidate::Exe { root: Root::Home, sub: ".local/bin" },
        ];
        let spec = LaunchSpec { stem: "demo", candidates: &CANDS };
        let name = spec.file_name();

        // 都不存在 → None（调用方回退裸名）。
        assert_eq!(spec.probe(Some(&data), Some(&home)), None);

        // 只有后位候选存在 → 用它。
        let local = home.join(".local").join("bin");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join(&name), b"").unwrap();
        assert_eq!(spec.probe(Some(&data), Some(&home)), Some(vec![path_string(&local.join(&name))]));

        // 首位候选出现 → 抢先。
        let bin = data.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(&name), b"").unwrap();
        assert_eq!(spec.probe(Some(&data), Some(&home)), Some(vec![path_string(&bin.join(&name))]));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn node_script_yields_node_prefixed_argv() {
        // codex 的 npm 全局形态：argv 必须是 ["node", "<...>/codex.js"]，不能是裸 exe。
        let home = tmp("npm");
        let key = "MEOWO_TEST_NPM_ROOT";
        let root = home.join("npm");
        let js = root.join("node_modules").join("@openai").join("codex").join("bin").join("codex.js");
        std::fs::create_dir_all(js.parent().unwrap()).unwrap();
        std::fs::write(&js, b"").unwrap();
        std::env::set_var(key, &root);

        static CANDS: [LaunchCandidate; 1] = [LaunchCandidate::NodeScript {
            root: Root::Env("MEOWO_TEST_NPM_ROOT"),
            rel: "node_modules/@openai/codex/bin/codex.js",
        }];
        let spec = LaunchSpec { stem: "codex", candidates: &CANDS };
        assert_eq!(spec.probe(None, Some(&home)), Some(vec!["node".to_string(), path_string(&js)]));

        // env 缺失 → 该候选跳过（而非 panic）。
        std::env::remove_var(key);
        assert_eq!(spec.probe(None, Some(&home)), None);

        let _ = std::fs::remove_dir_all(&home);
    }
}
