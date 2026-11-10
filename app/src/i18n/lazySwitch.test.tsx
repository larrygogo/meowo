import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";

const handlers: ((e: { payload: unknown }) => void)[] = [];
vi.mock("../api", () => ({ getSettings: () => new Promise(() => {}) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, handler: (e: { payload: unknown }) => void) => {
    handlers.push(handler);
    return Promise.resolve(() => {});
  },
}));

import { I18nProvider, loadedDict, useT } from "./index";

function Probe() {
  const t = useT();
  return <span data-testid="probe">{t.locale}</span>;
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

// 单独一个文件：这里的英文字典一开始没加载，验证「切到还没加载的语言」这条异步路径。
describe("i18n 切到未加载的语言", () => {
  it("加载期间保持旧语言（不空白、不出 key），到位后换成新语言", async () => {
    localStorage.setItem("meowo-lang", "zh");
    expect(loadedDict("en")).toBeNull();
    render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.getByTestId("probe").textContent).toBe("zh-CN");
    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    act(() => handlers.forEach((h) => h({ payload: { language: "en" } })));
    // 字典还在路上：旧语言照常显示。
    expect(screen.getByTestId("probe").textContent).toBe("zh-CN");
    await waitFor(() => expect(screen.getByTestId("probe").textContent).toBe("en-US"));
    expect(localStorage.getItem("meowo-lang")).toBe("en");
  });
});
