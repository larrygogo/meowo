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
});
