import { describe, expect, it } from "vitest";
import { applyGroupOrder, dropNeighbors, moveNextTo } from "./sidebarDrag";

describe("拖拽落点", () => {
  const list = [1, 2, 3, 4];
  it("落在某条下方 / 上方，给出新位置的上下邻居", () => {
    expect(dropNeighbors(list, 1, { id: 3, after: true })).toEqual({ above: 3, below: 4 });
    expect(dropNeighbors(list, 4, { id: 1, after: false })).toEqual({ above: null, below: 1 });
    expect(dropNeighbors(list, 1, { id: 4, after: true })).toEqual({ above: 4, below: null });
  });
  it("落回原位、落在自己身上、落到桶外都不动", () => {
    expect(dropNeighbors(list, 2, { id: 1, after: true })).toBeNull();
    expect(dropNeighbors(list, 2, { id: 3, after: false })).toBeNull();
    expect(dropNeighbors(list, 2, { id: 2, after: true })).toBeNull();
    expect(dropNeighbors(list, 2, { id: 9, after: true })).toBeNull();
  });
});

describe("本地先挪", () => {
  const items = [{ id: 1 }, { id: 2 }, { id: 3 }, { id: 4 }];
  const idOf = (item: { id: number }) => item.id;
  it("挪到 above 正下方，没有 above 时挪到 below 正上方", () => {
    expect(moveNextTo(items, idOf, 1, 3, 4).map(idOf)).toEqual([2, 3, 1, 4]);
    expect(moveNextTo(items, idOf, 4, null, 1).map(idOf)).toEqual([4, 1, 2, 3]);
  });
  it("邻居不在已加载列表里时原样返回", () => {
    expect(moveNextTo(items, idOf, 1, 9, null).map(idOf)).toEqual([1, 2, 3, 4]);
  });
});

describe("工作区组的手动顺序", () => {
  const groups = ["a", "b", "c", "new"].map((key) => ({ key }));
  it("排过的按顺序，新工作区放最上面，没排过时原样", () => {
    expect(applyGroupOrder(groups, ["c", "a", "b"], (group) => group.key).map((group) => group.key)).toEqual(["new", "c", "a", "b"]);
    expect(applyGroupOrder(groups, [], (group) => group.key).map((group) => group.key)).toEqual(["a", "b", "c", "new"]);
  });
});
