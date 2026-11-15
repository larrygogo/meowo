// 架构守卫：认终端输入框的规格只有一份，认框的实现也只有一处。
//
// 走查输入根因 1：认框的规格曾有三份（发送前收草稿、撤回后清原文、发送前判空各一份），每个功能只接
// 其中一份，某家缺哪一份，对应功能就静默失效或退成「请到终端处理」。合并之后靠这里拦着不让第二份
// 再长出来（后端的同款守卫在 meowo-agent 的 composer/tests.rs）。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = resolve(__dirname, "../..");

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "generated" ? [] : sources(path);
    return /\.(ts|tsx)$/.test(name) ? [path] : [];
  });
}
const rel = (path: string) => relative(SRC, path).replace(/\\/g, "/");
const files = sources(SRC).map((path) => ({ name: rel(path), text: readFileSync(path, "utf8") }));

describe("终端输入框规格只有一份", () => {
  it("扫到了源码", () => {
    expect(files.length).toBeGreaterThan(300);
    expect(files.some((file) => file.name === "views/chat/composer.ts")).toBe(true);
  });

  // 拼出来写，免得这份清单自己命中自己。
  const retired = [["draft", "_stash"], ["composer", "_guard"], ["composer", "_prompt"], ["composer", "_border"], ["input", "_clear"],
    ["row", "_prefix"], ["bottom", "_prefix"], ["composer", "Candidate"], ["composer", "Safety"], ["composerDraft", "Text"],
    ["composerDraft", "Block"], ["undoneInput", "Rows"], ["manualSend", "Verdict"]].map((parts) => parts.join(""));
  it("旧的三份规格与各自的认框函数不再出现", () => {
    const hits = files.flatMap((file) => retired.filter((word) => file.text.includes(word)).map((word) => `${file.name}: ${word}`));
    expect(hits).toEqual([]);
  });

  it("契约里输入框规格只有 composer 这一个字段", () => {
    const contract = readFileSync(join(SRC, "generated/contracts/ChatUi.ts"), "utf8");
    expect(contract).toMatch(/\bcomposer: ComposerSpec \| null\b/);
    for (const word of retired.slice(0, 5)) expect(contract).not.toContain(word);
    const undo = readFileSync(join(SRC, "generated/contracts/UndoControl.ts"), "utf8");
    expect(undo).toMatch(/\breturns_input: boolean\b/);
  });

  // 认框 = 读规格里这几条正则去匹配屏幕。只有 composer.ts 能做；别的模块要认输入框一律调它。
  // 测试文件可以摆规格样例（读框单测自带几份裁剪规格），不算认框。
  const framePatterns = /\.(top_pattern|bottom_pattern|row_pattern|input_start_pattern|empty_pattern|tail_pattern|prompt_prefix)\b/;
  it("只有 composer.ts 读规格里的框体规则", () => {
    const readers = files.filter((file) => !/\.test\.tsx?$/.test(file.name) && framePatterns.test(file.text)).map((file) => file.name);
    expect(readers).toEqual(["views/chat/composer.ts"]);
  });

  it("测试夹具不手抄各家的规格，取共享样例那一份", () => {
    const fixture = files.find((file) => file.name === "test/agents.ts")!.text;
    expect(fixture).toContain("composerFrames.specs");
    expect(fixture).not.toMatch(/top_pattern|bottom_pattern|row_pattern/);
  });

  // 按清空键、暂存键、退出键只经 composer.ts 的那几个函数：它们每一下都先认出输入框里有字才按
  //（这些键在空输入框上是退出 / 打开回退菜单）。别处直接拿规格里的键去写终端就绕过了这层核对。
  it("规格里的按键只在 composer.ts 里写进终端", () => {
    const keyReads = /\.(clear_inputs|exit_input)\b|\.stash\.input\b|stash\.input\b/;
    const writers = files.filter((file) => !/\.test\.tsx?$/.test(file.name) && keyReads.test(file.text)).map((file) => file.name);
    expect(writers).toEqual(["views/chat/composer.ts"]);
  });
});
