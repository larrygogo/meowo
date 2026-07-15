// demo 分镜的可见文案（中/英）。app 组件自身的标签走 useT；这里只放 demo 特有的
// 字幕 / 会话标题 / AI 正文 / 打字内容 / 收尾语。buildScript(lang) 与 DemoStage 取用。
export type DemoLang = "zh" | "en";

export type DemoStrings = {
  caps: string[]; // 8 段字幕，对应场景 1–8
  finale: string;
  sessions: { title: string; ai: string }[]; // 初始 4 张卡
  live: { s1a: string; s2a: string; s1b: string; s2wait: string };
  rename: string;
  note: string;
  search: string;
};

export const DEMO_STRINGS: Record<DemoLang, DemoStrings> = {
  zh: {
    caps: [
      "所有 AI 会话，一眼看全",
      "谁在等你回复，立刻知道",
      "卡片菜单，即点即管",
      "给会话挂个本地便签",
      "不用的收进归档",
      "搜索任意会话：标题 / 仓库名即时过滤",
      "需要时钉住，始终浮在最上层",
      "拖到边缘，吸附成一根状态条",
    ],
    finale: "你所有的 AI 编程会话，一眼看全",
    sessions: [
      { title: "重构吸边状态机", ai: "把状态机拆成了 3 个纯函数，正在补吸附边界的单测。" },
      { title: "接入账号用量面板", ai: "配额液柱组件写好了，底栏小屏已能读数。" },
      { title: "升级 tauri 到 2.3", ai: "已更新 Cargo.toml，等你确认几处 breaking change。" },
      { title: "修复 statusline 兼容性", ai: "兼容性修好并已合并，收工。" },
    ],
    live: {
      s1a: "重构完成，正在跑 cargo clippy 校验。",
      s2a: "在写 vitest 用例覆盖配额液柱。",
      s1b: "clippy 通过，写入 src/snap/mod.rs。",
      s2wait: "要应用这 3 处修改吗？(y / n)",
    },
    rename: "评审用量面板方案",
    note: "记得先确认 API key",
    search: "tauri",
  },
  en: {
    caps: [
      "See every AI session at a glance",
      "Know instantly who's waiting on you",
      "The card menu — point and manage",
      "Pin a local note to a session",
      "Archive what you're done with",
      "Search any session — filter by title or repo instantly",
      "Pin it when needed; always on top",
      "Drag to the edge; it snaps into a strip",
    ],
    finale: "See all your AI coding sessions at a glance",
    sessions: [
      { title: "Refactor edge-snap state machine", ai: "Split the state machine into 3 pure functions; adding boundary tests." },
      { title: "Wire up the usage panel", ai: "Quota-bar component done; the bottom readout works." },
      { title: "Bump tauri to 2.3", ai: "Updated Cargo.toml; a few breaking changes to confirm." },
      { title: "Fix statusline compatibility", ai: "Compatibility fixed and merged. Done." },
    ],
    live: {
      s1a: "Refactor done; running cargo clippy.",
      s2a: "Writing vitest cases for the quota bar.",
      s1b: "clippy passed; wrote src/snap/mod.rs.",
      s2wait: "Apply these 3 changes? (y / n)",
    },
    rename: "Review the usage-panel plan",
    note: "Confirm the API key first",
    search: "tauri",
  },
};
