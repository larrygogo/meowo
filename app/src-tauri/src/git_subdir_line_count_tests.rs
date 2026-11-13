//! 会话开在仓库子目录时未跟踪文件的行数（UI 优化收尾 #1）。
//! status 给的路径相对仓库根，以前读盘时拼到会话 cwd 上：子目录会话里的未跟踪文件全部数不到行数，
//! 侧栏徽标与「更改」面板的 +N 都少算。

use super::*;

struct Repo(std::path::PathBuf);
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = git_output(dir.to_str().unwrap(), args).unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 仓库根有 root.txt（2 行），子目录 sub/ 已跟踪一个文件、另有未跟踪的 sub/new.txt（3 行）。
fn subdir_repo(tag: &str) -> Repo {
    let root =
        std::env::temp_dir().join(format!("meowo-subdir-lines-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("sub").join("kept.txt"), "k\n").unwrap();
    git(&root, &["add", "-A"]);
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );
    std::fs::write(root.join("root.txt"), "a\nb\n").unwrap();
    std::fs::write(root.join("sub").join("new.txt"), "x\ny\nz\n").unwrap();
    Repo(root)
}

/// 正斜杠（前端规范化后的形态）与本平台原生分隔符两种 cwd 都要数对。
fn cwd_forms(sub: &Path) -> Vec<String> {
    let native = sub.to_string_lossy().into_owned();
    let forward = native.replace('\\', "/");
    if native == forward {
        vec![native]
    } else {
        vec![native, forward]
    }
}

#[test]
fn sidebar_counts_untracked_lines_from_subdir_session() {
    let repo = subdir_repo("sidebar");
    for cwd in cwd_forms(&repo.0.join("sub")) {
        invalidate_caches();
        assert_eq!(
            sidebar_diff_counts_blocking(&cwd),
            Some((5, 0, 2)),
            "cwd={cwd}"
        );
    }
}

#[test]
fn summary_counts_untracked_lines_from_subdir_session() {
    let repo = subdir_repo("summary");
    for cwd in cwd_forms(&repo.0.join("sub")) {
        invalidate_caches();
        let files = git_diff_summary_blocking(&cwd).unwrap().files;
        let lines = |path: &str| {
            files
                .iter()
                .find(|f| f.path == path)
                .and_then(|f| f.additions)
        };
        assert_eq!(lines("root.txt"), Some(2), "cwd={cwd}");
        assert_eq!(lines("sub/new.txt"), Some(3), "cwd={cwd}");
    }
}

/// 做空第三轮 #9：点开未跟踪文件的 diff 与行数同一个毛病。面板退回按会话 cwd 列改动时，路径相对仓库根，
/// 以前拼在子目录上读不到（报「路径不存在」）。
#[test]
fn untracked_file_diff_reads_from_subdir_session() {
    let repo = subdir_repo("diff");
    for cwd in cwd_forms(&repo.0.join("sub")) {
        let diff = git_file_diff_blocking(&cwd, "sub/new.txt", true, None)
            .unwrap_or_else(|e| panic!("cwd={cwd}: {e}"));
        assert!(diff.diff.contains("+x\n+y\n+z"), "cwd={cwd}: {}", diff.diff);
        let root_file = git_file_diff_blocking(&cwd, "root.txt", true, None)
            .unwrap_or_else(|e| panic!("cwd={cwd}: {e}"));
        assert!(
            root_file.diff.contains("+a\n+b"),
            "cwd={cwd}: {}",
            root_file.diff
        );
        // 仍挡住爬出仓库根。
        assert!(git_file_diff_blocking(&cwd, "../../outside.txt", true, None).is_err());
    }
}
