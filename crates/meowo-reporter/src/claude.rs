//! claude（Anthropic Claude Code）在本机的实况解析。变体表见 `meowo_agent::plugins::claude`。
//! 检测/接线/状态/账号凭据全部经此一处解析路径，不再各自重推「claude 的目录/可执行到底在哪」。

/// claude 在本机的实况（数据目录 `CLAUDE_CONFIG_DIR` 优先，否则 `~/.claude`；hooks 规格；
/// 凭据位置；启动 argv）。未探测到数据目录时退回默认落点，故通常不为 None。
pub fn claude_install() -> Option<meowo_agent::Installation> {
    meowo_agent::by_id("claude")?.resolve()
}

/// claude 的启动 argv（单元素）。
///
/// resume/launch 用：meowo-app 拉起的终端继承的是 **app 启动那一刻的 PATH 快照**，未必含刚装好的
/// claude（native installer 只改持久 PATH），故变体表把绝对路径候选排在前面——否则 wt/powershell
/// 会报「系统找不到指定的文件」(0x80070002)。
///
/// **返回值不保证是绝对路径**，共三种情形：
/// - 命中 `~/.local/bin` 或 npm 包内的可执行 → 绝对路径；
/// - 命中 `LaunchCandidate::OnPath`（claude 只在 PATH 上）→ **裸名 `"claude"`**。刻意不固化成
///   绝对路径：PATH 上的 claude 常是 shim，固化它会绕过 shim 做的环境准备；
/// - 候选全不中（未安装）→ 同样回退裸名，交给 PATH 兜底。
///
/// 后两种在字面上都是 `"claude"`，靠 [`claude_installed`] 区分。
pub fn claude_launch_argv() -> Vec<String> {
    claude_install()
        .map(|i| i.launch_argv())
        .unwrap_or_else(|| vec!["claude".to_string()])
}

/// claude 可执行是否真实落在某个已知位置——**含「在 PATH 上」这一条**（`OnPath` 候选）。
/// 与启动同源：杜绝「检测说已安装、启动却找不到文件」。
pub fn claude_installed() -> bool {
    claude_install().is_some_and(|i| i.is_launchable())
}

#[cfg(test)]
mod tests {
    /// 与启动同源是本模块存在的理由：`claude_installed()` 为真时，`claude_launch_argv()` 的首元素
    /// 必须**真能启动**。
    ///
    /// 但「能启动」不等于「是绝对路径」：`LaunchCandidate::OnPath` 命中时 `is_launchable()` 为真，
    /// 而 argv 是裸名（刻意不固化 shim 路径）。故按两种情形分别断言——一律要求绝对路径的话，
    /// 在「claude 只在 PATH 上、不在 ~/.local/bin」的机器上会假失败。
    #[test]
    fn installed_implies_launch_argv_is_runnable() {
        if !super::claude_installed() {
            return; // 本机没装 claude，跳过（CI 上常见）
        }
        let argv = super::claude_launch_argv();
        assert!(!argv.is_empty());
        if argv[0] == "claude" {
            // OnPath 命中：裸名交给 PATH 解析，那它就必须真的在 PATH 上。
            let bin = if cfg!(windows) { "claude.exe" } else { "claude" };
            assert!(crate::agent::exe_on_path(bin), "回退裸名时 claude 应在 PATH 上");
        } else {
            assert!(
                std::path::Path::new(&argv[0]).is_file(),
                "启动 argv 指向的文件应存在：{}",
                argv[0]
            );
        }
    }
}
