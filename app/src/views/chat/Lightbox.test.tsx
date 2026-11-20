import { cleanup, render } from "@testing-library/react";
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
