//! PTY 调研测试共用的进程启动配置。

use portable_pty::CommandBuilder;
use std::path::Path;

/// 构造拉起真实 agent 的命令：TERM + **数据库隔离**，一律经此构造，别在测试里手写 env。
///
/// 这些测试拉起的是**真实** agent 进程，它的 hook 会照常上报会话。MEOWO_DB 不指进
/// 测试目录的话，每跑一轮就往用户生产 ~/.meowo/board.db 塞一条空会话，按 last_event_at
/// 排在最前面，把真实会话挤出侧栏首页（曾经攒到 47 条）。隔离规则收在这一处，
/// 新增 PTY 测试时不会因为漏抄一行 env 而重演污染。
pub fn agent_command(exe: &str, cwd: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new(exe);
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("MEOWO_DB", cwd.join("board.db"));
    command
}
