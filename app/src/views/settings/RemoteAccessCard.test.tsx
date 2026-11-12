// 手机远程配对卡:扫码地址必须内含 token(二维码编码的正是这串 URL),开关落盘走
// set_settings,启动失败如实红字。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  remoteAccessInfo: vi.fn(),
  testBarkPush: vi.fn(),
  regenerateRemoteToken: vi.fn(),
}));
vi.mock("../../api", async (o) => ({ ...(await o<typeof import("../../api")>()), ...api }));
// 重新生成前的确认框是原生小窗（confirm_dialog），测试里直接当用户点了「重新生成」。
vi.mock("../../confirm", () => ({ appConfirm: () => Promise.resolve(true), appAlert: () => Promise.resolve() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

import { RemoteAccessCard } from "./RemoteAccessCard";
import { SETTINGS_DEFAULTS } from "./state";
import { zh } from "../../i18n/zh";

const TOKEN = "deadbeefdeadbeefdeadbeefdeadbeef11223344";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.setSettings.mockResolvedValue(undefined);
});
afterEach(() => cleanup());

describe("RemoteAccessCard", () => {
  it("开启后二维码在场,复制拿到含 token 的完整地址;有 Tailscale 默认选它", async () => {
    api.getSettings.mockResolvedValue({
      ...SETTINGS_DEFAULTS,
      remote_access_enabled: true,
      remote_access_port: 18620,
      remote_access_token: TOKEN,
    });
    // 后端 Tailscale 优先排序;桌面猜不出手机在哪个网,Tailscale 是唯一不挑网的地址,
    // 默认必须落它,而不是局域网 IP。
    api.remoteAccessInfo.mockResolvedValue({
      enabled: true,
      port: 18620,
      token: TOKEN,
      ips: [
        { ip: "100.64.0.7", kind: "tailscale" },
        { ip: "192.168.1.5", kind: "lan" },
      ],
      lastError: null,
    });
    // 标注入参:vi.fn 零参推断会让 mock.calls[0][0] 变成空元组取值报 TS2493。
    const writeText = vi.fn((_text: string) => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<RemoteAccessCard />);

    // 地址不明文上屏(含 token 的凭据串),入口只有复制按钮与二维码本体。
    fireEvent.click(await screen.findByText(zh.remote.copy));
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const copied = writeText.mock.calls[0][0];
    expect(copied).toMatch(/^http:\/\/100\.64\.0\.7:18620\/#token=/);
    expect(copied).toContain(`#token=${TOKEN}`);
    expect(screen.queryByText(copied)).toBeNull();
    // 二维码就地生成(uqr renderSVG),扫码区确有 <svg>。
    expect(document.querySelector(".remote-qr svg")).toBeTruthy();
    // 选中候选的可达性说明如实标注。
    expect(screen.getByText(zh.remote.hintTailscale)).toBeTruthy();
  });

  // 手机推送(Bark):远程访问关着也能配;测试用输入框里的草稿,不必先保存;失焦才落盘。
  it("Bark 推送:测试用草稿地址,失败给原因,失焦落盘 bark_url", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS });
    api.remoteAccessInfo.mockResolvedValue({ enabled: false, port: 18620, token: TOKEN, ips: [], lastError: null });
    api.testBarkPush.mockRejectedValueOnce("push/invalid_url").mockResolvedValueOnce(undefined);
    render(<RemoteAccessCard />);
    const input = await screen.findByRole("textbox", { name: zh.remote.pushLabel });
    fireEvent.change(input, { target: { value: "https://api.day.app/KEY/" } });
    fireEvent.click(screen.getByRole("button", { name: zh.remote.pushTest }));
    expect(await screen.findByText("推送地址无效，请从 Bark App 里复制")).toBeTruthy();
    expect(api.testBarkPush).toHaveBeenCalledWith("https://api.day.app/KEY/");
    expect(api.setSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: zh.remote.pushTest }));
    expect(await screen.findByText(zh.remote.pushTestOk)).toBeTruthy();
    fireEvent.blur(input);
    await waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(expect.objectContaining({ bark_url: "https://api.day.app/KEY/" })));
  });

  it("关闭态点开关:落盘 remote_access_enabled=true", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS, remote_access_enabled: false });
    api.remoteAccessInfo.mockResolvedValue({ enabled: false, port: 18620, token: "", ips: [], lastError: null });
    render(<RemoteAccessCard />);

    // 关闭态只显开关 + 端口,不渲染二维码;也不再单占一行写「关闭时手机无法连接」
    // (开关的说明已经讲清楚了,设置页实拍那一行是孤零零的空行)。
    await screen.findByRole("switch", { name: zh.remote.enable });
    expect(screen.queryByText(zh.remote.offHint)).toBeNull();
    expect(document.querySelector(".remote-qr")).toBeNull();

    fireEvent.click(screen.getByRole("switch", { name: zh.remote.enable }));
    await waitFor(() =>
      expect(api.setSettings).toHaveBeenCalledWith(
        expect.objectContaining({ remote_access_enabled: true }),
      ),
    );
  });

  it("启动失败(端口被占)红字显示原因", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS, remote_access_enabled: true, remote_access_token: TOKEN });
    api.remoteAccessInfo.mockResolvedValue({
      enabled: true,
      port: 18620,
      token: TOKEN,
      ips: [],
      lastError: "绑定端口 18620 失败：地址已被占用",
    });
    render(<RemoteAccessCard />);

    expect(await screen.findByText(zh.remote.startError("绑定端口 18620 失败：地址已被占用"))).toBeTruthy();
    // 无可达 IP → 回退手输提示,不出二维码。
    expect(await screen.findByText(zh.remote.noIp)).toBeTruthy();
    expect(document.querySelector(".remote-qr")).toBeNull();
  });

  // T-1（2026-11-12 走查）：重新生成失败此前被 .catch(() => {}) 吞掉，二维码不变、零提示，
  // 用户会以为丢失手机上的旧令牌已被吊销。失败要说「旧令牌仍有效」并能就地重试；成功也要说一句。
  // 兄弟场景：同卡的端口/开关/绑定模式保存失败已走 SettingsError（patchError），Bark 测试失败有原因。
  it("重新生成令牌失败:明确说旧令牌仍有效,点重试再发一次,成功给确认", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS_DEFAULTS, remote_access_enabled: true, remote_access_token: TOKEN });
    api.remoteAccessInfo.mockResolvedValue({
      enabled: true, port: 18620, boundPort: 18620, token: TOKEN,
      ips: [{ ip: "192.168.1.5", kind: "lan" }], lastError: null,
    });
    api.regenerateRemoteToken.mockRejectedValueOnce("拒绝访问。 (os error 5)").mockResolvedValueOnce(undefined);
    render(<RemoteAccessCard />);
    fireEvent.click(await screen.findByRole("button", { name: zh.remote.regenerate }));
    const alert = await screen.findByText(zh.remote.regenerateFailed("拒绝访问。 (os error 5)"));
    expect(alert.closest("[role=alert]")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.settings.retry }));
    expect(await screen.findByText(zh.remote.regenerateDone)).toBeTruthy();
    expect(screen.queryByText(zh.remote.regenerateFailed("拒绝访问。 (os error 5)"))).toBeNull();
    expect(api.regenerateRemoteToken).toHaveBeenCalledTimes(2);
  });
});
