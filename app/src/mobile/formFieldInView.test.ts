import { afterEach, describe, expect, it, vi } from "vitest";
import { keepFormFieldInView } from "./formFieldInView";

function rect(top: number, bottom: number): DOMRect {
  return { top, bottom, height: bottom - top, left: 0, right: 375, width: 375, x: 0, y: top, toJSON: () => ({}) } as DOMRect;
}

function mount(fieldTop: number, inForm = true) {
  document.body.innerHTML = `<div class="chat-scroll"><div class="${inForm ? "chat-form" : "other"}"><input /></div></div>`;
  const scroller = document.querySelector<HTMLElement>(".chat-scroll")!;
  const input = document.querySelector("input")!;
  scroller.getBoundingClientRect = () => rect(44, 476);
  input.getBoundingClientRect = () => rect(fieldTop, fieldTop + 32);
  const scrollTo = vi.fn();
  scroller.scrollTo = scrollTo as unknown as HTMLElement["scrollTo"];
  input.focus();
  return scrollTo;
}

describe("keepFormFieldInView：键盘弹起后把表单里聚焦的框挪回可视区", () => {
  afterEach(() => { vi.unstubAllGlobals(); document.body.innerHTML = ""; });

  it("框落到可视区下沿以下（键盘后面）：滚到对话区中间", () => {
    vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => { fn(0); return 0; });
    const scrollTo = mount(600);
    keepFormFieldInView();
    // 框中心 616，对话区中心 260：往下滚 356。
    expect(scrollTo).toHaveBeenCalledWith({ top: 356, behavior: "instant" });
  });

  it("框贴着底边、会被钉住的「提交」那一排盖住：也挪", () => {
    vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => { fn(0); return 0; });
    const scrollTo = mount(400);
    keepFormFieldInView();
    expect(scrollTo).toHaveBeenCalledTimes(1);
  });

  it("框本来就看得见：不动", () => {
    vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => { fn(0); return 0; });
    const scrollTo = mount(200);
    keepFormFieldInView();
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("焦点不在表单里（底部输入框等）：不动", () => {
    vi.stubGlobal("requestAnimationFrame", (fn: FrameRequestCallback) => { fn(0); return 0; });
    const scrollTo = mount(600, false);
    keepFormFieldInView();
    expect(scrollTo).not.toHaveBeenCalled();
  });
});
