import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { bootI18n, I18nProvider } from "../../i18n";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { save } = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save }));
vi.mock("../../remoteMode", async (importOriginal) => ({ ...(await importOriginal<typeof import("../../remoteMode")>()), remoteUi: () => true }));

import { useConversationExport } from "./useConversationExport";

const wrapper = ({ children }: { children: ReactNode }) => <I18nProvider>{children}</I18nProvider>;

describe("useConversationExport（手机端）", () => {
  // 字典按需加载（i18n/index.tsx）：jsdom 的 navigator.language 是 en-US，与各窗口入口一样先把字典取到手。
  beforeAll(() => bootI18n());
  afterEach(() => vi.restoreAllMocks());

  /** 手机上导出走浏览器下载：不弹宿主保存框、不调宿主写文件（远程拒绝项），文件落在手机里。 */
  it("导出改为浏览器下载，不碰宿主", async () => {
    const createUrl = vi.fn(() => "blob:test");
    Object.assign(URL, { createObjectURL: createUrl, revokeObjectURL: vi.fn() });
    const clicked: HTMLAnchorElement[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this);
    });
    const onNotice = vi.fn();
    const { result } = renderHook(() => useConversationExport({
      items: [{ type: "user_text", text: "你好" } as never],
      title: "排查: 登录/超时",
      provider: null,
      agents: null,
      onNotice,
      onError: vi.fn(),
    }), { wrapper });
    await act(() => result.current.exportConversation());
    expect(clicked).toHaveLength(1);
    // 文件名去掉了不能用的字符。
    expect(clicked[0].download).toBe("排查  登录 超时.md");
    expect(clicked[0].href).toBe("blob:test");
    expect(createUrl).toHaveBeenCalledTimes(1);
    expect(save).not.toHaveBeenCalled();
    expect(invoke.mock.calls.some((call) => (call as unknown[])[0] === "export_text_file")).toBe(false);
    expect(onNotice).toHaveBeenCalledTimes(1);
  });
});
