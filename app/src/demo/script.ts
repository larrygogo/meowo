// demo 分镜:八个场景,约 26s @12fps(实时→待交互→重命名→便签→归档→搜索→吸边→收尾)。
// 卡片 hover 效果靠 hoverEl 加 .demo-hover 模拟(假光标不触发 :hover)。
// 选择器对应 Sticker.tsx 现有 DOM:.tabseg 首子是 .tabseg-slider(立体滑块),故 tab 从 nth-child(2) 起
// (2 全部 / 3 待交互 / 4 运行中 / 5 已归档);卡片=.stk-scroll .stk-card:nth-child(n),铅笔=.stk-rename,归档=.stk-arch,重命名输入框=.stk-edit。
import { Timeline } from "./timeline";
import { store, notify } from "./mock";
import { makeSession } from "./data";
import { clickEl, moveToEl, typeText, pressKey, setCursor, hoverEl } from "./cursor";

function mut(fn: () => void): () => void {
  return () => {
    fn();
    notify();
  };
}

export function buildScript(): Timeline {
  const tl = new Timeline(12);
  const s1 = makeSession({ title: "重构吸边状态机", project: "larrygogo/meowo", activity: "▸ cargo clippy --workspace", ctx: 62, todoDone: 3, todoTotal: 5 });
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

  // ── 场景 2(4–7.5s):转待交互 + tab 过滤 ──
  tl.at(4.2, mut(() => { store.stage.caption = "谁在等你回复,立刻知道"; }));
  tl.at(4.6, mut(() => {
    s2.session.status = "waiting";
    s2.current_activity = "等待回复:是否应用这 3 处修改?";
  }));
  moveToEl(tl, 5.0, 5.6, ".tabs .stab:nth-child(3)"); // 待交互(滑块占 nth-child(1)，故 +1)
  tl.at(5.7, () => clickEl(".tabs .stab:nth-child(3)"));
  moveToEl(tl, 6.6, 7.1, ".tabs .stab:nth-child(2)"); // 回到 全部
  tl.at(7.2, () => clickEl(".tabs .stab:nth-child(2)"));

  // ── 场景 3(7.5–10.5s):悬停卡片 → 点铅笔重命名 ──
  tl.at(7.6, mut(() => { store.stage.caption = "悬停卡片,即点即管"; }));
  tl.at(8.0, () => hoverEl(".stk-scroll .stk-card:nth-child(2)")); // 卡片抬起 + 操作按钮浮现
  moveToEl(tl, 8.1, 8.8, ".stk-scroll .stk-card:nth-child(2) .stk-rename");
  tl.at(8.9, () => clickEl(".stk-scroll .stk-card:nth-child(2) .stk-rename")); // 点铅笔进编辑
  typeText(tl, 9.1, ".stk-edit", "评审用量面板方案", 12);
  tl.at(10.1, () => pressKey(".stk-edit", "Enter"));

  // ── 场景 4(10.5–14s):悬停 → 加便签 ──
  tl.at(10.7, mut(() => { store.stage.caption = "给会话挂个本地便签"; }));
  tl.at(11.0, () => hoverEl(".stk-scroll .stk-card:nth-child(1)"));
  moveToEl(tl, 11.1, 11.8, ".stk-scroll .stk-card:nth-child(1) .stk-noteb");
  tl.at(11.9, () => clickEl(".stk-scroll .stk-card:nth-child(1) .stk-noteb")); // 打开便签编辑器
  typeText(tl, 12.1, ".stk-note-edit", "记得先确认 API key", 12);
  tl.at(13.3, () => pressKey(".stk-note-edit", "Enter")); // 保存 → 便签块出现

  // ── 场景 5(14–16.5s):悬停 → 归档 ──
  tl.at(14.0, mut(() => { store.stage.caption = "不用的收进归档"; }));
  tl.at(14.3, () => hoverEl(".stk-scroll .stk-card:nth-child(4)"));
  moveToEl(tl, 14.4, 15.1, ".stk-scroll .stk-card:nth-child(4) .stk-arch");
  tl.at(15.2, () => clickEl(".stk-scroll .stk-card:nth-child(4) .stk-arch"));
  tl.at(15.7, () => hoverEl(null)); // 清除卡片 hover

  // ── 场景 6(16.5–20s):底栏搜索过滤 ──
  tl.at(16.5, mut(() => { store.stage.caption = "搜索任意会话:标题 / 仓库名即时过滤"; }));
  moveToEl(tl, 16.7, 17.4, ".stk-bar-actions .stk-act:first-child");
  tl.at(17.5, () => clickEl(".stk-bar-actions .stk-act:first-child")); // 打开搜索
  typeText(tl, 17.8, ".stk-search-in", "tauri", 8); // 过滤到「升级 tauri 到 2.3」
  moveToEl(tl, 19.4, 19.8, ".stk-search-x");
  tl.at(19.9, () => clickEl(".stk-search-x")); // 关闭搜索，列表还原

  // ── 场景 7(20–24s):吸边缩略 + 偷看(滑向边缘的同时收成细条) ──
  tl.at(20.2, mut(() => { store.stage.caption = "吸边缩成一根状态条,不占地方"; }));
  tl.at(20.5, mut(() => { store.stage.mode = "docking"; }));
  tl.at(21.2, mut(() => { store.stage.mode = "strip"; }));
  moveToEl(tl, 21.7, 22.3, ".demo-window .cstrip");
  tl.at(22.5, mut(() => { store.stage.mode = "expanded"; }));
  tl.at(23.1, () => setCursor(440, 300)); // 光标移开
  tl.at(23.8, mut(() => { store.stage.mode = "strip"; }));

  // ── 场景 8(24–26s):收尾 ──
  tl.at(24.2, mut(() => { store.stage.caption = null; store.stage.finale = true; }));
  tl.at(26.0, () => {}); // 钉住总时长 ≈ 26s
  return tl;
}
