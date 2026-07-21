import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  listen: vi.fn(),
  callback: undefined as undefined | ((event: unknown) => void),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, callback: (event: unknown) => void) => {
    tauri.callback = callback;
    return tauri.listen(name, callback);
  },
}));

import { useTauriEvent } from "./useTauriEvent";

beforeEach(() => {
  tauri.listen.mockReset();
  tauri.callback = undefined;
});

describe("useTauriEvent", () => {
  it("subscribes once and dispatches to the latest handler", async () => {
    const dispose = vi.fn();
    tauri.listen.mockResolvedValue(dispose);
    const first = vi.fn();
    const second = vi.fn();
    const hook = renderHook(({ handler }) => useTauriEvent<{ value: number }>("tick", handler), {
      initialProps: { handler: first },
    });

    await act(async () => {});
    hook.rerender({ handler: second });
    act(() => tauri.callback?.({ payload: { value: 1 } }));

    expect(tauri.listen).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith({ payload: { value: 1 } });
    hook.unmount();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("disposes a registration that resolves after unmount", async () => {
    let resolve!: (dispose: () => void) => void;
    tauri.listen.mockReturnValue(new Promise<() => void>((done) => {
      resolve = done;
    }));
    const dispose = vi.fn();
    const hook = renderHook(() => useTauriEvent("late", () => {}));

    hook.unmount();
    await act(async () => resolve(dispose));

    expect(dispose).toHaveBeenCalledOnce();
  });
});
