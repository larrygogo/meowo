// 「操作进行中别用 disabled」的防回归（做空第一轮 #4，2026-11-12）。
// 第三轮设计准则早就定了「进行中用 aria-disabled + 处理函数拦截」，但只在审批卡、发送钮、
// 「更改」面板几处落了地，做空复查时标题栏「标记完成」「结束会话」、门卡「恢复会话」「接管」、
// 「加载更早」、账号页等十几处仍是 disabled={busy}：点下去的瞬间焦点掉到 body。
// 这里一是钉住 busyProps 的行为，二是扫源码，拦新增的 disabled={…busy|sending|pending…}。
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { busyProps } from "./busyButton";

describe("busyProps", () => {
  it("进行中：标 aria-disabled、不调处理函数、焦点留在按钮上", () => {
    const onClick = vi.fn();
    function Demo() {
      const [busy, setBusy] = useState(false);
      return <button type="button" {...busyProps(busy, () => { onClick(); setBusy(true); })}>go</button>;
    }
    render(<Demo />);
    const btn = screen.getByRole<HTMLButtonElement>("button", { name: "go" });
    btn.focus();
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
    // 关键断言：没有真 disabled，焦点不会被踢到 body。
    expect(btn.disabled).toBe(false);
    expect(btn.getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(btn);
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("空闲时不挂 aria-disabled 属性", () => {
    render(<button type="button" {...busyProps(false, () => {})}>idle</button>);
    expect(screen.getByRole("button", { name: "idle" }).hasAttribute("aria-disabled")).toBe(false);
  });

  it("进行中的 submit 按钮不触发表单提交", () => {
    const onSubmit = vi.fn((e: { preventDefault(): void }) => e.preventDefault());
    render(<form onSubmit={onSubmit}><button type="submit" {...busyProps(true)}>send</button></form>);
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(onSubmit).not.toHaveBeenCalled();
  });
});

// ---- 源码扫描 ----

const SRC = __dirname;
/** 表达式里出现这些「进行中」字样就算进行中型禁用。 */
const BUSY_WORD = /(busy|sending|pending|loading|saving|starting|stopping|installing|cleaning|checking|answering|refreshing|repairing|interrupting|archiving|submitting|materializing|adding|working|inflight)/i;

/**
 * 存量合理例外：`文件::表达式`（表达式去空白后比对）→ 理由。新增条目要写清楚为什么焦点不会被踢走。
 */
const ALLOW: Record<string, string> = {
  // 「在新 worktree 里开」那组输入框/勾选：启动由发送按钮或输入框里的回车触发，焦点不在这组控件上；
  // 启动成功窗口即切走。输入框也不适合 aria-disabled（仍可输入），保留真 disabled。
  "views/chat/NewSessionPage.tsx::busy": "WorktreeOption 启动中锁表单，焦点不在其上",
  "views/NewSessionPanel.tsx::busy": "同上（面板版新建会话）",
  // 别的账号正在登录时锁住本行「登录」：被锁的不是用户刚点的那颗（刚点的那颗变成「取消登录」）。
  "views/settings/AccountSection.tsx::anotherLoginPending": "锁的是其他行，不是刚点的按钮",
  // busy 只作豁免：启动中这一项恒为 false，进行中由同一元素上的 busyProps 标 aria-disabled。
  "views/NewSessionPanel.tsx::!canLaunch&&!busy": "启动中恰好不 disabled",
  // sending 只作「输入为空才禁」的豁免：发送中整项为 false，发送钮靠 aria-disabled（同一元素）。
  "views/ChatWindow.tsx::composerDisabled||(!stopMode&&!sending&&!prompt.trim()&&attachments.length===0)": "发送中恰好不 disabled",
};

function tsxFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === "generated" || name === "node_modules" || name === "test") continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) tsxFiles(p, out);
    else if (name.endsWith(".tsx") && !name.includes(".test.")) out.push(p);
  }
  return out;
}

/** 取 `disabled={…}` 的表达式（按花括号配对，表达式可跨行）。aria-disabled 不算。 */
function disabledExprs(text: string): { expr: string; line: number }[] {
  const out: { expr: string; line: number }[] = [];
  for (const m of text.matchAll(/(?<![-\w])disabled=\{/g)) {
    const open = m.index! + m[0].length - 1;
    let depth = 0;
    let end = open;
    for (let i = open; i < text.length; i++) {
      if (text[i] === "{") depth++;
      else if (text[i] === "}" && --depth === 0) { end = i; break; }
    }
    out.push({ expr: text.slice(open + 1, end), line: text.slice(0, m.index).split("\n").length });
  }
  return out;
}

describe("进行中不用 disabled（源码扫描）", () => {
  const hits: { key: string; at: string }[] = [];
  for (const file of tsxFiles(SRC)) {
    const rel = relative(SRC, file).split("\\").join("/");
    for (const { expr, line } of disabledExprs(readFileSync(file, "utf8"))) {
      if (!BUSY_WORD.test(expr)) continue;
      hits.push({ key: `${rel}::${expr.replace(/\s+/g, "")}`, at: `${rel}:${line}  disabled={${expr.trim()}}` });
    }
  }

  it("新增的进行中禁用一律改用 busyProps（src/busyButton.ts）", () => {
    expect(hits.filter((h) => !(h.key in ALLOW)).map((h) => h.at)).toEqual([]);
  });

  it("白名单没有失效条目（改掉了就从白名单删）", () => {
    const seen = new Set(hits.map((h) => h.key));
    expect(Object.keys(ALLOW).filter((k) => !seen.has(k))).toEqual([]);
  });

  it("扫描本身有效：认得出 disabled={busy} 与跨行表达式", () => {
    expect(disabledExprs(`<b disabled={busy} aria-disabled={x} />`).map((e) => e.expr)).toEqual(["busy"]);
    expect(disabledExprs(`<b disabled={a ||\n  sending} />`)[0]).toEqual({ expr: "a ||\n  sending", line: 1 });
  });
});
