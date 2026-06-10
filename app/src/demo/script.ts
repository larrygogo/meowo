// demo 分镜(占位版,Task 5 替换为完整五场景)。
import { Timeline } from "./timeline";
import { store, notify } from "./mock";
import { makeSession } from "./data";

export function buildScript(): Timeline {
  const tl = new Timeline(12);
  store.sessions = [
    makeSession({ title: "重构吸边状态机", project: "larrygogo/cc-kanban", activity: "▸ cargo clippy --workspace", ctx: 62, todoDone: 3, todoTotal: 5 }),
    makeSession({ title: "接入账号用量面板", project: "larrygogo/autopilot", activity: "▸ 编辑 src/views/Sticker.tsx", ctx: 41, todoDone: 1, todoTotal: 4 }),
    makeSession({ title: "升级 tauri 到 2.3", project: "larrygogo/cc-relay", status: "stale", agoMin: 12 }),
    makeSession({ title: "修复 statusline 兼容性", project: "larrygogo/clawmo-ios", status: "ended", connected: false, agoMin: 180 }),
  ];
  notify();
  tl.at(0.4, () => {
    store.stage.caption = "所有 Claude Code 会话,一眼看全";
    notify();
  });
  tl.at(5, () => {});
  return tl;
}
