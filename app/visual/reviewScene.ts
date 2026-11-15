const comment = (id: string, author: string, body: string) => ({ id, author, body, createdAt: "2026-06-01T04:00:00Z", url: null, state: null, path: null, line: null, side: null });
const review = {
  pr: { number: 162, state: "open", url: "https://github.com/meowo/meowo/pull/162", title: "重构登录页表单校验" },
  comments: [comment("c1", "alice", "这里要不要同时覆盖密码为空的情况？")],
  reviews: [{ ...comment("r1", "bob", "方向没问题，补一个边界测试就可以。"), state: "COMMENTED" }],
  files: [{ path: "src/components/LoginForm.tsx", status: "modified", additions: 6, deletions: 2, patch: "@@ -12,4 +12,4 @@\n export function LoginForm() {\n-  return <form onSubmit={save}>;\n+  return <form onSubmit={validateAndSave}>;\n }" }],
  monitor: { key: "meowo/meowo#162", enabled: true, lastError: null, events: [{ id: "e1", kind: "comment", summary: "alice: 这里要不要同时覆盖密码为空的情况？", createdAt: 1, wakeState: "pending" }] },
};

export function reviewSceneMock(cmd: string): unknown {
  if (cmd === "pr_review") return review;
  if (["pr_comment", "pr_line_comment", "pr_monitor_set"].includes(cmd)) return null;
  if (cmd === "pr_monitor_wake") return "queued";
  return undefined;
}
