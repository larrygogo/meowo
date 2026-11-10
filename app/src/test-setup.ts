// Node 22.4+ 起 globalThis 上自带实验性 Web Storage：未传 --localstorage-file 时
// `localStorage` 描述符存在、求值却是 undefined（并打 ExperimentalWarning）。vitest 往 globalThis
// 灌 jsdom 全局时不覆盖已存在的键，于是 jsdom 自己的 localStorage 被这个空壳遮蔽——
// 测试里一碰 localStorage.* 就 TypeError。补一个内存实现顶上（测试互不共享状态，无需持久化）。
class MemoryStorage implements Storage {
  private map = new Map<string, string>();
  get length() {
    return this.map.size;
  }
  clear() {
    this.map.clear();
  }
  getItem(key: string) {
    return this.map.has(key) ? this.map.get(key)! : null;
  }
  key(index: number) {
    return Array.from(this.map.keys())[index] ?? null;
  }
  removeItem(key: string) {
    this.map.delete(key);
  }
  setItem(key: string, value: string) {
    this.map.set(key, String(value));
  }
}

for (const name of ["localStorage", "sessionStorage"] as const) {
  // 只在确实拿不到可用实现时才顶替：将来 Node/vitest 修好了，这里自动让位。
  if (globalThis[name] === undefined) {
    Object.defineProperty(globalThis, name, { value: new MemoryStorage(), configurable: true });
  }
}

// 对话页的会话快照缓存是模块级的（views/chat/historyCache）：不清的话，上一个用例看过的会话
// 会在下一个用例切换时被直接恢复，用例之间互相串味。
import { afterEach } from "vitest";
import { clearHistoryCache } from "./views/chat/historyCache";
afterEach(() => clearHistoryCache());

// waitFor / findBy 的默认等待上限从 1 秒放宽到 3 秒：全量并发跑时机器很忙，虚拟列表、整窗渲染这类
// 单条 0.5–1 秒的用例会贴着 1 秒线偶发超时（做空第 3 轮：4 次全量 1 次红，单跑都过；此前 b17624e9
// 也修过同类偶发）。只拉长「等不到」的上限，等得到的用例耗时不变。
import { configure } from "@testing-library/react";
configure({ asyncUtilTimeout: 3000 });

// 每个测试文件收尾前留 600ms 宽限，再拆 jsdom。组件卸载后还挂着的短计时器（账号卡片刷新
// 转圈的最短 500ms、@tanstack/virtual 的滚动防抖）若在环境拆掉后才触发，setState 里一碰
// window 就抛 ReferenceError。vitest 2 静默吞掉，升到 3.2.7 后记成 Unhandled Error、整轮
// 退出码 1（2026-11-10 实测：About.account.test.tsx 单跑 3 次 1 次报，vitest 2 下 4 次 0 报）。
// 文件间并行，600ms 摊到全量墙钟上约 1–2s。
import { afterAll } from "vitest";
afterAll(() => new Promise<void>((resolve) => setTimeout(resolve, 600)));
