// 补看场景：现有矩阵拍不到、要靠真实操作才出现的状态（弹层、悬停、键盘焦点），
// 以及缩放 150%、620 最小宽 + 右侧面板、2560 超宽这些状态维度。只拍 dpr 2。
// 字段：name 文件名前缀；scene / params 交给 harness 页（?scene=…&<params>）；size [宽, 高]；
// steps 见 shoot.mjs 的 runSteps；sel 截图前必须在的元素；langs / themes 省略时是中文、深浅各一张。
const TA = ".chat-compose textarea";
const W620 = [620, 760];
const PHONE = [375, 812];

export const QA_SHOTS = [
  // ── 输入框：斜杠菜单、@ 补全 ──
  { name: "qa-slash", scene: "pop-idle", steps: [["type", TA, "/"]] },
  { name: "qa-slash-620", scene: "pop-idle", size: W620, steps: [["type", TA, "/co"]] },
  { name: "qa-at", scene: "pop-idle", steps: [["type", TA, "@"]] },
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
  // ── 手机页 ──
  { name: "qa-m-slash", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["type", TA, "/"]], sel: ".chat-sheet" },
  { name: "qa-m-at", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["type", TA, "@src"], ["tick", 400]], sel: "#chat-at-listbox" },
  { name: "qa-m-add", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["click", ".chat-attach-button"]], sel: ".chat-sheet" },
  { name: "qa-m-model", page: "mobile", scene: "mobile-idle", size: PHONE, steps: [["click", '.chat-compose button.chat-model-button:not(.chat-mode-button)']], sel: ".chat-window" },
  { name: "qa-m-lead", page: "mobile", scene: "mobile-lead", size: PHONE, sel: ".lead-gate-card", langs: ["zh", "en"] },
  { name: "qa-m-lead-open", page: "mobile", scene: "mobile-lead", size: PHONE, sel: ".lead-gate-card", steps: [["click", ".lead-gate-more"], ["click", ".lead-gate-option >> nth=1"]] },
  { name: "qa-m-approval-kbd", page: "mobile", scene: "mobile-chat", size: [375, 476], sel: ".chat-approval-actions", steps: [["click", TA], ["log", "(() => { const out = []; let el = document.querySelector('.chat-approval pre'); while (el && !el.classList.contains('chat-main')) { const cs = getComputedStyle(el); out.push([el.className || el.tagName, Math.round(el.getBoundingClientRect().height), cs.display, cs.flexShrink, cs.overflowY, cs.maxHeight]); el = el.parentElement; } return out; })()"]] },
  { name: "qa-m-settings-on", page: "mobile", scene: "mobile-settings", params: "on=1", size: PHONE, sel: ".pswitch.on", langs: ["zh", "en"] },
];
