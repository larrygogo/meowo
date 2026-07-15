// demo 专用:确定性时间轴。录制脚本逐帧调 seek(n),同一分镜每次产出逐帧一致:
//   - at(sec, fn) 一次性动作,按时间序执行且只执行一次;
//   - tween(from, to, apply) 区间插值,每帧调用、区间结束后钉在 k=1;
//   - seek 执行完动作后等两次 rAF 让 React 落地,再把页面全部 CSS 动画
//     钉到「相对各自首次出现时刻」的统一时间(新挂载动画从 0 播,无真实时钟参与)。
type Action = { at: number; run: () => void | Promise<void> };
type Ease = (x: number) => number;
type Tween = { from: number; to: number; apply: (k: number) => void; ease: Ease; finished?: boolean };
type Hooks = { paint: () => Promise<void>; sync: (ms: number) => void };

export const easeInOut: Ease = (x) =>
  x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2;

export class Timeline {
  readonly fps: number;
  duration = 0;
  private actions: Action[] = [];
  private tweens: Tween[] = [];
  private done = new Set<Action>();
  private hooks: Hooks;

  constructor(fps: number, hooks?: Hooks) {
    this.fps = fps;
    this.hooks = hooks ?? { paint: nextPaint, sync: syncAnimations };
  }

  at(sec: number, run: Action["run"]): void {
    this.actions.push({ at: sec, run });
    this.duration = Math.max(this.duration, sec);
  }

  tween(from: number, to: number, apply: (k: number) => void, ease: Ease = easeInOut): void {
    this.tweens.push({ from, to, apply, ease });
    this.duration = Math.max(this.duration, to);
  }

  /** 跳到第 n 帧(只向前)。 */
  async seek(frame: number): Promise<void> {
    const t = frame / this.fps;
    const due = this.actions
      .filter((a) => a.at <= t && !this.done.has(a))
      .sort((a, b) => a.at - b.at);
    for (const a of due) {
      this.done.add(a);
      await a.run();
    }
    for (const w of this.tweens) {
      // 完成后不再 apply:终值只钉一次,避免覆盖后续 at() 对同一目标的修改。
      if (w.finished || t < w.from) continue;
      const k = Math.min(1, (t - w.from) / Math.max(w.to - w.from, 1e-9));
      w.apply(w.ease(k));
      if (k >= 1) w.finished = true;
    }
    await this.hooks.paint();
    this.hooks.sync(t * 1000);
    await this.hooks.paint();
  }

  /**
   * 实时播放（用于网页内嵌演示，非录制）：按真实时钟触发 at() 动作、插值 tween()，
   * 但**不**冻结 CSS 动画——让过渡/关键帧自然播放。播到末尾（+尾巴）后调用 onEnd。
   * 返回停止函数。
   */
  play(onEnd: () => void, tailSec = 0.6): () => void {
    const start = performance.now();
    const fired = new Set<Action>();
    let raf = 0;
    let stopped = false;
    const frame = (now: number) => {
      if (stopped) return;
      const t = (now - start) / 1000;
      for (const a of this.actions) {
        if (a.at <= t && !fired.has(a)) {
          fired.add(a);
          void a.run();
        }
      }
      for (const w of this.tweens) {
        if (t < w.from) continue;
        const k = Math.min(1, (t - w.from) / Math.max(w.to - w.from, 1e-9));
        w.apply(w.ease(k));
      }
      if (t >= this.duration + tailSec) {
        onEnd();
        return;
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      stopped = true;
      cancelAnimationFrame(raf);
    };
  }
}

function nextPaint(): Promise<void> {
  return new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
}

/** 把页面全部 CSS 动画钉到「相对首次出现时刻」的统一时间轴(逐帧确定)。 */
const seen = new Map<Animation, number>();
function syncAnimations(ms: number): void {
  for (const a of document.getAnimations()) {
    let t0 = seen.get(a);
    if (t0 === undefined) {
      seen.set(a, ms);
      t0 = ms;
    }
    try {
      a.pause();
      a.currentTime = Math.max(0, ms - t0);
    } catch {
      /* 个别动画不可 seek,忽略 */
    }
  }
}
