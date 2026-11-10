import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";

// 设置拉取永不返回、事件订阅记下回调：语言只由本地缓存与手动派发的 settings-changed 决定。
const handlers: ((e: { payload: unknown }) => void)[] = [];
vi.mock("../api", () => ({ getSettings: () => new Promise(() => {}) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, handler: (e: { payload: unknown }) => void) => {
    handlers.push(handler);
    return Promise.resolve(() => {});
  },
}));

import { bootI18n, I18nProvider, loadedDict, useT } from "./index";

function Probe() {
  const t = useT();
  return <span data-testid="probe">{t.locale}</span>;
}

afterEach(() => {
  cleanup();
  handlers.length = 0;
  localStorage.clear();
});

// 用例有先后：模块级字典缓存在同一文件里共享，第一个用例必须在 en 还没加载时跑。
describe("i18n 字典按需加载", () => {
  it("英文字典不随入口同步打包；没等 boot 时 Provider 先不渲染，绝不先闪中文", async () => {
    expect(loadedDict("en")).toBeNull();
    expect(loadedDict("zh")?.locale).toBe("zh-CN");
    localStorage.setItem("meowo-lang", "en");
    render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.queryByTestId("probe")).toBeNull();
    expect(await screen.findByTestId("probe")).toHaveProperty("textContent", "en-US");
    expect(document.documentElement.lang).toBe("en");
  });

  it("入口 await bootI18n 之后，首帧同步就是缓存里的语言", async () => {
    localStorage.setItem("meowo-lang", "en");
    await bootI18n();
    render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.getByTestId("probe").textContent).toBe("en-US");
  });

  it("切换语言即时生效（已加载的字典同步换）", async () => {
    localStorage.setItem("meowo-lang", "zh");
    render(<I18nProvider><Probe /></I18nProvider>);
    expect(screen.getByTestId("probe").textContent).toBe("zh-CN");
    await waitFor(() => expect(handlers.length).toBeGreaterThan(0));
    act(() => handlers.forEach((h) => h({ payload: { language: "en" } })));
    expect(screen.getByTestId("probe").textContent).toBe("en-US");
    act(() => handlers.forEach((h) => h({ payload: { language: "zh" } })));
    expect(screen.getByTestId("probe").textContent).toBe("zh-CN");
  });
});
