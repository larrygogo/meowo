import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

const { list } = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("../../api", () => ({ promptQueueList: list }));
vi.mock("../../hooks/useTauriEvent", () => ({ useTauriEvent: () => {} }));
vi.mock("../../remoteMode", () => ({ remoteUi: () => false }));
import { usePromptQueue } from "./usePromptQueue";

const entry = (id: number, sessionId: number) => ({ id, sessionId, draft: { prompt: `第 ${id} 条`, attachments: [] }, state: "held", createdAt: 0, updatedAt: 0 });

afterEach(() => { vi.clearAllMocks(); });

// 2026-11-18 用户报：排队的消息会串到别的会话里面去发送。换会话那一帧、以及旧会话的刷新晚到时，
// 钩子不能把上一个会话的排队消息交给新会话的投递逻辑。
describe("排队消息只属于自己的会话", () => {
  it("换会话的那一帧就不再交出上一个会话的条目", async () => {
    list.mockResolvedValueOnce([entry(1, 10)]);
    const { result, rerender } = renderHook(({ sid }) => usePromptQueue(sid), { initialProps: { sid: 10 } });
    await waitFor(() => expect(result.current.items).toHaveLength(1));
    list.mockReturnValueOnce(new Promise(() => {}));
    rerender({ sid: 20 });
    expect(result.current.items).toEqual([]);
  });

  it("切走后才回来的旧会话刷新结果不会出现在新会话里", async () => {
    let resolveOld: (value: unknown) => void = () => {};
    list.mockReturnValueOnce(new Promise((resolve) => { resolveOld = resolve; }));
    const { result, rerender } = renderHook(({ sid }) => usePromptQueue(sid), { initialProps: { sid: 10 } });
    list.mockResolvedValueOnce([]);
    rerender({ sid: 20 });
    await act(async () => { resolveOld([entry(1, 10)]); });
    expect(result.current.items).toEqual([]);
  });
});
