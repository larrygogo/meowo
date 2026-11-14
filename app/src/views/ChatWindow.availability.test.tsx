// 「此刻做不到」的统一策略在对话窗里的表现（验收 7）与 Codex 思考档（验收 17）。
// 夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke, adapter, emptyComposerFrame } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { descriptors } from "../test/agents";

const ALL = ["claude", "codex", "kimi", "gemini", "opencode"];

function setup(sessionId: number, provider: string, status: string, extra: Record<string, unknown> = {}, more: (command: string, args?: Record<string, unknown>) => unknown = () => undefined) {
  const base = adapter.setup(sessionId, provider, status);
  invoke.mockImplementation((command, args) => {
    const custom = more(command, args);
    if (custom !== undefined) return custom;
    if (command === "get_chat_history") return Promise.resolve({ ...adapter.history(sessionId, provider, status), ...extra });
    if (command === "list_agents") return Promise.resolve(descriptors(ALL));
    if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame(provider));
    return base(command, args);
  });
}
const called = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

describe("Kimi 计划开关（走查输入 B1）", () => {
  it("回合在跑：不置灰，菜单先说明会排队，点了排进队列", async () => {
    setup(781, "kimi", "running", { agentModes: [{ dimension: "work", value: "default" }, { dimension: "permission", value: "manual" }] });
    render(<ChatWindow />);
    await screen.findByText("适配-781");
    fireEvent.click(await screen.findByRole("button", { name: /^切换模式: 工作模式 · / }));
    const menu = await screen.findByRole("menu");
    // 菜单顶上先说清楚：现在点会排到这一轮之后自动执行。
    expect(within(menu).getByRole("status").textContent).toBe(zh.chat.availability.queuedAfterTurn);
    const on = within(menu).getByRole("menuitem", { name: zh.modes.plan(true) });
    expect(on.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(on);
    // 排进宿主队列，带着要往终端打的 /plan on；此刻不往正在跑的回合里打字。
    await waitFor(() => expect(called("queue_session_settings").length).toBe(1));
    const request = (called("queue_session_settings")[0][1] as { request: { modes: Record<string, string>; terminal: { inputs: { data: string }[] }[] } }).request;
    expect(request.modes).toEqual({ work: "plan" });
    expect(request.terminal[0].inputs[0].data).toBe("/plan on");
    expect(adapter.writes(781).join("")).not.toContain("/plan");
  });

  it("会话空闲：直接切，不出排队说明", async () => {
    setup(782, "kimi", "waiting", { agentModes: [{ dimension: "work", value: "default" }] });
    render(<ChatWindow />);
    await screen.findByText("适配-782");
    fireEvent.click(await screen.findByRole("button", { name: /^切换模式: 工作模式 · / }));
    const menu = await screen.findByRole("menu");
    expect(within(menu).queryByRole("status")).toBeNull();
    fireEvent.click(within(menu).getByRole("menuitem", { name: zh.modes.plan(true) }));
    await waitFor(() => expect(adapter.writes(782)).toContain("/plan on"));
    expect(called("queue_session_settings").length).toBe(0);
  });
});

describe("Kimi 收紧权限（走查输入 B8）", () => {
  it("收紧的档置灰并写明要新建会话，点了不发请求、不报错", async () => {
    setup(783, "kimi", "waiting", { agentModes: [{ dimension: "permission", value: "auto" }] });
    render(<ChatWindow />);
    await screen.findByText("适配-783");
    fireEvent.click(await screen.findByRole("button", { name: /^切换模式: 权限模式 · / }));
    const menu = await screen.findByRole("menu");
    const items = within(menu).getAllByRole("menuitem");
    const tighten = items.find((item) => item.getAttribute("aria-disabled") === "true");
    // 只有不带参数的「每步都问」是收紧；另外两档（含当前档）不置灰。
    expect(items.filter((item) => item.getAttribute("aria-disabled") === "true").length).toBe(1);
    expect(tighten?.textContent).toContain(zh.chat.availability.needsNewSession);
    fireEvent.click(tighten!);
    await act(async () => {});
    expect(called("apply_session_settings").length).toBe(0);
    expect(screen.queryByText(zh.chat.settingsNeedsNewSession)).toBeNull();
    // 菜单没关：置灰的档点了不动。
    expect(screen.queryByRole("menu")).toBeTruthy();
  });

  it("放宽的档照常可切", async () => {
    setup(784, "kimi", "waiting", { agentModes: [{ dimension: "permission", value: "manual" }] }, (command) => command === "apply_session_settings"
      ? Promise.resolve({ kind: "restarted", sessionId: 784, confirmed: false, reason: null }) : undefined);
    render(<ChatWindow />);
    await screen.findByText("适配-784");
    fireEvent.click(await screen.findByRole("button", { name: /^切换模式: 权限模式 · / }));
    const menu = await screen.findByRole("menu");
    const loosen = within(menu).getAllByRole("menuitem").filter((item) => !item.className.includes("is-active"));
    expect(loosen.every((item) => item.getAttribute("aria-disabled") === null)).toBe(true);
    fireEvent.click(loosen[0]);
    await waitFor(() => expect(called("apply_session_settings").length).toBe(1));
  });
});

describe("五家的模式菜单在回合中：没有「列出来点了才报错」", () => {
  it.each([
    ["codex", /^切换模式: /],
    ["gemini", /^切换模式: /],
    ["opencode", /^切换模式: /],
  ] as const)("%s：回合在跑时每一档可点，点了排队", async (provider, name) => {
    setup(785, provider, "running");
    render(<ChatWindow />);
    await screen.findByText("适配-785");
    fireEvent.click((await screen.findAllByRole("button", { name }))[0]);
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("status").textContent).toBe(zh.chat.availability.queuedAfterTurn);
    const items = within(menu).getAllByRole("menuitem");
    expect(items.some((item) => item.getAttribute("aria-disabled") === "true")).toBe(false);
    fireEvent.click(items.find((item) => !item.className.includes("is-active"))!);
    await waitFor(() => expect(called("queue_session_settings").length).toBe(1));
    expect(document.querySelector(".chat-send-error[role='alert']")).toBeNull();
  });
});

describe("Codex 思考档（走查输入 B7）", () => {
  const catalog = [
    { id: "gpt-6.1-sol", label: "GPT-6.1-Sol", group: null, efforts: ["low", "medium", "high"], default_effort: "medium", is_default: true, uncertain: false },
  ];
  const withCatalog = (applied: unknown) => (command: string) => {
    if (command === "agent_model_catalog") return Promise.resolve(catalog);
    if (command === "apply_session_settings") return Promise.resolve(applied);
    if (command === "session_live_settings") return Promise.resolve({ model: "gpt-6.1-sol", effort: "medium", modes: [] });
    return undefined;
  };

  it("对话窗可见可切：选一档经会话服务随当前模型下发", async () => {
    setup(786, "codex", "waiting", { model: "GPT-6.1-Sol", effort: "medium" }, withCatalog({ kind: "applied", sessionId: 786, confirmed: true, reason: null }));
    render(<ChatWindow />);
    await screen.findByText("适配-786");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    // 模型钮上带当前档。
    await waitFor(() => expect(document.querySelector(".chat-model-pill-effort")?.textContent).toContain("Medium"));
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    const group = await screen.findByRole("radiogroup", { name: zh.chat.effortLabel });
    expect(within(group).getAllByRole("radio").map((radio) => radio.textContent)).toEqual(["Low", "Medium", "High"]);
    expect(within(group).getByRole("radio", { name: "Medium" }).getAttribute("aria-checked")).toBe("true");
    fireEvent.click(within(group).getByRole("radio", { name: "High" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("apply_session_settings", expect.objectContaining({ sessionId: 786, model: "gpt-6.1-sol", effort: "high" })));
    // 不往终端打 /effort。
    expect(adapter.writes(786).join("")).not.toContain("/effort");
    await waitFor(() => expect(document.querySelector(".chat-model-pill-effort")?.textContent).toContain("High"));
  });

  it("回合在跑：思考档排到这一轮之后，菜单先说明", async () => {
    setup(787, "codex", "running", { model: "GPT-6.1-Sol", effort: "medium" }, withCatalog({ kind: "applied", sessionId: 787, confirmed: true, reason: null }));
    render(<ChatWindow />);
    await screen.findByText("适配-787");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    const group = await screen.findByRole("radiogroup", { name: zh.chat.effortLabel });
    expect(screen.getByText(zh.chat.availability.queuedAfterTurn)).toBeTruthy();
    fireEvent.click(within(group).getByRole("radio", { name: "High" }));
    await waitFor(() => expect(called("queue_session_settings").length).toBe(1));
    expect(called("apply_session_settings").length).toBe(0);
  });

  it.each(["gemini", "opencode"])("%s 没有声明思考档能力：模型菜单里不出这一段", async (provider) => {
    setup(788, provider, "waiting", { model: "GPT-6.1-Sol", effort: "medium" }, (command) => command === "agent_model_catalog" ? Promise.resolve(catalog) : undefined);
    render(<ChatWindow />);
    await screen.findByText("适配-788");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("agent_model_catalog", expect.anything()));
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(await screen.findByRole("button", { name: "切换模型" }));
    await screen.findByRole("menu");
    expect(screen.queryByRole("radiogroup", { name: zh.chat.effortLabel })).toBeNull();
    expect(document.querySelector(".chat-model-pill-effort")).toBeNull();
  });
});
