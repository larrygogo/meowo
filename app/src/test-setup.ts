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
