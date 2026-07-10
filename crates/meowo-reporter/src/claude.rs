//! claude（Anthropic Claude Code）在本机的实况解析。变体表见 `meowo_agent::plugins::claude`。
//! 检测/接线/状态/账号凭据全部经此一处解析路径，不再各自重推「claude 的目录/可执行到底在哪」。

/// claude 在本机的实况（数据目录 `CLAUDE_CONFIG_DIR` 优先，否则 `~/.claude`；hooks 规格；
/// 凭据位置；启动 argv）。未探测到数据目录时退回默认落点，故通常不为 None。
pub fn claude_install() -> Option<meowo_agent::Installation> {
    meowo_agent::by_id("claude")?.resolve()
}

/// claude 的启动 argv（单元素：可执行绝对路径；候选都不中则回退裸名 "claude" 走 PATH）。
///
/// resume/launch 用：meowo-app 拉起的终端继承的是 **app 启动那一刻的 PATH 快照**，未必含刚装好的
/// claude（native installer 只改持久 PATH），故优先绝对路径——否则 wt/powershell 会报
/// 「系统找不到指定的文件」(0x80070002)。
pub fn claude_launch_argv() -> Vec<String> {
    claude_install()
        .map(|i| i.launch_argv())
        .unwrap_or_else(|| vec!["claude".to_string()])
}

/// claude 可执行是否真实落在某个已知位置（区别于 `claude_launch_argv` 找不到时回退裸名）。
/// 与启动同源：杜绝「检测说已安装、启动却找不到文件」。
pub fn claude_installed() -> bool {
    claude_install().is_some_and(|i| i.is_launchable())
}

#[cfg(test)]
mod tests {
    /// 与启动同源是本模块存在的理由：`claude_installed()` 为真时，`claude_launch_argv()`
    /// 必须给出一个**真实存在**的可执行（而非裸名兜底）。
    #[test]
    fn installed_implies_launch_argv_points_at_a_real_file() {
        if !super::claude_installed() {
            return; // 本机没装 claude，跳过（CI 上常见）
        }
        let argv = super::claude_launch_argv();
        assert!(!argv.is_empty());
        assert_ne!(argv[0], "claude", "已安装时不该回退裸名");
        assert!(
            std::path::Path::new(&argv[0]).is_file(),
            "启动 argv 指向的文件应存在：{}",
            argv[0]
        );
    }
}
