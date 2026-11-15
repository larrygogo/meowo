import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock, convertFileSrc: (p: string) => p }));

import { Message } from "./Message";
import { ChatActionsContext } from "./chatActions";
import type { ChatItem } from "../../api";

afterEach(cleanup);

/// 回合错误气泡(turn_error):CC 把 API Error 写成 model=<synthetic> 的系统插入文案,
/// 后端解析降级成独立的 turn_error 条目。此前它披着模型正文的皮平铺在对话里,
/// 用户读不出这是故障——本测试钉住「错误必须套错误皮」这条回归。
describe("Message turn_error", () => {
  const item: ChatItem = {
    type: "turn_error",
    id: "e1",
    timestamp: null,
    label: "API 请求出错",
    text: "API Error: Overloaded",
  };

  it("渲染成 role=alert 的错误气泡,标签与原文都可见", () => {
    render(<Message item={item} />);
    const alert = screen.getByRole("alert");
    expect(alert.className).toContain("is-turn-error");
    // 标签(与看板卡片 error_label 同源)和原文都要在——标签给一眼判断,原文给排查。
    expect(screen.getByText("API 请求出错")).toBeTruthy();
    expect(screen.getByText("API Error: Overloaded")).toBeTruthy();
  });

  it("不套模型正文的皮(is-assistant)", () => {
    render(<Message item={item} />);
    expect(screen.getByRole("alert").className).not.toContain("is-assistant");
  });

  it("最新的出错卡给「继续」，点击以用户身份发送「继续」；旧的出错卡不给", () => {
    const sendText = vi.fn().mockResolvedValue(true);
    const { rerender } = render(
      <ChatActionsContext.Provider value={{ sendText }}>
        <Message item={item} latest />
      </ChatActionsContext.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "继续" }));
    expect(sendText).toHaveBeenCalledWith("继续");
    rerender(
      <ChatActionsContext.Provider value={{ sendText }}>
        <Message item={item} />
      </ChatActionsContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: "继续" })).toBeNull();
  });

  it("登录失效：提示先登录，「去登录」直达设置的账号页，「继续」仍在", () => {
    invokeMock.mockResolvedValue(undefined);
    const sendText = vi.fn().mockResolvedValue(true);
    render(
      <ChatActionsContext.Provider value={{ sendText }}>
        <Message item={{ ...item, label: "需要重新登录", text: "Not logged in · Please run /login" }} latest />
      </ChatActionsContext.Provider>,
    );
    expect(screen.getByText("重新登录后点「继续」")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "去登录" }));
    expect(localStorage.getItem("meowo-settings-section")).toBe("account");
    expect(invokeMock).toHaveBeenCalledWith("open_settings");
    expect(screen.getByRole("button", { name: "继续" })).toBeTruthy();
  });

  it("连不上 API 提示查网络；一般 API 错误不加提示也不给「去登录」", () => {
    const sendText = vi.fn().mockResolvedValue(true);
    const { rerender } = render(
      <ChatActionsContext.Provider value={{ sendText }}>
        <Message item={{ ...item, label: "无法连接 API" }} latest />
      </ChatActionsContext.Provider>,
    );
    expect(screen.getByText("检查网络或代理后点「继续」")).toBeTruthy();
    rerender(
      <ChatActionsContext.Provider value={{ sendText }}>
        <Message item={item} latest />
      </ChatActionsContext.Provider>,
    );
    expect(screen.queryByText("检查网络或代理后点「继续」")).toBeNull();
    expect(screen.queryByRole("button", { name: "去登录" })).toBeNull();
  });

  // 实拍：Kimi 读了动图 webp 后每轮 400，用户连点三次「继续」全错。「继续」是死路。
  it("请求被拒：说明原因，只给「新开会话」，不给必错的「继续」", () => {
    const sendText = vi.fn().mockResolvedValue(true);
    const newSession = vi.fn();
    render(
      <ChatActionsContext.Provider value={{ sendText, newSession }}>
        <Message item={{ ...item, label: "请求被拒", text: "Invalid request Error" }} latest />
      </ChatActionsContext.Provider>,
    );
    expect(screen.getByText("请求被拒")).toBeTruthy();
    expect(screen.getByText("对话里有接口不收的内容，请新开会话")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "继续" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "新开会话" }));
    expect(newSession).toHaveBeenCalledTimes(1);
    expect(sendText).not.toHaveBeenCalled();
  });

  it("请求被拒的旧卡不出按钮；其他错误不出「新开会话」", () => {
    const sendText = vi.fn().mockResolvedValue(true);
    const newSession = vi.fn();
    const { rerender } = render(
      <ChatActionsContext.Provider value={{ sendText, newSession }}>
        <Message item={{ ...item, label: "请求被拒" }} />
      </ChatActionsContext.Provider>,
    );
    expect(screen.queryByRole("button")).toBeNull();
    rerender(
      <ChatActionsContext.Provider value={{ sendText, newSession }}>
        <Message item={item} latest />
      </ChatActionsContext.Provider>,
    );
    expect(screen.queryByRole("button", { name: "新开会话" })).toBeNull();
    expect(screen.getByRole("button", { name: "继续" })).toBeTruthy();
  });
});

describe("请求被拒的英文文案", () => {
  it("标签与提示都有英文", async () => {
    const { en } = await import("../../i18n/en");
    expect(en.errorLabels["请求被拒"]).toBe("Request rejected");
    expect(en.chat.turnErrorNewSession).toBe("New session");
  });
});
