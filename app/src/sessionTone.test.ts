import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { sessionTone } from "./api";

/// 优先级对照 Kimi：Agent 卡着等你决定（审批/提问/屏幕阻塞）> 上一轮出错 > 其余。
describe("sessionTone 优先级", () => {
  it("待处理压过出错：出错的会话又挂着审批时先归待处理", () => {
    expect(sessionTone(true, "waiting", "plan", true)).toBe("pending");
    expect(sessionTone(true, "running", null, true, 0, "blocked")).toBe("pending");
  });

  it("没有待处理时出错仍压过运行/等待", () => {
    expect(sessionTone(true, "running", null, true)).toBe("error");
    expect(sessionTone(true, "waiting", null, true)).toBe("error");
  });

  it("离线不看出错与待处理", () => {
    expect(sessionTone(false, "waiting", "plan", true)).toBe("offline");
  });
});
