// 设置 › GitHub：四种连接方式、设备码登录（显示代码、复制并打开、取消）、登录结果事件、退出前确认。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";

const api = vi.hoisted(() => ({
  githubStatus: vi.fn(),
  githubSetMode: vi.fn(),
  githubLoginStart: vi.fn(),
  githubLoginCancel: vi.fn(),
  githubLogout: vi.fn(),
  openLink: vi.fn(),
}));
vi.mock("../../api", async (o) => ({ ...(await o<typeof import("../../api")>()), ...api }));
const confirm = vi.hoisted(() => ({ appConfirm: vi.fn(), appAlert: vi.fn() }));
vi.mock("../../confirm", () => confirm);
const events = vi.hoisted(() => ({ handlers: new Map<string, (e: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (e: { payload: unknown }) => void) => {
    events.handlers.set(name, handler);
    return Promise.resolve(() => events.handlers.delete(name));
  },
}));

import { GithubSection, minutesLeft } from "./GithubSection";
import { zh } from "../../i18n/zh";
import type { GithubConnectionDto } from "../../api";

const base: GithubConnectionDto = {
  mode: "auto", active: "gh", account: null, needsRelogin: false, ghInstalled: true, credentialSupported: true, pending: null,
};
const code = { userCode: "WDJB-MJHT", verificationUri: "https://github.com/login/device", expiresAt: Date.now() + 15 * 60_000 };

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  confirm.appConfirm.mockReset();
  events.handlers.clear();
  api.openLink.mockResolvedValue(undefined);
  api.githubLoginCancel.mockResolvedValue(undefined);
});
afterEach(() => cleanup());

describe("GithubSection", () => {
  it("四个选项，选中当前方式；切换调后端并显示实际在用", async () => {
    api.githubStatus.mockResolvedValue(base);
    api.githubSetMode.mockResolvedValue({ ...base, mode: "off", active: "off" });
    render(<GithubSection />);
    const auto = await screen.findByRole("radio", { name: zh.github.modeAuto });
    expect(auto.getAttribute("aria-checked")).toBe("true");
    expect(screen.getAllByRole("radio")).toHaveLength(4);
    expect(screen.getByTestId("github-active").textContent).toBe(zh.github.activeGh(true));
    fireEvent.click(screen.getByRole("radio", { name: zh.github.modeOff }));
    await waitFor(() => expect(api.githubSetMode).toHaveBeenCalledWith("off"));
    await waitFor(() => expect(screen.getByTestId("github-active").textContent).toBe(zh.github.activeOff));
    expect(screen.getByText(zh.github.modeDesc.off)).toBeTruthy();
  });

  it("登录：显示代码，复制并打开 GitHub，取消后回到未登录", async () => {
    api.githubStatus.mockResolvedValue(base);
    api.githubLoginStart.mockResolvedValue(code);
    const writeText = vi.fn((_text: string) => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<GithubSection />);
    fireEvent.click(await screen.findByText(zh.github.signIn));
    expect((await screen.findByTestId("github-code")).textContent).toBe("WDJB-MJHT");
    expect(screen.getByText(zh.github.waiting)).toBeTruthy();
    fireEvent.click(screen.getByText(zh.github.copyAndOpen));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("WDJB-MJHT"));
    expect(api.openLink).toHaveBeenCalledWith("https://github.com/login/device");
    fireEvent.click(screen.getByText(zh.github.cancel));
    await waitFor(() => expect(api.githubLoginCancel).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("github-code")).toBeNull());
    expect(screen.getByText(zh.github.signIn)).toBeTruthy();
  });

  it("设置窗重开时接着显示进行中的代码；过期事件给出说明", async () => {
    api.githubStatus.mockResolvedValueOnce({ ...base, pending: code }).mockResolvedValue(base);
    render(<GithubSection />);
    expect((await screen.findByTestId("github-code")).textContent).toBe("WDJB-MJHT");
    await waitFor(() => expect(events.handlers.has("github-login-finished")).toBe(true));
    await act(async () => { events.handlers.get("github-login-finished")!({ payload: { state: "expired", error: null } }); });
    expect(await screen.findByText(zh.github.expired)).toBeTruthy();
  });

  it("登录成功事件后显示账号；退出要先确认，取消确认不退出", async () => {
    const signedIn = { ...base, active: "meowo", account: { login: "octocat", name: "The Octocat", avatarUrl: null } };
    api.githubStatus.mockResolvedValueOnce(base).mockResolvedValue(signedIn);
    render(<GithubSection />);
    await screen.findByText(zh.github.signIn);
    await waitFor(() => expect(events.handlers.has("github-login-finished")).toBe(true));
    await act(async () => { events.handlers.get("github-login-finished")!({ payload: { state: "ok", error: null } }); });
    expect(await screen.findByText("The Octocat · octocat")).toBeTruthy();
    expect(screen.getByTestId("github-active").textContent).toBe(zh.github.activeMeowo("octocat"));
    confirm.appConfirm.mockResolvedValueOnce(false);
    fireEvent.click(screen.getByText(zh.github.signOut));
    await waitFor(() => expect(confirm.appConfirm).toHaveBeenCalledTimes(1));
    expect(api.githubLogout).not.toHaveBeenCalled();
    confirm.appConfirm.mockResolvedValueOnce(true);
    api.githubLogout.mockResolvedValue(base);
    fireEvent.click(screen.getByText(zh.github.signOut));
    await waitFor(() => expect(api.githubLogout).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(zh.github.signIn)).toBeTruthy();
  });

  it("令牌被拒时提示重新登录；没有凭据库时不给登录按钮", async () => {
    api.githubStatus.mockResolvedValue({ ...base, active: "meowo", needsRelogin: true, account: { login: "octocat", name: null, avatarUrl: null } });
    render(<GithubSection />);
    expect(await screen.findByText(zh.github.reloginDesc)).toBeTruthy();
    expect(screen.getByText(zh.github.relogin)).toBeTruthy();
    cleanup();
    api.githubStatus.mockResolvedValue({ ...base, credentialSupported: false });
    render(<GithubSection />);
    expect(await screen.findByText(zh.github.unsupported)).toBeTruthy();
    expect(screen.queryByText(zh.github.signIn)).toBeNull();
  });

  it("登录请求失败：错误行带原因", async () => {
    api.githubStatus.mockResolvedValue(base);
    api.githubLoginStart.mockRejectedValue("github/network: dns");
    render(<GithubSection />);
    fireEvent.click(await screen.findByText(zh.github.signIn));
    expect((await screen.findByRole("alert")).textContent).toContain("GitHub");
  });

  it("剩余分钟向上取整、至少 1", () => {
    expect(minutesLeft(60_000 * 14.2, 0)).toBe(15);
    expect(minutesLeft(0, 10)).toBe(1);
  });
});
