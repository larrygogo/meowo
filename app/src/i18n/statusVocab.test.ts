import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";
import { zh } from "./zh";
import { en } from "./en";
import vocab from "./statusVocab.json";
import {
  deliveryStatusLabel, requestStatusLabel, sessionRowLabel, sessionStatusLabel, sessionToneLabels,
} from "./statusVocab";

/**
 * 状态词表守卫（验收 1、18）：同一个状态只有一个词，界面各处都从词表取。
 *
 * 三道检查，任何一道失败都说明「第二种说法」回来了：
 * 1. 字典源码（zh.ts / en.ts）里，词表的词不许再以整条字面量出现：要用就引用 `vocab.x.y`。
 * 2. 字典的成句里不许出现被收敛掉的旧说法（出错了、待审批、等待审批、已收到…）。
 * 3. 渲染代码与后端通知表里不许硬编码状态词。
 */
const I18N = dirname(fileURLToPath(import.meta.url));
const SRC = join(I18N, "..");
const read = (path: string) => readFileSync(path, "utf8").replace(/\r\n/g, "\n");

function walk(node: unknown, path: string, out: [string, string][]) {
  if (typeof node === "string") out.push([path, node]);
  else if (typeof node === "function") {
    try { walk((node as (...a: unknown[]) => unknown)(1, 2, 3), path, out); } catch { /* 参数形状不符的跳过 */ }
  } else if (Array.isArray(node)) node.forEach((v, i) => walk(v, `${path}[${i}]`, out));
  else if (node && typeof node === "object") for (const [k, v] of Object.entries(node)) walk(v, path ? `${path}.${k}` : k, out);
}
const strings = (dict: unknown) => { const out: [string, string][] = []; walk(dict, "", out); return out; };
const words = (lang: "zh" | "en") => Object.values(vocab[lang]).flatMap((group) => Object.values(group));

/** 字典里与词表同字、但说的不是会话 / 请求 / 送达状态的整条字面量（行内容片段 → 理由）。 */
const SAME_WORD_OTHER_MEANING: Record<"zh" | "en", Record<string, string>> = {
  zh: {},
  en: {
    "checkState:": "CI 检查在跑，是代码托管平台的状态，不是会话状态",
    "runGroup:": "设置页分组标题「运行」类设置，不是状态词",
  },
};

/** 被收敛掉的旧说法。键路径白名单只放「说的是另一件事」的，状态词不许进。 */
const BANNED_ZH: [RegExp, string][] = [
  [/出错了/, "出错"],
  [/待审批|等待审批|等待批准/, "待批准"],
  [/已收到/, "已排队"],
  [/进行中/, "运行中"],
  [/子代理/, "子任务"],
  [/已手动终止|已终止/, "已中断"],
];
const BANNED_EN: [RegExp, string][] = [
  [/\berrored\b/i, "Error"],
  [/pending approval|awaiting approval|waiting for approval/i, "Needs approval"],
  [/awaiting answer/i, "Needs answer"],
  [/^received$/i, "Queued"],
  [/\bin progress\b/i, "Running"],
  [/\bsubagents?\b/i, "Subtask"],
  [/interrupted by you/i, "Interrupted"],
];
const ALLOWED: Record<string, string> = {
  // 「进行中」在这些键里说的是「未归档的会话」（对照 Kimi 的 进行中 → 已完成），不是运行状态。
  "sticker.unarchive": "未归档的会话",
  "sticker.stripMore": "未归档的会话",
  "admin.statusOpen": "未归档的会话",
  "admin.reopen": "未归档的会话",
  "admin.reopenCount": "未归档的会话",
  "admin.batchReopenedToast": "未归档的会话",
  "chat.sidebarTabOpen": "未归档的会话",
  "chat.sidebarReopenFailed": "未归档的会话",
  "settings.archivedInAdminDesc": "未归档的会话",
  "settings.unarchiveFailed": "未归档的会话",
  "worktree.reason.shared_session": "未归档的会话",
  // PR 与 CI 检查的状态是代码托管平台的词，不是会话状态。
  "forge.prState.open": "PR 状态",
  "forge.checkState.pending": "CI 检查状态",
  // 扩展清单里的「子代理」是 Claude Code 的自定义 agent 定义（扩展种类），不是对话里派出的子任务。
  "extensions.kinds.agent": "扩展种类",
  "extensions.counts.agent": "扩展种类",
};

function guardedFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      // i18n 由前两道检查管；generated 是后端产物；test / demo / poster 是固件与营销物料。
      if (!["i18n", "generated", "test", "demo", "poster"].includes(entry.name)) guardedFiles(path, out);
      continue;
    }
    if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) && !/\.harness\.tsx?$/.test(entry.name)) out.push(path);
  }
  return out;
}

describe("状态词表", () => {
  it("中英文键一致，词不超过 20 字、不带破折号和分号", () => {
    const shape = (lang: "zh" | "en") => Object.entries(vocab[lang]).map(([group, entries]) => [group, Object.keys(entries)]);
    expect(shape("en")).toEqual(shape("zh"));
    for (const lang of ["zh", "en"] as const) {
      for (const word of words(lang)) {
        expect(word.length, word).toBeGreaterThan(0);
        expect(word.length, word).toBeLessThanOrEqual(20);
        expect(word).not.toMatch(/[—–；;]/);
      }
    }
  });

  it("同一组里两个状态不共用一个词", () => {
    for (const lang of ["zh", "en"] as const) {
      for (const [group, entries] of Object.entries(vocab[lang])) {
        const list = Object.values(entries);
        expect(new Set(list).size, `${lang}.${group}`).toBe(list.length);
      }
    }
  });

  it("字典挂的就是词表本身", () => {
    expect(zh.vocab).toBe(vocab.zh);
    expect(en.vocab).toBe(vocab.en);
  });

  it.each(["zh", "en"] as const)("%s 字典源码不再把词表的词写成字面量", (lang) => {
    const source = read(join(I18N, `${lang}.ts`));
    const allowed = SAME_WORD_OTHER_MEANING[lang];
    const offences: string[] = [];
    source.split("\n").forEach((line, index) => {
      const code = line.trim();
      if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*")) return;
      for (const word of new Set(words(lang))) {
        // 作为键（"回合出错": …）不算，只看值位置的整条字面量。
        const literal = new RegExp(`(?<![\\w"])"${word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"(?!\\s*:)`);
        if (!literal.test(code)) continue;
        if (Object.keys(allowed).some((fragment) => code.includes(fragment))) continue;
        offences.push(`${lang}.ts:${index + 1} 「${word}」应引用 vocab → ${code.slice(0, 120)}`);
      }
    });
    expect(offences).toEqual([]);
  });

  it("字典成句里没有被收敛掉的旧说法", () => {
    const scan = (dict: unknown, banned: [RegExp, string][]) => strings(dict)
      .filter(([path]) => !path.startsWith("vocab."))
      .flatMap(([path, text]) => banned
        .filter(([re]) => re.test(text) && !ALLOWED[path.replace(/\[\d+\]$/, "")])
        .map(([, use]) => `${path}: 「${text}」应说「${use}」`));
    expect(scan(zh, BANNED_ZH)).toEqual([]);
    expect(scan(en, BANNED_EN)).toEqual([]);
  });

  it("白名单里的键都还在字典里（键没了就该从白名单删掉）", () => {
    const paths = new Set(strings(zh).map(([path]) => path.replace(/\[\d+\]$/, "")));
    expect(Object.keys(ALLOWED).filter((path) => !paths.has(path))).toEqual([]);
  });

  it("渲染代码不硬编码状态词", () => {
    const files = guardedFiles(SRC);
    expect(files.length).toBeGreaterThan(100);
    const zhWords = [...new Set(words("zh"))];
    const offences: string[] = [];
    for (const file of files) {
      read(file).split("\n").forEach((line, index) => {
        const code = line.trim();
        if (code.startsWith("//") || code.startsWith("*") || code.startsWith("/*") || code.startsWith("{/*")) return;
        for (const word of zhWords) {
          if (new RegExp(`["'\`>]${word}["'\`<]`).test(code)) offences.push(`${relative(SRC, file)}:${index + 1} 硬编码「${word}」`);
        }
        for (const [re] of BANNED_ZH) {
          if (new RegExp(`["'\`>](?:${re.source})["'\`<]`).test(code)) offences.push(`${relative(SRC, file)}:${index + 1} 硬编码旧说法 ${re.source}`);
        }
      });
    }
    expect(offences).toEqual([]);
  });

  it("后端插件不把状态词当正文硬编码下发", () => {
    // TODO(合并后)：OpenCode 给越过的工具补的失败回执正文是硬编码中文「已中断」（走查显示 B15），
    // 英文界面会露中文。「显示」一路把它改成显式终态后，这条白名单要删掉。
    const allowed = new Set(["plugins/opencode/telemetry.rs"]);
    const root = join(SRC, "../src-tauri/crates/meowo-agent/src");
    const files: string[] = [];
    const collect = (dir: string) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) collect(path);
        else if (entry.name.endsWith(".rs")) files.push(path);
      }
    };
    collect(root);
    expect(files.length).toBeGreaterThan(20);
    const zhWords = [...new Set(words("zh"))];
    const offences: string[] = [];
    for (const file of files) {
      const name = relative(root, file).replace(/\\/g, "/");
      // 测试模块里的断言与样本不是下发给界面的正文。
      const production = read(file).split(/\n\s*#\[cfg\(test\)\]/)[0];
      production.split("\n").forEach((line, index) => {
        const code = line.trim();
        if (code.startsWith("//")) return;
        for (const word of zhWords) {
          if (code.includes(`"${word}"`) && !allowed.has(name)) offences.push(`${name}:${index + 1} 硬编码「${word}」`);
        }
      });
    }
    expect(offences).toEqual([]);
    // 白名单里的文件确实还有这个问题：修掉后这条会失败，提醒删白名单。
    expect(read(join(root, "plugins/opencode/telemetry.rs"))).toContain(`"已中断"`);
  });

  it("后端通知标题的状态词取自同一份词表，不再各写一句", () => {
    const settings = read(join(SRC, "../src-tauri/src/settings.rs"));
    expect(settings).toContain(`include_str!("../../src/i18n/statusVocab.json")`);
    for (const key of ["notify.error", "notify.waiting", "notify.pending.approval", "notify.pending.question", "notify.pending.plan", "notify.pending.gate", "notify.blocked"]) {
      expect(settings, key).not.toMatch(new RegExp(`"${key.replace(/\./g, "\\.")}"\\)\\s*=>\\s*"`));
      expect(settings, key).toMatch(new RegExp(`"${key.replace(/\./g, "\\.")}" => \\("(session|request)", "\\w+"\\)`));
    }
  });
});

describe("取词函数", () => {
  it.each([["zh", zh], ["en", en]] as const)("%s：会话六态加已中断都有词，未知状态不露内部标识", (_, t) => {
    for (const tone of ["running", "pending", "waiting", "offline", "ended", "error", "interrupted"]) {
      expect(sessionStatusLabel(t, tone)).toBe((t.vocab.session as Record<string, string>)[tone]);
      expect(sessionStatusLabel(t, tone)).not.toBe("");
    }
    expect(sessionStatusLabel(t, "weird")).toBe("");
    expect(sessionToneLabels(t).on).toBe(t.vocab.session.online);
  });

  it.each([["zh", zh], ["en", en]] as const)("%s：请求种类未知时说待处理，侧栏有审批时说待批准", (_, t) => {
    expect(requestStatusLabel(t, "approval")).toBe(t.vocab.request.approval);
    expect(requestStatusLabel(t, "question")).toBe(t.vocab.request.question);
    expect(requestStatusLabel(t, null)).toBe(t.vocab.request.blocked);
    expect(requestStatusLabel(t, "permission")).toBe(t.vocab.request.blocked);
    expect(sessionRowLabel(t, "running", true)).toBe(t.vocab.request.approval);
    expect(sessionRowLabel(t, "error", false)).toBe(t.vocab.session.error);
  });

  it.each([["zh", zh], ["en", en]] as const)("%s：六个送达状态各有一个词，瞬态 received 不显示", (_, t) => {
    const phases = ["sending", "pending", "accepted", "inserted", "uncertain", "failed"];
    const labels = phases.map((phase) => deliveryStatusLabel(t, phase));
    expect(labels.every(Boolean)).toBe(true);
    expect(new Set(labels).size).toBe(phases.length);
    expect(deliveryStatusLabel(t, "received")).toBe("");
  });
});
