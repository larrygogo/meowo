// 官网的几段演示（embed-*.html）在同一个页面里以 iframe 并排加载，同源，共用一份 localStorage。
// 以前每段一启动就 localStorage.clear()，右侧面板演示还要预先写入「面板开着、停在更改页签」——
// 谁后加载谁把别人清掉，功能页的面板时常打不开，手机演示反过来读到面板的记录、打开一页空白的
// 更改（2026-11-09 做空审查复现：8 次里 7 次）。
//
// 这里换成每个 iframe 自己的一份内存存储：互不干扰，每次打开都是干净的起点，也不碰访客浏览器里
// 真正的 localStorage。必须是入口文件的第一个 import（ES 模块按 import 顺序求值），
// 保证其它模块在顶层读 localStorage 之前就已换好。
class MemoryStorage implements Storage {
  private map = new Map<string, string>();
  get length() {
    return this.map.size;
  }
  clear() {
    this.map.clear();
  }
  getItem(key: string) {
    return this.map.has(key) ? (this.map.get(key) as string) : null;
  }
  key(index: number) {
    return [...this.map.keys()][index] ?? null;
  }
  removeItem(key: string) {
    this.map.delete(key);
  }
  setItem(key: string, value: string) {
    this.map.set(key, String(value));
  }
}

Object.defineProperty(window, "localStorage", { configurable: true, value: new MemoryStorage() });
