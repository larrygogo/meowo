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
}));
vi.mock("../../api", async (orig) => ({ ...(await orig<typeof import("../../api")>()), ...api }));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
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
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-7, "claude", "看看测试为什么挂了"));
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
    await waitFor(() => expect(onLaunched).toHaveBeenCalledWith(-3, "codex", ""));
    // 预填目录不在最近列表里也照用，不被最近一项顶掉。
    expect(api.newSession).toHaveBeenCalledWith(normalizePath("D:/other"), "codex", {});
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
});
