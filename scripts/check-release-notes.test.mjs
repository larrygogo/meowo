// node --test scripts/check-release-notes.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { check } from "./check-release-notes.mjs";

const dir = new URL("../docs/release-notes/", import.meta.url);

test("所有版本的说明都通过检查", () => {
  const files = readdirSync(dir).filter((f) => /^v\d+\.\d+\.\d+(-[\w.]+)?\.md$/.test(f));
  assert.ok(files.length >= 50);
  for (const f of files) assert.deepEqual(check(readFileSync(new URL(f, dir), "utf8")), [], f);
});

test("拦下开发者内容", () => {
  const dev = [
    "- 前端 111 个测试文件、1331 个用例全过",
    "- 1331 个测试全过，vitest 和 tsc 干净",
    "- 没在真机上验证过；macOS 没在真机验证",
    "- SQLite 依赖升级到修复了 CVE-2026-119375 的版本",
    "- 修掉 RUSTSEC 公告那个问题",
    "- Tauri 升到 2.12，依赖全部升级",
    "- 持续集成改成 Node 26，CI 全绿",
    "- 更新了官网和文档",
    "- 轮询间隔改成 5 秒",
    "- 结束会话更可靠——几秒内一定退出",
  ];
  for (const line of dev) assert.notDeepEqual(check(`一句话。\n\n## 其它\n\n${line}\n`), [], line);
});

test("不误拦正当的用户功能", () => {
  const ok = [
    "- 「更改」面板显示 PR 的 CI 结果和 GitHub Actions 检查",
    "- 文件预览能渲染 README 里的图片",
    "- 终端里 cargo 报错的路径可以点开",
    "- 设置的「关于」页加了打开官网的按钮",
    "- 托管终端里跑 e2e 测试时不再卡住",
    "- 修复了一个安全问题，建议更新",
  ];
  for (const line of ok) assert.deepEqual(check(`一句话。\n\n## 改进\n\n${line}\n`), [], line);
});

test("各种列表写法都计入条数上限", () => {
  for (const marker of ["-", "*", "+", "1."]) {
    const md = "一句话。\n\n" + Array.from({ length: 21 }, (_, i) => `${marker} 第 ${i} 条`).join("\n");
    assert.ok(check(md).some((e) => e.includes("共 21 条")), marker);
  }
});

test("长度超限和 Windows 换行", () => {
  const long = "- " + "很".repeat(51);
  assert.ok(check(`一句话。\r\n\r\n${long}\r\n`).some((e) => e.includes("上限 50")));
  assert.ok(check("开".repeat(121)).some((e) => e.includes("开头一段")));
});
