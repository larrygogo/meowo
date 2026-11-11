// 灵动岛读屏 / 键盘 / 减少动效 / 长文本（UI 优化 acceptance #16 #21 #33 #34，findings i5~i16）。
import { describe, it, expect, vi, afterEach } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { render, cleanup, fireEvent, screen, act } from "@testing-library/react";
import { Island } from "../Island";
import { IslandPanel } from "./IslandPanel";
import { Eyes, UsageRing, USAGE_FLIP_MS } from "./islandParts";
import type { Item } from "../sticker/types";
import { zh } from "../../i18n/zh";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  invoke.mockClear();
});

let nextId = 1;
const mk = (over: Partial<Item> & { status?: string; lastEventAt?: number } = {}): Item => {
  const id = nextId++;
  const { status = "running", lastEventAt = 1_000, ...rest } = over;
  return {
    session: { id, project_id: 1, cc_session_id: `s-${id}`, status, started_at: id, last_event_at: lastEventAt, ended_at: null },
    project_name: "proj", task_title: `会话 ${id}`, current_activity: null, column: "doing",
    todo_done: 0, todo_total: 0, todos: [], pid: null, archived: false,
    connected: true, pty_managed: true, endable: true, background: false,
    errored: false, error_label: null, error_raw: null, busy_subagents: 0,
    preview: null, profile_name: null, foreign: false, pending_review: null,
    provider: "claude",
    screen_state: status === "running" ? "working" : "idle",
    ...rest,
  } as unknown as Item;
};

const noop = () => {};
const renderIsland = (props: Partial<Parameters<typeof Island>[0]> = {}) =>
  render(
    <Island
      sessions={[]}
      compactStyle="counts"
      alert={null}
      usages={[]}
      onMeasure={noop}
      onActivate={noop}
      onOpenSession={noop}
      onPointerDownDrag={noop}
      {...props}
    />,
  );
const renderPanel = (sessions: Item[], onOpenSession: (id: number) => void = noop) =>
  render(
    <IslandPanel sessions={sessions} usage={{}} quotaProviders={[]} onOpenSession={onOpenSession} onMeasure={noop} onPointerDownDrag={noop} />,
  );

/** 打开系统「减少动效」：只认 prefers-reduced-motion 这一条查询。 */
const reduceMotion = () =>
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("prefers-reduced-motion"),
    media: query,
    addEventListener: noop,
    removeEventListener: noop,
    addListener: noop,
    removeListener: noop,
    onchange: null,
    dispatchEvent: () => false,
  }));

describe("#16 读屏与键盘", () => {
  it("收起态的名字汇总各状态的会话数，不再是固定的一句话", () => {
    renderIsland({ sessions: [mk(), mk(), mk({ pending_review: "approval" })] });
    const notch = document.querySelector(".isl-notch")!;
    const label = notch.getAttribute("aria-label")!;
    expect(label).not.toContain("点击展开会话列表");
    expect(label).toContain(zh.island.ariaCount(2, zh.island.toneLabel.running));
    expect(label).toContain(zh.island.ariaCount(1, zh.island.toneLabel.pending));
  });

  it("提醒态的名字说清是哪个会话、什么事、按 Enter 做什么，并经 live region 播报", () => {
    const onOpenSession = vi.fn();
    renderIsland({
      sessions: [mk()],
      alert: { sessionId: 9, kind: "approval", title: "修登录页", project: "web", detail: null },
      onOpenSession,
    });
    // 提醒里还有「打开」「关闭」按钮，整块不能再是 role=button（按钮里套按钮，读屏念不出里面的）：
    // 名字挂在正文那块按钮上。
    const main = screen.getByRole("button", { name: zh.island.alertAria("修登录页", zh.pending.approval) });
    expect(main.querySelector("button")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain("修登录页");
    fireEvent.keyDown(main, { key: "Enter" });
    expect(onOpenSession).toHaveBeenCalledWith(9);
    onOpenSession.mockClear();
    fireEvent.keyDown(main, { key: " " });
    expect(onOpenSession).toHaveBeenCalledWith(9);
  });

  it("面板会话行认空格键", () => {
    const onOpenSession = vi.fn();
    const item = mk();
    renderPanel([item], onOpenSession);
    fireEvent.keyDown(document.querySelector(".isl-row")!, { key: " " });
    expect(onOpenSession).toHaveBeenCalledWith(item.session.id);
  });

  it("分页圆点是普通按钮组：不冒充 tablist，当前页用 aria-current 标出", () => {
    renderPanel(Array.from({ length: 7 }, () => mk()));
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryAllByRole("tab")).toHaveLength(0);
    const dots = screen.getAllByRole("button", { name: /^第 \d 页$/ });
    expect(dots).toHaveLength(3);
    expect(dots[0].getAttribute("aria-current")).toBe("page");
    expect(dots[1].hasAttribute("aria-current")).toBe(false);
    fireEvent.click(dots[1]);
    expect(screen.getByRole("button", { name: zh.island.page(2) }).getAttribute("aria-current")).toBe("page");
  });
});

describe("#21 减少动效", () => {
  it("眼睛不跟鼠标、不眨眼、不做小动作：一分钟里画面不变，也不轮询光标", () => {
    reduceMotion();
    vi.useFakeTimers();
    const { container } = render(<Eyes tone="running" />);
    const before = container.innerHTML;
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(container.innerHTML).toBe(before);
    expect(invoke).not.toHaveBeenCalledWith("island_cursor");
  });

  it("额度环不轮播，停在第一家", () => {
    reduceMotion();
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 40 }, { provider: "codex", pct: 70 }]} />);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS * 3); });
    expect(container.querySelector(".isl-usage-wrap")!.getAttribute("data-provider")).toBe("claude");
  });

  it("没开减少动效时照旧轮播（对照）", () => {
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 40 }, { provider: "codex", pct: 70 }]} />);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS); });
    expect(container.querySelector(".isl-usage-wrap")!.getAttribute("data-provider")).toBe("codex");
  });
});

describe("#34 长文本与圆点", () => {
  it("出错提醒的状态词固定，详情进可省略的 detail 并带 title", () => {
    const detail = "API Error: 529 overloaded_error · the model is overloaded, please retry later";
    renderIsland({ sessions: [mk()], alert: { sessionId: 1, kind: "error", title: "修测试", project: "api", detail } });
    expect(document.querySelector(".isl-alert-sub .isl-state")!.textContent).toBe(zh.island.toneLabel.error);
    const el = document.querySelector(".isl-alert-detail")!;
    expect(el.textContent).toBe(detail);
    expect(el.getAttribute("title")).toBe(detail);
  });

  it("圆点样式超过 6 个折成 +N，按紧急程度排，+N 按折进去的最急状态上色", () => {
    const sessions = [
      mk({ lastEventAt: 9 }), mk({ lastEventAt: 8 }), mk({ lastEventAt: 7 }), mk({ lastEventAt: 6 }),
      mk({ status: "waiting", lastEventAt: 5 }), mk({ status: "waiting", lastEventAt: 4 }),
      mk({ errored: true, lastEventAt: 3 }), mk({ pending_review: "approval", lastEventAt: 2 }),
    ];
    renderIsland({ sessions, compactStyle: "minimal" });
    const dots = [...document.querySelectorAll(".isl-dots .isl-dot")];
    expect(dots).toHaveLength(6);
    expect(dots[0].classList.contains("is-pending")).toBe(true);
    expect(dots[1].classList.contains("is-error")).toBe(true);
    const more = document.querySelector(".isl-dots .isl-more")!;
    expect(more.textContent).toBe("+2");
    expect(more.classList.contains("is-running")).toBe(true);
  });

  it("三处长标题都带 title：聚焦样式、面板会话行、提醒", () => {
    const long = "把支付回调里重复扣款的问题彻底查清楚并补上幂等键和回归测试，顺便整理日志";
    const item = mk({ task_title: long, pending_review: "approval" });
    renderIsland({ sessions: [item], compactStyle: "focus" });
    expect(document.querySelector(".isl-focus-title")!.getAttribute("title")).toBe(long);
    cleanup();
    renderPanel([item]);
    expect(document.querySelector(".isl-row-title")!.getAttribute("title")).toBe(long);
    cleanup();
    renderIsland({ sessions: [item], alert: { sessionId: item.session.id, kind: "approval", title: long, project: "p", detail: null } });
    expect(document.querySelector(".isl-alert-title")!.getAttribute("title")).toBe(long);
  });
});

describe("#33 一种状态一种颜色", () => {
  // 只看灵动岛的样式块：所有表示「待处理」「等待输入」的颜色声明各自只引用一个变量。
  // 先去掉注释：注释夹在规则之间会被当成选择器的一部分。
  const css = readFileSync(join(__dirname, "../../styles.css"), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
  const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .map(([, sel, body]) => ({ sel: sel.trim(), body }))
    .filter(({ sel }) => /\.isl-/.test(sel) && !sel.startsWith("@"));
  const colorsOf = (tone: string) => {
    const tonePat = new RegExp(`(is-${tone}\\b|data-tone="${tone}")`);
    return rules
      .filter(({ sel }) => tonePat.test(sel))
      .flatMap(({ body }) => [...body.matchAll(/(?:^|;)\s*(color|background|--isl-strip-tone|--isl-dot-c)\s*:\s*([^;]+)/g)].map((m) => m[2].trim()))
      .filter((v) => v !== "transparent");
  };

  // 每条颜色声明引用的变量只能是该状态那一个（底色可以是它和黑的混色），不许出现别的状态色或写死的色值。
  const refsOf = (value: string) => [...value.matchAll(/var\((--[a-z0-9-]+)\)/g)].map((m) => m[1]);
  const expectOnly = (tone: string, allowed: string) => {
    const colors = colorsOf(tone);
    expect(colors.length).toBeGreaterThan(3);
    for (const value of colors) {
      expect({ value, refs: refsOf(value) }).toEqual({ value, refs: [allowed] });
    }
  };

  it("待处理只用 --isl-pending", () => expectOnly("pending", "--isl-pending"));

  it("等待输入只用 --isl-waiting", () => expectOnly("waiting", "--isl-waiting"));

  it("两个状态色与对话窗侧栏的状态点同源", () => {
    expect(css).toMatch(/--isl-pending:\s*var\(--cc-warn-vivid\)/);
    expect(css).toMatch(/--isl-waiting:\s*var\(--cc-warn-vivid\)/);
  });

  it("面板标题强调色不用状态色", () => {
    const accent = rules.find(({ sel }) => sel === ".isl-accent")!;
    expect(accent.body).not.toMatch(/--isl-(warn|pending|waiting|err|ok)\b/);
  });
});
