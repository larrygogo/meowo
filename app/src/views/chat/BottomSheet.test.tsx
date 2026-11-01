import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { hasEscLayers } from "../../escLayers";
import { BottomSheet, SheetOnPhone } from "./BottomSheet";

afterEach(cleanup);

describe("BottomSheet", () => {
  it("点遮罩、点抓手、按 Esc 都关闭；开着时占一层 Esc", () => {
    const onClose = vi.fn();
    render(<BottomSheet title="命令" onClose={onClose}><button type="button">/model</button></BottomSheet>);
    expect(screen.getByRole("dialog", { name: "命令" })).toBeTruthy();
    expect(hasEscLayers()).toBe(true);
    fireEvent.click(document.querySelector(".chat-sheet-scrim")!);
    fireEvent.click(document.querySelector(".chat-sheet-grab")!);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  /// 挂在已有菜单里（useMenuPopup 负责 Esc 与点外关闭）时不另占 Esc 层，免得一次 Esc 关两回。
  it("ownsEscape=false 时不处理 Esc", () => {
    const onClose = vi.fn();
    render(<BottomSheet title="权限模式" onClose={onClose} ownsEscape={false}>x</BottomSheet>);
    expect(hasEscLayers()).toBe(false);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  /// 下滑关闭：拖过阈值即关；拖一小段松手弹回不关；列表没在顶部时从列表里拖是在滚列表，不接管。
  it("下滑过阈值关闭，短拖弹回，列表未到顶时不接管", async () => {
    const onClose = vi.fn();
    render(<BottomSheet title="模型" onClose={onClose}><div style={{ height: 2000 }}>长列表</div></BottomSheet>);
    const panel = document.querySelector(".chat-sheet-panel") as HTMLElement;
    const grab = document.querySelector(".chat-sheet-grab")!;
    const pause = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    // 按真实手指的节奏拖：每 40ms 挪 15px，松手前停一下（速度判定看的是最后一段）。
    const drag = async (target: Element, from: number, to: number) => {
      fireEvent.touchStart(target, { touches: [{ clientY: from }] });
      for (let y = from + 10; y <= to; y += 15) {
        fireEvent.touchMove(target, { touches: [{ clientY: y }] });
        await pause(40);
      }
      fireEvent.touchEnd(target, { touches: [] });
      await pause(250);
    };
    await drag(grab, 100, 140);
    expect(onClose).not.toHaveBeenCalled();
    expect(panel.style.transform).toBe("");

    const body = document.querySelector(".chat-sheet-body") as HTMLElement;
    Object.defineProperty(body, "scrollTop", { configurable: true, value: 40 });
    await drag(screen.getByText("长列表"), 100, 400);
    expect(onClose).not.toHaveBeenCalled();

    await drag(grab, 100, 260);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("桌面原样渲染内容，手机包成底部面板", () => {
    const { rerender } = render(<SheetOnPhone phone={false} title="t" onClose={() => {}}><span>菜单</span></SheetOnPhone>);
    expect(document.querySelector(".chat-sheet")).toBeNull();
    rerender(<SheetOnPhone phone title="t" onClose={() => {}}><span>菜单</span></SheetOnPhone>);
    expect(document.querySelector(".chat-sheet")).toBeTruthy();
  });
});
