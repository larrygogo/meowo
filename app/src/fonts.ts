// 预览/演示页共享的内置字体（与 main.tsx 同一套，自托管、全平台一致、不联网）：
// Inter 可变做西文 + Noto Sans SC（思源黑体）做中文，按 Unicode 子集切分、本地按需加载。
// 只取 400(正文)/600(标题)两档控制体积；500 等会自动回退到最近档。
import "@fontsource-variable/inter";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/600.css";
