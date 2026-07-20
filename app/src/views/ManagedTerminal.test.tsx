import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
const write = vi.hoisted(() => vi.fn());
const eventHandlers = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, handler: (event: { payload: unknown }) => void) => {
    eventHandlers.set(event, handler);
    return Promise.resolve(() => eventHandlers.delete(event));
  }),
}));

// xterm 需要真实 canvas/DOM 度量，jsdom 跑不了；这里只关心遮罩状态机，故把终端替换成哑实现。
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    write = (data: Uint8Array | string, callback?: () => void) => { write(data); callback?.(); };
    open = vi.fn();
    reset = vi.fn();
    focus = vi.fn();
    dispose = vi.fn();
    loadAddon = vi.fn();
    onData = vi.fn(() => ({ dispose: vi.fn() }));
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit = vi.fn(); } }));

import { ManagedTerminal } from "./ManagedTerminal";

const noPty = { sessionId: 163, active: false, data: "", startOffset: 0, endOffset: 0, exited: false, exitCode: null };

describe("ManagedTerminal", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    write.mockReset();
    eventHandlers.clear();
    global.ResizeObserver = class {
      observe = vi.fn();
      disconnect = vi.fn();
    } as unknown as typeof ResizeObserver;
  });

  it("offers takeover when the session is still running in an external terminal", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") return Promise.resolve(noPty);
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="running" />);
    // 没有托管 PTY 的运行中会话必须给出接管入口，而不是把用户丢在一块黑屏上。
    expect(await screen.findByRole("button", { name: "结束外部进程并接管" })).toBeTruthy();
    expect(screen.getByText(/会话仍在外部终端运行/)).toBeTruthy();
  });

  it("offers a plain start for a disconnected session", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") return Promise.resolve(noPty);
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="ended" />);
    expect(await screen.findByRole("button", { name: "在 Meowo 中接管" })).toBeTruthy();
  });

  it("shows the initializing cover until the managed PTY produces its first output", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: "" });
      }
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="running" />);
    // active 但还没有首屏输出：必须停在初始化态，不能是无提示的黑屏。
    await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
    expect(screen.getByText("正在初始化 Agent…")).toBeTruthy();
  });

  it("stays in the initializing cover while a resuming TUI only emits control sequences", async () => {
    // claude --resume 的真实首帧：查光标位置、进 alt screen、清屏、清 40 行——
    // 一个字都还没画。此前只要有字节就撤遮罩，用户面对的就是一块无提示的纯黑屏。
    const loading = "\x1b[6n\x1b[?25l\x1b[?1049h\x1b[2J" + "\x1b[K\r\n".repeat(40) + "\x1b[H";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: btoa(loading) });
      }
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="running" />);
    await waitFor(() => expect(write).toHaveBeenCalled());
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.getByText("正在初始化 Agent…")).toBeTruthy();
  });

  it("leaves the initializing cover once the TUI actually paints something", async () => {
    // btoa 只吃 latin1，这里用纯 ASCII 表示 TUI 画出的第一段文字。
    const painted = "\x1b[?1049h\x1b[2J\x1b[H\x1b[32mWelcome to Claude Code\x1b[0m";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: btoa(painted) });
      }
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="running" />);
    await waitFor(() => expect(write).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("reports a trust prompt during direct GUI takeover and uncovers the TUI", async () => {
    const attention = vi.fn();
    const prompt = "\x1b[2JDo you trust the contents of this directory?\r\n  Yes\r\n  No";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: btoa(prompt), endOffset: prompt.length });
      }
      return Promise.resolve();
    });
    render(
      <ManagedTerminal
        sessionId={163}
        status="running"
        attentionMarkers={["do you trust the contents of this directory"]}
        onAttention={attention}
      />,
    );
    await waitFor(() => expect(attention).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("reports an already-painted trust prompt when provider markers arrive later", async () => {
    const attention = vi.fn();
    const prompt = "\x1b[2JDo you trust the files in this folder?\r\n  Yes\r\n  No";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: btoa(prompt), endOffset: prompt.length });
      }
      return Promise.resolve();
    });
    const view = render(<ManagedTerminal sessionId={163} status="running" onAttention={attention} />);
    await waitFor(() => expect(write).toHaveBeenCalled());
    expect(attention).not.toHaveBeenCalled();

    view.rerender(
      <ManagedTerminal
        sessionId={163}
        status="running"
        attentionMarkers={["do you trust the files in this folder"]}
        onAttention={attention}
      />,
    );
    await waitFor(() => expect(attention).toHaveBeenCalledTimes(1));
  });

  it("reports the token/auth step that appears after the trust step", async () => {
    const attention = vi.fn();
    const trust = "\x1b[2JDo you trust the files in this folder?\r\n  Yes\r\n  No";
    const auth = "\x1b[2JOAuth token has been revoked\r\nRun /login to sign in\r\nPress Enter to continue";
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return Promise.resolve({ ...noPty, active: true, data: btoa(trust), endOffset: trust.length });
      }
      return Promise.resolve();
    });
    render(
      <ManagedTerminal
        sessionId={163}
        status="running"
        attentionMarkers={["do you trust the files in this folder"]}
        onAttention={attention}
      />,
    );
    await waitFor(() => expect(attention).toHaveBeenCalledTimes(1));
    eventHandlers.get("pty-output")!({ payload: { sessionId: 163, offset: trust.length, data: btoa(auth) } });
    await waitFor(() => expect(attention).toHaveBeenCalledTimes(2));
    expect(attention.mock.calls[1][0].text).toContain("OAuth token has been revoked");
  });

  it("orders and deduplicates live output buffered while the initial snapshot is loading", async () => {
    let resolveSnapshot!: (value: typeof noPty) => void;
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        return new Promise((resolve) => { resolveSnapshot = resolve; });
      }
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="running" />);
    await waitFor(() => expect(eventHandlers.has("pty-output")).toBe(true));
    await waitFor(() => expect(resolveSnapshot).toBeTypeOf("function"));
    // DEF 已包含在稍后返回的 ABCDEF 快照里；offset 让前端只写一次完整内容。
    eventHandlers.get("pty-output")!({ payload: { sessionId: 163, offset: 3, data: btoa("DEF") } });
    resolveSnapshot({ ...noPty, active: true, data: btoa("ABCDEF"), endOffset: 6 });
    await waitFor(() => expect(write).toHaveBeenCalled());
    expect(write).toHaveBeenCalledTimes(1);
    expect(new TextDecoder().decode(write.mock.calls[0][0])).toBe("ABCDEF");
  });

  it("realigns the output offset when the PTY is restarted in place", async () => {
    // 结束终端 → 再接管：新 PTY 的 output_end 从 0 重新计数。若沿用上一个进程的
    // nextOffset（这里是 7），新输出会被判成「已写过」而整段丢弃，终端定格在旧内容上。
    let snapshots = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "managed_terminal_snapshot") {
        snapshots += 1;
        return snapshots === 1
          // 上一个进程留下 7 字节输出后退出。
          ? Promise.resolve({ ...noPty, data: btoa("OLDDATA"), endOffset: 7, exited: true, exitCode: 0 })
          // 接管后的新 PTY：偏移归零，还没有输出。
          : Promise.resolve({ ...noPty, active: true, data: "", endOffset: 0 });
      }
      return Promise.resolve();
    });
    render(<ManagedTerminal sessionId={163} status="ended" />);
    const takeover = await screen.findByRole("button", { name: "在 Meowo 中接管" });
    write.mockReset();
    takeover.click();
    await waitFor(() => expect(snapshots).toBe(2));
    // 新进程的首段输出比旧的短（2 < 7），偏移没归零的话会被整段吞掉。
    eventHandlers.get("pty-output")!({ payload: { sessionId: 163, offset: 0, data: btoa("HI") } });
    await waitFor(() => expect(write).toHaveBeenCalled());
    expect(new TextDecoder().decode(write.mock.calls[0][0])).toBe("HI");
  });
});
