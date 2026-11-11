// 审批卡 / 提问卡出现时读屏要听得到，但焦点不能被抢走（首焦点在 composer，防打字误拒）。
import { act, cleanup, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApprovalCard } from "./ApprovalCard";
import { CardAnnouncer } from "./cardAnnouncer";

afterEach(() => { cleanup(); vi.useRealTimers(); });

function Host() {
  const [card, setCard] = useState<null | "approval" | "question">(null);
  return (
    <>
      <CardAnnouncer />
      <textarea aria-label="composer" />
      <button type="button" onClick={() => setCard("approval")}>approval</button>
      <button type="button" onClick={() => setCard("question")}>question</button>
      <button type="button" onClick={() => setCard(null)}>none</button>
      {card === "approval" && <ApprovalCard key="a" title="Agent 请求权限" badge="待批准" actions={<button type="button">拒绝</button>} />}
      {card === "question" && <ApprovalCard key="q" title="Agent 正在等待你的回答" badge="待回答" actions={<button type="button">提交</button>} />}
    </>
  );
}

describe("卡片出现的读屏播报", () => {
  it("卡片挂载时 live region 播标题，焦点仍在 composer", () => {
    vi.useFakeTimers();
    render(<Host />);
    const region = screen.getByTestId("card-announcer");
    expect(region.getAttribute("aria-live")).toBe("polite");
    const composer = screen.getByLabelText("composer");
    composer.focus();
    act(() => { screen.getByText("approval").click(); composer.focus(); });
    act(() => { vi.advanceTimersByTime(100); });
    expect(region.textContent).toBe("Agent 请求权限");
    expect(document.activeElement).toBe(composer);
    act(() => { screen.getByText("question").click(); composer.focus(); });
    act(() => { vi.advanceTimersByTime(100); });
    expect(region.textContent).toBe("Agent 正在等待你的回答");
  });

  it("连着两张同标题的卡也会再播一次（先清空再写）", () => {
    vi.useFakeTimers();
    render(<Host />);
    const region = screen.getByTestId("card-announcer");
    act(() => { screen.getByText("approval").click(); });
    act(() => { vi.advanceTimersByTime(100); });
    act(() => { screen.getByText("none").click(); });
    act(() => { screen.getByText("approval").click(); });
    expect(region.textContent).toBe("");
    act(() => { vi.advanceTimersByTime(100); });
    expect(region.textContent).toBe("Agent 请求权限");
  });
});
