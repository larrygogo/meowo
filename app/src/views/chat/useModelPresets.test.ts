import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { chatUi } from "../../test/agents";
import { applyCatalogLabels, useModelPresets } from "./useModelPresets";

const agentModels = vi.hoisted(() => vi.fn());
const agentModelCatalog = vi.hoisted(() => vi.fn((): Promise<unknown[]> => Promise.resolve([])));
vi.mock("../../api", () => ({ agentModels, agentModelCatalog }));

afterEach(() => { cleanup(); agentModels.mockReset(); agentModelCatalog.mockReset(); agentModelCatalog.mockImplementation(() => Promise.resolve([])); localStorage.clear(); });

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

/// 菜单型 CLI（kimi）的模型目录读配置文件：不升级成内联选择（modelPresets 仍空），换会话立即隔离。
it("菜单型 CLI 读配置目录，换会话不串", async () => {
  agentModelCatalog.mockResolvedValueOnce([{ id: "kimi-code/k3", label: "K3", group: "managed:kimi-code", efforts: [] }]);
  agentModelCatalog.mockImplementationOnce(() => new Promise(() => {}));
  const ui = chatUi("kimi");
  const { result, rerender } = renderHook(({ id }) => useModelPresets("kimi", ui, id), { initialProps: { id: 1 } });
  await waitFor(() => expect(result.current.modelCatalog[0]?.label).toBe("K3"));
  expect(result.current.modelPresets).toEqual([]);
  rerender({ id: 2 });
  expect(result.current.modelCatalog).toEqual([]);
  expect(agentModelCatalog).toHaveBeenLastCalledWith("kimi", 2);
});

/// claude 的内置标签必随 CLI 改版过时（写着 Opus 5，装的已是 Opus 5.5）：读可执行里内嵌的
/// 模型目录换成真实显示名，1M 变体同步；目录只折进标签，不另列（菜单里不重复）。
it("claude 用模型目录改写预设标签", async () => {
  agentModels.mockResolvedValue([]);
  agentModelCatalog.mockResolvedValueOnce([
    { id: "opus", label: "Opus 5.5", group: null, efforts: [], default_effort: null, is_default: false, uncertain: false },
    { id: "fable", label: "Fable 5.1", group: null, efforts: [], default_effort: null, is_default: false, uncertain: false },
  ]);
  const ui = chatUi("claude");
  const { result } = renderHook(() => useModelPresets("claude", ui, 3));
  await waitFor(() => expect(result.current.modelPresets.find((m) => m.id === "opus")?.label).toBe("Opus 5.5"));
  expect(result.current.modelPresets.find((m) => m.id === "fable")?.label).toBe("Fable 5.1");
  // 目录里没有的组合别名保留原标签。
  expect(result.current.modelPresets.find((m) => m.id === "opusplan")?.label).toBe("Opus Plan");
  expect(result.current.modelCatalog).toEqual([]);
});

it("1M 变体跟随家族显示名", () => {
  const presets = [{ id: "opus", label: "Opus 5" }, { id: "opus[1m]", label: "Opus 5 (1M)" }, { id: "opusplan", label: "Opus Plan" }];
  expect(applyCatalogLabels(presets, [{ id: "opus", label: "Opus 5.5", group: null, efforts: [], default_effort: null, is_default: false, uncertain: false }]).map((p) => p.label))
    .toEqual(["Opus 5.5", "Opus 5.5 (1M)", "Opus Plan"]);
  expect(applyCatalogLabels(presets, [])).toBe(presets);
});
