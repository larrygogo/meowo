// 侧栏 200 会话首次渲染耗时（UI 优化 #54/#56：加 PR/CI 与改动数后劣化 ≤10%）。
// jsdom 计时噪声大：先热身一轮，再跑 15 轮取中位数；断言只拦数量级的退化（整表重建、
// 每行起一次请求之类），精确的前后对比看控制台打印的中位数。
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import { SIDEBAR_GROUP_MODE_KEY } from "./sticker/helpers";
import type { LiveSession } from "../api";

const ROWS = 200;

function rows(): LiveSession[] {
  return Array.from({ length: ROWS }, (_, i) => ({
    session: { id: i + 1, cc_session_id: `cc-${i + 1}`, status: i % 3 === 0 ? "running" : "ended", last_event_at: 1_700_000_000_000 - i * 60_000 },
    project_name: `repo-${i % 20}`,
    task_title: `会话 ${i + 1}`,
    connected: i % 3 === 0,
    pending_review: null,
    cwd: `C:/work/repo-${i % 20}`,
    provider: "claude",
  })) as unknown as LiveSession[];
}

// 一半仓库有 PR（四态轮换）、全部有改动：最重的显示情形。改前没有这条命令，返回值不被读取。
function badges(cwds: readonly string[]) {
  const states = ["open", "draft", "merged", "closed"];
  return cwds.map((cwd, i) => ({
    cwd,
    additions: 10 + i,
    deletions: i,
    files: 1 + (i % 4),
    pr: i % 2 === 0 ? { number: 100 + i, state: states[i % 4], checks: i % 3 === 0 ? "fail" : "pass", url: `https://github.com/o/r/pull/${100 + i}`, title: "t" } : null,
  }));
}

async function renderOnce(): Promise<number> {
  const list = rows();
  invoke.mockImplementation((command: string, args?: { cwds?: string[] }) => {
    if (command === "get_live_sessions_page") return Promise.resolve(list);
    if (command === "sidebar_git_badges") return Promise.resolve(badges(args?.cwds ?? []));
    return Promise.resolve();
  });
  const start = performance.now();
  render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
  await screen.findByText(`会话 ${ROWS}`, undefined, { timeout: 5000 });
  const elapsed = performance.now() - start;
  cleanup();
  return elapsed;
}

describe("ChatSidebar 200 行首次渲染", () => {
  afterEach(cleanup);
  it("中位数打印出来，且不超过 1.5 秒", async () => {
    localStorage.clear();
    // 平铺（不分组）才渲染副行：PR/CI 与改动数就挂在副行上，按最重的情形量。
    localStorage.setItem(SIDEBAR_GROUP_MODE_KEY, "none");
    await renderOnce();
    const samples: number[] = [];
    for (let i = 0; i < 15; i++) samples.push(await renderOnce());
    samples.sort((a, b) => a - b);
    const median = samples[7];
    console.log(`[sidebar-perf] rows=${ROWS} median=${median.toFixed(1)}ms samples=${samples.map((s) => s.toFixed(0)).join(",")}`);
    expect(median).toBeLessThan(1500);
  }, 60_000);
});
