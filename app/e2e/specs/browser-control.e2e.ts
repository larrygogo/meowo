import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

type Invoke = <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
type Point = { x: number; y: number };

const invoke: Invoke = <T,>(command: string, args: Record<string, unknown> = {}) =>
  browser.execute(async (cmd, payload) => {
    const api = (window as unknown as { __TAURI__?: { core?: { invoke?: Invoke } } }).__TAURI__?.core?.invoke;
    if (!api) throw new Error("当前窗口没有 Tauri E2E 桥");
    return api(cmd, payload);
  }, command, args) as Promise<T>;

async function findAppWindow(): Promise<string> {
  const current = await browser.getWindowHandle();
  for (const handle of await browser.getWindowHandles()) {
    await browser.switchToWindow(handle);
    const url = await browser.execute(() => location.href);
    if (!url.includes("about:blank")) return handle;
  }
  await browser.switchToWindow(current);
  throw new Error("找不到 Meowo 主窗口");
}

async function agentEvaluate<T>(expression: string): Promise<T> {
  const result = await invoke<{ text: string }>("browser_agent_e2e", {
    sessionId: 1,
    tool: "browser_evaluate",
    args: { expression },
  });
  return JSON.parse(result.text) as T;
}

const describeNative = process.platform === "win32" ? describe : describe.skip;

describeNative("面板浏览器：Agent 控制与人工接管", () => {
  let server: Server;
  let origin = "";
  let control = "";
  let paneId: number | null = null;
  /** 服务端最近一次收到的页面请求头（设备模拟要核对 UA 和附加请求头真的发出去了）。 */
  let lastHeaders: Record<string, string | string[] | undefined> = {};

  const nativeClick = async (point: Point) => {
    const trace = await invoke<string>("browser_pane_e2e_native_click", { id: paneId, ...point });
    await browser.pause(200);
    return trace;
  };
  const hitPoint = () => agentEvaluate<Point>(`(() => {
    const r = document.querySelector('#hit').getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);
  const framePoint = () => agentEvaluate<Point>(`(() => {
    const frame = document.querySelector('iframe');
    const outer = frame.getBoundingClientRect();
    const r = frame.contentDocument.querySelector('#frameHit').getBoundingClientRect();
    return { x: outer.left + r.left + r.width / 2, y: outer.top + r.top + r.height / 2 };
  })()`);
  const readClicks = () => agentEvaluate<number>("window.__clicks || 0");
  const readFrameClicks = () => agentEvaluate<number>("window.__frameClicks || 0");

  before(async () => {
    server = createServer((request, response) => {
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      if (request.url?.startsWith("/emu")) {
        lastHeaders = request.headers;
        // /emu-wide 的 #far 放在 1000px 处：只有缩放映射对了，人工点击才落得到它（不缩放时那里在
        // 面板外）。其余页面不放超宽内容：手机模拟下超宽内容会让页面像真手机一样缩小装下。
        const far = request.url.startsWith("/emu-wide")
          ? `<button id="far" style="position:absolute;left:1000px;top:480px;width:240px;height:120px" onclick="window.__far=(window.__far||0)+1">Far</button>`
          : "";
        response.end(`<!doctype html><html><head><meta name="viewport" content="width=device-width"></head><body style="margin:0">${far}</body></html>`);
        return;
      }
      if (request.url === "/frame") {
        response.end(`<!doctype html><html><body>
          <button id="frameHit" onclick="parent.__frameClicks=(parent.__frameClicks||0)+1">Frame hit</button>
        </body></html>`);
        return;
      }
      response.end(`<!doctype html><html><body>
        <button id="hit" onclick="window.__clicks=(window.__clicks||0)+1">Hit</button>
        <iframe src="/frame"></iframe>
      </body></html>`);
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;

    control = await findAppWindow();
    await browser.switchToWindow(control);
    await invoke("mark_onboarding_seen");
    await invoke("browser_agent_e2e", { sessionId: 1, tool: "browser_open", args: { url: origin } });
    const panes = await invoke<{ id: number }[]>("browser_pane_list", { owner: 1 });
    paneId = panes.at(-1)?.id ?? null;
    expect(paneId).not.toBeNull();

    // 子 WebView 不出现在 WDIO handles 中；显示真实 chat 窗口并给页面一块可见区域。
    await invoke("open_chat_window", { sessionId: 1 });
    await browser.pause(1_000);
    await invoke("browser_pane_set_bounds", { id: paneId, x: 40, y: 80, width: 900, height: 600, visible: true });
    await browser.pause(300);
  });

  after(async () => {
    if (control && paneId !== null) {
      await browser.switchToWindow(control);
      await invoke("browser_pane_set_agent_paused", { id: paneId, paused: false });
      await invoke("browser_pane_close", { id: paneId });
    }
    if (server) await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  });

  it("人工点击被锁、Agent 操作生效、接管恢复并中止长等待", async () => {
    const traces: string[] = [];
    expect(await readClicks()).toBe(0);
    const mainPoint = await hitPoint();
    const iframePoint = await framePoint();
    traces.push(`main=${JSON.stringify(mainPoint)} frame=${JSON.stringify(iframePoint)}`);
    traces.push(await nativeClick(mainPoint));
    expect(await readClicks()).toBe(0);

    await invoke("browser_agent_e2e", {
      sessionId: 1,
      tool: "browser_evaluate",
      args: { expression: "document.querySelector('#hit').click()" },
    });
    expect(await readClicks()).toBe(1);
    expect(await agentEvaluate<{ gate: string }>("({ gate: typeof window.__meowoInputGate })")).toEqual({ gate: "undefined" });

    const shot = await invoke<{ image?: string }>("browser_agent_e2e", {
      sessionId: 1,
      tool: "browser_screenshot",
      args: {},
    });
    expect(shot.image?.length ?? 0).toBeGreaterThan(1_000);

    traces.push(await nativeClick(iframePoint));
    expect(await readFrameClicks()).toBe(0);

    await browser.switchToWindow(control);
    await invoke("browser_pane_set_agent_paused", { id: paneId, paused: true });
    await browser.pause(300);
    traces.push(await nativeClick(mainPoint));
    traces.push(await nativeClick(iframePoint));
    await invoke("browser_pane_set_agent_paused", { id: paneId, paused: false });
    const clicksAfterHandback = await readClicks();
    if (clicksAfterHandback !== 2) throw new Error(`接管后点击计数 ${clicksAfterHandback}，原生轨迹：\n${traces.join("\n")}`);
    expect(clicksAfterHandback).toBe(2);
    expect(await readFrameClicks()).toBe(1);

    await browser.switchToWindow(control);
    const outcome = await browser.execute(async (id) => {
      const api = (window as unknown as { __TAURI__: { core: { invoke: Invoke } } }).__TAURI__.core.invoke;
      const waiting = api("browser_agent_e2e", {
        sessionId: 1,
        tool: "browser_wait_for",
        args: { seconds: 10 },
      }).then((value) => ({ ok: true, value }), (error) => ({ ok: false, message: String(error) }));
      await new Promise((resolve) => setTimeout(resolve, 300));
      await api("browser_pane_set_agent_paused", { id, paused: true });
      return waiting;
    }, paneId);
    expect(outcome).toHaveProperty("ok", false);
    expect(String((outcome as { message: unknown }).message)).toContain("browser/user_takeover");
  });

  it("设备模拟：快照报视口，桌面/手机切换改尺寸与请求，跳转和拖面板后保持，缩放后人工点击落点正确，接管时拒绝，还原恢复", async () => {
    const agent = (tool: string, args: Record<string, unknown> = {}) =>
      invoke<{ text: string }>("browser_agent_e2e", { sessionId: 1, tool, args });
    // 与快照同一口径：媒体查询看到的视口（手机模拟下页面缩小装超宽内容时 innerWidth 不准）。
    const size = () => agentEvaluate<[number, number]>("(() => { const s = (window.visualViewport && visualViewport.scale) || 1; return [Math.round(innerWidth * s), Math.round(innerHeight * s)]; })()");
    const emulationOf = async () => (await invoke<{ id: number; emulation: unknown }[]>("browser_pane_list", { owner: 1 }))
      .find((pane) => pane.id === paneId)?.emulation ?? null;
    await browser.switchToWindow(control);
    await invoke("browser_pane_set_agent_paused", { id: paneId, paused: false });
    await invoke("browser_pane_set_bounds", { id: paneId, x: 40, y: 80, width: 900, height: 600, visible: true });
    await browser.pause(300);

    // 预判：未模拟时快照写出面板实际视口。
    const opened = await agent("browser_navigate", { url: `${origin}emu` });
    expect(opened.text).toContain("视口：900×600（未模拟");

    // 桌面：只改尺寸不刷新（页面上的标记还在），视口变成 1280×800。
    await agentEvaluate("window.__marker = 1");
    const desktop = await agent("browser_emulate", { device: "desktop" });
    expect(desktop.text).toContain("模拟桌面 1280×800");
    expect(desktop.text).not.toContain("已刷新");
    expect(await size()).toEqual([1280, 800]);
    expect(await agentEvaluate<number>("window.__marker || 0")).toBe(1);

    // 手机 + 附加请求头：自动刷新，服务端收到 iPhone UA 和附加头。
    const mobile = await agent("browser_emulate", { device: "mobile", headers: { "X-Meowo-Test": "e2e" } });
    expect(mobile.text).toContain("已刷新");
    expect(await size()).toEqual([390, 844]);
    expect(String(lastHeaders["user-agent"])).toContain("iPhone");
    expect(lastHeaders["x-meowo-test"]).toBe("e2e");
    expect((await agentEvaluate<{ ua: string }>("({ ua: navigator.userAgent })")).ua).toContain("iPhone");
    const wire = JSON.stringify(await emulationOf());
    expect(wire).toContain("X-Meowo-Test");
    expect(wire).not.toContain("e2e");

    // 跳转后仍保持。
    await agent("browser_navigate", { url: `${origin}emu?next=1` });
    expect(await size()).toEqual([390, 844]);
    expect(String(lastHeaders["user-agent"])).toContain("iPhone");

    // 桌面模拟比面板大：缩到面板里。接管后按缩放后的屏幕位置人工点 #far，要点得中。
    await agent("browser_emulate", { device: "desktop" });
    await agent("browser_navigate", { url: `${origin}emu-wide` });
    expect(await size()).toEqual([1280, 800]);
    const scale = Math.min(900 / 1280, 600 / 800);
    const far = await agentEvaluate<Point>(`(() => { const r = document.querySelector('#far').getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
    await browser.switchToWindow(control);
    await invoke("browser_pane_set_agent_paused", { id: paneId, paused: true });
    await browser.pause(300);
    const trace = await nativeClick({ x: far.x * scale, y: far.y * scale });

    // 接管中 Agent 改模拟被拒，状态不变。
    const refused = await browser.execute(async () => {
      const api = (window as unknown as { __TAURI__: { core: { invoke: Invoke } } }).__TAURI__.core.invoke;
      return api("browser_agent_e2e", { sessionId: 1, tool: "browser_emulate", args: { device: "mobile" } })
        .then(() => "ok", (error) => String(error));
    });
    expect(refused).toContain("browser/user_takeover");
    expect(JSON.stringify(await emulationOf())).toContain('"width":1280');
    await invoke("browser_pane_set_agent_paused", { id: paneId, paused: false });
    const farClicks = await agentEvaluate<number>("window.__far || 0");
    if (farClicks !== 1) throw new Error(`缩放后人工点击没落到 #far（${farClicks}），轨迹：${trace}`);

    // 拖窄面板、收起面板：视口仍是模拟值。
    await invoke("browser_pane_set_bounds", { id: paneId, x: 40, y: 80, width: 600, height: 500, visible: true });
    await browser.pause(300);
    expect(await size()).toEqual([1280, 800]);
    await invoke("browser_pane_set_bounds", { id: paneId, x: 0, y: 0, width: 0, height: 0, visible: false });
    await browser.pause(300);
    expect(await size()).toEqual([1280, 800]);

    // 还原：回到面板尺寸和默认 UA。
    await invoke("browser_pane_set_bounds", { id: paneId, x: 40, y: 80, width: 900, height: 600, visible: true });
    await browser.pause(300);
    // 还原前是「桌面」（没改 UA 和请求头），只动尺寸，不用刷新。
    const reset = await agent("browser_emulate", { device: "reset" });
    expect(reset.text).toContain("已还原为面板实际大小");
    expect(reset.text).not.toContain("已刷新");
    expect(await size()).toEqual([900, 600]);
    expect((await agentEvaluate<{ ua: string }>("({ ua: navigator.userAgent })")).ua).not.toContain("iPhone");
    expect(String(lastHeaders["user-agent"])).not.toContain("iPhone");
    expect(lastHeaders["x-meowo-test"]).toBeUndefined();
    expect(await emulationOf()).toBeNull();

    // 参数不对：中文报错，不改状态。
    const bad = await browser.execute(async () => {
      const api = (window as unknown as { __TAURI__: { core: { invoke: Invoke } } }).__TAURI__.core.invoke;
      return api("browser_agent_e2e", { sessionId: 1, tool: "browser_emulate", args: { device: "watch" } })
        .then(() => "ok", (error) => String(error));
    });
    expect(bad).toContain("device 只能是");
    expect(await emulationOf()).toBeNull();
  });
});
