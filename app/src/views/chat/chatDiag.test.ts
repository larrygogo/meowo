import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn((..._args: unknown[]) => Promise.resolve()));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  bindChatDiagSession, chatDiagLine, logChatEvent, maskScreenLine, outgoingDiagEvents, safeDiagValue,
} from "./chatDiag";
import type { OutgoingPhase } from "./outgoingPrompts";

const REMOTE_FLAG = "__MEOWO_REMOTE__";
const logged = () => invoke.mock.calls.filter(([command]) => command === "diag_log")
  .map(([, args]) => args as { scope: string; message: string });

beforeEach(() => invoke.mockClear());
afterEach(() => { delete (globalThis as Record<string, unknown>)[REMOTE_FLAG]; });

/// 验收 19：四类事件各写一行日志，带 Provider、会话 id、原因，不带消息正文和密钥。
describe("对话窗运行日志的一行", () => {
  it("固定字段：种类、Provider、会话 id、原因，附加键值跟在后面", () => {
    expect(chatDiagLine(42, "kimi", { kind: "send_failed", reason: "terminal_write_failed", detail: { from: "sending", queue: false } }))
      .toBe("kind=send_failed · provider=kimi · session=42 · reason=terminal_write_failed · from=sending · queue=false");
  });

  it("Provider 未知时照实写 unknown，空的附加值不写", () => {
    expect(chatDiagLine(-3, null, { kind: "switch_unconfirmed", reason: "no_confirm_line", detail: { what: "model", level: undefined, note: "" } }))
      .toBe("kind=switch_unconfirmed · provider=unknown · session=-3 · reason=no_confirm_line · what=model");
  });

  it("值压成一行、截到 120 字、隐去像密钥的串", () => {
    expect(safeDiagValue("第一行\n第二行\t尾巴")).toBe("第一行 第二行 尾巴");
    expect(safeDiagValue("x".repeat(31) + " " + "y".repeat(200)).length).toBeLessThanOrEqual(121);
    for (const secret of ["sk-test-FAKEFAKEFAKE1234", "Bearer abc.def.ghi", "ghp_abcdefghijklmnopqrstuvwxyz0123456789", "A".repeat(40)]) {
      const line = chatDiagLine(1, "claude", { kind: "switch_failed", reason: `auth failed ${secret}` });
      expect(line, secret).not.toContain(secret);
      expect(line).toContain("[已隐去]");
    }
    // 普通的后端错误码原样保留。
    expect(safeDiagValue("settings/server_not_ready")).toBe("settings/server_not_ready");
  });

  it("终端画面只留轮廓：文字换成 x，数字换成 9，标点与框线原样", () => {
    expect(maskScreenLine("❯ 1. 允许这次 rm -rf build")).toBe("❯ 9. xxxx xx -xx xxxxx");
    expect(maskScreenLine("│ [ ] Deploy to prod? │")).toBe("│ [ ] xxxxxx xx xxxx? │");
    const masked = maskScreenLine("我的密码是 hunter2");
    expect(masked).not.toMatch(/密码|hunter/);
  });
});

describe("写日志", () => {
  it("经 diag_log 落到同一份日志，按会话 id 查到登记的 Provider", () => {
    bindChatDiagSession(77, "codex");
    logChatEvent(77, { kind: "send_unconfirmed", reason: "echo_timeout" });
    expect(logged()).toEqual([{ scope: "chat", message: "kind=send_unconfirmed · provider=codex · session=77 · reason=echo_timeout" }]);
  });

  it("手机远程页不写（远程桥不放行 diag_log）", () => {
    (globalThis as Record<string, unknown>)[REMOTE_FLAG] = true;
    logChatEvent(77, { kind: "send_failed", reason: "terminal_write_failed" });
    expect(logged()).toEqual([]);
  });
});

describe("发出的消息落进失败或未确认", () => {
  const prev = (entries: [number, OutgoingPhase][]) => new Map(entries);
  const prompt = (id: number, phase: OutgoingPhase, kind: "echo" | "queued" = "echo") => ({ id, phase, kind });

  it("sending → failed：没写进终端", () => {
    expect(outgoingDiagEvents(prev([[1, "sending"]]), [prompt(1, "failed")]))
      .toEqual([{ kind: "send_failed", reason: "terminal_write_failed", detail: { from: "sending", queue: false } }]);
  });

  it("pending → uncertain：写进去了，等回显超时；sending → uncertain：写入迟迟不返回", () => {
    expect(outgoingDiagEvents(prev([[1, "pending"], [2, "sending"]]), [prompt(1, "uncertain", "queued"), prompt(2, "uncertain")])).toEqual([
      { kind: "send_unconfirmed", reason: "echo_timeout", detail: { from: "pending", queue: true } },
      { kind: "send_unconfirmed", reason: "terminal_write_stalled", detail: { from: "sending", queue: false } },
    ]);
  });

  it("阶段没变、或走的是正常路径（已排队、已插入）不记", () => {
    expect(outgoingDiagEvents(prev([[1, "failed"], [2, "pending"], [3, "pending"], [4, "uncertain"]]),
      [prompt(1, "failed"), prompt(2, "accepted"), prompt(3, "inserted"), prompt(4, "uncertain")])).toEqual([]);
  });

  it("事件里没有消息正文的位置", () => {
    const events = outgoingDiagEvents(prev([[1, "sending"]]), [{ ...prompt(1, "failed"), text: "这是用户写的正文" } as never]);
    expect(JSON.stringify(events)).not.toContain("这是用户写的正文");
  });
});
