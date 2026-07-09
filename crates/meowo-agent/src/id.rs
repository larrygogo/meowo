//! Agent 身份：`&'static str`，与 DB 列 `sessions.provider` 及前端 provider key 同值。
//!
//! 刻意不复用 `meowo_store::ProviderKey` 枚举：本 crate 是纯逻辑层，不该被 DB 层的枚举反向约束
//! （加 agent 就得改枚举，正是要消除的痛点）。两者靠字符串互转，一致性由注册表配对测试守住
//! （见 `meowo_reporter::agent` 里的 enum↔registry 测试）。

use std::fmt;

/// Agent 身份串。构造只走 `AgentId::new`（const），故取值恒为注册表里声明过的那批。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentId(&'static str);

impl AgentId {
    pub const fn new(s: &'static str) -> Self {
        Self(s)
    }
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

pub const CLAUDE: AgentId = AgentId::new("claude");
pub const KIMI: AgentId = AgentId::new("kimi");
pub const CODEX: AgentId = AgentId::new("codex");
