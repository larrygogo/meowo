// 菜单型斜杠命令在补全列表与发送路径上的表现（走查输入 B1、B9）：回合在跑时置灰并写明原因，
// 点了、回车了都不往终端写；空闲时照常。夹具与 mock 见 chatWindow.harness.tsx（必须先于 ./ChatWindow 导入）。
import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke, adapter, chatUiReady, emptyComposerFrame } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";
import { chatUi, descriptors } from "../test/agents";

const ALL = ["claude", "codex", "kimi", "gemini", "opencode"] as const;
const { box, writes } = adapter;

/// 每家挑一条「只能开菜单」的命令：声明为菜单型、且没有同名的内联参数形式。
/// 夹具的补全表里没有它时补进去（Gemini 的 `/model`），保证五家都走到。
function menuUi(provider: string) {
  const ui = chatUi(provider)!;
  const name = ui.menu_slash_commands.find((command) => command !== ui.model_select_command)!;
  const listed = ui.slash_commands.some((command) => command.name === name);
  return { name, ui: listed ? ui : { ...ui, slash_commands: [...ui.slash_commands, { name, description: null, source: "builtin" as const }] } };
}

function setup(sessionId: number, provider: string, status: string) {
  const base = adapter.setup(sessionId, provider, status);
  const { name, ui } = menuUi(provider);
  invoke.mockImplementation((command, args) => {
    if (command === "agent_chat_ui") return Promise.resolve(ui);
    if (command === "list_agents") return Promise.resolve(descriptors([...ALL]));
    if (command === "managed_terminal_frame") return Promise.resolve(emptyComposerFrame(provider));
    return base(command, args);
  });
  return name;
}

const optionFor = (list: HTMLElement, name: string) =>
  within(list).getAllByRole("option").find((item) => item.querySelector("code")?.textContent === name)!;

describe("补全列表里的菜单型命令（五家）", () => {
  it.each(ALL.map((provider, index) => [provider, 900 + index] as const))("%s 回合在跑：置灰带原因，点击、回车、Tab 都不选中也不往终端写", async (provider, sessionId) => {
    const name = setup(sessionId, provider, "running");
    render(<ChatWindow />);
    await screen.findByText(`适配-${sessionId}`);
    await chatUiReady();
    // 整条命令敲完后它不再占补全位：少敲一个字让它出现在列表里。
    const typed = name.slice(0, -1);
    fireEvent.change(box(), { target: { value: typed } });
    const list = await screen.findByRole("listbox");
    const option = optionFor(list, name);
    expect(option.getAttribute("aria-disabled")).toBe("true");
    expect(option.textContent).toContain(zh.chat.availability.afterTurn);
    fireEvent.click(option);
    expect(box().value).toBe(typed);
    // 把高亮移到这一项再按回车、Tab：输入框不变，也没有把半截命令发出去。
    fireEvent.mouseEnter(option);
    expect(option.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(box(), { key: "Enter" });
    fireEvent.keyDown(box(), { key: "Tab" });
    await act(async () => {});
    expect(box().value).toBe(typed);
    expect(writes(sessionId)).toEqual([]);
    // 列表里别的命令不受影响。
    const others = within(list).getAllByRole("option").filter((item) => item !== option && !chatUi(provider)!.menu_slash_commands.includes(item.querySelector("code")?.textContent ?? ""));
    expect(others.every((item) => item.getAttribute("aria-disabled") === null)).toBe(true);
  });

  it.each(ALL.map((provider, index) => [provider, 910 + index] as const))("%s 回合在跑：手敲整条命令回车，得到同一句原因，草稿留着", async (provider, sessionId) => {
    const name = setup(sessionId, provider, "running");
    render(<ChatWindow />);
    await screen.findByText(`适配-${sessionId}`);
    await chatUiReady();
    fireEvent.change(box(), { target: { value: name } });
    fireEvent.keyDown(box(), { key: "Enter" });
    await screen.findByText(zh.chat.availability.afterTurn);
    expect(screen.queryByText(zh.chat.menuBusyOpen)).toBeNull();
    expect(box().value).toBe(name);
    expect(writes(sessionId)).toEqual([]);
  });

  it.each(ALL.map((provider, index) => [provider, 920 + index] as const))("%s 会话空闲：不置灰，点了写进输入框，回车发到终端", async (provider, sessionId) => {
    const name = setup(sessionId, provider, "waiting");
    render(<ChatWindow />);
    await screen.findByText(`适配-${sessionId}`);
    await chatUiReady();
    fireEvent.change(box(), { target: { value: name.slice(0, -1) } });
    const list = await screen.findByRole("listbox");
    const option = optionFor(list, name);
    expect(option.getAttribute("aria-disabled")).toBeNull();
    expect(option.textContent).not.toContain(zh.chat.availability.afterTurn);
    fireEvent.click(option);
    expect(box().value).toBe(`${name} `);
    fireEvent.keyDown(box(), { key: "Enter" });
    await waitFor(() => expect(writes(sessionId)).toContain(name), { timeout: 4000 });
  });

  it("claude 回合在跑：/model 带参数能内联执行，补全里不置灰", async () => {
    const base = adapter.setup(930, "claude", "running");
    invoke.mockImplementation((command, args) => command === "list_agents" ? Promise.resolve(descriptors([...ALL])) : base(command, args));
    render(<ChatWindow />);
    await screen.findByText("适配-930");
    await chatUiReady();
    fireEvent.change(box(), { target: { value: "/mode" } });
    const list = await screen.findByRole("listbox");
    const option = optionFor(list, "/model");
    expect(option.getAttribute("aria-disabled")).toBeNull();
    fireEvent.click(option);
    expect(box().value).toBe("/model ");
  });
});
