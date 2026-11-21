import { act, cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { Lightbox } from "./Lightbox";

afterEach(() => {
  cleanup();
  document.querySelectorAll(".chat-topbar").forEach((el) => el.remove());
});

describe("灯箱与窗口顶栏", () => {
  it("有窗口顶栏（Windows/Linux）时从顶栏下沿开始，工具栏不压在最小化/最大化/关闭上", () => {
    const bar = document.createElement("header");
    bar.className = "chat-topbar";
    bar.getBoundingClientRect = () => ({ top: 0, bottom: 40, left: 0, right: 1100, width: 1100, height: 40, x: 0, y: 0, toJSON: () => ({}) });
    document.body.appendChild(bar);
    render(<Lightbox src="asset://a.png" name="a.png" onClose={() => {}} />);
    expect((document.querySelector(".chat-lightbox") as HTMLElement).style.top).toBe("40px");
  });

  it("没有顶栏（macOS、手机页）时铺满整窗", () => {
    render(<Lightbox src="asset://a.png" name="a.png" onClose={() => {}} />);
    expect((document.querySelector(".chat-lightbox") as HTMLElement).style.top).toBe("");
  });
});

describe("灯箱触屏手势", () => {
  // jsdom 的 PointerEvent 不带坐标与 pointerId：用 MouseEvent 构造，再补上 pointer 字段。
  const fire = (target: Element, type: string, id: number, x: number, y: number, timeStamp = 0) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y });
    Object.defineProperties(event, { pointerId: { value: id }, pointerType: { value: "touch" }, timeStamp: { value: timeStamp } });
    act(() => { target.dispatchEvent(event); });
  };
  const zoom = () => document.querySelector(".chat-lightbox-zoom")!.textContent;
  const stage = () => document.querySelector(".chat-lightbox-stage")!;

  it("双指张开放大的是图，不是网页", () => {
    render(<Lightbox src="asset://a.png" name="a.png" onClose={() => {}} />);
    fire(stage(), "pointerdown", 1, 100, 100);
    fire(stage(), "pointerdown", 2, 200, 100);
    fire(stage(), "pointermove", 2, 300, 100);
    expect(zoom()).toBe("200%");
    fire(stage(), "pointerup", 2, 300, 100);
    fire(stage(), "pointerup", 1, 100, 100);
    expect(zoom()).toBe("200%");
  });

  it("双击在 250% 和复位之间切换，且不当成点空白关闭", () => {
    const closed: number[] = [];
    render(<Lightbox src="asset://a.png" name="a.png" onClose={() => closed.push(1)} />);
    const img = document.querySelector(".chat-lightbox-stage img")!;
    fire(img, "pointerdown", 1, 50, 50, 1000);
    fire(img, "pointerup", 1, 50, 50, 1000);
    fire(img, "pointerdown", 2, 50, 50, 1200);
    fire(img, "pointerup", 2, 50, 50, 1200);
    expect(zoom()).toBe("250%");
    fire(img, "pointerdown", 3, 50, 50, 2000);
    fire(img, "pointerup", 3, 50, 50, 2000);
    fire(img, "pointerdown", 4, 50, 50, 2100);
    fire(img, "pointerup", 4, 50, 50, 2100);
    expect(zoom()).toBe("100%");
    expect(closed).toHaveLength(0);
  });
});
