import { useEffect, useRef, useState } from "react";
import { type WorktreeCreateDto, type WorktreePreviewDto, worktreeCreate, worktreePreview } from "../api";
import { unquotePath } from "../paths";

/// 新建会话时「在新 worktree 里开」的选择（包 5）。默认不勾：不勾时 launch 一步都不多走，
/// 和改前逐字一样。勾上后实时预览会建在哪（用途、分支、目录、基点、撞名），启动时先建 worktree
/// 再照常调 new_session（cwd = 新 worktree）。

/** 没填用途时的兜底名 `MMDD-HHmm`，按本机时区（后端只知道 UTC）。 */
export function localSlug(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
}

/** 一次「开始」的请求号：手机网络抖动重试、或会话没起来再点一次时，后端认出是同一次，不建第二个。 */
export function newRequestId(): string {
  try {
    if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  } catch { /* 非安全上下文（手机经 http 访问）没有 randomUUID */ }
  return `r-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export type WorktreeChoice = ReturnType<typeof useWorktreeChoice>;

export function useWorktreeChoice(cwd: string) {
  const [enabled, setEnabled] = useState(false);
  const [slug, setSlug] = useState("");
  const [branch, setBranch] = useState("");
  const [preview, setPreview] = useState<WorktreePreviewDto | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  // 兜底名在打开页面时定下：输入框里的占位和最后建出来的必须是同一个名字。
  const autoSlugRef = useRef(localSlug());
  const requestRef = useRef<string | null>(null);
  // 输入变了就是另一次请求；没变（只是重试）沿用同一个请求号。
  useEffect(() => { requestRef.current = null; }, [cwd, slug, branch, enabled]);

  useEffect(() => {
    const dir = unquotePath(cwd);
    if (!enabled || !dir) { setPreview(null); setPreviewError(null); return; }
    let alive = true;
    // 打字时每个字都会触发：稍等再查，预览要跑几条 git 命令。
    const timer = window.setTimeout(() => {
      worktreePreview(dir, slug.trim() || null, branch.trim() || null, autoSlugRef.current)
        .then((p) => { if (alive) { setPreview(p); setPreviewError(null); } })
        .catch((e) => { if (alive) { setPreview(null); setPreviewError(String(e)); } });
    }, 250);
    return () => { alive = false; window.clearTimeout(timer); };
  }, [enabled, cwd, slug, branch]);

  async function create(dir: string, provider: string): Promise<WorktreeCreateDto> {
    if (!requestRef.current) requestRef.current = newRequestId();
    return worktreeCreate({
      repoCwd: dir,
      slug: slug.trim() || null,
      branch: branch.trim() || null,
      fallbackSlug: autoSlugRef.current,
      provider,
      requestId: requestRef.current,
    });
  }

  /** 撞名时套用建议名：用户填了分支就改分支，否则改用途（分支跟着用途走）。 */
  function applySuggestion() {
    if (!preview) return;
    if (branch.trim() && preview.suggestedBranch) setBranch(preview.suggestedBranch);
    else if (preview.suggestedSlug) setSlug(preview.suggestedSlug);
  }

  return {
    enabled, setEnabled, slug, setSlug, branch, setBranch, preview, previewError,
    autoSlug: autoSlugRef.current, create, applySuggestion,
  };
}
