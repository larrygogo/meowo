/**
 * 回归测试：贴纸看板空闲期不再持续刷新。
 *
 * 背景：WAL 模式下 meowo-app 每次读库都新开连接触碰 board.db-shm，曾被 db-watcher 误判为
 * 变更而发 board-changed，形成 read→watcher→refresh→read 的自持循环，空闲时贴纸列表一直跳。
 * 修复：db-watcher 用持久连接 + PRAGMA data_version 门控，只有别的连接真正提交写入才发事件；
 * 并把 -shm 事件从监听中排除。前端另加结构相等守卫兜底。
 *
 * 观测点：App 的 board-changed 监听器在 VITE_E2E 构建下把收到的次数累计到
 * window.__MEOWO_BOARD_CHANGED__。空闲若干秒内该计数应几乎不增长。
 */
describe("贴纸看板：空闲刷新回归", () => {
  it("空闲 6 秒内 board-changed 不再持续累计", async () => {
    // 等前端挂载：监听器 effect 运行时把计数初始化为 0（仅 E2E 构建）。
    await browser.waitUntil(async () => (await readCount()) !== null, {
      timeout: 30_000,
      timeoutMsg: "30s 内未检测到 board-changed 观测计数（该二进制是否以 VITE_E2E=1 + --features e2e 构建？）",
    });

    // 首屏加载会合法地发若干次 board-changed（首次导入 / 首轮 liveness）；等其平静下来。
    await waitIdle();

    const before = (await readCount()) ?? 0;
    // 空闲观察 6 秒。修复后 db-watcher 仅在真实写入时才发事件，纯空闲应几乎为 0。
    await browser.pause(6_000);
    const after = (await readCount()) ?? 0;

    // 阈值给到 2：容忍 liveness 5s 轮询在存活集变化时至多 1 次合法刷新及边界抖动。
    // 修复前此处会是几十次（每 1~2s 一次）。
    expect(after - before).toBeLessThanOrEqual(2);
  });
});

/** 读观测计数；未注入（非 E2E 构建/尚未挂载）时返回 null。 */
async function readCount(): Promise<number | null> {
  return browser.execute(() => {
    const w = window as unknown as { __MEOWO_BOARD_CHANGED__?: number };
    return typeof w.__MEOWO_BOARD_CHANGED__ === "number" ? w.__MEOWO_BOARD_CHANGED__ : null;
  });
}

/** 等计数平静：连续 3 秒不变即视为进入空闲（最多等 20 秒）。 */
async function waitIdle(): Promise<void> {
  let stable = 0;
  let prev = await readCount();
  for (let i = 0; i < 20; i++) {
    await browser.pause(1_000);
    const cur = await readCount();
    stable = cur === prev ? stable + 1 : 0;
    prev = cur;
    if (stable >= 3) return;
  }
}
