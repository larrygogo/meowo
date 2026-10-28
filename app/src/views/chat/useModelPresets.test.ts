import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { chatUi } from "../../test/agents";
import { useModelPresets } from "./useModelPresets";

const agentModels = vi.hoisted(() => vi.fn());
vi.mock("../../api", () => ({ agentModels }));

afterEach(() => { cleanup(); agentModels.mockReset(); localStorage.clear(); });

it("菜单型 CLI 的模型清单不能升级成内联选择能力", () => {
  agentModels.mockResolvedValue(["provider/model"]);
  const { result } = renderHook(() => useModelPresets("opencode", chatUi("opencode"), 1));
  expect(result.current.modelPresets).toEqual([]);
  expect(result.current.modelMenuCommand).toBe("/models");
  expect(result.current.modelDropdownReady).toBe(false);
  expect(agentModels).not.toHaveBeenCalled();
});

it("切换会话立即隔离旧清单，新查询失败也不会泄漏上一会话的模型", async () => {
  agentModels.mockResolvedValueOnce(["first-account/model"]);
  let reject!: (error: Error) => void;
  agentModels.mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }));
  const ui = chatUi("claude");
  const { result, rerender } = renderHook(({ id }) => useModelPresets("claude", ui, id), { initialProps: { id: 1 } });
  await waitFor(() => expect(result.current.modelPresets[0]?.id).toBe("first-account/model"));
  rerender({ id: 2 });
  expect(result.current.modelPresets.some((model) => model.id === "first-account/model")).toBe(false);
  await act(async () => reject(new Error("unavailable")));
  expect(result.current.modelPresets.some((model) => model.id === "first-account/model")).toBe(false);
  expect(agentModels).toHaveBeenLastCalledWith("claude", 2);
});

it("旧查询晚到不覆盖当前 Agent 的菜单能力", async () => {
  let resolve!: (models: string[]) => void;
  agentModels.mockImplementation(() => new Promise((done) => { resolve = done; }));
  const { result, rerender } = renderHook(({ provider }) => useModelPresets(provider, chatUi(provider), 1), {
    initialProps: { provider: "claude" },
  });
  rerender({ provider: "kimi" });
  await act(async () => resolve(["claude/late-model"]));
  expect(result.current.modelPresets).toEqual([]);
  expect(result.current.modelMenuCommand).toBe("/model");
});
