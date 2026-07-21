// demo 入口:装 mock → 渲染舞台 → 暴露 window.__demo 给录制脚本逐帧 seek。
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { bootAppearance } from "../appearance";
import { installMocks } from "./mock";
import { buildScript } from "./script";
import { DemoStage } from "./DemoStage";
import "../fonts";
import "../styles.css";
import "./demo.css";

declare global {
  interface Window {
    __demo: { fps: number; frames: number; seek: (f: number) => Promise<void> };
  }
}

localStorage.clear(); // tab/pin 记忆清零,保证每次录制起点一致
// 冻结时钟:逐帧录制耗时数分钟真实时间,而卡片相对时间戳(fmtAgo)走 Date.now()——
// 不冻结会在录制期间从「刚刚」漂到「N 分钟前」。固定值让整段 demo 时间戳稳定。
// (仅影响 Date.now;动画走 performance.now / 虚拟时间轴,不受影响。)
const FIXED_NOW = 1_780_000_000_000;
Date.now = () => FIXED_NOW;
installMocks();
// 走真实外观管线套用 data-sticker-style / data-theme / --cc-bg-rgb / 不透明度,
// 让 demo 与 app 像素级一致(mock 设 flat + neutral,即当前默认风格)。
bootAppearance({ scale: true });

(async () => {
  await detectHostOs(); // host_os → "windows":完整贴纸形态(含 pin/拖拽区)
  ReactDOM.createRoot(document.getElementById("root")!).render(<DemoStage />);
  const tl = buildScript();
  window.__demo = {
    fps: tl.fps,
    frames: Math.ceil((tl.duration + 0.4) * tl.fps),
    seek: (f) => tl.seek(f),
  };
})();
