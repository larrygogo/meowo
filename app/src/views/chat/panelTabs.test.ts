import { describe, expect, it } from "vitest";
import {
  activate, addTab, closeTabs, cycle, EMPTY_PANEL, keysToClose, loadPanel, moveTab, openSingleton, savePanel, setVisible,
  type PanelState, type PanelTab,
} from "./panelTabs";

const term = (key: string): PanelTab => ({ key, type: "term", shellId: null, cwd: "C:\\repo" });
const web = (key: string): PanelTab => ({ key, type: "browser", paneId: null, url: "about:blank", title: "" });
const with3 = (): PanelState => ({ tabs: [term("a"), term("b"), web("c")], active: "b", visible: true, expanded: false });

class MemoryStorage implements Storage {
  private map = new Map<string, string>();
  get length() { return this.map.size; }
  clear() { this.map.clear(); }
  getItem(key: string) { return this.map.get(key) ?? null; }
  key(index: number) { return [...this.map.keys()][index] ?? null; }
  removeItem(key: string) { this.map.delete(key); }
  setItem(key: string, value: string) { this.map.set(key, value); }
}

describe("面板页签模型", () => {
  it("改动和文件只开一个：再打开是切过去", () => {
    let state = openSingleton(EMPTY_PANEL, "diff");
    state = addTab(state, term("t"));
    const again = openSingleton(state, "diff");
    expect(again.tabs).toHaveLength(2);
    expect(again.active).toBe(state.tabs[0].key);
    expect(again.visible).toBe(true);
  });

  it("终端和浏览器可以开多个，新的加在最右边", () => {
    const state = addTab(addTab(EMPTY_PANEL, term("t1")), term("t2"));
    expect(state.tabs.map((tab) => tab.key)).toEqual(["t1", "t2"]);
    expect(state.active).toBe("t2");
  });

  it("关掉当前页签落到右边那个；没有右边落到左边", () => {
    expect(closeTabs(with3(), ["b"]).active).toBe("c");
    expect(closeTabs({ ...with3(), active: "c" }, ["c"]).active).toBe("b");
    // 关的不是当前页签，当前页签不变。
    expect(closeTabs(with3(), ["a"]).active).toBe("b");
  });

  it("关掉最后一个页签，面板收起", () => {
    const state = closeTabs({ ...with3(), expanded: true }, ["a", "b", "c"]);
    expect(state).toEqual({ tabs: [], active: null, visible: false, expanded: false });
  });

  it("右键菜单：关闭其他 / 关闭右侧 / 关闭所有", () => {
    expect(keysToClose(with3(), "b", "others")).toEqual(["a", "c"]);
    expect(keysToClose(with3(), "a", "right")).toEqual(["b", "c"]);
    expect(keysToClose(with3(), "b", "all")).toEqual(["a", "b", "c"]);
  });

  it("拖拽排序和 Ctrl+Tab 循环", () => {
    expect(moveTab(with3(), "c", 0).tabs.map((tab) => tab.key)).toEqual(["c", "a", "b"]);
    expect(cycle(with3(), 1).active).toBe("c");
    expect(cycle({ ...with3(), active: "c" }, 1).active).toBe("a");
    expect(cycle(with3(), -1).active).toBe("a");
  });

  it("收起面板页签都留着；再打开回到原来的页签", () => {
    const hidden = setVisible({ ...with3(), expanded: true }, false);
    expect(hidden.tabs).toHaveLength(3);
    expect(hidden.expanded).toBe(false);
    expect(setVisible(hidden, true).active).toBe("b");
    expect(activate(hidden, "nope")).toBe(hidden);
  });

  it("按会话存取；坏数据当空面板", () => {
    const storage = new MemoryStorage();
    savePanel(7, with3(), storage);
    expect(loadPanel(7, storage)).toEqual(with3());
    expect(loadPanel(8, storage)).toEqual(EMPTY_PANEL);
    storage.setItem("meowo-panel-tabs.v1:9", "{not json");
    expect(loadPanel(9, storage)).toEqual(EMPTY_PANEL);
    // 没有页签、面板也关着，就不留记录。
    savePanel(7, EMPTY_PANEL, storage);
    expect(storage.getItem("meowo-panel-tabs.v1:7")).toBeNull();
    // 只开着启动列表（一个页签都没有）的面板也记成开着。
    savePanel(7, { tabs: [], active: null, visible: true, expanded: false }, storage);
    expect(loadPanel(7, storage)).toEqual({ tabs: [], active: null, visible: true, expanded: false });
    // 有页签但关着：页签留着，面板关着。
    savePanel(7, { ...with3(), visible: false }, storage);
    expect(loadPanel(7, storage)).toMatchObject({ visible: false, active: "b" });
  });
});
