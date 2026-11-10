import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TokenGate } from "./TokenGate";
import { clearToken, getToken, parsePairingInput } from "./transport";

// 桌面「远程访问」的「复制地址」给的是整串 http://ip:port/#token=…；用户原样粘进配对页时，
// 此前整串被当成令牌发出去，必然 401（「令牌不对」），只有扫码能配上。
const TOKEN = "AbC123-xyz_789";

function stubFetch() {
  const fn = vi.fn(async (_url: string, init?: RequestInit) => {
    const sent = (init?.headers as Record<string, string>)["X-Meowo-Token"];
    return { ok: sent === TOKEN, status: sent === TOKEN ? 200 : 401, text: async () => "\"windows\"" };
  });
  vi.stubGlobal("fetch", fn);
  return fn;
}

beforeEach(() => clearToken());
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  clearToken();
});

describe("手机配对页手动输入", () => {
  it.each([
    ["整段地址", `http://192.168.1.8:18620/#token=${TOKEN}`],
    ["只有 fragment", `#token=${TOKEN}`],
    ["纯令牌（前后带空白）", `  ${TOKEN}\n`],
  ])("%s 都能配上", async (_label, input) => {
    const fetchFn = stubFetch();
    render(<TokenGate><div>已进入</div></TokenGate>);
    fireEvent.change(screen.getByLabelText(/令牌/), { target: { value: input } });
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(screen.getByText("已进入")).toBeTruthy());
    expect((fetchFn.mock.calls[0][1]?.headers as Record<string, string>)["X-Meowo-Token"]).toBe(TOKEN);
    expect(getToken()).toBe(TOKEN);
  });

  it("像地址却没带令牌：直接提示令牌不对，不发请求", () => {
    const fetchFn = stubFetch();
    render(<TokenGate><div>已进入</div></TokenGate>);
    fireEvent.change(screen.getByLabelText(/令牌/), { target: { value: "http://192.168.1.8:18620/" } });
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(fetchFn).not.toHaveBeenCalled();
  });
});

describe("parsePairingInput", () => {
  it("三种形态解析到同一个令牌", () => {
    expect(parsePairingInput(`http://10.0.0.2:18620/#token=${TOKEN}`)).toBe(TOKEN);
    expect(parsePairingInput(`#token=${TOKEN}`)).toBe(TOKEN);
    expect(parsePairingInput(TOKEN)).toBe(TOKEN);
    expect(parsePairingInput(`http://h/#token=${encodeURIComponent("a/b+c")}`)).toBe("a/b+c");
  });
  it("空白、无令牌的地址返回 null", () => {
    expect(parsePairingInput("   ")).toBeNull();
    expect(parsePairingInput("http://10.0.0.2:18620/")).toBeNull();
    expect(parsePairingInput("two words")).toBeNull();
  });
});
