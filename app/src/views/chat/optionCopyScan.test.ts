/// 扫描：对话窗的渲染与交互代码里不出现各家 CLI 的选项文案。
///
/// 为什么要有：哪个选项是「允许一次」、哪个是「拒绝」，此前由前端用正则认各家的英文文案
/// （`Approve for this session`、`switch to auto mode`），适配知识落在了界面层，还认错过
/// （只认出 `Allow always / Reject` 时「允许一次」发的是 Allow always，做空审查 2026-11-14）。
/// 现在角色由后端插件随识别规则声明，前端只按角色摆按钮。这条测试防止它再长回来：
/// 新接一家 CLI、或某家改了选项文案时，改的应该是 `plugins/<家>/mod.rs` 的选项规则，不是这里。
///
/// 扫的是代码，不是注释：注释里要引用原文说明取证出处。i18n 不在范围内（那是 Meowo 自己的按钮文案）。

import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

import manifest from "../../generated/cli-text-deps.json";

const SRC = path.resolve(__dirname, "../..");

/// 渲染与交互代码：对话窗、chat/ 下的模块、识别层、手机页与公共组件。测试、样本、生成物、文案不算。
function codeFiles(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (["generated", "i18n", "test", "node_modules"].includes(entry.name)) continue;
        walk(full);
      } else if (/\.(ts|tsx)$/.test(entry.name) && !/\.test\.(ts|tsx)$/.test(entry.name) && !/harness/i.test(entry.name)) {
        out.push(full);
      }
    }
  };
  walk(SRC);
  return out;
}

/// 去掉注释：块注释、整行注释、行尾注释（`//` 前面是空白才算，免得误伤字符串里的 `https://`）。
function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .map((line) => (/^\s*\/\//.test(line) ? "" : line.replace(/\s\/\/(?!\/?["'`]).*$/, "")))
    .join("\n");
}

/// 各家审批框、选择框的选项文案（真机屏幕与程序文件里的原文）。只有一个通用词的（Yes、No、Reject）
/// 扫了也证明不了什么，不列。
const OPTION_COPY = [
  // claude
  "switch to auto mode", "use auto mode", "don't ask again", "don’t ask again", "always allow access",
  "tell Claude what to do", "allow all edits during this session", "Tab to amend",
  // kimi
  "Approve once", "Approve for this session", "Reject with feedback", "Type feedback",
  // gemini
  "Allow for this session", "all future sessions", "suggest changes", "Modify with external editor", "Keep trying",
  // opencode
  "Allow always", "Permission required",
  // gemini / opencode 共有
  "Allow once",
];

/// 后端清单里登记的选项规则片段（`option:` 与 `option-ready:` 条目的搜索片段），与上面的手列互补：
/// 插件新增一条选项规则并登记了片段，这里自动跟着扫。
function declaredOptionNeedles(): string[] {
  const agents = (manifest as { agents: { entries: { rule: string; needle: string }[] }[] }).agents;
  return agents.flatMap((agent) => agent.entries)
    .filter((entry) => /^option(?:-ready)?:/.test(entry.rule) && entry.needle.trim().length >= 6)
    .map((entry) => entry.needle);
}

describe("渲染与交互代码里没有各家 CLI 的选项文案", () => {
  const files = codeFiles();

  it("扫描范围包含对话窗、卡片模型、卡片组件与识别层", () => {
    const names = files.map((file) => path.relative(SRC, file).replace(/\\/g, "/"));
    for (const expected of [
      "views/ChatWindow.tsx",
      "views/chat/requestCard.ts",
      "views/chat/ApprovalRequestCard.tsx",
      "views/chat/TerminalRequestCards.tsx",
      "views/chat/terminalApprovalDetails.ts",
      "terminalAttention.ts",
    ]) expect(names).toContain(expected);
  });

  it("后端清单里确实登记了选项规则的片段（清单为空时这条扫描等于没扫）", () => {
    expect(declaredOptionNeedles().length).toBeGreaterThanOrEqual(8);
  });

  it("代码（去掉注释）里不含任何一条选项文案", () => {
    const needles = [...new Set([...OPTION_COPY, ...declaredOptionNeedles()].map((needle) => needle.toLowerCase()))];
    const hits: string[] = [];
    for (const file of files) {
      const code = stripComments(fs.readFileSync(file, "utf8")).toLowerCase();
      for (const needle of needles) {
        if (code.includes(needle)) hits.push(`${path.relative(SRC, file).replace(/\\/g, "/")}: ${needle}`);
      }
    }
    expect(hits, "选项文案属于后端插件的选项规则（plugins/<家>/mod.rs 的 options），前端只认角色").toEqual([]);
  });

  /// 角色归位只看 `role`：卡片模型里不再有拿正则去试选项文字的代码。
  it("卡片模型与卡片组件不拿正则去认选项的文字", () => {
    for (const name of ["views/chat/requestCard.ts", "views/chat/ApprovalRequestCard.tsx", "views/chat/TerminalRequestCards.tsx"]) {
      const code = stripComments(fs.readFileSync(path.join(SRC, name), "utf8"));
      // 正则可以用在别处（工具名是不是 shell、决定词的格式、去框线），但不能拿去试选项的文字。
      expect(code, name).not.toMatch(/\.(?:test|exec|match)\([^)\n]*\blabel\b/);
      expect(code, name).not.toMatch(/\blabel\b[^;\n]*\.(?:match|search)\(/);
      expect(code, name).not.toMatch(/new RegExp/);
    }
  });

  it("去注释的做法本身：注释里的原文不算，代码里的算", () => {
    const sample = [
      "/// 真机：`Approve once` / `Reject`",
      "const a = 1; // kimi 的 Approve for this session",
      "/* Allow always */",
      'const url = "https://example.com"; const b = "Allow once";',
    ].join("\n");
    const code = stripComments(sample);
    expect(code).not.toContain("Approve once");
    expect(code).not.toContain("Approve for this session");
    expect(code).not.toContain("Allow always");
    expect(code).toContain("https://example.com");
    expect(code).toContain("Allow once");
  });
});
