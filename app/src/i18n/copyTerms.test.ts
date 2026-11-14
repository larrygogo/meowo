import { describe, expect, it } from "vitest";
import { zh } from "./zh";
import { en } from "./en";
import { ENTRIES } from "./errors";

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
  // 做空第一轮 #10（2026-11-12）：「请手动检查」「请重启 Meowo」「check the Terminal tab —」这类把人推出去
  // 收拾的句式在验收 13 只扫 chat 命名空间时漏网（errors.ts 的 worktree / 账号 / 内部状态三处）。
  // 这里全字典加后端错误映射一起扫。能在 Meowo 里做的给按钮，做不到的写原因（remote-no-desktop-deadend）。
  // 做空第二轮 R10（2026-11-12）收紧：第一轮放行了「手动切换 / 手动发送」，「请手动切到对应标签页」「请手动添加」
  // 「send it manually」照样把事推给用户，「重开 Meowo 可恢复」换个说法仍是叫人重启。现在「请…手动」所有变体、
  // 「手动 + 切换/切到/添加/发送/输入」、「重开/重启 Meowo」与英文 manually、restart/reopen/relaunch Meowo 一律拦。
  // 「手动排序 / 已手动终止 / 手动确认 / 你手动设置的」是在说模式或事实，不是叫人去做，规则本身不命中。
  const ZH_PUSH_OUT = /请?手动(?:检查|清理|处理|删除)|请[^，。！？]{0,10}?手动|手动(?:切换|切到|添加|发送|输入)|(?:到|去|切到|回到|在)(?:外部)?终端(?:页)?(?:里|中)?(?:检查|清理|清空|处理)|刷新后重试|请刷新|刷新再试|请重启|(?:重开|重启)\s*Meowo/;
  const EN_PUSH_OUT = /check (?:it|them|the [\w ]+?) manually|manually check|check the terminal|\b(?:restart|reopen|relaunch)(?:ing)?\s+meowo|refresh (?:and|to) (?:retry|try)|\bmanually\b/i;
  // 合理例外（键路径 → 理由）。只放「说的是一种模式/动作名」的，叫人自己去做的不许进来。
  const EN_ALLOWED = new Map([
    ["chat.sidebarSwitchToManual", "按钮名：切到「手动排序」模式（zh「改用手动排序」），不是叫人手动做什么"],
  ]);
  it("中英文案与后端错误都不叫人手动检查、去终端清理、刷新重试或重启", () => {
    expect(strings(zh).filter(([, s]) => ZH_PUSH_OUT.test(s))).toEqual([]);
    expect(strings(en).filter(([p, s]) => EN_PUSH_OUT.test(s) && !EN_ALLOWED.has(p))).toEqual([]);
    const errs = ENTRIES.flatMap((e) => [
      ...(e.zh && ZH_PUSH_OUT.test(e.zh) ? [`${e.m} | ${e.zh}`] : []),
      ...(EN_PUSH_OUT.test(e.en) ? [`${e.m} | ${e.en}`] : []),
    ]);
    expect(errs).toEqual([]);
  });
  // 做空第二轮 R10（2026-11-12）：进度横幅两位秒数时 21 字，超出主句 ≤20 的规范（meowo-copy-style）。
  it("终端就绪等待横幅两位秒数也不超过 20 字", () => {
    expect(zh.chat.terminalWaitingReady(99).length).toBeLessThanOrEqual(20);
  });
  // 规范禁破折号（meowo-copy-style）：英文错误映射里结尾挂 ` —` 的半截句（Agent 启动后立即退出，#10）。
  it("后端错误映射不用破折号", () => {
    expect(ENTRIES.filter((e) => /[—–]|——/.test(`${e.zh ?? ""}${e.en}`)).map((e) => e.m)).toEqual([]);
  });
  it("英文不再说 board/sticker、不用 waiting on/for you", () => {
    const bad = strings(en).filter(([, s]) => /\bboard\b|sticker|waiting (on|for) you\b/i.test(s));
    expect(bad).toEqual([]);
  });
});
