// demo 分镜:五个场景,约 20s @12fps。
// 选择器对应 Sticker.tsx 现有 DOM:tab=.tabs .stab:nth-child(n)(1 全部/2 待交互/3 运行中/4 已归档),
// 卡片=.stk-scroll .stk-card:nth-child(n),铅笔=.stk-rename,归档=.stk-arch,重命名输入框=.stk-edit。
import { Timeline } from "./timeline";
import { store, notify } from "./mock";
import { makeSession } from "./data";
import { clickEl, moveToEl, typeText, pressKey, setCursor } from "./cursor";

function mut(fn: () => void): () => void {
  return () => {
    fn();
    notify();
  };
}

export function buildScript(): Timeline {
  const tl = new Timeline(12);
  const s1 = makeSession({ title: "重构吸边状态机", project: "larrygogo/cc-kanban", activity: "▸ cargo clippy --workspace", ctx: 62, todoDone: 3, todoTotal: 5 });
  const s2 = makeSession({ title: "接入账号用量面板", project: "larrygogo/autopilot", activity: "▸ 编辑 src/views/Sticker.tsx", ctx: 41, todoDone: 1, todoTotal: 4 });
  const s3 = makeSession({ title: "升级 tauri 到 2.3", project: "larrygogo/cc-relay", status: "stale", agoMin: 12 });
  const s4 = makeSession({ title: "修复 statusline 兼容性", project: "larrygogo/clawmo-ios", status: "ended", connected: false, agoMin: 180 });
  store.sessions = [s1, s2, s3, s4];
  notify();
  // 光标初始位:DOM 挂载后第一帧再落(buildScript 时 React 还没渲染完)。
  tl.at(0, () => setCursor(640, 520));

  // ── 场景 1(0–4.5s):实时变化 ──
  tl.at(0.4, mut(() => { store.stage.caption = "所有 Claude Code 会话,一眼看全"; }));
  tl.at(1.4, mut(() => { s1.current_activity = "▸ cargo test --workspace"; s1.context_pct = 63; }));
  tl.at(2.4, mut(() => { s2.current_activity = "▸ 运行 bunx vitest run"; s2.todo_done = 2; }));
  tl.at(3.4, mut(() => { s1.current_activity = "▸ 写入 src/snap/mod.rs"; s1.context_pct = 64; s1.todo_done = 4; }));

  // ── 场景 2(4.5–8.5s):转待交互 + tab 过滤 ──
  tl.at(4.6, mut(() => { store.stage.caption = "谁在等你回复,立刻知道"; }));
  tl.at(5.0, mut(() => {
    s2.session.status = "waiting";
    s2.current_activity = "等待回复:是否应用这 3 处修改?";
  }));
  moveToEl(tl, 5.4, 6.1, ".tabs .stab:nth-child(2)");
  tl.at(6.2, () => clickEl(".tabs .stab:nth-child(2)"));
  moveToEl(tl, 7.2, 7.7, ".tabs .stab:nth-child(1)");
  tl.at(7.8, () => clickEl(".tabs .stab:nth-child(1)"));

  // ── 场景 3(8.5–13s):重命名 + 归档 ──
  tl.at(8.7, mut(() => { store.stage.caption = "重命名、归档,即点即管"; }));
  moveToEl(tl, 8.8, 9.3, ".stk-scroll .stk-card:nth-child(2) .stk-rename");
  tl.at(9.4, () => clickEl(".stk-scroll .stk-card:nth-child(2) .stk-rename"));
  typeText(tl, 9.6, ".stk-edit", "评审用量面板方案", 11);
  tl.at(10.6, () => pressKey(".stk-edit", "Enter"));
  moveToEl(tl, 11.2, 11.8, ".stk-scroll .stk-card:nth-child(4) .stk-arch");
  tl.at(11.9, () => clickEl(".stk-scroll .stk-card:nth-child(4) .stk-arch"));

  // ── 场景 4(13–17.5s):吸边缩略 + 偷看 ──
  tl.at(13.2, mut(() => { store.stage.caption = "吸边缩成一根状态条,不占地方"; }));
  tl.at(13.4, mut(() => { store.stage.mode = "docking"; }));
  tl.at(14.2, mut(() => { store.stage.mode = "strip"; }));
  moveToEl(tl, 14.6, 15.2, ".demo-window .cstrip");
  tl.at(15.4, mut(() => { store.stage.mode = "expanded"; }));
  tl.at(16.0, () => setCursor(500, 300)); // 光标移开
  tl.at(16.8, mut(() => { store.stage.mode = "strip"; }));

  // ── 场景 5(17.5–20s):收尾 ──
  tl.at(17.6, mut(() => { store.stage.caption = null; store.stage.finale = true; }));
  tl.at(19.6, () => {}); // 钉住总时长 ≈ 20s
  return tl;
}
