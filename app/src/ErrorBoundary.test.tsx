import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

// react-markdown 换成会抛错的桩：模拟预检没拦住的渲染异常（比如某个未知的病态输入）。
vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => {
    if (children.includes("BOOM")) throw new Error("markdown 渲染炸了");
    return <p>{children}</p>;
  },
}));

import { WindowErrorBoundary } from "./ErrorBoundary";
import { Message } from "./views/chat/Message";
import { ChatMarkdown } from "./views/ChatMarkdown";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function Thrower(): never {
  throw new Error("窗口里某个组件炸了");
}

describe("窗口级 ErrorBoundary", () => {
  it("子树抛错：显示「窗口遇到问题」和「重新加载」，并记录日志，不白屏", () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    const reload = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, reload } });
    try {
      render(<WindowErrorBoundary><Thrower /></WindowErrorBoundary>);
      expect(screen.getByRole("alert").textContent).toContain("窗口遇到问题");
      fireEvent.click(screen.getByRole("button", { name: "重新加载" }));
      expect(reload).toHaveBeenCalled();
      expect(logged.mock.calls.some((args) => String(args[0]).includes("[window]"))).toBe(true);
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
    }
  });
});

describe("消息级 ErrorBoundary", () => {
  it("一条消息渲染失败只把它换成原文，旁边的消息照常", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const { container } = render(
      <>
        <Message item={{ type: "assistant_text", id: "bad", timestamp: null, text: "**BOOM** 原文保留" }} />
        <Message item={{ type: "assistant_text", id: "ok", timestamp: null, text: "正常的一条" }} />
      </>,
    );
    expect(container.querySelector("pre.chat-md-plain")?.textContent).toBe("**BOOM** 原文保留");
    expect(screen.getByText("正常的一条")).toBeTruthy();
  });

  it("流式正文变了就重试：后一帧正常时恢复排版", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const { container, rerender } = render(<ChatMarkdown text="BOOM 半截" />);
    expect(container.querySelector("pre.chat-md-plain")).toBeTruthy();
    rerender(<ChatMarkdown text="完整的一段" />);
    expect(container.querySelector("pre.chat-md-plain")).toBeNull();
    expect(screen.getByText("完整的一段")).toBeTruthy();
  });
});
