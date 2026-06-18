// demo 分镜:五个场景,约 20s @12fps。
// 选择器对应 Sticker.tsx 现有 DOM:.tabseg 首子是 .tabseg-slider(立体滑块),故 tab 从 nth-child(2) 起
// (2 全部 / 3 待交互 / 4 运行中 / 5 已归档);卡片=.stk-scroll .stk-card:nth-child(n),铅笔=.stk-rename,归档=.stk-arch,重命名输入框=.stk-edit。
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
  moveToEl(tl, 5.4, 6.1, ".tabs .stab:nth-child(3)"); // 待交互(滑块占 nth-child(1)，故 +1)
  tl.at(6.2, () => clickEl(".tabs .stab:nth-child(3)"));
  moveToEl(tl, 7.2, 7.7, ".tabs .stab:nth-child(2)"); // 回到 全部
  tl.at(7.8, () => clickEl(".tabs .stab:nth-child(2)"));

  // ── 场景 3(8.5–13s):重命名 + 归档 ──
  tl.at(8.7, mut(() => { store.stage.caption = "重命名、归档,即点即管"; }));
  moveToEl(tl, 8.8, 9.3, ".stk-scroll .stk-card:nth-child(2) .stk-rename");
  tl.at(9.4, () => clickEl(".stk-scroll .stk-card:nth-child(2) .stk-rename"));
  typeText(tl, 9.6, ".stk-edit", "评审用量面板方案", 11);
  tl.at(10.6, () => pressKey(".stk-edit", "Enter"));
  moveToEl(tl, 11.2, 11.8, ".stk-scroll .stk-card:nth-child(4) .stk-arch");
  tl.at(11.9, () => clickEl(".stk-scroll .stk-card:nth-child(4) .stk-arch"));

  // ── 场景 4(13–17s):底栏搜索过滤 ──
  tl.at(13.0, mut(() => { store.stage.caption = "搜索任意会话:标题 / 仓库名即时过滤"; }));
  moveToEl(tl, 13.2, 13.9, ".stk-bar-actions .stk-act:first-child");
  tl.at(14.0, () => clickEl(".stk-bar-actions .stk-act:first-child")); // 打开搜索
  typeText(tl, 14.3, ".stk-search-in", "tauri", 8); // 过滤到「升级 tauri 到 2.3」
  moveToEl(tl, 16.2, 16.6, ".stk-search-x");
  tl.at(16.7, () => clickEl(".stk-search-x")); // 关闭搜索，列表还原

  // ── 场景 5(17–21.5s):吸边缩略 + 偷看 ──
  tl.at(17.2, mut(() => { store.stage.caption = "吸边缩成一根状态条,不占地方"; }));
  tl.at(17.4, mut(() => { store.stage.mode = "docking"; }));
  tl.at(18.2, mut(() => { store.stage.mode = "strip"; }));
  moveToEl(tl, 18.6, 19.2, ".demo-window .cstrip");
  tl.at(19.4, mut(() => { store.stage.mode = "expanded"; }));
  tl.at(20.0, () => setCursor(500, 300)); // 光标移开
  tl.at(20.8, mut(() => { store.stage.mode = "strip"; }));

  // ── 场景 6(21.5–24s):收尾 ──
  tl.at(21.6, mut(() => { store.stage.caption = null; store.stage.finale = true; }));
  tl.at(23.6, () => {}); // 钉住总时长 ≈ 24s
  return tl;
}
