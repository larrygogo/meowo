import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ChatHistory, ModelCatalogEntry } from "../../api";
import { zh } from "../../i18n/zh";
import { chatUi } from "../../test/agents";
import { useModelControls, type ModelControlsDeps } from "./useModelControls";

// 假 PTY：out 是终端累计输出；快照按 since 切片并 base64，与真实 managed_terminal_snapshot 同形。
const pty = vi.hoisted(() => ({ out: "", onWrite: null as null | ((sessionId: number, data: string) => void) }));
const managedTerminalSnapshot = vi.hoisted(() => vi.fn(async (_sessionId: number, since?: number) => ({
  active: true,
  endOffset: pty.out.length,
  data: since === undefined ? "" : btoa(pty.out.slice(since)),
})));
const writeManagedTerminal = vi.hoisted(() => vi.fn(async (sessionId: number, data: string) => { pty.onWrite?.(sessionId, data); }));
const setSessionLaunchSelection = vi.hoisted(() => vi.fn(async () => {}));
const submitToTerminal = vi.hoisted(() => vi.fn(async (sessionId: number, content: string) => { pty.onWrite?.(sessionId, content); }));
const stashComposerDraft = vi.hoisted(() => vi.fn(async () => {}));
vi.mock("../../api", () => ({ managedTerminalSnapshot, writeManagedTerminal, setSessionLaunchSelection }));
vi.mock("./terminalSubmit", () => ({ submitToTerminal, stashComposerDraft }));

beforeEach(() => { vi.useFakeTimers(); pty.out = ""; pty.onWrite = null; });
afterEach(() => { cleanup(); vi.useRealTimers(); vi.clearAllMocks(); });

function makeDeps(overrides: Partial<ModelControlsDeps> = {}) {
  let history = { agentModes: [], effort: "low", model: "claude-opus" } as unknown as ChatHistory;
  let selections: Record<string, string> = {};
  const deps: ModelControlsDeps = {
    sessionId: 1,
    chatUi: chatUi("claude"),
    model: "claude-opus",
    modelCatalog: [],
    sending: false,
    toneRef: { current: "idle" },
    retryRef: { current: null },
    withSendGuard: vi.fn(async (body: () => Promise<boolean>) => body()),
    attentionAbort: () => null,
    isCurrentOperation: () => true,
    setSendError: vi.fn(),
    setHistory: vi.fn((update) => { history = typeof update === "function" ? update(history)! : update!; }),
    setStoredSelections: vi.fn((update) => { selections = typeof update === "function" ? update(selections) : update; }),
    ...overrides,
  };
  return { deps, history: () => history, selections: () => selections };
}

/** 推进假时钟直到动作结束：切换动作内部是 200ms/120ms/250ms 的轮询。 */
async function settle<T>(promise: Promise<T>): Promise<T> {
  let done = false;
  const tracked = promise.finally(() => { done = true; });
  for (let i = 0; i < 100 && !done; i += 1) await act(async () => { await vi.advanceTimersByTimeAsync(100); });
  return tracked;
}

it("切模型：只认 CLI 确认行，确认后写恢复配置并更新已存选择", async () => {
  const { deps, selections } = makeDeps();
  pty.onWrite = (_sid, data) => { if (data === "/model opus") pty.out += "Set model to Opus\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus", label: "Opus" }));
  expect(submitToTerminal).toHaveBeenCalledWith(1, "/model opus", deps.attentionAbort);
  expect(setSessionLaunchSelection).toHaveBeenCalledWith(1, "model", "opus");
  expect(selections()).toEqual({ model: "opus" });
  expect(deps.setSendError).not.toHaveBeenCalled();
});

// Claude Code 2.1.322 起确认行只给显示名、不带别名：「Set model to `Opus 5.5` and saved as your default for new sessions」。
// 改前要求这一行里含预设 id（opus1m），1M 档永远判成未确认、也不记住选择（2026-11-12 leader 报「切模型无法生效」，
// transcript 实证 CLI 其实已经切过去了）。
it("切模型：新版确认行只有显示名（1M 档）也算确认，并记住这次选择", async () => {
  const { deps, selections } = makeDeps();
  pty.onWrite = (_sid, data) => { if (data === "/model opus[1m]") pty.out += "/model opus[1m]\r\n  L  Set model to `Opus 5.5` and saved as your default for new sessions\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus[1m]", label: "Opus 5.5 (1M)" }));
  expect(deps.setSendError).not.toHaveBeenCalled();
  expect(setSessionLaunchSelection).toHaveBeenCalledWith(1, "model", "opus[1m]");
  expect(selections()).toEqual({ model: "opus[1m]" });
});

// 做空审查：CLI 有自动降档路径，降到别的家族时也会打印「Set model to <实际模型>」，不能当成切换成功。
it("切模型：确认行报的是别的家族（被降档）不算确认", async () => {
  const { deps } = makeDeps();
  pty.onWrite = (_sid, data) => { if (data === "/model opus[1m]") pty.out += "Set model to `Sonnet 5`\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus[1m]", label: "Opus 5.5 (1M)" }));
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.modelSwitchUnconfirmed);
  expect(setSessionLaunchSelection).not.toHaveBeenCalled();
});

it("切模型：只有命令回显、没有确认行，仍报未确认", async () => {
  const { deps } = makeDeps();
  pty.onWrite = (_sid, data) => { pty.out += `${data}\r\n`; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus[1m]", label: "Opus 5.5 (1M)" }));
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.modelSwitchUnconfirmed);
  expect(setSessionLaunchSelection).not.toHaveBeenCalled();
});

it("切模型：菜单里出现模型名不算确认，超时报未确认且不改恢复配置", async () => {
  const { deps } = makeDeps();
  pty.onWrite = () => { pty.out += "  1. opus  2. sonnet\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus", label: "Opus" }));
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.modelSwitchUnconfirmed);
  expect(setSessionLaunchSelection).not.toHaveBeenCalled();
});

it("运行中拒绝切模型/档位/模式，不往终端写任何东西", async () => {
  const { deps } = makeDeps({ toneRef: { current: "running" } });
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectModel({ id: "opus", label: "Opus" }));
  await settle(result.current.selectEffort("high"));
  await settle(result.current.changeMode("permission", [{ data: "\u001b[Z", submit: false }]));
  expect(deps.setSendError).toHaveBeenCalledTimes(3);
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.menuBusy);
  expect(submitToTerminal).not.toHaveBeenCalled();
  expect(writeManagedTerminal).not.toHaveBeenCalled();
});

it("切档位：确认后乐观写进 history，期间 effortBusy 置位、结束复位", async () => {
  const { deps, history } = makeDeps();
  pty.onWrite = (_sid, data) => { if (data === "/effort high") pty.out += "Set effort level to High\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  let pending!: Promise<void>;
  act(() => { pending = result.current.selectEffort("high"); });
  expect(result.current.effortBusy).toBe(true);
  await settle(pending);
  expect(result.current.effortBusy).toBe(false);
  expect(history().effort).toBe("high");
  expect(deps.setSendError).not.toHaveBeenCalled();
});

it("切档位：CLI 拒绝时把原话带给用户，history 不动", async () => {
  const { deps, history } = makeDeps();
  pty.onWrite = () => { pty.out += "Effort 'max' isn't supported by this model\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectEffort("max"));
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.effortRejected("Effort 'max' isn't supported by this model"));
  expect(history().effort).toBe("low");
  expect(result.current.effortBusy).toBe(false);
});

it("切档位：确认行报的是别的档位不算数，超时报未确认", async () => {
  const { deps, history } = makeDeps();
  pty.onWrite = () => { pty.out += "Set effort level to High\r\n"; };
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.selectEffort("max"));
  expect(deps.setSendError).toHaveBeenCalledWith(zh.chat.effortUnconfirmed);
  expect(history().effort).toBe("low");
});

it("档位清单：插件静态表优先，否则取模型目录里当前模型声明的档位", () => {
  const claude = renderHook(() => useModelControls(makeDeps().deps));
  expect(claude.result.current.effortLevels).toEqual(["low", "medium", "high", "xhigh", "max"]);
  const catalog: ModelCatalogEntry[] = [
    { id: "kimi-a", label: "Kimi A", group: null, efforts: ["low", "high"], default_effort: null, is_default: false, uncertain: false },
    { id: "kimi-b", label: "Kimi B", group: null, efforts: ["off"], default_effort: null, is_default: false, uncertain: false },
  ];
  const kimi = renderHook(() => useModelControls(makeDeps({ chatUi: chatUi("kimi"), model: "kimi-a", modelCatalog: catalog }).deps));
  expect(kimi.result.current.effortLevels).toEqual(["low", "high"]);
  const none = renderHook(() => useModelControls(makeDeps({ chatUi: chatUi("codex") }).deps));
  expect(none.result.current.effortLevels).toEqual([]);
});

it("切模式：写按键并登记接管重放，随后按屏幕回显更新显示值", async () => {
  const { deps, history } = makeDeps();
  pty.out = "manual mode on\r\n"; // 按键之前的旧指示不能被当成回显
  pty.onWrite = () => { pty.out += "accept edits on\r\n"; };
  const markers = chatUi("claude")!.mode_controls[0].screen_markers;
  const { result } = renderHook(() => useModelControls(deps));
  await settle(result.current.changeMode("permission", [{ data: "\u001b[Z", submit: false }], undefined, markers));
  expect(writeManagedTerminal).toHaveBeenCalledWith(1, "\u001b[Z");
  expect(deps.retryRef.current).toBeTypeOf("function");
  await act(async () => { await vi.advanceTimersByTimeAsync(3_500); });
  expect(history().agentModes).toEqual([{ dimension: "permission", value: "acceptEdits" }]);
});

it("循环到目标模式：逐次按 cycle 键直到落点；绕一圈到不了就停手并说明", async () => {
  const markers = chatUi("claude")!.mode_controls[0].screen_markers;
  const order = ["accept edits on", "plan mode on", "manual mode on"];
  const reach = makeDeps();
  let presses = 0;
  pty.onWrite = () => { pty.out += `${order[presses++ % order.length]}\r\n`; };
  const first = renderHook(() => useModelControls(reach.deps));
  await settle(first.result.current.cycleToMode("permission", "plan", "\u001b[Z", markers));
  expect(presses).toBe(2);
  expect(reach.history().agentModes).toEqual([{ dimension: "permission", value: "plan" }]);
  expect(reach.deps.setSendError).not.toHaveBeenCalled();

  const stuck = makeDeps();
  pty.onWrite = () => { pty.out += "manual mode on\r\n"; };
  const second = renderHook(() => useModelControls(stuck.deps));
  await settle(second.result.current.cycleToMode("permission", "dontAsk", "\u001b[Z", markers));
  expect(writeManagedTerminal).toHaveBeenCalledTimes(2 + markers.length);
  expect(stuck.deps.setSendError).toHaveBeenCalledWith(zh.chat.modeUnreachable);
});

it("换会话作废旧会话还在跑的模式回显，滞后帧不写进新会话", async () => {
  const { deps, history } = makeDeps();
  const markers = chatUi("claude")!.mode_controls[0].screen_markers;
  const { result, rerender } = renderHook((props: ModelControlsDeps) => useModelControls(props), { initialProps: deps });
  await settle(result.current.changeMode("permission", [{ data: "\u001b[Z", submit: false }], undefined, markers));
  // 回显轮询已在后台跑（3s 预算）；此刻切到 2 号会话，旧会话的屏幕随后才画出新模式。
  rerender({ ...deps, sessionId: 2 });
  pty.out += "plan mode on\r\n";
  await act(async () => { await vi.advanceTimersByTimeAsync(3_500); });
  expect(history().agentModes).toEqual([]);
});
