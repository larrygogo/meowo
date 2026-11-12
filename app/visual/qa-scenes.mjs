// 补看场景：现有矩阵拍不到、要靠真实操作才出现的状态（弹层、悬停、键盘焦点），
// 以及缩放 150%、620 最小宽 + 右侧面板、2560 超宽这些状态维度。只拍 dpr 2。
// 字段：name 文件名前缀；scene / params 交给 harness 页（?scene=…&<params>）；size [宽, 高]；
// steps 见 shoot.mjs 的 runSteps；sel 截图前必须在的元素；langs / themes 省略时是中文、深浅各一张。
const TA = ".chat-compose textarea";
const W620 = [620, 760];
const PHONE = [375, 812];
const SETTINGS = [620, 460];
const MODEL_BTN = ".chat-compose .chat-model-select .chat-model-button";

export const QA_SHOTS = [
  // ── 输入框：斜杠菜单、@ 补全 ──
  { name: "qa-slash", scene: "pop-idle", steps: [["type", TA, "/"]] },
  { name: "qa-slash-620", scene: "pop-idle", size: W620, steps: [["type", TA, "/co"]] },
  { name: "qa-at", scene: "pop-idle", sel: "#chat-at-listbox", steps: [["type", TA, "@"], ["tick", 300]] },
  // ── 侧栏与标题栏的菜单 ──
  { name: "qa-rowmenu", scene: "pop-idle", steps: [["rclick", ".chat-sidebar-item"]] },
  { name: "qa-hover-row", scene: "pop-idle", steps: [["hover", ".chat-sidebar-item >> nth=1"]] },
  { name: "qa-kbd-focus", scene: "pop-idle", steps: [["keys", "Tab", 3]] },
  { name: "qa-tooltip", scene: "pop-idle", steps: [["hover", ".chat-bar button >> nth=0"], ["tick", 900]] },
  // ── 状态维度 ──
  { name: "qa-chat-ui150", scene: "chat", params: "ui=1.5", langs: ["zh", "en"] },
  { name: "qa-chat-ui150-620", scene: "chat", params: "ui=1.5", size: W620 },
  { name: "qa-chat-en-620", scene: "chat", size: W620, langs: ["en"] },
  { name: "qa-panel-en-620", scene: "panel-changes", size: W620, langs: ["en"] },
  { name: "qa-panel-ui150", scene: "panel-changes", params: "ui=1.5" },
  // 面板最大化（占满整张内容面）：内容列封顶居中后，贴内容宽的小控件容易留在面板左缘没跟上
  // （2026-11-12 用户截图：「全部 / 本会话 / 本轮」孤零零贴在最左边）。矩阵里原本没有这个状态。
  { name: "qa-panel-max", scene: "panel-changes", params: "max=1", sel: ".scm-row" },
  { name: "qa-panel-max-2560", scene: "panel-changes", params: "max=1", size: [2560, 1440], sel: ".scm-row" },
  { name: "qa-diff-max-2560", scene: "panel-diff", params: "max=1", size: [2560, 1440], sel: ".chat-diff-code" },
  { name: "qa-files-max-2560", scene: "panel-files", params: "max=1", size: [2560, 1440], sel: ".chat-diff-panel" },
  // Markdown 预览最大化：正文封顶居中，头行（返回 / 文件名 / 预览·源码 / 打开）要跟着同一列走。
  { name: "qa-md-max-2560", scene: "flow-md", params: "max=1", size: [2560, 1440], sel: ".chat-diff-mdview", steps: [["click", ".chat-diff-tree-file:has-text('README.md')"]] },
  { name: "qa-md-max", scene: "flow-md", params: "max=1", sel: ".chat-diff-mdview", steps: [["click", ".chat-diff-tree-file:has-text('README.md')"]] },
  // 「更改」的树状视图、缩放 150% 下的「文件」页签：行高与图标列要跟着缩放和层级走。
  { name: "qa-panel-tree", scene: "panel-changes", sel: ".scm-row.is-dir", steps: [["click", ".scm-tools .chat-diff-layout button >> nth=1"]] },
  { name: "qa-files-ui150", scene: "panel-files", params: "ui=1.5", sel: ".chat-diff-tree-file" },
  // ── 对话流里靠后端状态出现的东西（harness 见 qaFlow.ts）──
  { name: "qa-lead", scene: "flow-lead", sel: ".lead-gate-card", langs: ["zh", "en"] },
  { name: "qa-lead-open", scene: "flow-lead", sel: ".lead-gate-card", steps: [["click", ".lead-gate-more"], ["click", ".lead-gate-option >> nth=1"]] },
  { name: "qa-lead-620", scene: "flow-lead", sel: ".lead-gate-card", size: W620 },
  { name: "qa-lead-panel", scene: "flow-lead", sel: ".panel-lead", steps: [["click", ".dock-workbar .ui-pill"]] },
  { name: "qa-queued", scene: "flow-queued", sel: ".chat-queued-turn" },
  { name: "qa-sync", scene: "flow-sync", sel: ".chat-sync-warn", steps: [["tick", 9000]] },
  { name: "qa-load-error", scene: "flow-error", sel: ".chat-empty.is-error", steps: [["tick", 9000]] },
  { name: "qa-loading", scene: "flow-loading", sel: ".chat-window" },
  { name: "qa-unsupported", scene: "flow-unsupported", sel: ".chat-empty" },
  { name: "qa-earlier", scene: "flow-earlier", sel: ".chat-load-earlier", steps: [["eval", "document.querySelector('.chat-scroll').scrollTop = 0"]] },
  { name: "qa-handoff", scene: "flow-handoff", sel: ".chat-window", steps: [["eval", "document.querySelector('.chat-scroll').scrollTop = 0"]] },
  { name: "qa-wt-running", scene: "flow-wt-running", sel: ".wt-setup" },
  { name: "qa-wt-failed", scene: "flow-wt-failed", sel: ".wt-setup", steps: [["click", '[data-testid="wt-setup-log"]']] },
  { name: "qa-wt-trust", scene: "flow-wt-trust", sel: ".wt-trust", steps: [["click", '[data-testid="wt-setup-review"]']] },
  { name: "qa-broker", scene: "flow-broker", sel: ".chat-approval-actions", langs: ["zh", "en"] },
  { name: "qa-broker-danger", scene: "flow-broker-danger", sel: ".chat-approval-actions" },
  { name: "qa-broker-reason", scene: "flow-broker", sel: ".chat-approval-reason", steps: [["click", ".chat-approval .is-deny-reason"]] },
  { name: "qa-screen-trust", scene: "flow-screen-trust", sel: ".chat-screen-approval" },
  { name: "qa-screen-command", scene: "flow-screen-command", sel: ".chat-screen-approval" },
  { name: "qa-screen-plan", scene: "flow-screen-plan", sel: ".chat-screen-approval" },
  { name: "qa-screen-question", scene: "flow-screen-question", sel: ".chat-screen-approval" },
  { name: "qa-screen-prompt", scene: "flow-screen-prompt", sel: ".chat-screen-approval" },
  { name: "qa-image", scene: "flow-image", sel: ".chat-image-thumb-btn" },
  { name: "qa-lightbox", scene: "flow-image", sel: ".chat-lightbox", steps: [["click", ".chat-image-thumb-btn"]] },
  { name: "qa-drop", scene: "flow-drop", sel: ".chat-drop-overlay" },
  { name: "qa-search", scene: "flow-states", sel: ".chat-search-host", steps: [["key", "Control+f"], ["type", ".chat-search-input", "schema"]] },
  { name: "qa-msgnav", scene: "flow-states", sel: ".chat-msg-nav", steps: [["hover", ".chat-msg-nav [data-nav-id] >> nth=0", { force: true }]] },
  { name: "qa-mode-menu", scene: "pop-idle", steps: [["click", ".chat-compose .chat-mode-button"]] },
  { name: "qa-model-menu", scene: "pop-idle", sel: ".chat-model-quick", steps: [["click", MODEL_BTN]] },
  { name: "qa-model-picker", scene: "pop-idle", sel: ".chat-model-picker", steps: [["click", MODEL_BTN], ["click", ".chat-model-quick-more"]], langs: ["zh", "en"] },
  { name: "qa-at-search", scene: "pop-idle", sel: "#chat-at-listbox", steps: [["type", TA, "@log"], ["tick", 400]] },
  { name: "qa-annotate", scene: "flow-annotate", sel: ".chat-annotator", steps: [["click", ".chat-attachment-annotate"], ["tick", 600]] },
  { name: "qa-attachment", scene: "flow-annotate", sel: ".chat-attachment.is-image" },
  // ── 标题栏 ──
  { name: "qa-title-menu", scene: "pop-idle", sel: ".chat-title-actions", steps: [["click", ".chat-title-btn"]], langs: ["zh", "en"] },
  { name: "qa-title-overflow", scene: "panel-changes", size: W620, sel: ".chat-title-actions", steps: [["click", ".chat-title-btn"]] },
  { name: "qa-open-menu", scene: "pop-idle", sel: ".tb-open-menu", steps: [["click", ".tb-open-chev"]] },
  { name: "qa-rename", scene: "pop-idle", sel: ".chat-modal .ns-input", steps: [["click", ".chat-title-btn"], ["click", ".chat-title-actions .dd-item:has-text('重命名')"]] },
  { name: "qa-rename-error", scene: "flow-rename-error", sel: ".chat-modal-error", steps: [["click", ".chat-title-btn"], ["click", ".chat-title-actions .dd-item:has-text('重命名')"], ["type", ".chat-modal .ns-input", " v2"], ["key", "Enter"]] },
  { name: "qa-context-tip", scene: "pop-idle", sel: ".tip", steps: [["hover", ".chat-context"], ["tick", 500]] },
  // ── 侧栏 ──
  { name: "qa-side-batch", scene: "pop-idle", sel: ".chat-sidebar-batch", steps: [["click", ".chat-sidebar-item >> nth=0", { modifiers: ["Control"] }], ["click", ".chat-sidebar-item >> nth=1", { modifiers: ["Control"] }]], langs: ["zh", "en"] },
  { name: "qa-side-rename", scene: "pop-idle", sel: ".chat-sidebar-edit", steps: [["rclick", ".chat-sidebar-item >> nth=1"], ["click", ".ctx-menu .ctx-item >> nth=2"]] },
  { name: "qa-side-search", scene: "pop-idle", sel: ".chat-sidebar-deep-hit", steps: [["type", ".chat-sidebar-search input", "schema"], ["tick", 500], ["click", ".chat-sidebar-deep .chat-sidebar-more"]] },
  { name: "qa-side-empty", scene: "flow-side-empty", sel: ".chat-sidebar-empty" },
  { name: "qa-side-error", scene: "flow-side-error", sel: ".chat-sidebar-empty.is-error" },
  { name: "qa-side-done", scene: "pop-idle", sel: ".chat-sidebar-cleanbar", steps: [["click", ".chat-sidebar-tabs button >> nth=1"], ["click", ".chat-sidebar-cleanbar button"]] },
  { name: "qa-side-ws", scene: "flow-plain", sel: ".chat-sidebar-tabs", steps: [["click", ".chat-sidebar-tabs button >> nth=2"]] },
  { name: "qa-side-ws-undo", scene: "flow-plain", sel: ".chat-sidebar-tabs", steps: [["click", ".chat-sidebar-tabs button >> nth=2"], ["hover", ".chat-ws-dir-row >> nth=1"], ["rclick", ".chat-ws-dir-row >> nth=1"]] },
  { name: "qa-side-filter", scene: "pop-idle", sel: ".sf-menu", steps: [["click", ".chat-sidebar-filter"]] },
  { name: "qa-admin", scene: "flow-plain", params: "admin=done", sel: ".session-admin", langs: ["zh", "en"] },
  { name: "qa-admin-batch", scene: "flow-plain", params: "admin=done", sel: ".sa-batch", steps: [["click", "tbody .sa-col-cb .sa-cb >> nth=0"], ["click", "tbody .sa-col-cb .sa-cb >> nth=1"]] },
  { name: "qa-admin-toast", scene: "flow-plain", params: "admin=done", sel: ".sa-toast", steps: [["click", "tbody .sa-col-cb .sa-cb >> nth=0"], ["click", ".sa-batch .sa-btn-q"]] },
  { name: "qa-admin-620", scene: "flow-plain", params: "admin=done", size: W620, sel: ".session-admin" },
  // ── 右侧面板 ──
  { name: "qa-md", scene: "flow-md", sel: ".chat-diff-mdview", steps: [["click", ".chat-diff-tree-file:has-text('README.md')"]] },
  { name: "qa-browser", scene: "flow-browser", sel: ".browser-pane", langs: ["zh", "en"] },
  { name: "qa-browser-blank", scene: "flow-browser-blank", sel: ".browser-pane" },
  { name: "qa-shell", scene: "flow-shell", sel: ".shell-term" },
  // ── 托管终端页签 ──
  // 浅色那张和深色那张除 DEV 角标外逐像素相同，这是产品定的：终端页签整窗恒为深色（styles.css
  // 「终端视图强制深色」，xterm 底色写死深底，浅色主题下只变终端那一块会割裂）。浅色图留着当回归：
  // 哪天有浅色规则漏进终端页签，两张图就会出现差异。
  { name: "qa-term", scene: "flow-term", sel: ".managed-terminal", steps: [["click", ".chat-terminal-toggle"], ["tick", 800]] },
  { name: "qa-term-search", scene: "flow-term", sel: ".term-search", steps: [["click", ".chat-terminal-toggle"], ["tick", 800], ["key", "Control+f"], ["type", ".term-search input", "Read"]] },
  { name: "qa-term-link", scene: "flow-term", sel: ".managed-terminal", steps: [["click", ".chat-terminal-toggle"], ["tick", 800], ["hover", ".managed-terminal .xterm-screen", { position: { x: 520, y: 118 } }], ["tick", 300]] },
  { name: "qa-term-exited", scene: "flow-term-exited", sel: ".managed-terminal-exit-card", steps: [["click", ".chat-terminal-toggle"], ["tick", 800]] },
  { name: "qa-term-clean", scene: "flow-term-clean", sel: ".managed-terminal-exit-card", steps: [["click", ".chat-terminal-toggle"], ["tick", 800]] },
  { name: "qa-term-idle", scene: "flow-term-idle", sel: ".managed-terminal", steps: [["click", ".chat-terminal-toggle"], ["tick", 800]] },
  { name: "qa-term-error", scene: "flow-term-error", sel: ".managed-terminal", steps: [["click", ".chat-terminal-toggle"], ["tick", 800], ["hover", ".managed-terminal"], ["click", ".managed-terminal-actions button >> nth=0"]] },
  { name: "qa-term-approval", scene: "flow-term-approval", sel: ".chat-terminal-approval", steps: [["click", ".chat-terminal-toggle"], ["tick", 800]] },
  // ── 设置窗：更新状态、下拉展开、输入框聚焦、保存失败 ──
  ...["checking", "error", "available", "downloading", "ready"].map((u) => ({ name: "qa-set-update-" + u, scene: "settingswin-about", params: "upd=" + u, size: SETTINGS, sel: ".settings", steps: [["tick", 11000]] })),
  { name: "qa-set-dropdown", scene: "settingswin-general", size: SETTINGS, sel: ".dd-menu", steps: [["click", ".dd > .dd-btn"]] },
  { name: "qa-set-dropdown-sessions", scene: "settingswin-sessions", size: SETTINGS, sel: ".dd-menu", steps: [["click", ".dd > .dd-btn >> nth=1"]] },
  { name: "qa-set-focus", scene: "settingswin-network", size: SETTINGS, sel: ".settings", steps: [["click", ".main-body input[type=text], .main-body input:not([type])"]] },
  { name: "qa-set-search-focus", scene: "settingswin-general", size: SETTINGS, sel: ".settings", steps: [["click", ".side-search input"]] },
  { name: "qa-set-error", scene: "settingswin-general", params: "fail=1", size: SETTINGS, sel: ".settings-error", steps: [["click", ".pswitch >> nth=1"]], langs: ["zh", "en"] },
  { name: "qa-set-error-remote", scene: "settingswin-remote", params: "fail=1", size: SETTINGS, sel: ".settings-error" },
  { name: "qa-set-kbd", scene: "settingswin-appearance", size: SETTINGS, sel: ".settings", steps: [["keys", "Tab", 4]] },
  { name: "qa-set-hover", scene: "settingswin-general", size: SETTINGS, sel: ".settings", steps: [["hover", ".side-nav .nav-item >> nth=2"]] },
  { name: "qa-set-en", scene: "settingswin-general", size: SETTINGS, sel: ".settings", langs: ["en"] },
  { name: "qa-set-en-remote", scene: "settingswin-remote", size: SETTINGS, sel: ".settings", langs: ["en"] },
  { name: "qa-set-en-sessions", scene: "settingswin-sessions", size: SETTINGS, sel: ".settings", langs: ["en"] },
  // ── 定尺寸窗口的英文（文案最长）──
  ...["shortcuts", "appearance", "network", "account", "extensions", "about"].map((sec) => ({ name: "qa-set-en-" + sec, scene: "settingswin-" + sec, size: SETTINGS, sel: ".settings", langs: ["en"], themes: ["light"] })),
  ...[1, 2, 3, 4].map((n) => ({ name: "qa-onboarding-en-" + n, scene: "onboarding-" + n, size: [460, 600], langs: ["en"], themes: ["dark"] })),
  { name: "qa-updater-en", scene: "updater-available", size: [400, 500], sel: ".up-notes-body", langs: ["en"], themes: ["light"] },
  { name: "qa-updater-error-en", scene: "updater-error", size: [400, 252], sel: ".up-err", langs: ["en"], themes: ["dark"] },
  { name: "qa-confirm-en", scene: "confirm-danger", size: [420, 132], langs: ["en"] },
  { name: "qa-newsession-en", scene: "newsession-window-full", size: [460, 520], sel: ".wt-opt-body", langs: ["en"] },
  { name: "qa-confirm-kbd", scene: "confirm-danger", size: [420, 132], steps: [["key", "Tab"]] },
  { name: "qa-onboarding-hover", scene: "onboarding-1", size: [460, 600], themes: ["light"], steps: [["hover", ".onboarding .btn-primary"]] },
  // ── 其余状态维度：超宽、英文、键盘焦点 ──
  { name: "qa-flow-2560", scene: "flow-states", size: [2560, 1440] },
  { name: "qa-lead-2560", scene: "flow-lead", size: [2560, 1440], sel: ".lead-gate-card", themes: ["light"] },
  { name: "qa-flow-en", scene: "flow-states", langs: ["en"] },
  { name: "qa-flow-ui150", scene: "flow-states", params: "ui=1.5" },
  { name: "qa-question-ui150", scene: "flow-question", params: "ui=1.5", size: W620 },
  { name: "qa-kbd-approval", scene: "chat", steps: [["keys", "Tab", 2]] },
  { name: "qa-kbd-sidebar", scene: "pop-idle", steps: [["click", ".chat-sidebar-search input"], ["keys", "Tab", 2]] },
  { name: "qa-hover-send", scene: "pop-idle", steps: [["type", TA, "hi"], ["hover", ".chat-send-button"]] },
  { name: "qa-nsp-focus", scene: "newsession-page", sel: ".nsp-composer", steps: [["click", ".nsp-input"]] },
  { name: "qa-nsp-en-620", scene: "newsession-page-wt", size: W620, sel: ".nsp-composer", langs: ["en"] },
  // ── 灵动岛：缩放 150%、悬停 ──
  { name: "qa-island-panel-ui150", scene: "island-panel", params: "ui=1.5", size: [1000, 460], sel: ".island-shell", langs: ["zh", "en"], themes: ["dark"] },
  { name: "qa-island-usage-ui150", scene: "island-usage", params: "ui=1.5", size: [1000, 460], sel: ".island-shell", themes: ["dark"] },
  { name: "qa-island-alert-ui150", scene: "island-alert", params: "ui=1.5", size: [1000, 160], langs: ["zh", "en"], themes: ["dark"] },
  { name: "qa-island-compact-ui150", scene: "island-compact", params: "ui=1.5", size: [1000, 120], themes: ["dark"] },
  { name: "qa-island-panel-hover", scene: "island-panel", size: [700, 300], sel: ".island-shell", themes: ["dark"], steps: [["hover", ".island-shell [role=button], .island-shell button >> nth=1"]] },
  { name: "qa-island-panel-kbd", scene: "island-panel", size: [700, 300], sel: ".island-shell", themes: ["dark"], steps: [["keys", "Tab", 2]] },
  // ── 手机页 ──
  { name: "qa-m-model", page: "mobile", scene: "mobile-idle", size: PHONE, sel: ".chat-sheet", steps: [["click", MODEL_BTN]] },
  { name: "qa-m-mode", page: "mobile", scene: "mobile-idle", size: PHONE, sel: ".chat-sheet", steps: [["click", ".chat-compose .chat-mode-button"]] },
  { name: "qa-m-lead-kbd", page: "mobile", scene: "mobile-lead", size: [375, 476], sel: ".lead-gate-card", steps: [["click", ".lead-gate-note"]] },
  { name: "qa-m-list-hover", page: "mobile", scene: "mobile-list", size: PHONE, sel: ".remote-session-list", langs: ["en"] },
  { name: "qa-m-slash", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["type", TA, "/"]], sel: ".chat-sheet" },
  { name: "qa-m-at", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["type", TA, "@src"], ["tick", 400]], sel: "#chat-at-listbox" },
  { name: "qa-m-add", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["click", ".chat-attach-button"]], sel: ".chat-sheet" },
  { name: "qa-m-lead", page: "mobile", scene: "mobile-lead", size: PHONE, sel: ".lead-gate-card", langs: ["zh", "en"] },
  { name: "qa-m-lead-open", page: "mobile", scene: "mobile-lead", size: PHONE, sel: ".lead-gate-card", steps: [["click", ".lead-gate-more"], ["click", ".lead-gate-option >> nth=1"]] },
  // 后台任务面板（一条在跑、三条跑完）：面板头两行（标题 + 关闭钮 / 整行分段筛选），转圈是运行绿。
  { name: "qa-m-tasks", page: "mobile", scene: "mobile-tasks", size: PHONE, sel: ".panel-tasks", steps: [["click", ".dock-workbar .ui-pill >> nth=0"]], langs: ["zh", "en"] },
  { name: "qa-m-tasks-430", page: "mobile", scene: "mobile-tasks", size: [430, 932], sel: ".panel-tasks", steps: [["click", ".dock-workbar .ui-pill >> nth=0"]] },
  { name: "qa-m-tasks-done", page: "mobile", scene: "mobile-tasks", size: PHONE, sel: ".panel-tasks", steps: [["click", ".dock-workbar .ui-pill >> nth=0"], ["click", ".chat-task-filter button >> nth=2"]], themes: ["dark"] },
  { name: "qa-m-subagents", page: "mobile", scene: "mobile-tasks", size: PHONE, sel: ".panel-tasks", steps: [["click", ".dock-workbar .ui-pill >> nth=1"]], themes: ["dark"] },
  { name: "qa-m-approval-kbd", page: "mobile", scene: "mobile-chat", size: [375, 476], sel: ".chat-approval-actions", steps: [["click", TA]] },
  { name: "qa-m-settings-on", page: "mobile", scene: "mobile-settings", params: "on=1", size: PHONE, sel: ".pswitch.on", langs: ["zh", "en"] },
];
