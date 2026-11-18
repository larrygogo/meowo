// 运行条的自动重试说法：「API 无响应、等待重试」此前不认（没有次数），只显示「工作中…」。
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { I18nProvider } from "../../i18n";
import { en } from "../../i18n/en";
import { zh } from "../../i18n/zh";
import { RunningBar, useCliStatusStore, type CliStatusStore } from "./RunningBar";

afterEach(cleanup);

function Bar({ onStore }: { onStore: (store: CliStatusStore) => void }) {
  const store = useCliStatusStore();
  onStore(store);
  return <RunningBar store={store} turnStartedAt={Date.now()} override={null} fallbackVerb="工作中…" activity={null} />;
}

describe("运行条：自动重试", () => {
  it("API 无响应时说出倒计时与「检查网络」，用警示文字色", () => {
    let store: CliStatusStore | null = null;
    const { container } = render(<I18nProvider initial="zh"><Bar onStore={(s) => { store = s; }} /></I18nProvider>);
    act(() => store!.setRetry({ kind: "stalled", retryIn: "1m 9s" }));
    const verb = container.querySelector(".chat-running-verb")!;
    expect(verb.textContent).toBe("API 无响应，1 分 09 秒后重试，检查网络");
    expect(verb.className).toContain("is-retry");
    act(() => store!.setRetry({ kind: "attempt", attempt: 2, max: 10 }));
    expect(verb.textContent).toBe("请求失败，正在重试（第 2/10 次）");
  });

  it("倒计时换算：秒、分秒、认不出照原样", () => {
    expect(zh.chat.workingStalled("45s")).toBe("API 无响应，45 秒后重试，检查网络");
    expect(zh.chat.workingStalled("5m")).toBe("API 无响应，5 分后重试，检查网络");
    expect(zh.chat.workingStalled("1h 3m")).toBe("API 无响应，1 小时 3 分后重试，检查网络");
    expect(zh.chat.workingStalled("soon")).toBe("API 无响应，soon 后重试，检查网络");
    expect(en.chat.workingStalled("1m 9s")).toBe("No response from the API. Retrying in 1m 9s. Check your network.");
  });
});
