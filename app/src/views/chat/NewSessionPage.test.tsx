import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  managedConversationCreate: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  getSettings: vi.fn(),
  listAgents: vi.fn(),
  getAccounts: vi.fn(),
  listSubdirectories: vi.fn(),
  getSessionAdminPage: vi.fn(),
  savePastedAttachment: vi.fn(),
  leadProviderInstallations: vi.fn(),
}));
vi.mock("../../api", async (orig) => ({ ...(await orig<typeof import("../../api")>()), ...api }));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
// 附件缩略图（ImageRef）要把本地路径换成 asset URL；jsdom 里没有 Tauri 内核。
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<typeof import("@tauri-apps/api/core")>()), convertFileSrc: (path: string) => `asset://${path}` }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}), emit: vi.fn() }));

import { NewSessionPage } from "./NewSessionPage";
import { descriptors } from "../../test/agents";
import { zh } from "../../i18n/zh";
import { normalizePath } from "../../paths";

beforeEach(() => {
  localStorage.removeItem("meowo.newSession.managed");
  Object.values(api).forEach((m) => m.mockReset());
  dialog.open.mockReset();
  api.recentCwds.mockResolvedValue(["C:/work/meowo", "C:/work/site"]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.leadProviderInstallations.mockResolvedValue([]);
  api.getSettings.mockResolvedValue({ default_agent: "claude" });
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
  api.getAccounts.mockResolvedValue([
    { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
    { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
  ]);
});
afterEach(() => cleanup());

describe("新建会话页（对话窗主区，对照 Kimi）", () => {
  it("ticking Meowo managed creates a managed session with the account ID and hides CLI launch flags", async () => {
    api.listAgents.mockResolvedValue(descriptors(["codex"]).map((agent) => ({ ...agent, supports_managed_conversation: agent.id === "codex" })));
    api.getSettings.mockResolvedValue({ default_agent: "codex", active_profile: { codex: "work" } });
    api.getAccounts.mockResolvedValue([{ provider: "codex", account: { login_label: "API Key" }, active_profile_name: "工作账号" }]);
    api.managedConversationCreate.mockResolvedValue({ sessionId: 407 });
    const onLaunched=vi.fn();render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={onLaunched}/>);
    fireEvent.click(await screen.findByRole("checkbox",{name:zh.chat.engineManaged}));
    await screen.findByText(zh.newSession.activeProfile("工作账号"));
    expect(screen.queryByTestId("nsp-option-approval")).toBeNull();
    fireEvent.change(screen.getByTestId("nsp-input"),{target:{value:"  preserve exact first message  "}});
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(()=>expect(api.managedConversationCreate).toHaveBeenCalledWith("codex",normalizePath("C:/isolated"),"work"));
    expect(api.newSession).not.toHaveBeenCalled();
    await waitFor(()=>expect(onLaunched).toHaveBeenCalledWith(407,"codex","  preserve exact first message  ",[]));
  });

  it.each(["codex", "kimi", "gemini", "opencode"])("%s uses descriptor-managed mode without a Claude-only fallback", async (provider) => {
    api.listAgents.mockResolvedValue(
      descriptors([provider]).map((agent) => ({ ...agent, supports_managed_conversation: agent.id === provider })),
    );
    api.getSettings.mockResolvedValue({ default_agent: provider });
    api.getAccounts.mockResolvedValue([]);
    api.managedConversationCreate.mockResolvedValue({ sessionId: 407 });
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{ cwd: "C:/isolated", provider }} onLaunched={onLaunched} />);
    fireEvent.click(await screen.findByRole("checkbox", { name: zh.chat.engineManaged }));
    fireEvent.change(screen.getByTestId("nsp-input"), { target: { value: "managed first message" } });
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(api.managedConversationCreate).toHaveBeenCalledWith(provider, normalizePath("C:/isolated"), null));
    expect(api.newSession).not.toHaveBeenCalled();
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(407, provider, "managed first message", []));
  });

  // 2026-11-13 改定：默认原生 CLI（与 main 一致，保留斜杠命令、撤销、终端视图与启动选项）；托管是勾选项，
  // 按 provider 记住上次选择。兄弟场景：四家默认都是原生；勾过一次后同 provider 下次默认托管、别家不受影响。
  it.each(["codex", "kimi", "gemini", "opencode"])("%s defaults to the native CLI with its original launch path", async(provider)=>{
    api.listAgents.mockResolvedValue(descriptors([provider]).map((agent)=>({...agent,supports_managed_conversation:agent.id===provider})));
    api.getSettings.mockResolvedValue({ default_agent: provider });
    api.newSession.mockResolvedValue(-408);const onLaunched=vi.fn();
    render(<NewSessionPage prefill={{cwd:"C:/isolated",provider}} onLaunched={onLaunched}/>);
    expect((await screen.findByRole("checkbox",{name:zh.chat.engineManaged}) as HTMLInputElement).checked).toBe(false);
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(()=>expect(api.newSession).toHaveBeenCalled());expect(api.managedConversationCreate).not.toHaveBeenCalled();
  });

  // 关卡 Q「完整闭环、不接受手动继续」：装了 Lead 且托管可自动继续的 provider 没选过时默认托管；
  // 兄弟场景：装了但不能自动继续 → 原生；拉安装状态失败（手机桥不放行）→ 原生；用户选过原生 → 尊重选择。
  it.each([
    ["installed with automatic continue", [{ provider: "codex", automaticContinue: true }], undefined, true],
    ["installed without automatic continue", [{ provider: "codex", automaticContinue: false }], undefined, false],
    ["another provider has Lead", [{ provider: "kimi", automaticContinue: true }], undefined, false],
    ["installations unavailable", "reject", undefined, false],
    ["user chose native before", [{ provider: "codex", automaticContinue: true }], false, false],
  ] as const)("Lead default: %s", async (_name, rows, stored, expected) => {
    if (stored !== undefined) localStorage.setItem("meowo.newSession.managed", JSON.stringify({ codex: stored }));
    if (rows === "reject") api.leadProviderInstallations.mockRejectedValue("remote/command_forbidden");
    else api.leadProviderInstallations.mockResolvedValue(rows);
    api.listAgents.mockResolvedValue(descriptors(["codex","kimi"]).map((agent)=>({...agent,supports_managed_conversation:true})));
    render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={vi.fn()}/>);
    const box = await screen.findByRole("checkbox",{name:zh.chat.engineManaged}) as HTMLInputElement;
    await waitFor(()=>expect(api.leadProviderInstallations).toHaveBeenCalled());
    await waitFor(()=>expect(box.checked).toBe(expected));
  });

  it("remembers the managed choice per provider", async()=>{
    api.listAgents.mockResolvedValue(descriptors(["codex","kimi"]).map((agent)=>({...agent,supports_managed_conversation:true})));
    const first=render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={vi.fn()}/>);
    fireEvent.click(await screen.findByRole("checkbox",{name:zh.chat.engineManaged}));
    first.unmount();
    const again=render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={vi.fn()}/>);
    expect((await screen.findByRole("checkbox",{name:zh.chat.engineManaged}) as HTMLInputElement).checked).toBe(true);
    again.unmount();
    render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"kimi"}} onLaunched={vi.fn()}/>);
    expect((await screen.findByRole("checkbox",{name:zh.chat.engineManaged}) as HTMLInputElement).checked).toBe(false);
  });

  it("native CLI keeps its launch options", async()=>{
    api.listAgents.mockResolvedValue(descriptors(["codex"]).map((agent)=>({...agent,supports_managed_conversation:agent.id==="codex"})));
    api.newSession.mockResolvedValue(-408);const onLaunched=vi.fn();
    render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={onLaunched}/>);
    await screen.findByRole("checkbox",{name:zh.chat.engineManaged});
    expect(await screen.findByTestId("nsp-option-approval")).toBeTruthy();
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(()=>expect(api.newSession).toHaveBeenCalled());expect(api.managedConversationCreate).not.toHaveBeenCalled();
  });

  it("an unverified native version reports an error and never silently falls back to a PTY",async()=>{
    api.listAgents.mockResolvedValue(descriptors(["codex"]).map((agent)=>({...agent,supports_managed_conversation:agent.id==="codex"})));
    api.managedConversationCreate.mockRejectedValue("conversation/version_unsupported");
    render(<NewSessionPage prefill={{cwd:"C:/isolated",provider:"codex"}} onLaunched={vi.fn()}/>);
    fireEvent.click(await screen.findByRole("checkbox",{name:zh.chat.engineManaged}));
    fireEvent.change(screen.getByTestId("nsp-input"),{target:{value:"keep my draft"}});fireEvent.click(screen.getByTestId("nsp-send"));
    expect(await screen.findByRole("alert")).toBeTruthy();expect(api.newSession).not.toHaveBeenCalled();
    expect((screen.getByTestId("nsp-input") as HTMLTextAreaElement).value).toBe("keep my draft");
  });

  it("默认选中最近的工作区，写下第一句话回车即启动，并把这句话交回去", async () => {
    api.newSession.mockResolvedValue(-7);
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{}} onLaunched={onLaunched} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    const input = screen.getByTestId("nsp-input");
    expect(document.activeElement).toBe(input);
    fireEvent.change(input, { target: { value: "  看看测试为什么挂了 " } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-7, "claude", "看看测试为什么挂了", []));
    expect(api.newSession).toHaveBeenCalledWith(normalizePath("C:/work/meowo"), "claude", {});
  });

  it("Shift+Enter 换行不启动；不写话直接点开始也能起会话", async () => {
    api.newSession.mockResolvedValue(-3);
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{ cwd: "D:\\other", provider: "codex" }} onLaunched={onLaunched} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("other"));
    fireEvent.keyDown(screen.getByTestId("nsp-input"), { key: "Enter", shiftKey: true });
    expect(api.newSession).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-3, "codex", "", []));
    // 预填目录不在最近列表里也照用，不被最近一项顶掉。
    expect(api.newSession).toHaveBeenCalledWith(normalizePath("D:/other"), "codex", {});
  });

  it("粘贴截图成附件缩略图，可删除；纯文字粘贴不拦截；只有图片也能发出去", async () => {
    api.newSession.mockResolvedValue(-5);
    api.savePastedAttachment.mockImplementation((name: string) => Promise.resolve(`C:/tmp/meowo-paste/1-0/${name}`));
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{}} onLaunched={onLaunched} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    const input = screen.getByTestId("nsp-input") as HTMLTextAreaElement;
    // 纯文字：clipboardData 里没有文件，不 preventDefault，交给浏览器默认粘贴。
    const textPaste = new Event("paste", { bubbles: true, cancelable: true });
    Object.assign(textPaste, { clipboardData: { files: [] } });
    input.dispatchEvent(textPaste);
    expect(textPaste.defaultPrevented).toBe(false);
    // jsdom 的 File/Blob 实现参差，只按组件用到的形状（name + arrayBuffer + size）伪造。
    const file = (name: string) => ({ name, size: 3, arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) }) as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file("shot.png"), file("b.png")] } });
    expect(await screen.findByAltText("shot.png")).toBeTruthy();
    expect(api.savePastedAttachment).toHaveBeenCalledWith("shot.png", "AQID");
    expect(input.value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: `${zh.chat.removeAttachment} b.png` }));
    await waitFor(() => expect(screen.queryByAltText("b.png")).toBeNull());
    fireEvent.click(screen.getByTestId("nsp-send"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-5, "claude", "", [
      expect.objectContaining({ path: "C:/tmp/meowo-paste/1-0/shot.png", image: true }),
    ]));
  });

  it("粘贴的图还在落盘时不发出去，落盘回来才能发", async () => {
    api.newSession.mockResolvedValue(-6);
    let finish: (path: string) => void = () => {};
    api.savePastedAttachment.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{}} onLaunched={onLaunched} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    const input = screen.getByTestId("nsp-input");
    const file = { name: "shot.png", size: 3, arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer) } as unknown as File;
    fireEvent.paste(input, { clipboardData: { files: [file] } });
    fireEvent.change(input, { target: { value: "看图" } });
    await waitFor(() => expect(api.savePastedAttachment).toHaveBeenCalled());
    fireEvent.keyDown(input, { key: "Enter" });
    expect((screen.getByTestId("nsp-send") as HTMLButtonElement).disabled).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(api.newSession).not.toHaveBeenCalled();
    finish("C:/tmp/meowo-paste/1-0/shot.png");
    expect(await screen.findByAltText("shot.png")).toBeTruthy();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-6, "claude", "看图", [expect.objectContaining({ image: true })]));
  });

  it("没有工作区时提示先选，不发起启动", async () => {
    api.recentCwds.mockResolvedValue([]);
    render(<NewSessionPage prefill={{}} onLaunched={vi.fn()} />);
    await screen.findByTestId("nsp-agent");
    fireEvent.click(screen.getByTestId("nsp-send"));
    expect((await screen.findByTestId("nsp-error")).textContent).toBe(zh.newSession.workspaceRequired);
    expect(api.newSession).not.toHaveBeenCalled();
  });

  it("工作区下拉里「选择文件夹…」走系统目录对话框", async () => {
    dialog.open.mockResolvedValue("E:\\picked\\proj");
    render(<NewSessionPage prefill={{}} onLaunched={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    fireEvent.click(screen.getByTestId("nsp-workspace").querySelector("button")!);
    // 菜单照 Kimi：「最近的文件夹」一节，每项名字 + 完整路径，当前工作区标为选中。
    expect(screen.getByText(zh.newSession.recentFolders)).toBeTruthy();
    const options = screen.getAllByRole("option");
    expect(options[0].getAttribute("aria-selected")).toBe("true");
    expect(options[0].textContent).toContain("meowo");
    expect(options[1].getAttribute("aria-selected")).toBe("false");
    fireEvent.click(await screen.findByText(zh.newSession.pickFolder));
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("proj"));
  });

  it("启动失败说原因，第一句话留在输入框里", async () => {
    api.newSession.mockRejectedValue("terminal/cwd_missing");
    const onLaunched = vi.fn();
    render(<NewSessionPage prefill={{}} onLaunched={onLaunched} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    fireEvent.change(screen.getByTestId("nsp-input"), { target: { value: "你好" } });
    fireEvent.click(screen.getByTestId("nsp-send"));
    await screen.findByTestId("nsp-error");
    expect(onLaunched).not.toHaveBeenCalled();
    expect((screen.getByTestId("nsp-input") as HTMLTextAreaElement).value).toBe("你好");
  });

  it("拖进窗口的文件夹设为工作区，拖进文件则说明不是文件夹", async () => {
    api.listSubdirectories.mockImplementation((path: string) =>
      path.endsWith(".txt") ? Promise.reject("不是目录") : Promise.resolve({ path, parent: null, dirs: [] }));
    const { rerender } = render(<NewSessionPage prefill={{}} droppedPath={null} onLaunched={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    rerender(<NewSessionPage prefill={{}} droppedPath={{ path: "F:/dropped/app", nonce: 1 }} onLaunched={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("app"));
    rerender(<NewSessionPage prefill={{}} droppedPath={{ path: "F:/notes.txt", nonce: 2 }} onLaunched={vi.fn()} />);
    expect((await screen.findByTestId("nsp-error")).textContent).toBe(zh.newSession.dropNotFolder);
    // 工作区保持上一次拖进来的文件夹。
    expect(screen.getByTestId("nsp-workspace").textContent).toContain("app");
  });

  it("输入框下列出这个工作区的最近会话，点一条回到它，查看更多带着工作区去管理页", async () => {
    const row = (id: number, title: string) => ({ session: { id, last_event_at: Date.now() - 3 * 3600_000 }, provider: "claude", task_title: title });
    api.getSessionAdminPage.mockResolvedValue({ items: [row(11, "修登录页"), row(12, "")], total: 9 });
    const onOpenSession = vi.fn();
    const onMoreSessions = vi.fn();
    render(<NewSessionPage prefill={{}} onLaunched={vi.fn()} onOpenSession={onOpenSession} onMoreSessions={onMoreSessions} />);
    const section = await screen.findByTestId("nsp-recent");
    expect(api.getSessionAdminPage).toHaveBeenCalledWith(expect.objectContaining({ workspaces: [normalizePath("C:/work/meowo")], page: 0, pageSize: 5 }));
    expect(section.textContent).toContain("修登录页");
    expect(section.textContent).toContain("3h");
    // 还没输入过的会话用占位标题，不是空白行。
    expect(section.textContent).toContain(zh.sticker.waitingFirstInput);
    fireEvent.click(screen.getByRole("button", { name: /修登录页/ }));
    expect(onOpenSession).toHaveBeenCalledWith(11);
    fireEvent.click(screen.getByRole("button", { name: zh.newSession.moreSessions }));
    expect(onMoreSessions).toHaveBeenCalledWith(normalizePath("C:/work/meowo"));
  });
});
