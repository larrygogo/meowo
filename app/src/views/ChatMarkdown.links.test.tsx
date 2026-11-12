import { describe, it, expect, vi, afterEach } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { act, cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { ChatMarkdown, codeLinkKind, stripLineSuffix } from "./ChatMarkdown";

afterEach(() => {
  cleanup();
  invoke.mockClear();
});

describe("行内代码里的网址和路径", () => {
  it("只认网址和绝对路径；斜杠命令、相对路径、普通代码不算", () => {
    expect(codeLinkKind("https://github.com/larrygogo/meowo/pull/142")).toBe("url");
    expect(codeLinkKind("C:\\Users\\me\\meowo\\Meowo_0.8.0_x64-installer.exe")).toBe("path");
    expect(codeLinkKind("D:/dev/web-app")).toBe("path");
    expect(codeLinkKind("\\\\nas\\share\\a.txt")).toBe("path");
    expect(codeLinkKind("/Users/me/project/a.ts")).toBe("path");
    expect(codeLinkKind("/model")).toBeNull();
    expect(codeLinkKind("src/main.ts")).toBeNull();
    expect(codeLinkKind("npm test")).toBeNull();
    expect(codeLinkKind("javascript:alert(1)")).toBeNull();
  });

  it("点路径在文件夹里选中，点网址在浏览器打开", () => {
    const path = "C:\\Users\\me\\Meowo_0.8.0_x64-installer.exe";
    const { container } = render(<ChatMarkdown text={`装这个：\`${path}\`，PR 在 \`https://github.com/x/y/pull/1\``} />);
    const codes = container.querySelectorAll("code.chat-md-link-code");
    expect(codes).toHaveLength(2);
    fireEvent.click(codes[0]);
    expect(invoke).toHaveBeenCalledWith("reveal_terminal_path", { cwd: "", path });
    fireEvent.click(codes[1]);
    expect(invoke).toHaveBeenCalledWith("open_link", expect.objectContaining({ url: "https://github.com/x/y/pull/1" }));
  });

  it("带行号的路径：定位前剥掉 :行[:列] 与 (行,列)", () => {
    expect(stripLineSuffix("C:\\repo\\src\\main.ts:42")).toBe("C:\\repo\\src\\main.ts");
    expect(stripLineSuffix("/home/me/app/a.ts:10:5")).toBe("/home/me/app/a.ts");
    expect(stripLineSuffix("C:\\repo\\a.cpp(12,5)")).toBe("C:\\repo\\a.cpp");
    expect(stripLineSuffix("C:\\repo\\a.cpp(12)")).toBe("C:\\repo\\a.cpp");
    expect(stripLineSuffix("C:\\repo\\v1.2")).toBe("C:\\repo\\v1.2");
    const { container } = render(<ChatMarkdown text={"`/home/me/app/a.ts:10:5`"} />);
    fireEvent.click(container.querySelector("code.chat-md-link-code")!);
    expect(invoke).toHaveBeenCalledWith("reveal_terminal_path", { cwd: "", path: "/home/me/app/a.ts" });
  });

  it("定位失败就地提示，不静默", async () => {
    invoke.mockImplementationOnce(() => Promise.reject("路径不存在"));
    const { container } = render(<ChatMarkdown text={"`C:\\gone\\a.exe`"} />);
    fireEvent.click(container.querySelector("code.chat-md-link-code")!);
    await waitFor(() => expect(container.querySelector(".chat-md-link-error [role=status]")?.textContent).toBe("路径不存在"));
  });

  // 做空第一轮 #13（2026-11-12）：原先 4 秒自收，带原因的提示没读完就没了。现在停到用户关或下一次成功。
  it("失败提示不自动消失：10 秒后仍在，× 关掉后焦点回链接", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      invoke.mockImplementationOnce(() => Promise.reject("路径不存在"));
      const { container, getByRole } = render(<ChatMarkdown text={"`C:\\gone\\a.exe`"} />);
      const code = container.querySelector<HTMLElement>("code.chat-md-link-code")!;
      fireEvent.click(code);
      await waitFor(() => expect(container.querySelector(".chat-md-link-error")).not.toBeNull());
      await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
      expect(container.querySelector(".chat-md-link-error [role=status]")?.textContent).toBe("路径不存在");
      const close = getByRole("button", { name: "关闭" });
      close.focus();
      fireEvent.click(close);
      expect(container.querySelector(".chat-md-link-error")).toBeNull();
      expect(document.activeElement).toBe(code);
    } finally {
      vi.useRealTimers();
    }
  });

  it("再点一次成功就收起失败提示", async () => {
    invoke.mockImplementationOnce(() => Promise.reject("路径不存在"));
    const { container } = render(<ChatMarkdown text={"`C:\\gone\\a.exe`"} />);
    const code = container.querySelector<HTMLElement>("code.chat-md-link-code")!;
    fireEvent.click(code);
    await waitFor(() => expect(container.querySelector(".chat-md-link-error")).not.toBeNull());
    fireEvent.click(code);
    await waitFor(() => expect(container.querySelector(".chat-md-link-error")).toBeNull());
  });

  it("拖选文字松手不算点击", () => {
    const { container } = render(<ChatMarkdown text={"`C:\\Users\\me\\a.exe`"} />);
    const code = container.querySelector("code.chat-md-link-code")!;
    const spy = vi.spyOn(window, "getSelection").mockReturnValue({ toString: () => "Users" } as unknown as Selection);
    fireEvent.click(code);
    spy.mockRestore();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("代码块里的路径不变成链接", () => {
    const { container } = render(<ChatMarkdown text={"```\nC:\\Users\\me\\a.exe\n```"} />);
    expect(container.querySelector("code.chat-md-link-code")).toBeNull();
  });
});
