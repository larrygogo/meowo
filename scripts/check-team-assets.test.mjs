// node --test scripts/check-team-assets.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { scan, DEFAULT_ROOT } from "./check-team-assets.mjs";

function fixture(files) {
  const root = mkdtempSync(join(tmpdir(), "team-assets-"));
  for (const [name, text] of Object.entries(files)) {
    mkdirSync(join(root, name, ".."), { recursive: true });
    writeFileSync(join(root, name), text);
  }
  return root;
}

test("仓库里的团队托管文件按通用规则 0 命中", () => {
  assert.deepEqual(scan(DEFAULT_ROOT), []);
});

test("占位符和示例写法不算命中", () => {
  const root = fixture({ "a.md": String.raw`读 {{PERSONAL_DIR}}/config.json；路径示例 C:\Users\<名字>\x 和 /Users/<名字>/x；/path/to/taste.md；node server.js` });
  assert.deepEqual(scan(root, ["nod"]), []);
});

test("本机路径、出处日期、邮箱、私有词都会被抓到", () => {
  const root = fixture({
    "agents/a.md": String.raw`读 C:\Users\someone\Desktop\workspace\x\craft.md`,
    "b.md": "（用户 2026-11-24 同意）",
    "c.json": "{\"who\": \"a@b.com\", \"home\": \"/home/someone/x\"}",
    "d.md": "项目 Nod 的教训；Acme-Portal 的规矩",
  });
  const rules = scan(root, ["nod", "acme-portal"]).map((h) => `${h.file.replaceAll("\\", "/")}:${h.rule}`).sort();
  assert.deepEqual(rules, [
    "agents/a.md:个人工作区路径",
    "agents/a.md:本机用户目录（Windows）",
    "b.md:出处日期",
    "c.json:本机用户目录（Linux）",
    "c.json:邮箱",
    "d.md:私有词表第 1 个词",
    "d.md:私有词表第 2 个词",
  ]);
});

test("私有词命中只报序号，不把词本身打进公开的 CI 日志", () => {
  const root = fixture({ "a.md": "secret-client 的规则" });
  const [hit] = scan(root, ["secret-client"]);
  assert.equal(hit.text, "");
  assert.ok(!JSON.stringify(hit).includes("secret-client"));
});
