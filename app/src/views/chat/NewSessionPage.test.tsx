import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  getSettings: vi.fn(),
  listAgents: vi.fn(),
  getAccounts: vi.fn(),
  listSubdirectories: vi.fn(),
  getSessionAdminPage: vi.fn(),
  savePastedAttachment: vi.fn(),
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
  Object.values(api).forEach((m) => m.mockReset());
  dialog.open.mockReset();
  api.recentCwds.mockResolvedValue(["C:/work/meowo", "C:/work/site"]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.getSettings.mockResolvedValue({ default_agent: "claude" });
  api.listAgents.mockResolvedValue(descriptors(["claude", "codex"]));
  api.getAccounts.mockResolvedValue([
    { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
    { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
  ]);
});
afterEach(() => cleanup());

describe("新建会话页（对话窗主区，对照 Kimi）", () => {
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
    // 落盘期间发送钮是 aria-disabled（N-1：不用 disabled 踢走焦点），回车/点击都被拦下。
    expect(screen.getByTestId("nsp-send").getAttribute("aria-disabled")).toBe("true");
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

  /// N-1（走查 2026-11-12）：启动期间输入框原先 disabled，聚焦控件被禁用后焦点掉到 body，
  /// 失败后要先点回输入框。兄弟场景：点发送钮启动（焦点在钮上）与回车启动（焦点在输入框）都测。
  it("启动失败后焦点回到输入框，不掉到 body", async () => {
    let reject: (reason: unknown) => void = () => {};
    api.newSession.mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
    render(<NewSessionPage prefill={{}} onLaunched={vi.fn()} />);
    await waitFor(() => expect(screen.getByTestId("nsp-workspace").textContent).toContain("meowo"));
    const input = screen.getByTestId("nsp-input") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "帮我修登录" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(api.newSession).toHaveBeenCalled());
    // 启动中：只读而不是禁用，焦点还在。
    expect(input.disabled).toBe(false);
    expect(input.readOnly).toBe(true);
    expect(document.activeElement).toBe(input);
    reject("terminal/cwd_missing");
    await screen.findByTestId("nsp-error");
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(document.activeElement).toBe(input);
    expect(input.value).toBe("帮我修登录");

    // 点发送钮启动：钮在启动中保持可聚焦（aria-disabled），失败后同样回到输入框。
    api.newSession.mockRejectedValue("terminal/cwd_missing");
    const send = screen.getByTestId("nsp-send");
    send.focus();
    fireEvent.click(send);
    await waitFor(() => expect(api.newSession).toHaveBeenCalledTimes(2));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(document.activeElement).toBe(input);
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
