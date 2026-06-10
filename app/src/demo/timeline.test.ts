import { expect, test } from "vitest";
import { Timeline } from "./timeline";

const noopHooks = { paint: async () => {}, sync: (_ms: number) => {} };

test("seek 按时间序执行到期动作,且只执行一次", async () => {
  const tl = new Timeline(10, noopHooks);
  const log: string[] = [];
  tl.at(0.3, () => {
    log.push("b");
  });
  tl.at(0.1, () => {
    log.push("a");
  });
  await tl.seek(3); // t=0.3
  expect(log).toEqual(["a", "b"]);
  await tl.seek(4);
  expect(log).toEqual(["a", "b"]);
});

test("tween 在区间内插值、区间后钉在 1 且只钉一次(不覆盖后续动作)", async () => {
  const tl = new Timeline(10, noopHooks);
  const ks: number[] = [];
  tl.tween(0.0, 1.0, (k) => ks.push(k), (x) => x);
  await tl.seek(0); // k=0
  await tl.seek(5); // k=0.5
  await tl.seek(20); // k=1(超出区间,钉一次)
  await tl.seek(21); // 已完成,不再 apply
  expect(ks).toEqual([0, 0.5, 1]);
});

test("tween 开始前不调用 apply", async () => {
  const tl = new Timeline(10, noopHooks);
  const ks: number[] = [];
  tl.tween(1.0, 2.0, (k) => ks.push(k), (x) => x);
  await tl.seek(5); // t=0.5 < from
  expect(ks).toEqual([]);
});

test("duration 取动作与 tween 的最大时刻", () => {
  const tl = new Timeline(10, noopHooks);
  tl.at(3, () => {});
  tl.tween(1, 5, () => {});
  expect(tl.duration).toBe(5);
});
