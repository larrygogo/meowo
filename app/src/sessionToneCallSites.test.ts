import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { historyTone, liveSessionTone, type ChatHistory, type LiveSession } from "./api";

/// 会话状态同源（验收 15、做空 ⑩）。
///
/// 侧栏、名单、对话窗标题栏、发送路径算「这条会话此刻在干嘛」，要用同一批事实。此前各调用点把字段
/// 逐个手传给 sessionTone，漏了三处：对话窗发送路径不传后台子任务数（传的是 undefined），
/// 侧栏与名单里「标记完成要不要确认」不传 screen_assumed。现在调用点只把整条记录交给
/// historyTone / liveSessionTone，这里扫描源码保证没有别处再手传。

const SRC = __dirname;

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "generated" ? [] : sources(path);
    return /\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}

describe("会话状态的调用点", () => {
  it("界面代码不直接调 sessionTone，只走 historyTone 与 liveSessionTone", () => {
    const direct = sources(SRC)
      .filter((path) => path !== join(SRC, "api.ts"))
      .filter((path) => /(?<![\w.])sessionTone\(/.test(readFileSync(path, "utf8")))
      .map((path) => path.slice(SRC.length + 1).split("\\").join("/"));
    expect(direct).toEqual([]);
  });

  it("两个入口都带上后台子任务数与「屏幕状态是猜的」", () => {
    const history = (extra: Partial<ChatHistory>) => ({ connected: true, status: "waiting", pendingReview: null, errored: false, busySubagents: 0, screenState: null, screenAssumed: false, ...extra }) as ChatHistory;
    // 主回合停了、后台子任务还在跑：运行中。
    expect(historyTone(history({ busySubagents: 2 }))).toBe("running");
    expect(historyTone(history({ busySubagents: 0 }))).toBe("waiting");
    // 屏幕 idle 只是没认出来（assumed）时，不拿它覆盖 hook 记的运行中。
    expect(historyTone(history({ status: "running", screenState: "idle", screenAssumed: true }))).toBe("running");
    expect(historyTone(history({ status: "running", screenState: "idle", screenAssumed: false }))).toBe("waiting");
    expect(historyTone(null)).toBe("offline");

    const row = (extra: Partial<LiveSession>, status = "waiting") => ({ connected: true, session: { status }, pending_review: null, errored: false, busy_subagents: 0, screen_state: null, screen_assumed: false, ...extra }) as unknown as LiveSession;
    expect(liveSessionTone(row({ busy_subagents: 1 }))).toBe("running");
    expect(liveSessionTone(row({ screen_state: "idle", screen_assumed: true }, "running"))).toBe("running");
    expect(liveSessionTone(row({ screen_state: "idle", screen_assumed: false }, "running"))).toBe("waiting");
  });
});
