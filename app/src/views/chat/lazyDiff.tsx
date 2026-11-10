// SidePanel 的「文件」页签、SourceControl 的单文件 diff 要用 GitDiffView 里的 FilesView /
// ChangesView。直接静态引的话，GitDiffView（连同 FileMarkdown 带进来的 rehype-raw 整套
// HTML 解析器）就被拖进引用方所在的 chunk，ChatWindow 对它的懒加载失效（vite 构建警告实测）。
// 这里按需加载；GitDiffView 一旦求值过（桌面端 ChatWindow 会预取，测试里直接 import），
// 它自己会登记进来，之后同步渲染，不再多闪一帧空白。
import { useEffect, useState, type ComponentProps } from "react";

type DiffModule = Pick<typeof import("./GitDiffView"), "FilesView" | "ChangesView">;

let loaded: DiffModule | null = null;
let pending: Promise<DiffModule> | null = null;

/** GitDiffView 模块求值时调用：让已加载的情况走同步渲染。 */
export function registerDiffModule(mod: DiffModule) {
  loaded = mod;
}

export function loadDiffModule(): Promise<DiffModule> {
  if (loaded) return Promise.resolve(loaded);
  pending ??= import("./GitDiffView").then((mod) => {
    loaded = mod;
    return mod;
  });
  return pending;
}

function useDiffModule(): DiffModule | null {
  const [mod, setMod] = useState<DiffModule | null>(loaded);
  useEffect(() => {
    if (mod) return;
    let alive = true;
    void loadDiffModule().then((m) => { if (alive) setMod(m); }).catch(() => {});
    return () => { alive = false; };
  }, [mod]);
  return mod ?? loaded;
}

export function LazyFilesView(props: ComponentProps<DiffModule["FilesView"]>) {
  const mod = useDiffModule();
  return mod ? <mod.FilesView {...props} /> : null;
}

export function LazyChangesView(props: ComponentProps<DiffModule["ChangesView"]>) {
  const mod = useDiffModule();
  return mod ? <mod.ChangesView {...props} /> : null;
}
