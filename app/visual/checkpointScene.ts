import { CHAT_SESSION_ID } from "./fixtures";

const ROOT = "C:/dev/web-app";
const FILES = [
  { path: "src/components/LoginForm.tsx", status: "M", additions: 6, deletions: 2 },
  { path: "src/schemas/login.ts", status: "A", additions: 18, deletions: 0 },
  { path: "assets/old-login.png", status: "D", additions: null, deletions: null },
];
const repo = (files: unknown[] = FILES) => ({
  root: ROOT, name: "web-app", startRef: "refs/meowo/checkpoints/1/turn/2/start", endRef: "refs/meowo/checkpoints/1/turn/2/end", files, error: null,
});

export function checkpointSceneMock(cmd: string): unknown {
  if (cmd === "checkpoint_turns") return [{ turn: 2, repos: [repo([])] }, { turn: 1, repos: [{ ...repo([]), root: "C:/dev/api", name: "api" }] }];
  if (cmd === "checkpoint_turn_detail") return { turn: 2, repos: [repo()] };
  if (cmd === "checkpoint_file_diff") return {
    path: "src/components/LoginForm.tsx", status: "M", truncated: false,
    diff: ["diff --git a/src/components/LoginForm.tsx b/src/components/LoginForm.tsx", "--- a/src/components/LoginForm.tsx", "+++ b/src/components/LoginForm.tsx", "@@ -12,4 +12,4 @@", " export function LoginForm() {", '-  return <form onSubmit={save}>;', '+  return <form onSubmit={validateAndSave}>;', " }"].join("\n"),
  };
  if (cmd === "checkpoint_restore" || cmd === "checkpoint_undo") return { undoId: 1, results: [{ root: ROOT, restored: true, error: null }] };
  return undefined;
}

export const CHECKPOINT_SESSION_ID = CHAT_SESSION_ID;
