//! 把原目录里被 git 忽略的本地文件（`.env*` 之类）复制进新 worktree。
//!
//! 只复制**被忽略**的文件：被跟踪的文件 worktree 里本来就有，未跟踪又没被忽略的是用户还没提交的
//! 半成品，不该被悄悄带进别的分支。模式用 gitignore 语法（`*` `?` `**` `[...]`、`!` 取反、
//! `/` 开头锚定根目录、`/` 结尾只认目录），后写的规则覆盖先写的。
//!
//! 列文件先用 `git ls-files -o -i --exclude-standard --directory` 把整个被忽略的目录收成一行
//! （设计时实测：全量列 11s，剪枝后 0.1s），只对模式确实要进去的目录再展开。
//! node_modules、target 一律不进：依赖交给 setup 重装，链接或复制都会把两个 worktree 绑在一起。

use std::path::Path;

/// 复制总量上限：超过的文件记成「没复制」，不中断建立。
pub(crate) const MAX_TOTAL_BYTES: u64 = 100 * 1024 * 1024;

/// 永不进入的目录名。
const SKIP_DIRS: &[&str] = &["node_modules", "target", ".git"];

#[derive(Debug, Clone)]
struct Pattern {
    negate: bool,
    anchored: bool,
    dir_only: bool,
    segments: Vec<String>,
}

fn parse(raw: &str) -> Option<Pattern> {
    let mut p = raw.trim();
    if p.is_empty() || p.starts_with('#') {
        return None;
    }
    let negate = p.starts_with('!');
    if negate {
        p = &p[1..];
    }
    let dir_only = p.ends_with('/');
    let p = p.trim_end_matches('/');
    let (anchored, body) = match p.strip_prefix('/') {
        Some(rest) => (true, rest),
        // 中间带斜杠的模式相对根目录（gitignore 规则）。
        None => (p.contains('/'), p),
    };
    let segments: Vec<String> = body
        .split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if segments.is_empty() {
        return None;
    }
    Some(Pattern {
        negate,
        anchored,
        dir_only,
        segments,
    })
}

/// 单段 glob：`*` 不跨斜杠、`?` 一个字符、`[abc]` / `[!a-z]` 字符集。
fn glob(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = s.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('*') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('?') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some('[') => {
                let Some(end) = p.iter().position(|c| *c == ']') else {
                    return t.first() == Some(&'[') && go(&p[1..], &t[1..]);
                };
                let Some(&c) = t.first() else { return false };
                let set = &p[1..end];
                let (neg, set) = match set.first() {
                    Some('!') | Some('^') => (true, &set[1..]),
                    _ => (false, set),
                };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= set[i] <= c && c <= set[i + 2];
                        i += 3;
                    } else {
                        hit |= set[i] == c;
                        i += 1;
                    }
                }
                hit != neg && go(&p[end + 1..], &t[1..])
            }
            Some(ch) => t.first() == Some(ch) && go(&p[1..], &t[1..]),
        }
    }
    go(&p, &t)
}

/// 多段匹配，`**` 吃掉零到多段。
fn match_segments(pat: &[String], path: &[&str]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(s) if s == "**" => (0..=path.len()).any(|i| match_segments(&pat[1..], &path[i..])),
        Some(s) => !path.is_empty() && glob(s, path[0]) && match_segments(&pat[1..], &path[1..]),
    }
}

/// 模式是否命中「这一条路径本身」（不看祖先）。
fn hits(p: &Pattern, segs: &[&str], is_dir: bool) -> bool {
    if p.dir_only && !is_dir {
        return false;
    }
    if p.anchored {
        match_segments(&p.segments, segs)
    } else if p.segments.len() == 1 {
        segs.last().is_some_and(|name| glob(&p.segments[0], name))
    } else {
        match_segments(&p.segments, segs)
    }
}

/// 一组复制规则。
pub(crate) struct Rules {
    patterns: Vec<Pattern>,
}

impl Rules {
    pub(crate) fn new(raw: &[String]) -> Self {
        Rules {
            patterns: raw.iter().filter_map(|r| parse(r)).collect(),
        }
    }

    /// 路径（相对仓库根、正斜杠）要不要复制。祖先目录被选中时里面的文件也算选中，
    /// 和 gitignore 一样，最后一条命中的规则说了算。
    pub(crate) fn selected(&self, rel: &str, is_dir: bool) -> bool {
        let segs: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        let mut verdict = false;
        for p in &self.patterns {
            // 先看祖先目录（gitignore：目录被排除，里面的一切都跟着），再看自己。
            let hit = (1..segs.len()).any(|k| hits(p, &segs[..k], true)) || hits(p, &segs, is_dir);
            if hit {
                verdict = !p.negate;
            }
        }
        verdict
    }

    /// 某条锚定的正向模式可能命中 `dir` 里面的东西：只有这时才展开一个被整体忽略的目录。
    /// 不锚定的模式（如 `.env*`）不进被忽略的目录：`node_modules/x/.env` 这种不是用户要的。
    fn could_descend(&self, dir: &str) -> bool {
        let segs: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        self.patterns.iter().any(|p| {
            if p.negate || !p.anchored {
                return false;
            }
            for (i, d) in segs.iter().enumerate() {
                match p.segments.get(i) {
                    None => return false,
                    Some(s) if s == "**" => return true,
                    Some(s) if !glob(s, d) => return false,
                    Some(_) => {}
                }
            }
            p.segments.len() > segs.len()
        })
    }
}

fn skipped_dir(rel: &str) -> bool {
    rel.split('/').any(|seg| SKIP_DIRS.contains(&seg))
}

/// 复制结果。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct CopyReport {
    pub(crate) copied: Vec<String>,
    /// 超过总量上限没复制的。
    pub(crate) skipped: Vec<String>,
}

/// 列出 `src` 里要复制的文件（相对路径）。git 失败时返回空：复制是锦上添花，不让它挡住建立。
pub(crate) fn plan(src: &Path, rules: &Rules) -> Vec<String> {
    let out = super::command("git")
        .arg("-C")
        .arg(src)
        .args([
            "ls-files",
            "-z",
            "-o",
            "-i",
            "--exclude-standard",
            "--directory",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in out.stdout.split(|b| *b == 0) {
        if entry.is_empty() {
            continue;
        }
        let rel = String::from_utf8_lossy(entry).replace('\\', "/");
        if let Some(dir) = rel.strip_suffix('/') {
            if skipped_dir(dir) {
                continue;
            }
            if rules.selected(dir, true) {
                walk(src, dir, rules, true, &mut files);
            } else if rules.could_descend(dir) {
                walk(src, dir, rules, false, &mut files);
            }
        } else if !skipped_dir(&rel) && rules.selected(&rel, false) {
            files.push(rel);
        }
    }
    files.sort();
    files
}

/// 展开一个被忽略的目录。`all` = 目录本身已被选中，下面全要（仍过取反规则）；
/// 否则只沿着模式能命中的子目录往下走。不跟进链接：链接指向哪都可能，复制过去也是错的。
fn walk(root: &Path, dir: &str, rules: &Rules, all: bool, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{dir}/{name}");
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if all || rules.selected(&rel, true) {
                walk(root, &rel, rules, true, out);
            } else if rules.could_descend(&rel) {
                walk(root, &rel, rules, false, out);
            }
        } else if rules.selected(&rel, false) {
            out.push(rel);
        }
    }
}

/// 按清单复制，已存在的不覆盖。累计超过 `limit` 字节后的文件记进 skipped。
pub(crate) fn copy_files(src: &Path, dst: &Path, files: &[String], limit: u64) -> CopyReport {
    let mut report = CopyReport::default();
    let mut total = 0u64;
    for rel in files {
        let from = src.join(rel);
        let to = dst.join(rel);
        let Ok(meta) = std::fs::metadata(&from) else {
            continue;
        };
        if total.saturating_add(meta.len()) > limit {
            report.skipped.push(rel.clone());
            continue;
        }
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
        }
        if std::fs::copy(&from, &to).is_ok() {
            total += meta.len();
            report.copied.push(rel.clone());
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(list: &[&str]) -> Rules {
        Rules::new(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn gitignore_语法_取反与锚定() {
        let r = rules(&[".env*", "apps/*/.env.local", "!.env.example"]);
        assert!(r.selected(".env", false));
        assert!(r.selected(".env.local", false));
        assert!(
            r.selected("pkg/.env", false),
            "不锚定的模式按文件名匹配任意层"
        );
        assert!(!r.selected(".env.example", false), "后写的取反覆盖前面");
        assert!(r.selected("apps/web/.env.local", false));
        let one = rules(&["apps/*/.env.local"]);
        assert!(one.selected("apps/web/.env.local", false));
        assert!(!one.selected("apps/web/deep/.env.local", false), "* 不跨层");
        assert!(!r.selected("README.md", false));
        let anchored = rules(&["/.env*"]);
        assert!(anchored.selected(".env", false));
        assert!(!anchored.selected("pkg/.env", false), "/ 开头只认根目录");
    }

    #[test]
    fn 目录模式与双星() {
        let r = rules(&[".lead/", "config/**/*.secret"]);
        assert!(
            r.selected(".lead/x/notes.md", false),
            "目录被选中，里面的文件跟着选中"
        );
        assert!(!r.selected(".lead", false), "只认目录的模式不命中同名文件");
        assert!(r.selected("config/a/b/k.secret", false));
        assert!(r.selected("config/k.secret", false));
        assert!(r.could_descend("config"));
        assert!(r.could_descend("config/a"));
        assert!(!r.could_descend("build"));
    }

    #[test]
    fn 不锚定模式不进被忽略的目录() {
        let r = rules(&[".env*"]);
        assert!(!r.could_descend("node_modules"));
        assert!(!r.could_descend("dist"));
        let r = rules(&["apps/*/.env.local"]);
        assert!(r.could_descend("apps"));
        assert!(r.could_descend("apps/web"));
        assert!(!r.could_descend("apps/web/src"));
    }

    #[test]
    fn 字符集() {
        assert!(glob("[a-c]x", "bx"));
        assert!(!glob("[!a-c]x", "bx"));
        assert!(glob("?.txt", "a.txt"));
    }
}
