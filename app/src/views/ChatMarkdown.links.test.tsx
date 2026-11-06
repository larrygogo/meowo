import { describe, it, expect, vi, afterEach } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { cleanup, fireEvent, render } from "@testing-library/react";
import { ChatMarkdown, codeLinkKind } from "./ChatMarkdown";

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
