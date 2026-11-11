import { describe, expect, it } from "vitest";
import { makeSession } from "../../demo/data";
import type { Item } from "../sticker/types";
import { islandCounts, islandTone } from "../Island";
import { attentionFocus } from "./attention";

function mk(patch: (s: Item) => void = () => {}): Item {
  const s = makeSession({ title: "t", project: "p" });
  patch(s);
  return s;
}
const pending = () => mk((s) => { s.pending_review = "approval"; });
const errored = () => mk((s) => { s.errored = true; });
const unreadDone = () => mk((s) => { s.session.status = "waiting"; s.unread = true; });
const readDone = () => mk((s) => { s.session.status = "waiting"; });

describe("attentionFocus", () => {
  it("没人需要你：回轨顶，有人在跑是在跑的神态，否则发呆", () => {
    expect(attentionFocus([mk(), readDone()], null)).toEqual({ id: null, tone: "running" });
    expect(attentionFocus([readDone()], null)).toEqual({ id: null, tone: "idle" });
    expect(attentionFocus([], null)).toEqual({ id: null, tone: "idle" });
  });

  it("出错与待处理并存：站在待处理那一行，神态是待处理", () => {
    const e = errored();
    const p = pending();
    expect(attentionFocus([e, p], null)).toEqual({ id: p.session.id, tone: "pending" });
  });

  it("同一档里按名单从上到下", () => {
    const a = pending();
    const b = pending();
    expect(attentionFocus([a, b], null).id).toBe(a.session.id);
    expect(attentionFocus([b, a], null).id).toBe(b.session.id);
  });

  it("跑完没看过的叫眼睛，看过的不叫", () => {
    const u = unreadDone();
    expect(attentionFocus([readDone(), u], null)).toEqual({ id: u.session.id, tone: "waiting" });
  });

  it("眼睛站着的那个还需要你：新来的更要紧的也不抢", () => {
    const e = errored();
    const p = pending();
    expect(attentionFocus([p, e], e.session.id)).toEqual({ id: e.session.id, tone: "error" });
  });

  it("眼睛站着的那个处理完了：换到下一个", () => {
    const done = readDone();
    const p = pending();
    expect(attentionFocus([done, p], done.session.id).id).toBe(p.session.id);
  });

  it("和灵动岛同一个次序：同一组会话，岛的神态与黑轨的神态一致", () => {
    const sets = [[errored(), pending()], [errored(), mk()], [pending(), mk()], [mk(), readDone()]];
    for (const items of sets) {
      expect(islandTone(islandCounts(items))).toBe(attentionFocus(items, null).tone);
    }
  });

  it("断开的会话不叫眼睛", () => {
    const off = mk((s) => { s.pending_review = "approval"; s.connected = false; });
    expect(attentionFocus([off], null).id).toBeNull();
  });
});
