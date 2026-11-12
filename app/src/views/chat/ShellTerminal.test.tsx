// 面板终端里的 shell 退出后有「重启」（walkthrough P-13，2026-11-12：以前只剩一行「进程已退出」，
// 得关掉页签再点「+」、目录还要重选；托管终端的退出卡早就有出路，两者对齐）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ShellTerminal } from "./ShellTerminal";

const dataHandler = vi.hoisted(() => ({ current: null as ((data: string) => void) | null }));
const shellEvent = vi.hoisted(() => ({ current: null as ((event: { kind: string; code?: number | null; data?: string; replay?: boolean }) => void) | null }));
const writeSpy = vi.hoisted(() => vi.fn(async () => {}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    open = vi.fn();
    focus = vi.fn();
    dispose = vi.fn();
    loadAddon = vi.fn();
    write = vi.fn();
    hasSelection = () => false;
    getSelection = () => "";
    attachCustomKeyEventHandler = vi.fn();
    onData = (handler: (data: string) => void) => { dataHandler.current = handler; return { dispose: vi.fn() }; };
    onResize = () => ({ dispose: vi.fn() });
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit = vi.fn(); } }));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("@xterm/addon-webgl", () => ({ WebglAddon: class { constructor() { throw new Error("no webgl"); } } }));
vi.mock("@xterm/addon-unicode-graphemes", () => ({ UnicodeGraphemesAddon: class {} }));
vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  attachShellTerminal: vi.fn(async (_id: number, onEvent: (event: never) => void) => { shellEvent.current = onEvent as never; return 1; }),
  detachShellTerminal: vi.fn(async () => {}),
  resizeShellTerminal: vi.fn(async () => {}),
  writeShellTerminal: writeSpy,
}));

// jsdom 没有 ResizeObserver（组件拿它跟面板宽度重排终端）。
class NoopResizeObserver { observe() {} unobserve() {} disconnect() {} }
vi.stubGlobal("ResizeObserver", NoopResizeObserver);

afterEach(cleanup);

describe("ShellTerminal 退出后重启（P-13）", () => {
  it("退出条上有「重启」，点它或在终端里按 Enter 都重启；退出后别的键不往进程里写", async () => {
    const onRestart = vi.fn();
    render(<ShellTerminal id={3} visible onRestart={onRestart} />);
    await act(async () => {});
    dataHandler.current?.("ls\r");
    expect(writeSpy).toHaveBeenCalledWith(3, "ls\r");
    act(() => shellEvent.current?.({ kind: "exit", code: 0 }));
    expect(screen.getByRole("status").textContent).toContain("进程已退出（0）");
    expect(screen.getByText("按 Enter 重启")).toBeTruthy();
    fireEvent.click(screen.getByTestId("shell-restart"));
    expect(onRestart).toHaveBeenCalledTimes(1);
    writeSpy.mockClear();
    dataHandler.current?.("x");
    expect(writeSpy).not.toHaveBeenCalled();
    dataHandler.current?.("\r");
    expect(onRestart).toHaveBeenCalledTimes(2);
  });
});
