import { describe, expect, it } from "vitest";
import { createSendQueue } from "./terminalSubmit";

/** 先上屏再发送：同会话的消息排队写终端，一条写完才写下一条；前一条失败不挡后面的。 */
describe("createSendQueue", () => {
  it("同一会话按顺序依次写，不互相插入", async () => {
    const queue = createSendQueue();
    const log: string[] = [];
    let releaseFirst!: () => void;
    const first = queue.enqueue(1, () => new Promise<boolean>((resolve) => {
      log.push("first:start");
      releaseFirst = () => { log.push("first:end"); resolve(true); };
    }));
    const second = queue.enqueue(1, async () => { log.push("second"); return true; });
    await Promise.resolve();
    expect(log).toEqual(["first:start"]);
    expect(queue.busy(1)).toBe(true);
    releaseFirst();
    expect(await first).toBe(true);
    expect(await second).toBe(true);
    expect(log).toEqual(["first:start", "first:end", "second"]);
    expect(queue.busy(1)).toBe(false);
  });

  it("前一条失败或抛错，后一条照常发；抛错算发送失败", async () => {
    const queue = createSendQueue();
    const failed = queue.enqueue(1, () => Promise.reject(new Error("写入失败")));
    const next = queue.enqueue(1, async () => true);
    expect(await failed).toBe(false);
    expect(await next).toBe(true);
  });

  it("不同会话互不等待", async () => {
    const queue = createSendQueue();
    let release!: () => void;
    const slow = queue.enqueue(1, () => new Promise<boolean>((resolve) => { release = () => resolve(true); }));
    expect(await queue.enqueue(2, async () => true)).toBe(true);
    expect(queue.busy(1)).toBe(true);
    expect(queue.busy(2)).toBe(false);
    release();
    await slow;
  });
});
