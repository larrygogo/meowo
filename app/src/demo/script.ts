// demo 分镜:九个场景,约 42.6s @20fps(实时 AI 正文→待交互→重命名→便签→归档→搜索→置顶→吸边→收尾)。
// 节奏刻意放慢:打字 ~7 字/秒、动作间留白、吸边过渡拉长(见 demo.css),让动画与文字都来得及看。
// 卡片 hover 效果靠 hoverEl 加 .demo-hover 模拟(假光标不触发 :hover)。
// 选择器对应 Sticker.tsx 当前 DOM:
//   - tab:.tabs .stab:nth-child(n),.tabseg 首子是 .tabseg-slider,故 tab 从 nth-child(2) 起
//     (2 全部 / 3 待交互 / 4 运行中 / 5 已归档);
//   - 卡片:虚拟列表每张卡片外包 .stk-vitem[data-index=i](i 为 shown 序号,demo 未星标即原序 0..3);
//   - 卡片菜单:卡片右上角常显的 ⋯ 按钮 .stk-menu-btn → 打开 .ctx-menu,菜单项按 button 顺序
//     nth-of-type:1 星标 / 2 便签 / 3 重命名 / 4 归档;
//   - 重命名输入框 .stk-edit、便签输入框 .stk-note-edit;
//   - 底栏动作 .stk-bar-actions .stk-act:nth-child(1 新建 / 2 搜索 / 3 设置 / 4 置顶),
//     搜索框 .stk-search-in、关闭 .stk-search-x。
import { Timeline } from "./timeline";
import { store, notify } from "./mock";
import { makeSession } from "./data";
import { clickEl, moveToEl, typeText, pressKey, setCursor, hoverEl } from "./cursor";
import { DEMO_STRINGS, type DemoLang } from "./strings";

function mut(fn: () => void): () => void {
  return () => {
    fn();
    notify();
  };
}

// 卡片/菜单选择器(i = data-index,k = 菜单项 button 序号)。
const card = (i: number) => `.stk-vitem[data-index="${i}"] .stk-card`;
const menuBtn = (i: number) => `.stk-vitem[data-index="${i}"] .stk-menu-btn`;
const menuItem = (k: number) => `.ctx-menu .ctx-item:nth-of-type(${k})`;
const CPS = 7; // 打字速度(字/秒),放慢到看得清

export function buildScript(lang: DemoLang = "zh"): Timeline {
  const S = DEMO_STRINGS[lang];
  const tl = new Timeline(20);
  const s1 = makeSession({ title: S.sessions[0].title, project: "larrygogo/meowo", ctx: 62, todoDone: 3, todoTotal: 5, lastAi: S.sessions[0].ai });
  const s2 = makeSession({ title: S.sessions[1].title, project: "larrygogo/autopilot", ctx: 41, todoDone: 1, todoTotal: 4, lastAi: S.sessions[1].ai });
  const s3 = makeSession({ title: S.sessions[2].title, project: "larrygogo/cc-relay", status: "stale", agoMin: 12, lastAi: S.sessions[2].ai });
  const s4 = makeSession({ title: S.sessions[3].title, project: "larrygogo/clawmo-ios", status: "ended", connected: false, agoMin: 180, lastAi: S.sessions[3].ai });
  store.sessions = [s1, s2, s3, s4];
  notify();
  // 光标初始位:DOM 挂载后第一帧再落(buildScript 时 React 还没渲染完)。
  tl.at(0, () => setCursor(640, 520));

  // ── 场景 1(0–4.8s):实时 AI 正文 + Context 百分比在跳 ──
  tl.at(0.5, mut(() => { store.stage.caption = S.caps[0]; }));
  tl.at(1.7, mut(() => { s1.last_ai_text = S.live.s1a; s1.context_pct = 63; }));
  tl.at(3.0, mut(() => { s2.last_ai_text = S.live.s2a; s2.context_pct = 43; s2.todo_done = 2; }));
  tl.at(4.2, mut(() => { s1.last_ai_text = S.live.s1b; s1.context_pct = 64; s1.todo_done = 4; }));

  // ── 场景 2(4.8–9.8s):转待交互 + tab 过滤 ──
  tl.at(5.0, mut(() => { store.stage.caption = S.caps[1]; }));
  tl.at(5.6, mut(() => {
    s2.session.status = "waiting";
    s2.last_ai_text = S.live.s2wait;
  }));
  moveToEl(tl, 6.3, 7.1, ".tabs .stab:nth-child(3)"); // 待交互(滑块占 nth-child(1)，故 +1)
  tl.at(7.3, () => clickEl(".tabs .stab:nth-child(3)"));
  moveToEl(tl, 8.4, 9.1, ".tabs .stab:nth-child(2)"); // 回到 全部
  tl.at(9.3, () => clickEl(".tabs .stab:nth-child(2)"));

  // ── 场景 3(9.8–15.0s):卡片菜单 → 重命名 ──
  tl.at(10.0, mut(() => { store.stage.caption = S.caps[2]; }));
  tl.at(10.4, () => hoverEl(card(1))); // 卡片抬起(扁平风只淡入底色)
  moveToEl(tl, 10.7, 11.4, menuBtn(1));
  tl.at(11.6, () => clickEl(menuBtn(1)));   // 打开卡片菜单
  moveToEl(tl, 12.1, 12.8, menuItem(3));    // 停顿看清菜单,再移到「重命名」
  tl.at(13.0, () => clickEl(menuItem(3)));  // 进编辑
  typeText(tl, 13.3, ".stk-edit", S.rename, CPS);
  tl.at(14.7, () => pressKey(".stk-edit", "Enter"));

  // ── 场景 4(15.0–20.9s):卡片菜单 → 加便签 ──
  tl.at(15.2, mut(() => { store.stage.caption = S.caps[3]; }));
  tl.at(15.6, () => hoverEl(card(0)));
  moveToEl(tl, 15.9, 16.6, menuBtn(0));
  tl.at(16.8, () => clickEl(menuBtn(0)));
  moveToEl(tl, 17.3, 18.0, menuItem(2));  // 「便签」
  tl.at(18.2, () => clickEl(menuItem(2))); // 打开便签编辑器
  typeText(tl, 18.5, ".stk-note-edit", S.note, CPS);
  tl.at(20.7, () => pressKey(".stk-note-edit", "Enter")); // 保存 → 便签块出现

  // ── 场景 5(20.9–24.6s):卡片菜单 → 归档 ──
  tl.at(21.0, mut(() => { store.stage.caption = S.caps[4]; }));
  tl.at(21.4, () => hoverEl(card(3)));
  moveToEl(tl, 21.7, 22.4, menuBtn(3));
  tl.at(22.6, () => clickEl(menuBtn(3)));
  moveToEl(tl, 23.1, 23.8, menuItem(4));  // 「归档」
  tl.at(24.0, () => clickEl(menuItem(4)));
  tl.at(24.3, () => hoverEl(null)); // 清除卡片 hover

  // ── 场景 6(24.6–29.4s):底栏搜索过滤 ──
  tl.at(24.8, mut(() => { store.stage.caption = S.caps[5]; }));
  moveToEl(tl, 25.3, 26.1, ".stk-bar-actions .stk-act:nth-child(2)");
  tl.at(26.3, () => clickEl(".stk-bar-actions .stk-act:nth-child(2)")); // 打开搜索(第 2 个动作)
  typeText(tl, 26.7, ".stk-search-in", S.search, 5); // 过滤到 tauri 那条
  moveToEl(tl, 28.5, 29.2, ".stk-search-x");
  tl.at(29.4, () => clickEl(".stk-search-x")); // 关闭搜索，列表还原

  // ── 场景 7(29.6–33.0s):置顶(pin)→ 图钉点亮 ──
  tl.at(29.7, mut(() => { store.stage.caption = S.caps[6]; }));
  moveToEl(tl, 30.2, 31.0, ".stk-bar-actions .stk-act:nth-child(4)"); // 图钉(第 4 个动作)
  tl.at(31.2, () => clickEl(".stk-bar-actions .stk-act:nth-child(4)")); // 点亮为置顶态

  // ── 场景 8(33.0–40.0s):吸边——右缘高亮提示 → 收成细条 → 偷看(过渡 0.8s,不掉帧) ──
  tl.at(33.2, mut(() => { store.stage.caption = S.caps[7]; }));
  tl.at(33.9, mut(() => { store.stage.glow = true; }));   // 拖近右缘:对应侧发光脉动提示
  tl.at(35.3, mut(() => { store.stage.glow = false; store.stage.mode = "strip"; })); // 松手→收成细条(→36.1)
  moveToEl(tl, 36.7, 37.5, ".demo-window .cstrip");
  tl.at(37.8, mut(() => { store.stage.mode = "expanded"; }));  // 悬停偷看,展开回原位(→38.6)
  tl.at(38.8, () => setCursor(320, 240)); // 光标移开(左侧),准备收回
  tl.at(39.7, mut(() => { store.stage.mode = "strip"; }));     // 自动收回(→40.5)

  // ── 场景 9(40.6–42.6s):收尾 ──
  tl.at(40.8, mut(() => { store.stage.caption = null; store.stage.finale = true; }));
  tl.at(42.6, () => {}); // 钉住总时长 ≈ 42.6s
  return tl;
}
