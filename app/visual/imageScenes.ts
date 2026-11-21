// 图片场景：agent 发来的图片消息、工具行缩略图、正文 Markdown 图片的各种状态。桌面是 flow-images-<段>，手机是
// mobile-images-<段>，两边用同一份数据。
//
// 时间线按前后端约定（任务目录 research/contract.md）的形态写：发图工具调用 kind=image、summary 是参数原文 JSON、
// 回执是 `[Image: source: <~/.meowo/images 下的路径>]` 或 `[Image error: <code>] 原因`；正文图片走 resolve_chat_image。
// 图片本身是现画的 PNG（canvas），按路径末段映射：截图、图表、照片类参考图、超高长图、48px 图标。
//   main：单张带说明、多张相册、长图、小图标、正在发送
//   states：四种失败、工具行缩略图（截图、读图、写出图片）、正文 Markdown 图片（成功、失败、加载中）
//   real-<家>：五家真机发图（与截图）样本经后端真实解析的产物，见下方 REAL_SAMPLES。
import type { ChatItem } from "../src/api";
import { FIXED_NOW } from "./fixtures";
import claudeSend from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/claude.83386c1d.items.json";
import codexSilent from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/codex.01a1f9bf.items.json";
import codexSend from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/codex.probe-send-image.items.json";
import geminiShot from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/gemini.probe-screenshot.items.json";
import geminiSend from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/gemini.probe-send-image.items.json";
import kimiSend from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/kimi.probe-send-image.items.json";
import opencodeShot from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.probe-screenshot.items.json";
import opencodeSend from "../src-tauri/crates/meowo-agent/tests/fixtures/chat-replay/opencode.probe-send-image.items.json";

type Lang = "zh" | "en";
type Internals = { convertFileSrc?: (path: string) => string };

const ROOT = "C:\\Users\\demo\\.meowo\\images";

function canvas(w: number, h: number, draw: (g: CanvasRenderingContext2D) => void): string {
  const c = document.createElement("canvas");
  c.width = w; c.height = h;
  const g = c.getContext("2d");
  if (!g) return "";
  draw(g);
  return c.toDataURL("image/png");
}
const box = (g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, fill: string, r = 10) => {
  g.fillStyle = fill; g.beginPath(); g.roundRect(x, y, w, h, r); g.fill();
};

/** 登录页截图（浅色网页，1280×800）。 */
function loginShot(): string {
  return canvas(1280, 800, (g) => {
    g.fillStyle = "#f5f6f8"; g.fillRect(0, 0, 1280, 800);
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, 1280, 64);
    box(g, 40, 18, 120, 28, "#1f2937", 6);
    box(g, 440, 150, 400, 470, "#ffffff", 16);
    g.fillStyle = "#111827"; g.font = "bold 30px sans-serif"; g.fillText("Sign in", 480, 220);
    box(g, 480, 260, 320, 48, "#eef0f3", 8); box(g, 480, 330, 320, 48, "#eef0f3", 8);
    box(g, 480, 420, 320, 50, "#2f6fed", 8);
    g.fillStyle = "#fff"; g.font = "20px sans-serif"; g.fillText("Continue", 598, 452);
    g.fillStyle = "#9ca3af"; g.font = "16px sans-serif"; g.fillText("Forgot password?", 572, 520);
  });
}
/** 对照场景的登录页：改前输入框矮一些、主按钮是灰的；改后和 loginShot 一样。 */
function drawLogin(g: CanvasRenderingContext2D, after: boolean) {
  g.fillStyle = "#f5f6f8"; g.fillRect(0, 0, 1280, 800);
  g.fillStyle = "#ffffff"; g.fillRect(0, 0, 1280, 64);
  box(g, 40, 18, 120, 28, "#1f2937", 6);
  box(g, 440, 150, 400, 470, "#ffffff", 16);
  g.fillStyle = "#111827"; g.font = "bold 30px sans-serif"; g.fillText("Sign in", 480, 220);
  const h = after ? 48 : 40;
  box(g, 480, 260, 320, h, "#eef0f3", 8); box(g, 480, 282 + h, 320, h, "#eef0f3", 8);
  box(g, 480, 420, 320, 50, after ? "#2f6fed" : "#6b7280", 8);
  g.fillStyle = "#fff"; g.font = "20px sans-serif"; g.fillText("Continue", 598, 452);
  g.fillStyle = "#9ca3af"; g.font = "16px sans-serif"; g.fillText("Forgot password?", 572, 520);
}
/** 差异图：和后端 chat_images.rs diff_images 同一种画法（没变的淡灰、变了的标红）。 */
function loginDiff(): string {
  const read = (after: boolean) => {
    const c = document.createElement("canvas"); c.width = 1280; c.height = 800;
    const g = c.getContext("2d"); if (!g) return null;
    drawLogin(g, after);
    return g.getImageData(0, 0, 1280, 800).data;
  };
  const a = read(false); const b = read(true);
  return canvas(1280, 800, (g) => {
    if (!a || !b) return;
    const out = g.createImageData(1280, 800);
    for (let i = 0; i < out.data.length; i += 4) {
      const changed = [0, 1, 2].some((k) => Math.abs(a[i + k] - b[i + k]) > 24);
      if (changed) { out.data[i] = 230; out.data[i + 1] = 40; out.data[i + 2] = 40; }
      else { const gray = (b[i] * 299 + b[i + 1] * 587 + b[i + 2] * 114) / 1000; const faint = 255 - (255 - gray) / 5; out.data[i] = out.data[i + 1] = out.data[i + 2] = faint; }
      out.data[i + 3] = 255;
    }
    g.putImageData(out, 0, 0);
  });
}

/** 柱状图（AI 画的图表，800×500）。 */
function chart(): string {
  return canvas(800, 500, (g) => {
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, 800, 500);
    g.strokeStyle = "#e5e7eb"; g.lineWidth = 1;
    for (let i = 0; i < 5; i++) { g.beginPath(); g.moveTo(60, 80 + i * 80); g.lineTo(760, 80 + i * 80); g.stroke(); }
    const bars = [180, 260, 220, 320, 290, 360];
    bars.forEach((h, i) => box(g, 90 + i * 110, 400 - h, 60, h, i === 5 ? "#2f6fed" : "#9db8f5", 6));
    g.fillStyle = "#111827"; g.font = "bold 22px sans-serif"; g.fillText("Weekly sessions", 60, 50);
  });
}
/** 参考图（照片感的渐变风景，1024×768）。 */
function photo(): string {
  return canvas(1024, 768, (g) => {
    const sky = g.createLinearGradient(0, 0, 0, 768);
    sky.addColorStop(0, "#f6c38b"); sky.addColorStop(0.55, "#e88b6a"); sky.addColorStop(1, "#5b3a63");
    g.fillStyle = sky; g.fillRect(0, 0, 1024, 768);
    g.fillStyle = "#fbe3b5"; g.beginPath(); g.arc(700, 360, 70, 0, Math.PI * 2); g.fill();
    g.fillStyle = "#3b2846"; g.beginPath(); g.moveTo(0, 560); g.lineTo(260, 380); g.lineTo(480, 540); g.lineTo(720, 420); g.lineTo(1024, 600); g.lineTo(1024, 768); g.lineTo(0, 768); g.fill();
  });
}
/** 手机整页长截图（390×2600）。 */
function longShot(): string {
  return canvas(390, 2600, (g) => {
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, 390, 2600);
    box(g, 0, 0, 390, 220, "#2f6fed", 0);
    g.fillStyle = "#fff"; g.font = "bold 28px sans-serif"; g.fillText("Pricing", 24, 130);
    for (let i = 0; i < 9; i++) {
      box(g, 20, 260 + i * 260, 350, 220, i % 2 ? "#f3f4f6" : "#eef3fe", 14);
      g.fillStyle = "#111827"; g.font = "bold 20px sans-serif"; g.fillText(`Plan ${i + 1}`, 44, 310 + i * 260);
    }
  });
}
/** 应用图标（48×48，透明底）。 */
function icon(): string {
  return canvas(48, 48, (g) => {
    box(g, 2, 2, 44, 44, "#1d1f24", 11);
    g.fillStyle = "#c8f169"; g.beginPath(); g.arc(18, 24, 5, 0, Math.PI * 2); g.arc(31, 24, 5, 0, Math.PI * 2); g.fill();
  });
}

let images: Record<string, string> | null = null;
function imageFor(path: string): string {
  images ??= { "login-before.png": canvas(1280, 800, (g) => drawLogin(g, false)), "login-after.png": canvas(1280, 800, (g) => drawLogin(g, true)), "login-diff.png": loginDiff(), "login.png": loginShot(), "chart.png": chart(), "palette.png": photo(), "pricing-long.png": longShot(), "icon-48.png": icon(), "ref.png": photo() };
  const name = path.split(/[\\/]/).pop() ?? "";
  // 真机样本的图在探针时的持久目录里（sent/ 是发来的图表，tool/ 是浏览器截图），这台机器上没有：换成同类的现画图。
  return images[name] ?? (/[\\/]sent[\\/]/.test(path) ? images["chart.png"] : /[\\/]tool[\\/]/.test(path) ? images["login.png"] : path);
}

/// 五家真机样本（后端回放测试的快照，与 replayImages.test.tsx 同一份）：每家的发图会话在前、截图会话在后。
/// 不改内容，只把时间整体平移到冻结时刻附近（每份样本内部的间隔不变，份与份之间隔一分钟），免得拍出「几天前」。
const REAL_SAMPLES: Record<string, unknown[]> = {
  claude: [claudeSend],
  codex: [codexSend, codexSilent],
  kimi: [kimiSend],
  gemini: [geminiSend, geminiShot],
  opencode: [opencodeSend, opencodeShot],
};
const REAL_CWD = "C:\\probe\\work";

function realItems(provider: string): ChatItem[] {
  const samples = (REAL_SAMPLES[provider] ?? []) as ChatItem[][];
  return samples.flatMap((sample, index) => {
    const stamps = sample.map((item) => Date.parse(item.timestamp ?? "")).filter(Number.isFinite);
    const shift = FIXED_NOW - 30_000 - (samples.length - 1 - index) * 60_000 - Math.max(...stamps);
    return sample.map((item) => {
      const at = Date.parse(item.timestamp ?? "");
      return { ...item, timestamp: Number.isFinite(at) ? new Date(at + shift).toISOString() : item.timestamp } as ChatItem;
    });
  });
}

/// 挂载前：图片路径映射到现画的 PNG。
export function imageSceneBoot(): void {
  const it = (window as unknown as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__;
  if (it) it.convertFileSrc = (path) => imageFor(path);
}

const sendCall = (id: string, ts: string, source: string, caption: string | null) => ({
  type: "tool_use", id, timestamp: ts, name: "mcp__meowo__send_image", kind: "image",
  summary: JSON.stringify(caption ? { source, caption } : { source }),
}) as unknown as ChatItem;
const receipt = (id: string, ts: string, text: string, isError = false) => ({
  type: "tool_result", id: `${id}-r`, timestamp: ts, tool_use_id: id, text, is_error: isError,
}) as unknown as ChatItem;
const ref = (sub: string, name: string) => `[Image: source: ${ROOT}\\${sub}\\${name}]`;

export function imageSceneItems(part: string, lang: Lang): { items: ChatItem[]; running: boolean } {
  if (part.startsWith("real-")) return { items: realItems(part.slice("real-".length)), running: false };
  const zh = lang === "zh";
  const at = (s: number) => new Date(FIXED_NOW - s * 1000).toISOString();
  const say = (id: string, s: number, text: string) => ({ type: "assistant_text", id, timestamp: at(s), text }) as unknown as ChatItem;
  const user = (id: string, s: number, text: string) => ({ type: "user_text", id, timestamp: at(s), text }) as unknown as ChatItem;
  if (part === "compare") {
    const compareCall = (id: string, s: number) => ({
      type: "tool_use", id, timestamp: at(s), name: "mcp__meowo__compare_images", kind: "other",
      summary: JSON.stringify({ before: "C:\\dev\\web-app\\shots\\before.png", after: "C:\\dev\\web-app\\shots\\after.png" }),
    }) as unknown as ChatItem;
    const caption = zh ? "登录页：输入框加高到 48px，主按钮换成品牌蓝。" : "Login page: inputs are 48px tall, primary button uses the brand blue.";
    return {
      running: false,
      items: [
        user("u1", 300, zh ? "登录页按设计稿改一下，改完给我看对照。" : "Update the login page to match the design and show me a before/after."),
        compareCall("c1", 200),
        receipt("c1", at(199), [
          "对照已发给用户……",
          ref("sent", "login-before.png"), ref("sent", "login-after.png"), ref("sent", "login-diff.png"),
          `[Image compare: ${JSON.stringify({ changed: "4.38%", before: [1280, 800], after: [1280, 800], bounds: [480, 260, 320, 210], caption })}]`,
        ].join("\n")),
        say("a1", 190, zh ? "改动只在登录卡片里，其他地方没变。" : "Changes stay inside the sign-in card; nothing else moved."),
        // 整页长截图：舞台限高、标「长图」。
        compareCall("c2", 120),
        receipt("c2", at(119), [
          ref("sent", "pricing-long.png"), ref("sent", "pricing-long.png"),
          `[Image compare: ${JSON.stringify({ changed: "2.96%", before: [390, 2600], after: [390, 2600], bounds: [20, 2340, 350, 220], caption: null })}]`,
        ].join("\n")),
      ],
    };
  }
  if (part === "main") {
    return {
      running: true,
      items: [
        user("u1", 300, zh ? "登录页改完了吗？截个图给我看看，顺便把三版配色和图标也发我。" : "Is the login page done? Send me a screenshot, plus the three palettes and the icon."),
        sendCall("s1", at(280), "C:\\dev\\web-app\\shots\\login.png", zh ? "登录页改好了：输入框加高到 48px，主按钮换成品牌蓝。" : "Login page is done: inputs are 48px tall and the primary button uses the brand blue."),
        receipt("s1", at(279), ref("sent", "login.png")),
        sendCall("s2", at(270), "C:\\dev\\web-app\\shots", zh ? "三版配色对比，我倾向第二版。" : "Three palettes side by side. I lean toward the second."),
        receipt("s2", at(269), [ref("sent", "palette.png"), ref("sent", "chart.png"), ref("sent", "login.png")].join("\n")),
        say("a1", 260, zh ? "价格页在手机上的整页截图也一起发你：" : "Here is the full-page pricing screenshot on mobile too:"),
        sendCall("s3", at(250), "C:\\dev\\web-app\\shots\\pricing-long.png", null),
        receipt("s3", at(249), ref("sent", "pricing-long.png")),
        sendCall("s4", at(240), "C:\\dev\\web-app\\public\\icon-48.png", zh ? "新的应用图标（48×48）。" : "The new app icon (48×48)."),
        receipt("s4", at(239), ref("sent", "icon-48.png")),
        sendCall("s5", at(10), "C:\\dev\\web-app\\shots\\dashboard.png", zh ? "仪表盘也截好了。" : "The dashboard is captured too."),
      ],
    };
  }
  return {
    running: false,
    items: [
      user("u2", 300, zh ? "把那几张图发我。" : "Send me those images."),
      sendCall("f1", at(290), "C:\\dev\\web-app\\shots\\old.png", null),
      receipt("f1", at(289), zh ? "[Image error: not_found] 文件不存在：C:\\dev\\web-app\\shots\\old.png" : "[Image error: not_found] File not found", true),
      sendCall("f2", at(285), "C:\\dev\\web-app\\README.md", null),
      receipt("f2", at(284), "[Image error: not_image] README.md", true),
      sendCall("f3", at(280), "C:\\dev\\web-app\\raw\\scan.tiff", null),
      receipt("f3", at(279), "[Image error: too_large] 32 MB", true),
      // 发图只收本地文件：agent 直接给网址会被拒（说明里叫它先下载到本地）。
      sendCall("f4", at(275), "https://example.com/banner.png", null),
      receipt("f4", at(274), zh
        ? "[Image error: forbidden] send_image 只接受本地图片文件，不下载网址。要发网络上的图，先用你自己的下载或取网页工具把它下载到本地，再把本地路径发过来"
        : "[Image error: forbidden] send_image only accepts local image files and does not download URLs. Download it locally first, then send the local path", true),
      { type: "tool_use", id: "t1", timestamp: at(200), name: "mcp__meowo__browser_screenshot", kind: "other", summary: "" } as unknown as ChatItem,
      receipt("t1", at(199), (zh ? "面板浏览器截图（182 KB）" : "Panel browser screenshot (182 KB)") + "\n" + ref("tool", "login.png")),
      { type: "tool_use", id: "t2", timestamp: at(190), name: "Read", kind: "read", summary: "docs/design/ref.png", detail: { kind: "read", path: "C:\\dev\\web-app\\docs\\design\\ref.png", offset: null, limit: null } } as unknown as ChatItem,
      receipt("t2", at(189), ref("tool", "ref.png")),
      { type: "tool_use", id: "t3", timestamp: at(180), name: "Write", kind: "write", summary: "public/icon-48.png", detail: { kind: "write", path: "C:\\dev\\web-app\\public\\icon-48.png", content: "", lines: 0, truncated: false } } as unknown as ChatItem,
      receipt("t3", at(179), "File created successfully"),
      say("a2", 170, zh
        ? "上面几张没能发出去。设计稿里的图：\n\n![架构图](docs/chart.png)\n\n参考站的横幅（网络图，点了才下载）：![](https://example.com/hero.png)。旧图 ![旧版首页](C:\\dev\\web-app\\old\\home.png) 已经删了。"
        : "Those didn't go through. From the design doc:\n\n![Architecture](docs/chart.png)\n\nThe reference banner (a web image, downloads only when clicked): ![](https://example.com/hero.png). The old one ![Old home](C:\\dev\\web-app\\old\\home.png) was deleted."),
    ],
  };
}

/// 图片场景的 IPC 返回值；返回 undefined 表示这条不归它管。
export function imageSceneMock(part: string, lang: Lang, chat: (cmd: string) => unknown): (cmd: string, args: unknown) => unknown {
  const { items, running } = imageSceneItems(part, lang);
  return (cmd, args) => {
    if (cmd === "get_chat_history") {
      const base = chat(cmd) as Record<string, unknown>;
      const real = part.startsWith("real-") ? part.slice("real-".length) : null;
      return { ...base, ...(real ? { provider: real } : {}), cwd: real ? REAL_CWD : "C:\\dev\\web-app", items, todos: [], pendingReview: null, status: running ? "running" : "waiting" };
    }
    if (cmd === "pending_interaction") return { approval: null, question: null };
    if (cmd === "resolve_chat_image") {
      const src = (args as { src?: string }).src ?? "";
      // 网络图：一张一直在下载（加载中），其余成功；本地图：old 目录下的已删除。
      if (src.includes("hero.png")) return new Promise(() => {});
      if (src.includes("old")) return { error: { code: "not_found", message: "" } };
      const name = src.split(/[\\/]/).pop() ?? "image.png";
      return { path: `${ROOT}\\local\\${name}` };
    }
    return undefined;
  };
}
