import { beforeEach, expect, test } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { clearMocks } from "@tauri-apps/api/mocks";
import { makeSession } from "./data";
import { installMocks, store } from "./mock";

beforeEach(() => {
  clearMocks();
  store.sessions = [];
});

test("get_live_sessions 返回 store 内容", async () => {
  installMocks();
  store.sessions = [makeSession({ title: "A", project: "x/y" })];
  const r = await invoke("get_live_sessions");
  expect(r).toHaveLength(1);
});

test("rename_session 改 store 里的标题", async () => {
  installMocks();
  const s = makeSession({ title: "旧名", project: "x/y" });
  store.sessions = [s];
  await invoke("rename_session", { cwd: s.cwd, sessionId: s.session.cc_session_id, title: "新名" });
  expect(store.sessions[0].task_title).toBe("新名");
});

test("set_archived 切换归档位", async () => {
  installMocks();
  const s = makeSession({ title: "A", project: "x/y" });
  store.sessions = [s];
  await invoke("set_archived", { sessionId: s.session.id, archived: true });
  expect(store.sessions[0].archived).toBe(true);
  expect(store.sessions[0].archived_at).not.toBeNull();
});
