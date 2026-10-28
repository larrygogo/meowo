import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

// 执行真正要安装的桥接模板，而非另一份仿实现；事件载荷取自 OpenCode 官方 SDK / docs/plugins（2026-10）。
const rust = readFileSync(new URL("../app/src-tauri/crates/meowo-agent/src/config.rs", import.meta.url), "utf8");
const template = rust.match(/const TEMPLATE: &str = r#"([\s\S]*?)"#;/)?.[1];
assert.ok(template, "找不到 OpenCode 桥接模板");

async function bridge(spawn, timers = {}) {
  const context = vm.createContext({ Bun: { spawn }, setTimeout, clearTimeout, ...timers });
  vm.runInContext(template
    .replaceAll("__MEOWO_REPORTER__", JSON.stringify("C:/Program Files/Meowo/meowo-reporter.exe"))
    .replaceAll("__MEOWO_PROVIDER__", JSON.stringify("opencode"))
    .replace("export const MeowoReporter", "globalThis.MeowoReporter"), context);
  return context.MeowoReporter({ directory: "C:/project" });
}

function reporter(captured, exited = Promise.resolve(0)) {
  return {
    stdin: { write: (json) => captured.push(JSON.parse(json)), end() {} },
    exited, unref() {}, kill() {},
  };
}

test("OpenCode 官方生命周期、审批和压缩事件能转换为 reporter 载荷", async () => {
  const captured = [];
  const hooks = await bridge((argv) => {
    assert.equal(argv[0], "C:/Program Files/Meowo/meowo-reporter.exe");
    assert.equal(argv[2], "opencode");
    return reporter(captured);
  });
  for (const event of [
    { type: "session.created", properties: { info: { id: "ses_1", directory: "C:/worktree" } } },
    { type: "permission.asked", properties: { id: "per_1", sessionID: "ses_1", permission: "bash", patterns: ["git *"], metadata: { command: "git status" } } },
    { type: "permission.replied", properties: { sessionID: "ses_1", requestID: "per_1", reply: "reject" } },
    { type: "session.error", properties: { sessionID: "ses_1", error: { name: "APIError" } } },
    { type: "session.compacted", properties: { sessionID: "ses_1" } },
    { type: "session.idle", properties: { sessionID: "ses_1" } },
  ]) await hooks.event({ event });
  assert.deepEqual(captured.map((payload) => payload.hook_event_name), ["SessionStart", "PermissionRequest", "PermissionResult", "StopFailure", "PostCompact", "Stop"]);
  assert.equal(captured[0].cwd, "C:/worktree");
  assert.equal(captured[1].tool_input.command, "git status");
  assert.ok(captured.every((payload) => payload.session_id === "ses_1"));
  await hooks.event({ event: { type: "session.error", properties: {} } });
  assert.equal(captured.length, 6, "缺失 sessionID 不能污染其他会话");
});

test("前一个 reporter 未落库时，紧邻的审批解除必须等候", async () => {
  const captured = [];
  let release;
  const pending = new Promise((resolve) => { release = resolve; });
  const hooks = await bridge(() => reporter(captured, captured.length === 0 ? pending : Promise.resolve(0)));
  const asked = hooks.event({ event: { type: "permission.asked", properties: { sessionID: "s", permission: "bash" } } });
  const replied = hooks.event({ event: { type: "permission.replied", properties: { sessionID: "s" } } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(captured.length, 1);
  release(0);
  await Promise.all([asked, replied]);
  assert.deepEqual(captured.map((payload) => payload.hook_event_name), ["PermissionRequest", "PermissionResult"]);
});

test("reporter 派生失败后，后续事件仍能继续上报", async () => {
  const captured = [];
  let attempts = 0;
  const hooks = await bridge(() => {
    if (attempts++ === 0) throw new Error("spawn failed");
    return reporter(captured);
  });
  await hooks.event({ event: { type: "permission.asked", properties: { sessionID: "s" } } });
  await hooks.event({ event: { type: "permission.replied", properties: { sessionID: "s" } } });
  assert.equal(captured[0].hook_event_name, "PermissionResult");
});

test("reporter 超时且 kill 失败时也能解除队列阻塞", async () => {
  const captured = [];
  let timeout;
  const hooks = await bridge(() => ({
    ...reporter(captured, new Promise(() => {})),
    kill() { throw new Error("already exited"); },
  }), {
    setTimeout(callback) { timeout = callback; return 1; },
    clearTimeout() {},
  });
  const event = hooks.event({ event: { type: "session.idle", properties: { sessionID: "s" } } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(typeof timeout, "function");
  timeout();
  await event;
  assert.equal(captured[0].hook_event_name, "Stop");
});
