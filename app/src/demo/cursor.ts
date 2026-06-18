// demo 专用:假光标的移动/点击/打字。点击派发真实 DOM 事件驱动 React 组件,
// 涟漪用 fill:forwards 的 CSS 动画(结束即透明),不 setTimeout 移除,保持逐帧确定。
import { Timeline } from "./timeline";

export const cursor = { x: 440, y: 540 };

export function setCursor(x: number, y: number): void {
  cursor.x = x;
  cursor.y = y;
  const el = document.getElementById("demo-cursor");
  if (el) el.style.transform = `translate(${x}px, ${y}px)`;
}

/** from→to 秒内把光标平滑移到 sel 中心(目标位置在补间首帧时再解析,容许元素晚挂载)。 */
export function moveToEl(tl: Timeline, from: number, to: number, sel: string): void {
  let sx = 0, sy = 0, tx = 0, ty = 0, init = false;
  tl.tween(from, to, (k) => {
    if (!init) {
      init = true;
      sx = cursor.x;
      sy = cursor.y;
      const el = document.querySelector(sel);
      if (!el) {
        console.warn("[demo] moveToEl 未找到:", sel);
        tx = sx;
        ty = sy;
      } else {
        const r = el.getBoundingClientRect();
        tx = r.left + r.width / 2;
        ty = r.top + r.height / 2;
      }
    }
    setCursor(sx + (tx - sx) * k, sy + (ty - sy) * k);
  });
}

/** 模拟卡片 hover:假光标不触发 CSS :hover，故给目标卡片加 .demo-hover 镜像(抬起 + 浮现操作按钮)。
   传 null 清除所有。sel 命中卡片本身或其子元素均可(向上找最近的 .stk-card)。 */
export function hoverEl(sel: string | null): void {
  document.querySelectorAll(".stk-card.demo-hover").forEach((el) => el.classList.remove("demo-hover"));
  if (!sel) return;
  const el = document.querySelector(sel)?.closest(".stk-card");
  if (el) el.classList.add("demo-hover");
}

/** 点击 sel:光标钉到元素中心、出涟漪、派发真实 click。 */
export function clickEl(sel: string): void {
  const el = document.querySelector<HTMLElement>(sel);
  if (!el) {
    console.warn("[demo] clickEl 未找到:", sel);
    return;
  }
  const r = el.getBoundingClientRect();
  setCursor(r.left + r.width / 2, r.top + r.height / 2);
  ripple(cursor.x, cursor.y);
  el.click();
}

function ripple(x: number, y: number): void {
  const d = document.createElement("div");
  d.className = "demo-ripple";
  d.style.left = `${x}px`;
  d.style.top = `${y}px`;
  document.body.appendChild(d);
}

/** 受控 input 逐字输入(React 18 监听原生 input 事件,需走原型上的 value setter)。 */
export function typeText(tl: Timeline, startSec: number, sel: string, text: string, cps = 14): void {
  for (let i = 1; i <= text.length; i++) {
    tl.at(startSec + i / cps, () => {
      const el = document.querySelector<HTMLInputElement>(sel);
      if (!el) return;
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(el, text.slice(0, i));
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
}

export function pressKey(sel: string, key: string): void {
  document
    .querySelector<HTMLInputElement>(sel)
    ?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}
