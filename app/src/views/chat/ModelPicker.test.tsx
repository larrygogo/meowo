import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { zh } from "../../i18n/zh";
import { ModelPicker, type ModelPickerItem } from "./ModelPicker";

const ITEMS: ModelPickerItem[] = [
  { id: "sonnet", label: "Sonnet", desc: "日常编码", provider: "claude" },
  { id: "provider:codex", label: zh.chat.switchProviderDefaultModel, provider: "Codex" },
];

function renderPicker(overrides: Partial<Parameters<typeof ModelPicker>[0]> = {}) {
  const props = {
    items: ITEMS,
    current: "Sonnet",
    currentProvider: "claude",
    onSelect: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<ModelPicker {...props} />);
  return props;
}

describe("ModelPicker 模型市场", () => {
  beforeEach(() => localStorage.clear());
  afterEach(cleanup);

  it("按提供商分组并直接选择模型", () => {
    const props = renderPicker();
    expect(screen.getByText("claude")).toBeTruthy();
    expect(screen.getByText(`${zh.chat.switchProvider} · Codex`)).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sonnet" }));
    expect(props.onSelect).toHaveBeenCalledWith("sonnet");
  });

  it("搜索同时匹配模型与提供商，无结果显示空态", () => {
    renderPicker();
    const input = screen.getByRole("textbox", { name: zh.chat.modelSearchPlaceholder });
    fireEvent.change(input, { target: { value: "codex" } });
    expect(screen.queryByRole("menuitem", { name: "Sonnet" })).toBeNull();
    expect(screen.getByRole("menuitem", { name: zh.chat.switchProviderDefaultModel })).toBeTruthy();
    fireEvent.change(input, { target: { value: "不存在的模型" } });
    expect(screen.getByText(zh.chat.modelSearchEmpty)).toBeTruthy();
  });

  it("收藏独立触发、写入本地并在重开后保留", () => {
    const props = renderPicker();
    const sonnetRow = screen.getByRole("menuitem", { name: "Sonnet" }).parentElement!;
    fireEvent.click(sonnetRow.querySelector(".chat-model-picker-star")!);
    expect(props.onSelect).not.toHaveBeenCalled();
    expect(JSON.parse(localStorage.getItem("meowo-starred-models") || "[]")).toEqual(["sonnet"]);
    expect(screen.getByRole("button", { name: zh.chat.modelUnstar })).toBeTruthy();
    cleanup();
    renderPicker();
    expect(screen.getByRole("button", { name: zh.chat.modelUnstar })).toBeTruthy();
  });

  it("Esc 经统一弹层壳关闭", () => {
    const props = renderPicker();
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });
});
