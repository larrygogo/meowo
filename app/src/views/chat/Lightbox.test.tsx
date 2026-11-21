import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { collectGallery, galleryAttrs, Lightbox, type GalleryItem } from "./Lightbox";
import { ImageReplyContext } from "./imageReply";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  document.querySelectorAll(".chat-topbar").forEach((el) => el.remove());
});

const one: GalleryItem[] = [{ src: "asset://a.png", name: "a.png" }];
const three: GalleryItem[] = [
  { src: "asset://a.png", name: "a.png", caption: "第一张" },
  { src: "asset://b.png", name: "b.png", caption: "第二张" },
  { src: "asset://c.png", name: "c.png" },
];
const img = () => document.querySelector(".chat-lightbox-stage img") as HTMLImageElement;
const stage = () => document.querySelector(".chat-lightbox-stage")!;
const scaleOf = () => Number(/scale\(([\d.]+)\)/.exec(img().style.transform)?.[1]);
const offsetOf = () => /translate\(([-\d.]+)px, ([-\d.]+)px\)/.exec(img().style.transform)!.slice(1).map(Number);
// jsdom 的 PointerEvent 不带坐标与 pointerId：用 MouseEvent 构造，再补上 pointer 字段。
const fire = (target: Element, type: string, id: number, x: number, y: number, timeStamp = 0, pointerType = "touch") => {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y });
  Object.defineProperties(event, { pointerId: { value: id }, pointerType: { value: pointerType }, timeStamp: { value: timeStamp } });
  act(() => { target.dispatchEvent(event); });
};

describe("灯箱与窗口顶栏", () => {
  it("有窗口顶栏（Windows/Linux）时从顶栏下沿开始，工具栏不压在最小化/最大化/关闭上", () => {
    const bar = document.createElement("header");
    bar.className = "chat-topbar";
    bar.getBoundingClientRect = () => ({ top: 0, bottom: 40, left: 0, right: 1100, width: 1100, height: 40, x: 0, y: 0, toJSON: () => ({}) });
    document.body.appendChild(bar);
    render(<Lightbox items={one} onClose={() => {}} />);
    expect((document.querySelector(".chat-lightbox") as HTMLElement).style.top).toBe("40px");
  });

  it("没有顶栏（macOS、手机页）时铺满整窗", () => {
    render(<Lightbox items={one} onClose={() => {}} />);
    expect((document.querySelector(".chat-lightbox") as HTMLElement).style.top).toBe("");
  });
});

describe("相册：多张图左右切换", () => {
  it("单张不给计数和箭头", () => {
    render(<Lightbox items={one} onClose={() => {}} />);
    expect(document.querySelector(".chat-lightbox-count")).toBeNull();
    expect(document.querySelector(".chat-lightbox-nav")).toBeNull();
  });

  it("键盘 ← → 与箭头按钮切换，计数和说明跟着变，首尾循环", () => {
    render(<Lightbox items={three} index={1} onClose={() => {}} />);
    expect(screen.getByText("2 / 3")).toBeTruthy();
    expect(screen.getByText("第二张")).toBeTruthy();
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(img().getAttribute("src")).toBe("asset://c.png");
    expect(document.querySelector(".chat-lightbox-caption")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "下一张" }));
    expect(screen.getByText("1 / 3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "上一张" }));
    expect(img().getAttribute("src")).toBe("asset://c.png");
  });

  it("相邻两张同一地址：翻过去重新加载，转圈会消失", () => {
    const same: GalleryItem[] = [{ src: "asset://a.png", name: "a.png" }, { src: "asset://a.png", name: "a.png" }];
    render(<Lightbox items={same} onClose={() => {}} />);
    fireEvent.load(img());
    expect(document.querySelector(".chat-lightbox-loading")).toBeNull();
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(document.querySelector(".chat-lightbox-loading")).not.toBeNull();
    fireEvent.load(img());
    expect(document.querySelector(".chat-lightbox-loading")).toBeNull();
  });

  it("切换后缩放复位", () => {
    render(<Lightbox items={three} onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "+" });
    expect(scaleOf()).toBeGreaterThan(1);
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(scaleOf()).toBe(1);
  });

  it("从对话里按页面顺序收集能点开的图，定位到点的那张", () => {
    const root = document.createElement("main");
    root.className = "chat-scroll";
    const mk = (name: string, path?: string) => {
      const button = document.createElement("button");
      for (const [k, v] of Object.entries(galleryAttrs({ name, path, src: path ? null : `asset://${name}` }))) button.setAttribute(k, v);
      root.appendChild(button);
      return button;
    };
    mk("a.png");
    const second = mk("b.png", "C:/img/b.png");
    mk("c.png");
    root.appendChild(document.createElement("button")); // 没有 data-gallery 的（失败、未加载）不算
    document.body.appendChild(root);
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = { convertFileSrc: (p: string) => `asset://${p}` };
    const { items, index } = collectGallery(second, { src: "asset://real-b", name: "b.png", path: "C:/img/b.png" });
    expect(items.map((item) => item.name)).toEqual(["a.png", "b.png", "c.png"]);
    expect(index).toBe(1);
    expect(items[1].src).toBe("asset://real-b");
    root.remove();
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it("收着的子任务块里的图不进相册", () => {
    const root = document.createElement("main");
    root.className = "chat-scroll";
    root.innerHTML = '<button data-gallery-name="a.png" data-gallery-src="asset://a"></button>'
      + '<details><summary>子任务</summary><button data-gallery-name="hidden.png" data-gallery-src="asset://h"></button></details>'
      + '<details open><button data-gallery-name="shown.png" data-gallery-src="asset://s"></button></details>';
    document.body.appendChild(root);
    const opener = root.querySelector("button")!;
    expect(collectGallery(opener, { src: "asset://a", name: "a.png" }).items.map((item) => item.name)).toEqual(["a.png", "shown.png"]);
    root.remove();
  });

  it("不在对话里打开（找不到滚动区）时只有自己一张", () => {
    const lonely = document.createElement("button");
    expect(collectGallery(lonely, one[0])).toEqual({ items: one, index: 0 });
  });
});

describe("灯箱触屏手势", () => {
  it("双指张开放大的是图，不是网页", () => {
    render(<Lightbox items={one} onClose={() => {}} />);
    fire(stage(), "pointerdown", 1, 100, 100);
    fire(stage(), "pointerdown", 2, 200, 100);
    fire(stage(), "pointermove", 2, 300, 100);
    expect(scaleOf()).toBe(2);
    fire(stage(), "pointerup", 2, 300, 100);
    fire(stage(), "pointerup", 1, 100, 100);
    expect(scaleOf()).toBe(2);
  });

  it("双指缩放以两指中点为中心：中点下的内容留在手指下", () => {
    // jsdom 的舞台没有尺寸，centered() 退回原始坐标：舞台中心按 (0,0) 算。
    render(<Lightbox items={one} onClose={() => {}} />);
    fire(stage(), "pointerdown", 1, 100, 200);
    fire(stage(), "pointerdown", 2, 140, 200);
    fire(stage(), "pointermove", 2, 220, 200); // 距离 40 → 120，放大 3 倍；中点 (120,200) → (160,200)
    expect(scaleOf()).toBe(3);
    // 起手中点 (120,200) 下的内容点 = (120,200)；放大 3 倍后应落在当前中点 (160,200)：offset = 160 - 120*3, 200 - 200*3
    expect(offsetOf()).toEqual([160 - 360, 200 - 600]);
  });

  it("捏小后抬一指，剩下那指只平移：不关闭、不翻页，松手回弹", () => {
    const onClose = vi.fn();
    render(<Lightbox items={three} onClose={onClose} />);
    fire(stage(), "pointerdown", 1, 150, 400);
    fire(stage(), "pointerdown", 2, 250, 400);
    fire(stage(), "pointermove", 2, 200, 400);
    expect(scaleOf()).toBe(0.5);
    fire(stage(), "pointerup", 2, 200, 400);
    fire(stage(), "pointermove", 1, 150, 560); // 往下拖 160
    fire(stage(), "pointermove", 1, 0, 560); // 再往左拖 150
    fire(stage(), "pointerup", 1, 0, 560);
    expect(onClose).not.toHaveBeenCalled();
    expect(img().getAttribute("src")).toBe("asset://a.png");
    expect(scaleOf()).toBe(1);
  });

  it("读不到的图显示失败说明，不显示复制", () => {
    vi.stubGlobal("ClipboardItem", class {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { write: vi.fn() } });
    try {
      render(<Lightbox items={one} onClose={() => {}} />);
      expect(screen.getByRole("status", { name: "图片加载中" })).toBeTruthy();
      fireEvent.error(img());
      expect(screen.getByRole("alert").textContent).toContain("a.png");
      expect(screen.queryByRole("button", { name: "复制图片" })).toBeNull();
    } finally {
      vi.unstubAllGlobals();
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("轻点两下放大、再两下复位，且不关闭", () => {
    const onClose = vi.fn();
    render(<Lightbox items={one} onClose={onClose} />);
    fire(img(), "pointerdown", 1, 50, 50, 1000);
    fire(img(), "pointerup", 1, 50, 50, 1000);
    fire(img(), "pointerdown", 2, 50, 50, 1200);
    fire(img(), "pointerup", 2, 50, 50, 1200);
    expect(scaleOf()).toBe(2.5);
    fire(img(), "pointerdown", 3, 50, 50, 2000);
    fire(img(), "pointerup", 3, 50, 50, 2000);
    fire(img(), "pointerdown", 4, 50, 50, 2100);
    fire(img(), "pointerup", 4, 50, 50, 2100);
    expect(scaleOf()).toBe(1);
    expect(onClose).not.toHaveBeenCalled();
  });

  it("轻点一下切换界面显隐（等一拍确认不是双击）", () => {
    vi.useFakeTimers();
    render(<Lightbox items={one} onClose={() => {}} />);
    fire(img(), "pointerdown", 1, 50, 50, 1000);
    fire(img(), "pointerup", 1, 50, 50, 1000);
    expect(document.querySelector(".chat-lightbox")!.getAttribute("data-chrome")).toBeNull();
    act(() => { vi.advanceTimersByTime(320); });
    expect(document.querySelector(".chat-lightbox")!.getAttribute("data-chrome")).toBe("hidden");
  });

  it("适应大小时左滑切到下一张，滑得不够回弹", () => {
    render(<Lightbox items={three} onClose={() => {}} />);
    fire(stage(), "pointerdown", 1, 300, 300);
    fire(stage(), "pointermove", 1, 270, 302);
    fire(stage(), "pointerup", 1, 270, 302);
    expect(img().getAttribute("src")).toBe("asset://a.png");
    expect(offsetOf()).toEqual([0, 0]);
    fire(stage(), "pointerdown", 2, 300, 300);
    fire(stage(), "pointermove", 2, 200, 305);
    fire(stage(), "pointerup", 2, 200, 305);
    expect(img().getAttribute("src")).toBe("asset://b.png");
  });

  it("适应大小时下滑关闭", () => {
    const onClose = vi.fn();
    render(<Lightbox items={one} onClose={onClose} />);
    fire(stage(), "pointerdown", 1, 200, 200);
    fire(stage(), "pointermove", 1, 202, 260);
    fire(stage(), "pointermove", 1, 204, 340);
    fire(stage(), "pointerup", 1, 204, 340);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("放大后单指是平移，不翻页", () => {
    render(<Lightbox items={three} onClose={() => {}} />);
    fireEvent.keyDown(window, { key: "+" });
    fire(stage(), "pointerdown", 1, 300, 300);
    fire(stage(), "pointermove", 1, 150, 300);
    fire(stage(), "pointerup", 1, 150, 300);
    expect(img().getAttribute("src")).toBe("asset://a.png");
    expect(offsetOf()[0]).toBe(-150);
  });
});

describe("复制图片", () => {
  it("没有剪贴板写图能力（手机远程页走 http）时不出现按钮", () => {
    render(<Lightbox items={one} onClose={() => {}} />);
    expect(screen.queryByRole("button", { name: "复制图片" })).toBeNull();
  });

  it("有能力时复制成 PNG 写进剪贴板，按钮显示已复制", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("ClipboardItem", class { constructor(public data: Record<string, Blob>) {} });
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { write } });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, blob: () => Promise.resolve(new Blob(["x"], { type: "image/png" })) }));
    try {
      render(<Lightbox items={one} onClose={() => {}} />);
      fireEvent.load(img()); // 图加载好之后才给复制
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "复制图片" })); });
      expect(write).toHaveBeenCalledTimes(1);
      expect(screen.getByText("已复制")).toBeTruthy();
    } finally {
      vi.unstubAllGlobals();
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });
});

describe("圈出来回复", () => {
  it("对话窗提供了回复入口、图有本地路径且已加载：点了关掉大图，把路径交给标注", () => {
    const reply = vi.fn();
    const onClose = vi.fn();
    render(
      <ImageReplyContext.Provider value={reply}>
        <Lightbox items={[{ src: "asset://a.png", name: "a.png", path: "C:\img\a.png" }]} onClose={onClose} />
      </ImageReplyContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: "圈出来回复" })).toBeNull();
    fireEvent.load(img());
    fireEvent.click(screen.getByRole("button", { name: "圈出来回复" }));
    expect(onClose).toHaveBeenCalled();
    expect(reply).toHaveBeenCalledWith("C:\img\a.png");
  });

  it("没有输入框（无回复入口）或图没有本地路径时不出现", () => {
    const { unmount } = render(<Lightbox items={[{ src: "asset://a.png", name: "a.png", path: "C:\a.png" }]} onClose={() => {}} />);
    fireEvent.load(img());
    expect(screen.queryByRole("button", { name: "圈出来回复" })).toBeNull();
    unmount();
    render(<ImageReplyContext.Provider value={() => {}}><Lightbox items={one} onClose={() => {}} /></ImageReplyContext.Provider>);
    fireEvent.load(img());
    expect(screen.queryByRole("button", { name: "圈出来回复" })).toBeNull();
  });
});
