import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModelMenu } from "./ModelMenu";

afterEach(cleanup);

const base = {
  title: "Claude Code",
  items: [{ id: "sonnet", label: "Sonnet" }, { id: "opus", label: "Opus" }],
  currentId: "opus" as string | null,
  probing: false,
  unavailable: null,
  cacheHint: false,
  onSelect: () => {},
  phone: false,
  onClose: () => {},
};

describe("ModelMenu", () => {
  it("当前模型打勾，点它只关菜单不重复切换；点别的才切", () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(<ModelMenu {...base} onSelect={onSelect} onClose={onClose} />);
    fireEvent.click(screen.getByRole("menuitem", { name: "Opus" }));
    expect(onSelect).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("menuitem", { name: "Sonnet" }));
    expect(onSelect).toHaveBeenCalledWith("sonnet");
  });

  /// 换模型的代价写在做决定的地方（对照 Kimi），但会话还空着时没有缓存可失效，不说。
  it("会话有内容才提示缓存失效；有更多可选才给更多模型", () => {
    const { rerender } = render(<ModelMenu {...base} />);
    expect(screen.queryByText(/提示词缓存失效/)).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "更多模型…" })).toBeNull();
    const onMore = vi.fn();
    rerender(<ModelMenu {...base} cacheHint onMore={onMore} />);
    expect(screen.getByText(/提示词缓存失效/)).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "更多模型…" }));
    expect(onMore).toHaveBeenCalled();
  });

  it("清单未学到：探测中转圈，读不到给去终端的出口", () => {
    const { rerender } = render(<ModelMenu {...base} items={[]} probing />);
    expect(screen.getByRole("status").textContent).toContain("正在读取模型清单");
    const onGoTerminal = vi.fn();
    rerender(<ModelMenu {...base} items={[]} unavailable="读不到模型清单" onGoTerminal={onGoTerminal} />);
    expect(screen.getByText("读不到模型清单")).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "去终端页切换模型" }));
    expect(onGoTerminal).toHaveBeenCalled();
  });

  it("手机上包成底部面板", () => {
    render(<ModelMenu {...base} phone />);
    expect(document.querySelector(".chat-sheet .chat-model-quick")).toBeTruthy();
  });
});

describe("ModelMenu 收藏段", () => {
  /// 对照 Kimi 的 starredModels：模型市场里收藏的其他引擎置顶，附引擎名区分同名的「默认模型」。
  it("收藏的其他引擎置顶并带引擎名", () => {
    const onSelect = vi.fn();
    render(<ModelMenu {...base} starred={[{ id: "provider:codex", label: "默认模型", detail: "Codex" }]} onSelect={onSelect} />);
    expect(screen.getByText("已收藏")).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: /默认模型/ }));
    expect(onSelect).toHaveBeenCalledWith("provider:codex");
  });
});

describe("ModelMenu 思考程度", () => {
  it("给了档位才显示分段；当前档不重复切换，忙时禁用", () => {
    const onPick = vi.fn();
    const { rerender } = render(<ModelMenu {...base} />);
    expect(screen.queryByRole("radiogroup")).toBeNull();
    rerender(<ModelMenu {...base} effort={{ levels: ["low", "high", "xhigh", "max"], current: "high", busy: false, onPick }} />);
    expect(screen.getByRole("radio", { name: "XHigh" })).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: "High" }));
    expect(onPick).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("radio", { name: "Max" }));
    expect(onPick).toHaveBeenCalledWith("max");
    rerender(<ModelMenu {...base} effort={{ levels: ["low", "high"], current: "high", busy: true, onPick }} />);
    expect((screen.getByRole("radio", { name: "Low" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
