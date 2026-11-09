// 官网吉祥物 <meowo-eyes>：应用里真实的灵动岛眼睛（Eyes），包成自定义元素给静态官网用。
// 构建见 vite.mascot.config.ts。用户定的官网方向（2026-11-09）：深色 + 眼睛做贯穿全站的吉祥物，
// 要求复用真实组件、不手画仿制品，所以表情、眨眼、小动作都和岛上一模一样。
//
// 用法：<meowo-eyes tone="pending" size="120"></meowo-eyes>
//   tone   running | pending | waiting | error | idle（同灵动岛的 IslandTone）
//   cue    star | surprised | blush | wink（一闪而过的表情，这里会一直保持）
//   quota  额度已用百分比（≥85 发愁，100 转蚊香圈）
//   busy   有这个属性 = 同时在跑的会话多（专注脸）
//   errors 出错的会话数（≥2 抓狂）
//   size   显示宽度（px），默认 120
import { createRoot, type Root } from "react-dom/client";
import { Eyes, type EyeCue, type IslandTone } from "../views/island/islandParts";
import css from "virtual:mascot-css";

/** Eyes 的天然宽度：SVG 26px + 左右各 6px 留给身边的小符号。 */
const BASE_WIDTH = 38;
const BASE_HEIGHT = 16;

// 瞳孔跟鼠标：岛上靠 invoke("island_cursor") 问后端鼠标相对岛的偏移。网页里没有后端，
// 在这里垫一个只认这一条命令的 __TAURI_INTERNALS__：取离鼠标最近的那双眼睛，按它的缩放折算偏移。
// 页面上同时看得到的眼睛一般只有一双，最近的那双看得最准就够了。
const mouse = { x: -1, y: -1 };
window.addEventListener("pointermove", (e) => {
  mouse.x = e.clientX;
  mouse.y = e.clientY;
}, { passive: true });

function cursorOffset(): [number, number] {
  if (mouse.x < 0) return [0, 0];
  let best: [number, number] = [0, 0];
  let bestDist = Infinity;
  for (const el of document.querySelectorAll<HTMLElement>("meowo-eyes")) {
    const r = el.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight || r.width === 0) continue;
    const dx = mouse.x - (r.left + r.width / 2);
    const dy = mouse.y - (r.top + r.height / 2);
    const dist = Math.hypot(dx, dy);
    if (dist < bestDist) {
      bestDist = dist;
      const k = r.width / BASE_WIDTH;
      best = [dx / k, dy / k];
    }
  }
  return best;
}

const w = window as unknown as { __TAURI_INTERNALS__?: { invoke: (cmd: string) => Promise<unknown> } };
if (!w.__TAURI_INTERNALS__) {
  w.__TAURI_INTERNALS__ = {
    invoke: (cmd: string) => Promise.resolve(cmd === "island_cursor" ? cursorOffset() : null),
  };
}

let sheet: CSSStyleSheet | null = null;
function sharedSheet(): CSSStyleSheet {
  if (!sheet) {
    sheet = new CSSStyleSheet();
    sheet.replaceSync(`${css}\n:host { display: inline-block; line-height: 0; vertical-align: middle; }\n.wrap { transform-origin: top left; width: ${BASE_WIDTH}px; height: ${BASE_HEIGHT}px; }`);
  }
  return sheet;
}

const TONES: IslandTone[] = ["running", "pending", "waiting", "error", "idle"];
const CUES: EyeCue[] = ["star", "surprised", "blush", "wink"];

class MeowoEyes extends HTMLElement {
  static observedAttributes = ["tone", "cue", "quota", "busy", "errors", "size"];
  private root: Root | null = null;
  private mount: HTMLDivElement | null = null;

  connectedCallback() {
    if (!this.shadowRoot) {
      const shadow = this.attachShadow({ mode: "open" });
      shadow.adoptedStyleSheets = [sharedSheet()];
      this.mount = document.createElement("div");
      this.mount.className = "wrap";
      shadow.appendChild(this.mount);
      this.root = createRoot(this.mount);
    }
    this.render();
  }

  disconnectedCallback() {
    // 留着 root：React 重排节点时会先断开再接上，卸载了就会闪一下。
  }

  attributeChangedCallback() {
    if (this.root) this.render();
  }

  private render() {
    const tone = (TONES as string[]).includes(this.getAttribute("tone") ?? "") ? (this.getAttribute("tone") as IslandTone) : "running";
    const cueAttr = this.getAttribute("cue");
    const cue = (CUES as string[]).includes(cueAttr ?? "") ? (cueAttr as EyeCue) : null;
    const quotaAttr = this.getAttribute("quota");
    const quota = quotaAttr == null || quotaAttr === "" ? null : Number(quotaAttr);
    const size = Number(this.getAttribute("size")) || 120;
    const k = size / BASE_WIDTH;
    this.style.width = `${size}px`;
    this.style.height = `${BASE_HEIGHT * k}px`;
    if (this.mount) this.mount.style.transform = `scale(${k})`;
    this.root?.render(
      <Eyes
        tone={tone}
        cue={cue}
        quotaPct={quota}
        busy={this.hasAttribute("busy")}
        errors={Number(this.getAttribute("errors")) || 0}
      />,
    );
  }
}

if (!customElements.get("meowo-eyes")) customElements.define("meowo-eyes", MeowoEyes);
