// Lead 面板展开到右侧面板（页签）：入口、页签、停靠区让位、关页签复原、远程与无任务时不出入口。
// 拿真实的 usePanel + DockWork + SidePanel 拼出对话窗里的那段接线（leadTabLink 与对话窗共用），
// 只把后端调用和 jsdom 里起不来的终端、浏览器、diff 换成桩。
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useMemo } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LeadGateDto, LeadTaskDto } from "../../api";

const snapshot = vi.fn<(cwd: string, session?: string | null) => Promise<LeadTaskDto | null>>();
const answer = vi.fn<(cwd: string, gateId: string, choice: string, note: string, version?: string) => Promise<void>>();
vi.mock("../../api", async (importActual) => ({
  ...(await importActual<typeof import("../../api")>()),
  leadTaskSnapshot: (cwd: string, session?: string | null) => snapshot(cwd, session),
  answerLeadGate: (...args: Parameters<typeof answer>) => answer(...args),
  listShellTerminals: () => Promise.resolve([]),
  listBrowserPanes: () => Promise.resolve([]),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("./ShellTerminal", () => ({ ShellTerminal: () => <div /> }));
vi.mock("./BrowserPane", () => ({ BrowserPane: () => <div /> }));
vi.mock("./lazyDiff", () => ({ LazyChangesView: () => <div />, LazyFilesView: () => <div /> }));

const { DockWork, leadTabLink } = await import("./DockWork");
const { SidePanel } = await import("./SidePanel");
const { usePanel } = await import("./usePanel");
const { useLeadTask } = await import("./LeadPanel");
const { EMPTY_PANEL } = await import("./panelTabs");

const history = { plan: undefined, agentModes: [], pendingReview: null, todos: [] };
const CWD = "C:\\repo";

function task(over: Partial<LeadTaskDto> = {}): LeadTaskDto {
  return {
    slug: "20261118-demo",
    phase: "implement",
    spec: "# 侧边面板看 Lead（panel-lead-tab）",
    checks: [
      { id: "1", check: "入口可点", passes: true, waived: null, leader: false, blocked: null },
      { id: "2", check: "页签可滚动", passes: false, waived: null, leader: false, blocked: null },
    ],
    decisions: "",
    progress: "- 写完入口",
    gate: null,
    ...over,
  };
}

const gateA: LeadGateDto = {
  id: "A-1", kind: "A", title: "确认方案", summary: "按方案一做", options: [], status: "pending", answer: null, version: "v1",
};

/** 对话窗里那段接线的缩影：同一份 Lead 任务交给停靠区和侧边面板，leadTabLink 算页签状态。 */
function Harness({ remote = false, sessionId = 1 }: { remote?: boolean; sessionId?: number }) {
  const panel = usePanel(sessionId, CWD, () => {});
  const [taskNow, refresh, failure] = useLeadTask(CWD, "cc-1");
  const lead = useMemo(() => ({ task: taskNow, refresh, failure }), [taskNow, refresh, failure]);
  const leadTab = leadTabLink(panel.state, panel.state.visible, panel.actions.openLead, remote);
  return (
    <>
      <div className="chat-dock" data-testid="dock">
        <DockWork sessionId={sessionId} cwd={CWD} ccSessionId="cc-1" items={[]} history={history} loading={false} lead={lead} leadTab={leadTab} />
      </div>
      {!remote && (
        <div data-testid="side">
          <SidePanel
            state={panel.state} actions={panel.actions} shells={panel.shells} panes={panel.panes} cwd={CWD} summary={null} width={420}
            comments={[]} onAddComment={() => {}} onRemoveComment={() => {}} touchedPaths={[]} turnTouchedPaths={[]} browserSuggestions={[]}
            selectedRoot={null} onSelectRoot={() => {}} onChanged={() => {}} lead={lead}
          />
        </div>
      )}
    </>
  );
}

const dock = () => within(screen.getByTestId("dock"));
const side = () => within(screen.getByTestId("side"));

beforeEach(() => {
  snapshot.mockReset();
  answer.mockReset();
  localStorage.clear();
});
afterEach(cleanup);

describe("Lead 面板在侧边面板打开", () => {
  it("点入口：面板出现 Lead 页签并激活，停靠浮层收起；关卡卡片挪进页签，停靠区不再有第二张", async () => {
    snapshot.mockResolvedValue(task({ gate: gateA }));
    render(<Harness />);
    const pill = await dock().findByRole("button", { name: /Lead/ });
    // 关卡在等：页签开之前，停靠区照常有关卡卡片。
    expect(document.querySelectorAll(".lead-gate-card")).toHaveLength(1);
    expect(screen.getByTestId("dock").querySelector(".lead-gate-card")).toBeTruthy();
    // 面板一开始是收起的（没有页签）。
    expect(document.querySelector(".side-panel.is-collapsed")).toBeTruthy();

    fireEvent.click(pill);
    fireEvent.click(dock().getByRole("button", { name: "在侧边面板打开" }));

    const tab = side().getByRole("tab", { name: "Lead" });
    expect(tab.getAttribute("aria-selected")).toBe("true");
    expect(document.querySelector(".side-panel.is-collapsed")).toBeNull();
    // 停靠浮层收起，页签里是完整面板（标题、页签分段、验收项都在）。
    expect(screen.getByTestId("dock").querySelector(".dock-work-panel")).toBeNull();
    const body = screen.getByTestId("side").querySelector(".side-panel-lead") as HTMLElement;
    expect(body.querySelector(".panel-lead.is-in-panel")).toBeTruthy();
    expect(within(body).getByText("侧边面板看 Lead")).toBeTruthy();
    expect(within(body).getByText("页签可滚动")).toBeTruthy();
    // 页签里不再有「在侧边面板打开」和停靠浮层的 ×。
    expect(within(body).queryByRole("button", { name: "在侧边面板打开" })).toBeNull();
    // 关卡卡片只剩页签里这一张；页签就在眼前，停靠区连提醒行也不留。
    expect(document.querySelectorAll(".lead-gate-card")).toHaveLength(1);
    expect(body.querySelector(".lead-gate-card")).toBeTruthy();
    expect(screen.getByTestId("dock").querySelector(".lead-gate-moved")).toBeNull();

    // 页签里同样能作答。
    answer.mockResolvedValue();
    fireEvent.click(within(body).getByRole("button", { name: "确认，开工" }));
    await waitFor(() => expect(answer).toHaveBeenCalledWith(CWD, "A-1", "confirm", "", "v1"));
  });

  it("面板收起后停靠区留一行提醒，点它或点胶囊都回到 Lead 页签", async () => {
    snapshot.mockResolvedValue(task({ gate: gateA }));
    render(<Harness />);
    fireEvent.click(await dock().findByRole("button", { name: /Lead/ }));
    fireEvent.click(dock().getByRole("button", { name: "在侧边面板打开" }));

    fireEvent.click(side().getByRole("button", { name: "收起面板" }));
    expect(document.querySelector(".side-panel.is-collapsed")).toBeTruthy();
    // 收起的面板不挂关卡卡片（藏着的卡没人看得见，读屏还会播报它）；停靠区也不放回卡片，只留一行。
    expect(document.querySelectorAll(".lead-gate-card")).toHaveLength(0);
    const moved = screen.getByTestId("dock").querySelector(".lead-gate-moved") as HTMLElement;
    expect(moved.textContent).toContain("已在侧边面板打开");

    fireEvent.click(moved);
    expect(document.querySelector(".side-panel.is-collapsed")).toBeNull();
    expect(side().getByRole("tab", { name: "Lead" }).getAttribute("aria-selected")).toBe("true");

    // 胶囊此时是「去页签」：不弹停靠浮层。
    fireEvent.click(side().getByRole("button", { name: "收起面板" }));
    const pill = dock().getAllByRole("button").find((b) => b.classList.contains("ui-pill")) as HTMLElement;
    expect(pill.getAttribute("data-tip")).toBe("已在侧边面板打开，点击查看");
    fireEvent.click(pill);
    expect(screen.getByTestId("dock").querySelector(".dock-work-panel")).toBeNull();
    expect(document.querySelector(".side-panel.is-collapsed")).toBeNull();
  });

  it("再点一次入口不会开出第二个 Lead 页签", async () => {
    snapshot.mockResolvedValue(task());
    render(<Harness />);
    const pill = await dock().findByRole("button", { name: /Lead/ });
    fireEvent.click(pill);
    fireEvent.click(dock().getByRole("button", { name: "在侧边面板打开" }));
    // 「+」菜单里也不再列 Lead。
    fireEvent.click(side().getByRole("button", { name: "新建页签" }));
    expect(screen.queryByRole("menuitem", { name: /Lead/ })).toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(dock().getAllByRole("button").find((b) => b.classList.contains("ui-pill")) as HTMLElement);
    expect(side().getAllByRole("tab", { name: "Lead" })).toHaveLength(1);
  });

  it("关掉 Lead 页签：停靠区恢复原样（关卡卡片回来，胶囊重新弹停靠浮层）", async () => {
    snapshot.mockResolvedValue(task({ gate: gateA }));
    render(<Harness />);
    fireEvent.click(await dock().findByRole("button", { name: /Lead/ }));
    fireEvent.click(dock().getByRole("button", { name: "在侧边面板打开" }));
    expect(screen.getByTestId("dock").querySelector(".lead-gate-card")).toBeNull();

    const tab = side().getByRole("tab", { name: "Lead" });
    await act(async () => { fireEvent.click(tab.querySelector(".side-panel-tab-close") as Element); });
    await waitFor(() => expect(side().queryByRole("tab", { name: "Lead" })).toBeNull());
    expect(screen.getByTestId("dock").querySelector(".lead-gate-card")).toBeTruthy();
    expect(screen.getByTestId("dock").querySelector(".lead-gate-moved")).toBeNull();
    const pill = dock().getAllByRole("button").find((b) => b.classList.contains("ui-pill")) as HTMLElement;
    expect(pill.getAttribute("data-tip")).toBeNull();
    fireEvent.click(pill);
    expect(screen.getByTestId("dock").querySelector(".dock-work-panel")).toBeTruthy();
  });

  it("有任务时面板的「+」菜单也能开 Lead 页签", async () => {
    snapshot.mockResolvedValue(task());
    render(<Harness />);
    await dock().findByRole("button", { name: /Lead/ });
    fireEvent.click(side().getByRole("button", { name: "新建页签" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Lead/ }));
    expect(side().getByRole("tab", { name: "Lead" }).getAttribute("aria-selected")).toBe("true");
  });

  it("没有 Lead 任务的会话：停靠区没有入口，「+」菜单也不列 Lead", async () => {
    snapshot.mockResolvedValue(null);
    render(<Harness />);
    await waitFor(() => expect(snapshot).toHaveBeenCalled());
    expect(dock().queryByRole("button", { name: /Lead/ })).toBeNull();
    expect(dock().queryByRole("button", { name: "在侧边面板打开" })).toBeNull();
    fireEvent.click(side().getByRole("button", { name: "新建页签" }));
    expect(screen.queryByRole("menuitem", { name: /Lead/ })).toBeNull();
  });

  it("任务没了（结束后被清掉）而页签还开着：页签如实说没有任务，不报错不空白", async () => {
    localStorage.setItem("meowo-panel-tabs.v1:1", JSON.stringify({ tabs: [{ key: "l", type: "lead" }], active: "l", visible: true, expanded: false, savedAt: 1 }));
    snapshot.mockResolvedValue(null);
    render(<Harness />);
    await waitFor(() => expect(snapshot).toHaveBeenCalled());
    expect(side().getByRole("status").textContent).toBe("这个会话目前没有 Lead 任务");
  });

  it("任务走到 done：页签照常显示完成状态", async () => {
    localStorage.setItem("meowo-panel-tabs.v1:1", JSON.stringify({ tabs: [{ key: "l", type: "lead" }], active: "l", visible: true, expanded: false, savedAt: 1 }));
    snapshot.mockResolvedValue(task({ phase: "done", checks: [{ id: "1", check: "入口可点", passes: true, waived: null, leader: false, blocked: null }] }));
    render(<Harness />);
    expect(await side().findByText("侧边面板看 Lead")).toBeTruthy();
    expect(side().getByRole("progressbar").getAttribute("aria-valuenow")).toBe("1");
  });

  it("手机远程页（没有右侧面板）：Lead 面板不出「在侧边面板打开」，停靠区保持原样", async () => {
    snapshot.mockResolvedValue(task({ gate: gateA }));
    render(<Harness remote />);
    fireEvent.click(await dock().findByRole("button", { name: /Lead/ }));
    expect(screen.getByTestId("dock").querySelector(".dock-work-panel")).toBeTruthy();
    expect(dock().queryByRole("button", { name: "在侧边面板打开" })).toBeNull();
    expect(screen.getByTestId("dock").querySelector(".lead-gate-card")).toBeTruthy();
  });
});

describe("leadTabLink", () => {
  const onOpen = () => {};
  it("没开 Lead 页签：open=false；开着且是当前页签、面板展开：shown=true", () => {
    expect(leadTabLink(EMPTY_PANEL, false, onOpen, false)).toEqual({ open: false, shown: false, onOpen });
    const state = { tabs: [{ key: "d", type: "diff" as const }, { key: "l", type: "lead" as const }], active: "l", visible: true, expanded: false };
    expect(leadTabLink(state, true, onOpen, false)).toMatchObject({ open: true, shown: true });
    expect(leadTabLink(state, false, onOpen, false)).toMatchObject({ open: true, shown: false });
    expect(leadTabLink({ ...state, active: "d" }, true, onOpen, false)).toMatchObject({ open: true, shown: false });
  });
  it("远程页一律 null", () => {
    expect(leadTabLink(EMPTY_PANEL, true, onOpen, true)).toBeNull();
  });
});
