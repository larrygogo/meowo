// demo 入口:装 mock → 渲染舞台 → 暴露 window.__demo 给录制脚本逐帧 seek。
import ReactDOM from "react-dom/client";
import { detectHostOs } from "../platform";
import { installMocks } from "./mock";
import { buildScript } from "./script";
import { DemoStage } from "./DemoStage";
import "../styles.css";
import "./demo.css";

declare global {
  interface Window {
    __demo: { fps: number; frames: number; seek: (f: number) => Promise<void> };
  }
}

localStorage.clear(); // tab/pin 记忆清零,保证每次录制起点一致
installMocks();

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
