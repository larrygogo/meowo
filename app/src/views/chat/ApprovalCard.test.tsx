import { useRef, useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApprovalCard, ApprovalCommandDetail, isRiskyCommand } from "./ApprovalCard";

afterEach(cleanup);

describe("isRiskyCommand 风险分级(C-9)", () => {
  it("破坏性命令判危险:rm -rf / sudo / git reset --hard / dd of=", () => {
    expect(isRiskyCommand("rm -rf build")).toBe(true);
    expect(isRiskyCommand("sudo apt install foo")).toBe(true);
    expect(isRiskyCommand("git reset --hard HEAD~1")).toBe(true);
    expect(isRiskyCommand("git clean -fd")).toBe(true);
    expect(isRiskyCommand("dd if=/dev/zero of=/dev/sda")).toBe(true);
    expect(isRiskyCommand("{\"command\":\"rm -rf node_modules\"}")).toBe(true);
  });

  it("安全命令不染色(宁漏勿冤):ls / cargo test / git status", () => {
    expect(isRiskyCommand("ls -la")).toBe(false);
    expect(isRiskyCommand("cargo test")).toBe(false);
    expect(isRiskyCommand("git status")).toBe(false);
    expect(isRiskyCommand("git push origin main")).toBe(false);
    expect(isRiskyCommand("{\"command\":\"cargo test\"}")).toBe(false);
  });
});

describe("ApprovalCard 外壳(G-16)", () => {
  it("role=alertdialog,非模态", () => {
    render(
      <ApprovalCard
        title="Agent 请求权限"
        sideActions={<button type="button">收起</button>}
        actions={<button type="button">允许一次</button>}
      />,
    );
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.getAttribute("aria-label")).toBe("Agent 请求权限");
    expect(dialog.getAttribute("aria-modal")).toBe("false");
  });

  // UI 优化 c3：挂载不再把焦点交给动作按钮（开窗就打字会误触「说明并拒绝」）。
  // 焦点无处落脚时交给 composer，打字永远进输入框。
  it("挂载不抢焦点：焦点在 body 时交给 returnFocusTo，不落到动作按钮", () => {
    function Harness() {
      const target = useRef<HTMLTextAreaElement>(null);
      return (
        <>
          <textarea ref={target} aria-label="composer" />
          <ApprovalCard title="Agent 请求权限" returnFocusTo={target} actions={<button type="button">允许一次</button>} />
        </>
      );
    }
    render(<Harness />);
    expect(document.activeElement).toBe(screen.getByLabelText("composer"));
  });

  it("焦点已在别处时挂载不动它", () => {
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    render(<ApprovalCard title="Agent 请求权限" actions={<button type="button">允许一次</button>} />);
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });
});

describe("ApprovalCard 焦点归还(C-9 overlay)", () => {
  // overlay 后卡片不再是 composer 的文档流邻居：收卡卸载时焦点会掉到 body，
  // 显式归还给 returnFocusTo（composer 输入框）；归还推迟到宏任务，给同帧
  // 挂载的新卡/其他焦点接管者让路。
  function Harness() {
    const target = useRef<HTMLTextAreaElement>(null);
    const [show, setShow] = useState(true);
    return (
      <>
        <textarea ref={target} aria-label="composer" />
        {show && (
          <ApprovalCard
            title="Agent 请求权限"
            returnFocusTo={target}
            actions={<button type="button" onClick={() => setShow(false)}>允许一次</button>}
          />
        )}
      </>
    );
  }

  it("焦点留在卡内时卸载，归还 returnFocusTo", async () => {
    render(<Harness />);
    // 焦点先进卡（键盘用户 Tab 进来点的）。
    act(() => { screen.getByRole("button", { name: "允许一次" }).focus(); });
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(screen.getByLabelText("composer"));
    });
  });

  it("焦点已不在卡内（用户点了别处）时不抢焦点", async () => {
    render(<Harness />);
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    fireEvent.click(screen.getByRole("button", { name: "允许一次" }));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });
});

describe("限高的卡片正文：关键内容进首屏、被裁的一侧有提示", () => {
  /// 做空审查 2026-11-11：620×480 下命令/选项被裁在折叠线下且毫无提示。jsdom 无布局，伪造溢出。
  it("正文溢出时标出被裁的一侧，并让滚动区进 Tab 序；不溢出时两者都不挂", () => {
    const { container, unmount } = render(<ApprovalCard title="Agent 请求权限"><span>说明</span></ApprovalCard>);
    const plain = container.querySelector<HTMLElement>(".chat-approval-copy")!;
    expect(plain.hasAttribute("data-clip")).toBe(false);
    expect(plain.hasAttribute("tabindex")).toBe(false);
    unmount();

    const sh = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(400);
    const ch = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(120);
    const again = render(<ApprovalCard title="Agent 请求权限"><span>说明</span></ApprovalCard>);
    const copy = again.container.querySelector<HTMLElement>(".chat-approval-copy")!;
    expect(copy.getAttribute("data-clip")).toBe("bottom");
    expect(copy.tabIndex).toBe(0);
    // 滚到中段：上下都被裁。
    copy.scrollTop = 100;
    fireEvent.scroll(copy);
    expect(copy.getAttribute("data-clip")).toBe("both");
    copy.scrollTop = 280;
    fireEvent.scroll(copy);
    expect(copy.getAttribute("data-clip")).toBe("top");
    sh.mockRestore();
    ch.mockRestore();
  });

  it("「复制命令」是带无障碍名与悬停提示的图标钮，不再独占一行文字", () => {
    render(
      <ApprovalCard title="Agent 请求权限">
        <ApprovalCommandDetail label="命令" command="echo hi" expandLabel="展开全部" collapseLabel="收起" copyLabel="复制命令" copiedLabel="已复制" />
      </ApprovalCard>,
    );
    const button = screen.getByRole("button", { name: "复制命令" });
    expect(button.textContent).toBe("");
    expect(button.getAttribute("data-tip")).toBe("复制命令");
    expect(button.querySelector("svg")).toBeTruthy();
  });
});

describe("ApprovalCommandDetail 长命令出口(C-10)", () => {
  const labels = {
    label: "完整参数",
    expandLabel: "展开全部",
    collapseLabel: "收起",
    copyLabel: "复制命令",
    copiedLabel: "已复制",
  };

  it("「展开全部」给详情挂上 is-expanded,再点收回", () => {
    // 展开钮只在命令超出限高时出现（chat #9）：jsdom 无布局，伪造 pre 溢出。
    const sh = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(400);
    const { container } = render(
      <ApprovalCard title="Agent 请求权限">
        <ApprovalCommandDetail command="very long command" {...labels} />
      </ApprovalCard>,
    );
    const detail = container.querySelector(".chat-approval-detail")!;
    expect(detail.className).not.toContain("is-expanded");
    fireEvent.click(screen.getByRole("button", { name: "展开全部" }));
    expect(detail.className).toContain("is-expanded");
    // 展开后按钮文案换成「收起」。
    fireEvent.click(screen.getByRole("button", { name: "收起" }));
    expect(detail.className).not.toContain("is-expanded");
    sh.mockRestore();
  });

  it("「复制命令」整段进剪贴板并给已复制反馈", async () => {
    const writeText = vi.fn((_text: string) => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <ApprovalCard title="Agent 请求权限">
        <ApprovalCommandDetail command="echo hello" {...labels} />
      </ApprovalCard>,
    );
    fireEvent.click(screen.getByRole("button", { name: "复制命令" }));
    expect(writeText).toHaveBeenCalledWith("echo hello");
    expect(await screen.findByRole("button", { name: "已复制" })).toBeTruthy();
  });
});

describe("标题行吸顶的卡（Lead 关卡）：关键内容排在标题下面", () => {
  /// 手机 375×376 键盘态实测：标题吸顶后占掉可视区最上面一段，关键块「从第一行起」时第一个选项正好被标题压住。
  /// jsdom 无布局：可视区 120、标题占 24、选项块在 200 处高 110——不算标题放得下（余 10），算上就放不下。
  function mount(sticky: boolean) {
    const style = document.createElement("style");
    style.textContent = sticky ? ".lead-gate-card .chat-approval-head { position: sticky; top: 0; }" : "";
    document.head.appendChild(style);
    const sh = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(400);
    const ch = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(120);
    const rect = (top: number, height: number) => ({ top, bottom: top + height, height, left: 0, right: 300, width: 300, x: 0, y: top, toJSON: () => ({}) }) as DOMRect;
    const br = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("chat-approval-head")) return rect(0, 24);
      if (this.classList.contains("lead-gate-options")) return rect(200, 110);
      return rect(0, 120);
    });
    const { container } = render(
      <ApprovalCard title="校验规则放前端还是后端" className="lead-gate-card">
        <div className="lead-gate-options"><label>前端</label></div>
      </ApprovalCard>,
    );
    const top = container.querySelector<HTMLElement>(".chat-approval-copy")!.scrollTop;
    sh.mockRestore(); ch.mockRestore(); br.mockRestore(); style.remove();
    return top;
  }

  it("标题不吸顶时照旧：选项块放得下，下沿贴住可视区（滚到 200）", () => {
    expect(mount(false)).toBe(200);
  });

  it("标题吸顶时可用高度扣掉标题：放不下就从第一行起，并让出标题的高度（滚到 176 而不是 200）", () => {
    expect(mount(true)).toBe(176);
  });
});
