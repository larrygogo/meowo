import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization）。
const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  availableTerminals: vi.fn(),
  getSettings: vi.fn(),
}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { NewSessionPanel } from "./NewSessionPanel";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.recentCwds.mockResolvedValue([]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.availableTerminals.mockResolvedValue(["wt"]);
  api.getSettings.mockResolvedValue({ default_agent: "claude", resume_terminal: "wt" });
});

// 本仓库测试不依赖 @testing-library/jest-dom（未安装该包，其余测试也都不用），
// toBeDisabled/toBeInTheDocument/toHaveTextContent 改用原生 DOM 断言等价替换。
afterEach(() => cleanup());

describe("NewSessionPanel", () => {
  it("目录为空时启动禁用", async () => {
    render(<NewSessionPanel onClose={() => {}} onLaunched={() => {}} />);
    const launch = await screen.findByTestId("ns-launch");
    expect((launch as HTMLButtonElement).disabled).toBe(true);
  });

  it("填目录后启动调 newSession 并回调", async () => {
    api.newSession.mockResolvedValue(undefined);
    const onLaunched = vi.fn();
    render(<NewSessionPanel onClose={() => {}} onLaunched={onLaunched} />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/proj", "claude", "wt"));
    await waitFor(() => expect(onLaunched).toHaveBeenCalled());
  });

  it("hooks 未装显示警告", async () => {
    api.checkProviderHooks.mockResolvedValue("missing");
    render(<NewSessionPanel onClose={() => {}} onLaunched={() => {}} />);
    expect(await screen.findByTestId("ns-hooks-warn")).toBeTruthy();
  });

  it("启动失败显示错误、不回调", async () => {
    api.newSession.mockRejectedValue("启动终端失败");
    const onLaunched = vi.fn();
    render(<NewSessionPanel onClose={() => {}} onLaunched={onLaunched} />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    expect((await screen.findByTestId("ns-error")).textContent).toContain("启动终端失败");
    expect(onLaunched).not.toHaveBeenCalled();
  });
});
