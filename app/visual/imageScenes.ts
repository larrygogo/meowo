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
import composerFrames from "../src/test/fixtures/composer-frames.json";
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
/** 方案卡场景的三种设置页布局（1280×800）：卡片式、列表式、分组标签式。 */
function settingsShot(kind: "cards" | "list" | "tabs"): string {
  return canvas(1280, 800, (g) => {
    const text = (str: string, x: number, y: number, size: number, color: string, bold = false) => {
      g.fillStyle = color; g.font = `${bold ? "bold " : ""}${size}px sans-serif`; g.fillText(str, x, y);
    };
    const toggle = (x: number, y: number, on: boolean) => {
      box(g, x, y, 52, 30, on ? "#2f6fed" : "#d1d5db", 15);
      g.fillStyle = "#fff"; g.beginPath(); g.arc(on ? x + 37 : x + 15, y + 15, 11, 0, 7); g.fill();
    };
    g.fillStyle = kind === "cards" ? "#f3f4f6" : "#ffffff"; g.fillRect(0, 0, 1280, 800);
    g.fillStyle = "#ffffff"; g.fillRect(0, 0, 1280, 80);
    g.fillStyle = "#e5e7eb"; g.fillRect(0, 80, 1280, 2);
    text("Settings", 56, 52, 30, "#111827", true);
    if (kind === "cards") {
      [[128, "General", ["Dark mode", "Open at login"]], [420, "Notifications", ["Desktop alerts", "Sounds"]]].forEach(([y, title, rows]) => {
        box(g, 56, y as number, 1168, 250, "#ffffff", 24);
        text(title as string, 96, (y as number) + 60, 26, "#111827", true);
        (rows as string[]).forEach((row, i) => { text(row, 96, (y as number) + 130 + i * 70, 22, "#374151"); toggle(1110, (y as number) + 105 + i * 70, i === 0); });
      });
    } else if (kind === "list") {
      let y = 120;
      [["GENERAL", ["Dark mode", "Open at login", "Language"]], ["NOTIFICATIONS", ["Desktop alerts", "Sounds"]]].forEach(([title, rows]) => {
        text(title as string, 56, y + 20, 18, "#6b7280", true); y += 40;
        (rows as string[]).forEach((row, i) => {
          text(row, 56, y + 44, 22, "#111827");
          if (row === "Language") text("English  ›", 1080, y + 44, 20, "#6b7280"); else toggle(1170, y + 22, i === 0);
          g.fillStyle = "#f3f4f6"; g.fillRect(56, y + 72, 1168, 2); y += 74;
        });
        y += 30;
      });
    } else {
      ["General", "Notifications", "Account", "About"].forEach((tab, i) => {
        if (i === 0) box(g, 56 + i * 170, 112, 150, 50, "#111827", 25);
        text(tab, 82 + i * 170, 145, 20, i === 0 ? "#ffffff" : "#6b7280");
      });
      box(g, 56, 196, 1168, 560, "#f9fafb", 20);
      ["Dark mode", "Open at login", "Language", "Font size"].forEach((row, i) => {
        text(row, 96, 262 + i * 120, 22, "#111827");
        text("A short explanation of this setting", 96, 294 + i * 120, 18, "#9ca3af");
        toggle(1110, 248 + i * 120, i % 2 === 0);
      });
    }
  });
}

function imageFor(path: string): string {
  images ??= { "login-before.png": canvas(1280, 800, (g) => drawLogin(g, false)), "login-after.png": canvas(1280, 800, (g) => drawLogin(g, true)), "login-diff.png": loginDiff(), "login.png": loginShot(), "chart.png": chart(), "palette.png": photo(), "pricing-long.png": longShot(), "icon-48.png": icon(), "ref.png": photo(), "settings-cards.png": settingsShot("cards"), "settings-list.png": settingsShot("list"), "settings-tabs.png": settingsShot("tabs") };
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
  if (part === "outline") {
    // 长回复：五个小标题 + 一段 45 行的代码，看目录按钮和代码块收起。
    const para = zh
      ? "这一处的问题在于状态在两个地方各存一份，改了一边另一边不知道。表单提交时读的是旧值，所以看起来像校验没生效。修法是只留一个来源，其余地方都从它派生。"
      : "The problem here is that the state lives in two places, so changing one leaves the other stale. Submit reads the old value, which looks like validation did not run. The fix is a single source that everything else derives from.";
    const code = ["```ts", ...Array.from({ length: 45 }, (_, i) => `export const rule${i} = (value: string) => value.length > ${i} || "${zh ? "太短" : "too short"}";`), "```"].join("\n");
    const titles = zh ? ["问题一：校验跑了两遍", "问题二：错误提示不跟随输入框", "问题三：提交按钮的禁用态", "问题四：手机上键盘挡住按钮", "建议的改法"] : ["Issue 1: validation runs twice", "Issue 2: errors don't follow inputs", "Issue 3: submit button disabled state", "Issue 4: keyboard covers the button on mobile", "Suggested fix"];
    const body = titles.map((title, i) => `## ${title}\n\n${para}\n\n${para}${i === 4 ? `\n\n${code}` : ""}`).join("\n\n");
    return {
      running: false,
      items: [
        user("u1", 300, zh ? "把登录表单的校验逻辑审一遍，问题都列出来。" : "Review the login form validation and list every issue."),
        say("a1", 120, body),
      ],
    };
  }
  if (part === "checks" || part === "suggest") {
    // 回复末尾的「本轮结果」：①改了 4 个文件、类型检查和单测通过后又改了一个文件；②只跑了测试、3 个失败；
    // ③改了代码没跑检查。过程收进回合折叠，点检查行要能展开并跳回那条命令。
    const edit = (id: string, s: number, path: string, add: number) => ({
      type: "tool_use", id, timestamp: at(s), name: "Edit", kind: "edit", summary: path,
      detail: { kind: "edit", path, old: "a", new: Array.from({ length: add }, (_, i) => `line ${i}`).join("\n"), replace_all: false, truncated: false },
    }) as unknown as ChatItem;
    const cmd = (id: string, s: number, command: string) => ({
      type: "tool_use", id, timestamp: at(s), name: "Bash", kind: "run", summary: command, detail: { kind: "command", command, description: null },
    }) as unknown as ChatItem;
    return {
      running: false,
      items: [
        user("u1", 900, zh ? "设置页做得好看一点。" : "Make the settings page look nicer."),
        edit("e1", 880, "src/views/settings/General.tsx", 42), receipt("e1", at(879), "ok"),
        edit("e2", 870, "src/styles.css", 31), receipt("e2", at(869), "ok"),
        edit("e3", 860, "src/views/settings/Section.tsx", 4), receipt("e3", at(859), "ok"),
        cmd("c1", 850, "cd app && bunx tsc --noEmit"), receipt("c1", at(840), ""),
        cmd("c2", 830, "bunx vitest run src/views/settings"), receipt("c2", at(800), " Test Files  12 passed (12)\n      Tests  162 passed (162)"),
        edit("e4", 790, "src/views/settings/Row.tsx", 9), receipt("e4", at(789), "ok"),
        say("a1", 780, zh ? "设置页改好了：分组标题加粗，开关对齐到右侧，间距统一成 12px。" : "Settings page updated: bold group titles, switches aligned right, 12px spacing throughout."),
        user("u2", 600, zh ? "跑一下后端测试。" : "Run the backend tests."),
        cmd("c3", 590, "cargo test -p meowo-agent"), receipt("c3", at(560), "test result: ok. 120 passed; 0 failed;\ntest result: FAILED. 37 passed; 3 failed;", true),
        // 同一类两行：后端测试失败、前端测试通过，各留一行（做空第五轮要的截图）。
        cmd("c6", 558, "cd app && bunx vitest run"), receipt("c6", at(552), "      Tests  4832 passed (4832)"),
        say("a2", 550, zh ? "有 3 个测试没过，都在 codex 的回执解析里。" : "3 tests failed, all in the codex receipt parser."),
        user("u3", 400, zh ? "按钮圆角改成 8px。" : "Make the button radius 8px."),
        edit("e5", 390, "src/components/Button.tsx", 2), receipt("e5", at(389), "ok"),
        say("a3", 380, zh ? "按钮圆角改成 8px 了。" : "Button radius is now 8px."),
      ],
    };
  }
  if (part === "deliver") {
    // AI 交文件（send_file）与主动提醒（notify_user）：一个刚交的安装包、一份已被清理的旧日志包、两条提醒
    // （一条弹出来了，一条因为用户正看着没弹）。任务 20261123-agent-abilities。
    const toolCall = (id: string, s: number, name: string) => ({ type: "tool_use", id, timestamp: at(s), name, kind: "other", summary: "" }) as unknown as ChatItem;
    const file = (name: string, size: number, dir: string, caption: string | null) => {
      const path = ["C:", "Users", "me", ".meowo", "files", "sent", dir, name].join("\\");
      return [`文件已交给用户：${name}。`, `[File: ${JSON.stringify({ name, size, path, sha256: dir, caption })}]`].join("\n");
    };
    const notify = (level: string, message: string, outcome: string) =>
      [`已提醒用户。`, `[Notify: ${JSON.stringify({ level, message, outcome })}]`].join("\n");
    return {
      running: false,
      items: [
        user("u1", 900, zh ? "把昨天的日志打个包给我。" : "Zip yesterday's logs for me."),
        toolCall("f0", 890, "mcp__meowo__send_file"),
        receipt("f0", at(889), file("logs-2026-11-22.zip", 3_481_233, "old0000000000000", null)),
        user("u2", 300, zh ? "打个安装包我试试，打好叫我。" : "Build an installer for me to try, ping me when it's ready."),
        say("a1", 200, zh ? "安装包打好了，基于 c113ea29。" : "The installer is ready, built from c113ea29."),
        toolCall("f1", 190, "mcp__meowo__send_file"),
        receipt("f1", at(189), file("Meowo_0.8.6_x64-setup.exe", 22_246_679, "0123456789abcdef", zh ? "装好后在 Claude 会话里按一次 Tab，看输入框还有没有直角框。" : "After installing, press Tab once in a Claude session and check the input box.")),
        toolCall("n1", 180, "mcp__meowo__notify_user"),
        receipt("n1", at(179), notify("done", zh ? "安装包打好了，在对话里，装上试试。" : "The installer is in the chat. Install and try it.", "shown")),
        toolCall("n2", 120, "mcp__meowo__notify_user"),
        receipt("n2", at(119), notify("needs_you", zh ? "要不要顺便把 0.8.6 草稿发布？这步要你点头。" : "Publish the 0.8.6 draft too? I need your go-ahead.", "viewing")),
      ],
    };
  }
  if (part === "choices") {
    const choicesCall = (id: string, s: number) => ({
      type: "tool_use", id, timestamp: at(s), name: "mcp__meowo__offer_choices", kind: "other", summary: "",
    }) as unknown as ChatItem;
    const choices = (question: string, options: unknown[], refs: string[]) => [
      zh ? "方案卡已显示给用户。" : "Options card shown to the user.",
      ...refs,
      `[Choices: ${JSON.stringify({ question, options })}]`,
    ].join("\n");
    return {
      running: false,
      items: [
        user("u1", 600, zh ? "导出按钮放哪儿？你给我几个选项。" : "Where should the export button go? Give me options."),
        choicesCall("k0", 590),
        receipt("k0", at(589), choices(zh ? "导出按钮放在哪里？" : "Where should the export button go?", [
          { label: zh ? "工具栏右侧" : "Toolbar, right", description: zh ? "和分享放一起，最好找" : "Next to Share; easiest to find", image: null },
          { label: zh ? "更多菜单里" : "In the More menu", description: zh ? "界面最干净，但多点一下" : "Cleanest, one extra click", image: null },
        ], [])),
        user("u2", 560, zh ? "选了方案 1：工具栏右侧\n补充：图标用下载箭头" : "Picked option 1: Toolbar, right\nNote: use a download arrow icon"),
        say("a1", 550, zh ? "好，导出按钮放到工具栏右侧了。" : "Done, the export button is on the right of the toolbar."),
        user("u3", 300, zh ? "设置页做得好看点。" : "Make the settings page look nicer."),
        choicesCall("k1", 200),
        receipt("k1", at(199), choices(zh ? "设置页用哪种布局？" : "Which layout for the settings page?", [
          { label: zh ? "卡片式" : "Cards", description: zh ? "每组设置一张卡，留白多" : "One card per group, airy", image: 0 },
          { label: zh ? "列表式" : "List", description: zh ? "像系统设置，信息密度高" : "Like system settings, dense", image: 1 },
          { label: zh ? "分组标签式" : "Tabs", description: zh ? "顶部标签切换分组" : "Tabs switch between groups", image: 2 },
        ], [ref("sent", "settings-cards.png"), ref("sent", "settings-list.png"), ref("sent", "settings-tabs.png")])),
        say("a2", 190, zh ? "三种都做出来截了图，你挑一个，我照着改完。" : "I built all three and took screenshots. Pick one and I'll finish it."),
      ],
    };
  }
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
      return { ...base, ...(real ? { provider: real } : {}), cwd: real ? REAL_CWD : "C:\\dev\\web-app", items, todos: [], pendingReview: null, status: running ? "running" : "waiting", ...(part === "suggest" ? { ptyManaged: true } : {}) };
    }
    if (cmd === "pending_interaction") return { approval: null, question: null };
    // 交文件：old 开头的那份快照已被回收（卡片显示「已清理」）。
    if (cmd === "delivered_file_exists") return !((args as { path?: string }).path ?? "").includes("old000");
    // 建议回复：Claude 回合结束后终端输入框里的暗色建议（2.1.330 实拍帧，见 composer-frames.json）。
    if (part === "suggest" && cmd === "agent_chat_ui") return { ...(chat(cmd) as Record<string, unknown>), composer: composerFrames.specs.claude };
    if (part === "suggest" && cmd === "managed_terminal_frame") {
      const frame = composerFrames.frames.find((entry) => entry.name === "claude-2.1.330-suggestion-zh")!;
      return { lines: frame.lines, cursorRow: frame.cursor!.row, cursorCol: frame.cursor!.col, cursorVisible: true, cursorDim: true, cols: 120, rows: 32 };
    }
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
