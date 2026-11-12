import { describe, expect, it } from "vitest";
import { zh } from "./zh";
import { en } from "./en";

// UI 调研 shell #3/#9/#12：界面早已换成灵动岛 + 对话窗，字典里还有「看板/贴纸」指向用户找不到的东西；
// 「工作区/工作空间」同屏两种叫法（关卡 A 定案统一「工作区」）；用户否掉的「等你」造词回流。
// 例外：只被已下线的 Sticker / CollapsedStrip（现仅 poster 海报生成器还在渲染）读的键，
// 说的就是那块界面本身，不算指错路。
const STICKER_ONLY = new Set([
  "sticker.expandBoard", // CollapsedStrip：只在 poster/main.tsx 渲染
  "sticker.archivedNotice", // 只有 Sticker.tsx 读
]);

function walk(node: unknown, path: string, out: [string, string][]) {
  if (typeof node === "string") out.push([path, node]);
  else if (typeof node === "function") {
    // 文案函数：用占位参数调一次拿到成句。
    try { walk((node as (...a: unknown[]) => unknown)(1, 2, 3), path, out); } catch { /* 参数形状不符的跳过 */ }
  } else if (Array.isArray(node)) node.forEach((v, i) => walk(v, `${path}[${i}]`, out));
  else if (node && typeof node === "object") for (const [k, v] of Object.entries(node)) walk(v, path ? `${path}.${k}` : k, out);
}
const strings = (dict: unknown) => { const out: [string, string][] = []; walk(dict, "", out); return out.filter(([p]) => !STICKER_ONLY.has(p)); };

describe("字典用词", () => {
  it("中文不再说看板/贴纸、不用「等你」、只叫工作区", () => {
    const bad = strings(zh).filter(([, s]) => /看板|贴纸|等你|工作空间/.test(s));
    expect(bad).toEqual([]);
  });
  // 文案规范（meowo-copy-style）禁分号，拆成两句。ux-backlog 7G-8 修过一轮，wsHideTip 与 leadWorkflowDesc
  // 又带着「；」新加进来（research-backlog.md「仍存在」第 2 条，2026-11-12），故钉一条规则防再犯。
  it("中文文案不用分号", () => {
    const bad = strings(zh).filter(([, s]) => /[；;]/.test(s));
    expect(bad).toEqual([]);
  });
  it("英文不再说 board/sticker、不用 waiting on/for you", () => {
    const bad = strings(en).filter(([, s]) => /\bboard\b|sticker|waiting (on|for) you\b/i.test(s));
    expect(bad).toEqual([]);
  });
});
