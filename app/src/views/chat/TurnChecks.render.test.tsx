import { afterEach, describe, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ChatItem } from "../../api";
import { TurnChanges } from "./TurnChanges";
import { revealItem, useRememberedOpen } from "./foldMemory";
import { zh } from "../../i18n/zh";
import { ChatActionsContext } from "./chatActions";
import { ChatCwdContext } from "./ToolActivity";

afterEach(cleanup);

const edit = (id: string, path: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Edit", kind: "edit", summary: path, detail: { kind: "edit", path, old: "a", new: "a\nb", replace_all: false, truncated: false } });
const ok = (id: string, text = "ok", isError = false): ChatItem =>
  ({ type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text, is_error: isError });
const run = (id: string, command: string): ChatItem =>
  ({ type: "tool_use", id, timestamp: null, name: "Bash", kind: "run", summary: command, detail: { kind: "command", command, description: null } });

describe("本轮结果卡的检查段", () => {
  it("列出检查与结果；检查之后又改了代码时标出来", () => {
    render(<TurnChanges items={[
      edit("e1", "src/General.tsx"), ok("e1"),
      run("c1", "bunx tsc --noEmit"), ok("c1"),
      run("c2", "bunx vitest run src/views/settings"), ok("c2", " Tests  162 passed (162)"),
      edit("e2", "src/Row.tsx"), ok("e2"),
      edit("e3", "README.md"), ok("e3"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => [row.className, row.querySelector(".chat-turn-check-kind")?.textContent, row.querySelector(".chat-turn-check-result")?.textContent])).toEqual([
      ["chat-turn-check is-pass is-stale", "类型检查", "通过 · 已过时"],
      ["chat-turn-check is-pass is-stale", "测试", "162 个通过 · 已过时"],
    ]);
    // README 是文档，不算「检查之后又改了」。
    expect(screen.getByText(zh.chat.checkStale(["类型检查", "测试"], ["Row.tsx"]))).toBeTruthy();
  });

  // 实拍（用户：「这些真的准吗？为什么经常显示出来」）：lint 点名了三个文件，之后改的是两个测试文件，
  // 不该标过时；第一次 lint 报 balance.tsx 有 1 个问题，接着 `eslint --fix balance.tsx` 通过，红色那行不该还挂着。
  it("lint 只在改到它查过的文件时才过时；报过的问题被同一工具复查通过后不再挂红", () => {
    const eslintOut = [
      "",
      "C:\\Users\\x\\cloud\\front\\src\\pages\\cloud-host\\balance.tsx",
      "  3:1  error  There should be at least one empty line between import groups  import/order",
      "",
      "✖ 1 problem (1 error, 0 warnings)",
      "  1 error and 0 warnings potentially fixable with the `--fix` option.",
    ].join("\n");
    render(<TurnChanges items={[
      edit("e1", "front/src/pages/cloud-host/balance.tsx"), ok("e1"),
      run("c1", "cd front && npx tsc --noEmit -p . 2>&1 | grep -E \"cloud-host\" | head -20; npx eslint src/pages/cloud-host/balance.tsx src/pages/cloud-host/feeCenter.ts src/services/cloud-host.ts 2>&1 | tail -15"), ok("c1", eslintOut),
      run("c2", "cd front && npx eslint --fix src/pages/cloud-host/balance.tsx && npx vitest run src/pages/cloud-host 2>&1 | tail -25"), ok("c2", " Tests  1 failed | 62 passed (63)"),
      edit("e2", "front/src/pages/cloud-host/CloudBalancePage.test.tsx"), ok("e2"),
      edit("e3", "front/src/pages/cloud-host/feeCenter.test.ts"), ok("e3"),
      run("c3", "cd front && npx vitest run src/pages/cloud-host 2>&1 | grep -E \"Tests \""), ok("c3", " Tests  76 passed (76)"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => [row.querySelector(".chat-turn-check-cmd")?.textContent, row.querySelector(".chat-turn-check-result")?.textContent])).toEqual([
      ["npx tsc --noEmit -p . 2>&1", "看不出结果 · 已过时"],
      ["npx eslint --fix src/pages/cloud-host/balance.tsx", "通过"],
      ["npx vitest run src/pages/cloud-host 2>&1", "76 个通过"],
    ]);
    expect(screen.getByText(zh.chat.checkStale(["类型检查"], ["CloudBalancePage.test.tsx", "feeCenter.test.ts"]))).toBeTruthy();
  });

  // 实拍同一轮：`eslint --fix balance.tsx` 通过之后，又用 `sed -i` 改了 balance.tsx。Edit 工具没记下，
  // 只有 Claude 回执末尾的提示知道。
  it("用命令改的文件（sed -i）也算检查之后又改了；git 一来一回、格式化、项目外的临时文件不算", () => {
    const note = (paths: string) => `\n[This command modified 1 file you've previously read: ${paths}. Call Read before editing.]`;
    render(<TurnChanges items={[
      edit("e1", "front/src/pages/cloud-host/balance.tsx"), ok("e1"),
      // 检查自己里头的 --fix 改了文件：先改后查，不算。
      run("c1", "cd front && npx eslint --fix src/pages/cloud-host/balance.tsx"), ok("c1", note("src\\pages\\cloud-host\\balance.tsx")),
      run("c2", "cd front && npx vitest run src/pages/cloud-host 2>&1 | tail -3"), ok("c2", " Tests  5 passed (5)"),
      // 先 stash 掉看测试会不会失败、再 pop 回来：改了又改回来，不算。
      run("g1", "git stash -q; npx vitest run src/x.test.ts; git stash pop -q"), ok("g1", note("src\\pages\\cloud-host\\balance.tsx")),
      // 写到临时目录里的日志：项目外，不算。
      run("s1", "node shoot.mjs > ../../tmp/dev.log"), ok("s1", "\n[This command modified 2 files you've previously read: ..\\..\\tmp\\dev.log, ..\\..\\tmp\\shoot.sh. Call Read before editing.]"),
    ]} />);
    expect(document.querySelectorAll(".chat-turn-check.is-stale")).toHaveLength(0);
    cleanup();
    render(<TurnChanges items={[
      edit("e1", "front/src/pages/cloud-host/balance.tsx"), ok("e1"),
      run("c1", "cd front && npx eslint --fix src/pages/cloud-host/balance.tsx"), ok("c1"),
      run("c2", "cd front && npx eslint src/pages/cloud-host/other.tsx"), ok("c2"),
      run("s1", "cd front && sed -i 's/a/b/' src/pages/cloud-host/balance.tsx && cat src/pages/cloud-host/balance.tsx"), ok("s1", note("src\\pages\\cloud-host\\balance.tsx")),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    // 改到的是第一条 lint 查过的文件；第二条查的是别的文件，不受影响。
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["通过 · 已过时", "通过"]);
    expect(screen.getByText(zh.chat.checkStale(["代码检查"], ["balance.tsx"]))).toBeTruthy();
  });

  // 实拍 2026-11-24（用户：「还是老是会误报」）：多仓根目录 cloud 下，先在 src-tauri 跑 cargo test / clippy、
  // 在根目录跑一个点名的 vitest，之后改了子仓 xbot-admin-front 的 index.tsx 再对它跑 eslint。前面几行全标「已过时」。
  it("改动碰不到的检查不标过时：工具链对不上、改的是别的子项目", () => {
    const root = "C:\\Users\\x\\cloud";
    const items = [
      edit("e0", `${root}\\src-tauri\\src\\engine\\fleet.rs`), ok("e0"),
      run("c1", "cd /c/Users/x/cloud/src-tauri && cargo test --bin xbot-gui engine::fleet 2>&1 | tail -3"), ok("c1", "test result: ok. 4 passed; 0 failed; 0 ignored"),
      run("c2", "cd /c/Users/x/cloud; npx vitest run src/lib/__tests__/error-i18n.test.ts 2>&1 | tail -5"), ok("c2", " Tests  10 passed (10)"),
      edit("e1", `${root}\\xbot-admin-front\\src\\pages\\activation-code\\list\\index.tsx`), ok("e1"),
      run("c3", "cd /c/Users/x/cloud/xbot-admin-front; npx eslint src/pages/activation-code/list/index.tsx"), ok("c3"),
    ];
    render(<ChatCwdContext.Provider value={root}><TurnChanges items={items} /></ChatCwdContext.Provider>);
    expect(document.querySelectorAll(".chat-turn-check.is-stale")).toHaveLength(0);
    cleanup();
    // 反过来：改的是根项目自己的文件、或 Rust 文件，照旧过时。
    render(<ChatCwdContext.Provider value={root}><TurnChanges items={[
      ...items,
      edit("e2", `${root}\\src\\lib\\errors.ts`), ok("e2"),
      edit("e3", `${root}\\src-tauri\\src\\auth.rs`), ok("e3"),
    ]} /></ChatCwdContext.Provider>);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["4 个通过 · 已过时", "10 个通过 · 已过时", "通过"]);
  });

  it("提示里点名的文件和检查命令里的文件同名时带上上一级目录，是同一个文件时不带", () => {
    render(<TurnChanges items={[
      run("c1", "cd front && npx eslint src/pages/activation-code/list/index.tsx"), ok("c1"),
      run("c2", "cd front && npx eslint src/pages/cloud-host/balance.tsx"), ok("c2"),
      edit("e1", "front/src/pages/subscription/index.tsx"), ok("e1"),
      edit("e2", "front/src/pages/cloud-host/balance.tsx"), ok("e2"),
      run("c3", "cd front && npx tsc --noEmit -p ."), ok("c3"),
    ]} />);
    expect(screen.getByText(zh.chat.checkStale(["代码检查"], ["balance.tsx"]))).toBeTruthy();
    cleanup();
    render(<TurnChanges items={[
      run("c1", "cd front && npx eslint src/pages/activation-code/list/index.tsx"), ok("c1"),
      run("c2", "cd front && npx vitest run src/pages/subscription"), ok("c2", " Tests  3 passed (3)"),
      edit("e1", "front/src/pages/subscription/index.tsx"), ok("e1"),
    ]} />);
    expect(screen.getByText(zh.chat.checkStale(["测试"], ["subscription/index.tsx"]))).toBeTruthy();
    cleanup();
    // 实拍：`cd web && tsc hello.ts` 之后改了 web/hello.ts，命令里写的是相对 web 的路径，是同一个文件，不带目录。
    render(<TurnChanges items={[
      run("c1", "cd C:/demo/web && bunx tsc --noEmit hello.ts 2>&1 | tail -4"), ok("c1"),
      edit("e1", "C:\\demo\\web\\hello.ts"), ok("e1"),
    ]} />);
    expect(screen.getByText(zh.chat.checkStale(["类型检查"], ["hello.ts"]))).toBeTruthy();
  });

  // 实拍：先通过，再 `git stash` 藏掉修复跑一遍确认用例会失败，再 pop 回来。卡片写「1 个没过」，用户让 AI 去修。
  it("夹在 git stash 和 stash pop 之间跑的检查（反证）不算这一轮的结果", () => {
    const cmd = "bunx vitest run src/views/chat/TurnChecks.render.test.tsx 2>&1";
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", cmd), ok("c1", " Tests  16 passed (16)"),
      run("c2", `git stash push -q src/a.ts; ${cmd} | Select-Object -Last 6; git stash pop -q; git status --short`), ok("c2", " Tests  1 failed | 15 passed (16)"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["16 个通过"]);
    expect(screen.queryByText(zh.chat.checkFix)).toBeNull();
  });

  it("stash 和 pop 分成几条命令跑也认；藏掉后一直没放回的照常算", () => {
    const cmd = "bunx vitest run src/a.test.ts";
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", cmd), ok("c1", " Tests  3 passed (3)"),
      run("s1", "git stash push -q src/a.ts"), ok("s1"),
      run("c2", cmd), ok("c2", " Tests  1 failed | 2 passed (3)"),
      run("s2", "git stash pop -q && git status --short"), ok("s2"),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check-result")].map((el) => el.textContent)).toEqual(["3 个通过"]);
    cleanup();
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("s1", "git stash"), ok("s1"),
      run("c2", cmd), ok("c2", " Tests  1 failed | 2 passed (3)"),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check-result")].map((el) => el.textContent)).toEqual(["1 个失败"]);
  });

  // 本机记录重放：AI 常把 tsc、eslint、构建接上 `| tail`、或用 ; 串几条，退出码就落不到它们身上，卡片一半以上是「?」。
  it("过了不吭声的工具（tsc、eslint、构建）只被截头尾、输出里没有一点出错的字眼：算通过", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "npx tsc --noEmit 2>&1 | tail -5; npx eslint src/a.ts 2>&1 | head -20; bun run build 2>&1 | tail -2"), ok("c1", "dist/assets/index.js  234.29 kB\n✓ built in 437ms"),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check-result")].map((el) => el.textContent)).toEqual(["通过"]);
  });

  it("挑行的过滤（grep）、输出里有出错字眼、前面 && 的一步可能没过：照旧写看不出", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "npx tsc --noEmit 2>&1 | grep -E \"src/a\" | head"), ok("c1", ""),
      run("c2", "npx eslint src/a.ts | tail -3; echo done"), ok("c2", "  3:1  warning  Unexpected console statement\ndone"),
      run("c3", "node gen.mjs && npx tsc -p app | tail -3; echo done"), ok("c3", "done"),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check-result")].map((el) => el.textContent)).toEqual(["看不出结果", "看不出结果", "看不出结果"]);
  });

  it("heredoc 正文里的命令字样不算跑过检查；喂给 bash 的照算", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "python - <<'EOF'\nopen('CONTRIBUTING.md','w').write('''run bun run build, then cargo clippy -- -D warnings; don't skip''')\nEOF\nnpx tsc --noEmit | tail -3"), ok("c1", ""),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check")].map((row) => row.querySelector(".chat-turn-check-kind")?.textContent)).toEqual(["类型检查"]);
    cleanup();
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "bash <<'EOF'\nnpx eslint src/a.ts\nEOF"), ok("c1", ""),
    ]} />);
    expect([...document.querySelectorAll(".chat-turn-check-kind")].map((el) => el.textContent)).toEqual(["代码检查"]);
  });

  it("lint 查过的文件之后又改了：照样标过时；没认出是哪个文件出问题的失败照样留着", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "npx eslint src/a.ts src/b.ts"), ok("c1", "✖ 2 problems (2 errors, 0 warnings)", true),
      run("c2", "npx eslint src/c.ts"), ok("c2"),
      edit("e2", "src/c.ts"), ok("e2"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["2 个问题", "通过 · 已过时"]);
    expect(screen.getByText(zh.chat.checkStale(["代码检查"], ["c.ts"]))).toBeTruthy();
  });

  it("类型检查之后改了文件、只重跑了测试：类型检查那一行标「已过时」，测试不标", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "bunx tsc --noEmit"), ok("c1"),
      edit("e2", "src/b.ts"), ok("e2"),
      run("c2", "bunx vitest run"), ok("c2", "Tests  5 passed (5)"),
    ]} />);
    const rows = [...document.querySelectorAll(".chat-turn-check")];
    expect(rows.map((row) => row.querySelector(".chat-turn-check-result")?.textContent)).toEqual(["通过 · 已过时", "5 个通过"]);
    expect(screen.getByText(zh.chat.checkStale(["类型检查"], ["b.ts"]))).toBeTruthy();
  });

  it("提示写成人话：哪项检查之后改了哪个文件；命令显示真正跑检查的那段，不显示开头的变量赋值", () => {
    render(<TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", 'SP="C:/Users/x/Temp/claude/abc"; cd app && FORCE_COLOR=0 bunx vitest run src/derive'), ok("c1", "Tests  1 passed (1)"),
      edit("e2", "src/derive.ts"), ok("e2"),
    ]} />);
    expect(document.querySelector(".chat-turn-check-cmd")?.textContent).toBe("bunx vitest run src/derive");
    expect(document.querySelector(".chat-turn-checks-text")?.textContent).toBe("测试跑完后又改了 derive.ts，这处改动还没验证过");
  });

  it("「让 AI 重跑」只把要求填进输入框：点名改过的文件和要重跑的命令；没跑检查时给「让 AI 跑检查」", () => {
    const filled: string[] = [];
    const actions = { sendText: async () => true, fillPrompt: (text: string) => { filled.push(text); } };
    render(<ChatActionsContext.Provider value={actions}><TurnChanges items={[
      edit("e1", "src/a.ts"), ok("e1"),
      run("c1", "bunx tsc --noEmit && bunx vitest run"), ok("c1", "Tests  3 passed (3)"),
      edit("e2", "src/derive.ts"), ok("e2"),
    ]} /></ChatActionsContext.Provider>);
    fireEvent.click(screen.getByText(zh.chat.checkRerun));
    expect(filled).toEqual([zh.chat.checkRerunPrompt(["src/derive.ts"], ["bunx tsc --noEmit", "bunx vitest run"])]);
    cleanup();
    render(<ChatActionsContext.Provider value={actions}><TurnChanges items={[edit("e1", "src/a.ts"), ok("e1")]} /></ChatActionsContext.Provider>);
    fireEvent.click(screen.getByText(zh.chat.checkRunNow));
    expect(filled[1]).toBe(zh.chat.checkRunPrompt);
  });

  it("没过、之后也没改代码：出「让 AI 修」；没过但之后改过代码的归到「重跑」", () => {
    const filled: string[] = [];
    const actions = { sendText: async () => true, fillPrompt: (text: string) => { filled.push(text); } };
    render(<ChatActionsContext.Provider value={actions}><TurnChanges items={[
      run("c1", "cargo test -p meowo-agent"), ok("c1", "test result: FAILED. 40 passed; 3 failed;", true),
    ]} /></ChatActionsContext.Provider>);
    expect(document.querySelector(".chat-turn-checks-note.is-err .chat-turn-checks-text")?.textContent).toBe("测试没过，之后也没再改代码");
    fireEvent.click(screen.getByText(zh.chat.checkFix));
    expect(filled).toEqual([zh.chat.checkFixPrompt(["cargo test -p meowo-agent"])]);
    cleanup();
    render(<ChatActionsContext.Provider value={actions}><TurnChanges items={[
      run("c1", "cargo test"), ok("c1", "test result: FAILED. 40 passed; 3 failed;", true),
      edit("e1", "src/lib.rs"), ok("e1"),
    ]} /></ChatActionsContext.Provider>);
    expect(screen.queryByText(zh.chat.checkFix)).toBeNull();
    expect(screen.getByText(zh.chat.checkRerun)).toBeTruthy();
  });

  it("合成一行的类型检查 + 测试，之后只重跑了测试：这一行只剩类型检查，重跑要求只列 tsc", () => {
    const filled: string[] = [];
    const actions = { sendText: async () => true, fillPrompt: (text: string) => { filled.push(text); } };
    render(<ChatActionsContext.Provider value={actions}><TurnChanges items={[
      run("c1", "bunx tsc --noEmit && bunx vitest run"), ok("c1", "Tests  3 passed (3)"),
      edit("e1", "src/b.ts"), ok("e1"),
      run("c2", "bunx vitest run"), ok("c2", "Tests  3 passed (3)"),
    ]} /></ChatActionsContext.Provider>);
    expect([...document.querySelectorAll(".chat-turn-check-cmd")].map((node) => node.textContent)).toEqual(["bunx tsc --noEmit", "bunx vitest run"]);
    fireEvent.click(screen.getByText(zh.chat.checkRerun));
    expect(filled).toEqual([zh.chat.checkRerunPrompt(["src/b.ts"], ["bunx tsc --noEmit"])]);
  });

  it("对话窗之外（没提供 fillPrompt）不出按钮", () => {
    render(<TurnChanges items={[edit("e1", "src/a.ts"), ok("e1")]} />);
    expect(screen.queryByText(zh.chat.checkRunNow)).toBeNull();
  });

  it("改了代码却没跑检查：如实写；只改文档：不提", () => {
    render(<TurnChanges items={[edit("e1", "src/a.ts"), ok("e1")]} />);
    expect(screen.getByText(zh.chat.checkNone)).toBeTruthy();
    cleanup();
    render(<TurnChanges items={[edit("e1", "docs/guide.md"), ok("e1")]} />);
    expect(screen.queryByText(zh.chat.checkNone)).toBeNull();
  });

  it("只跑了检查、没改文件：卡片只有检查段，失败写出数量", () => {
    render(<TurnChanges items={[run("c1", "cargo test"), ok("c1", "test result: FAILED. 40 passed; 3 failed;", true)]} />);
    expect(screen.queryByText(zh.chat.turnChangedFiles(0))).toBeNull();
    const row = document.querySelector(".chat-turn-check")!;
    expect(row.classList.contains("is-fail")).toBe(true);
    expect(row.querySelector(".chat-turn-check-result")?.textContent).toBe("3 个失败");
  });

  it("没有改动也没有检查：不出卡片", () => {
    render(<TurnChanges items={[run("c1", "git status"), ok("c1")]} />);
    expect(document.querySelector(".chat-turn-changes")).toBeNull();
  });
});

function Fold({ id, ids }: { id: string; ids: string[] }) {
  const [open] = useRememberedOpen(`fold:${id}`, false, (target) => ids.includes(target));
  return <div data-testid={id}>{open ? "open" : "closed"}</div>;
}

describe("本轮结果卡的文件列表", () => {
  it("代码文件排前，文档和 .lead 任务记录排后：前三行里能看到真正改的代码", () => {
    render(<TurnChanges items={[
      edit("e1", ".lead/task/SPEC.md"), ok("e1"),
      edit("e2", ".lead/task/acceptance.json"), ok("e2"),
      edit("e3", ".lead/task/progress.md"), ok("e3"),
      edit("e4", "src/derive.ts"), ok("e4"),
      edit("e5", "README.md"), ok("e5"),
    ]} />);
    const names = [...document.querySelectorAll(".chat-turn-changes-name")].map((node) => node.textContent);
    expect(names).toEqual(["derive.ts", "SPEC.md", "acceptance.json"]);
  });
});

describe("跳到这条命令", () => {
  it("已挂载的折叠：装着目标的展开，别的不动；按键名点到的也展开", () => {
    render(<><Fold id="a" ids={["t1"]} /><Fold id="b" ids={["t2"]} /><Fold id="c" ids={[]} /></>);
    act(() => revealItem("t1", ["fold:c"]));
    expect(screen.getByTestId("a").textContent).toBe("open");
    expect(screen.getByTestId("b").textContent).toBe("closed");
    expect(screen.getByTestId("c").textContent).toBe("open");
  });

  it("随后才挂载的内层折叠：挂载时认出目标就直接展开", () => {
    act(() => revealItem("t9", []));
    render(<Fold id="inner" ids={["t9"]} />);
    expect(screen.getByTestId("inner").textContent).toBe("open");
  });
});
