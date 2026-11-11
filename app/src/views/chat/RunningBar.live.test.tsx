// 运行条读屏播报（调研 i10）：live region 只放稳定状态句，CLI 动词（每几秒换）和活动行
// （每次工具调用换）不在 live region 里；回合结束播一次「回复完成」。
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { I18nProvider } from "../../i18n";
import { RunningBar, TurnAnnouncer, useCliStatusStore } from "./RunningBar";

afterEach(cleanup);

function Bar({ verb }: { verb: string }) {
  const store = useCliStatusStore();
  return <RunningBar store={store} turnStartedAt={Date.now()} override={verb} fallbackVerb="工作中…" activity={{ text: "Read: src/a.ts", tip: undefined }} />;
}

const liveRegions = (root: HTMLElement) =>
  Array.from(root.querySelectorAll<HTMLElement>('[aria-live]:not([aria-live="off"]), [role="status"], [role="log"], [role="alert"]'));

describe("运行条 live region", () => {
  it("状态动词与活动行都不在任何 live region 里", () => {
    const { container } = render(<I18nProvider initial="zh"><Bar verb="Pondering…" /></I18nProvider>);
    const verb = container.querySelector(".chat-running-verb")!;
    const activity = container.querySelector(".chat-running-activity")!;
    expect(verb.textContent).toBe("Pondering…");
    for (const region of liveRegions(container)) {
      expect(region.contains(verb)).toBe(false);
      expect(region.contains(activity)).toBe(false);
    }
  });

  it("回合进行中播「工作中」，结束播一次「回复完成」，切会话不播", () => {
    const view = (running: boolean, sessionId = 1) => <I18nProvider initial="zh"><TurnAnnouncer running={running} sessionId={sessionId} /></I18nProvider>;
    const { container, rerender } = render(view(false));
    const region = container.querySelector<HTMLElement>('[role="status"]')!;
    expect(region).not.toBeNull();
    expect(region.getAttribute("aria-live")).toBe("polite");
    expect(region.textContent).toBe("");
    act(() => rerender(view(true)));
    expect(region.textContent).toBe("工作中");
    // 运行中状态句保持不变：上游动词怎么换，这里都不会重播。
    act(() => rerender(view(true)));
    expect(region.textContent).toBe("工作中");
    act(() => rerender(view(false)));
    expect(region.textContent).toBe("回复完成");
    // 同一节点持续存在（卸载重挂的 live region 读屏不播）。
    expect(container.querySelector('[role="status"]')).toBe(region);
    // 切到别的会话：清空，不把上一个会话的「回复完成」算到新会话头上。
    act(() => rerender(view(true, 2)));
    act(() => rerender(view(false, 3)));
    expect(region.textContent).toBe("");
  });
});
