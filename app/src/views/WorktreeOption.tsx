import { useT } from "../i18n";
import { formatBackendError } from "../i18n/errors";
import type { WorktreeChoice } from "./useWorktreeChoice";

/// 新建会话页 / 面板里的「在新 worktree 里开」（包 5）。两处共用，只管长相；状态在 useWorktreeChoice。
export function WorktreeOption({ choice, disabled }: { choice: WorktreeChoice; disabled?: boolean }) {
  const t = useT();
  const p = choice.preview;
  const conflict = p
    ? p.dirExists ? t.wtnew.dirExists
      : p.branchExists ? t.wtnew.branchExists
      : p.branchCheckedOut ? t.wtnew.checkedOut
      : null
    : null;
  // 建议名：用户填了分支就建议分支，否则建议用途。
  const suggestion = p ? (choice.branch.trim() ? p.suggestedBranch : p.suggestedSlug) : null;
  return (
    <div className="wt-opt" data-testid="wt-option">
      <label className="wt-opt-toggle" data-tip={t.wtnew.toggleTip}>
        <input
          type="checkbox"
          data-testid="wt-toggle"
          checked={choice.enabled}
          disabled={disabled}
          onChange={(event) => choice.setEnabled(event.target.checked)}
        />
        <span>{t.wtnew.toggle}</span>
      </label>
      {choice.enabled && (
        <div className="wt-opt-body">
          <div className="wt-opt-fields">
            <label className="wt-opt-field">
              <span className="wt-opt-label">{t.wtnew.slug}</span>
              <input
                className="wt-opt-input"
                data-testid="wt-slug"
                value={choice.slug}
                disabled={disabled}
                placeholder={t.wtnew.slugPlaceholder(choice.autoSlug)}
                onChange={(event) => choice.setSlug(event.target.value)}
              />
            </label>
            <label className="wt-opt-field">
              <span className="wt-opt-label">{t.wtnew.branch}</span>
              <input
                className="wt-opt-input"
                data-testid="wt-branch"
                value={choice.branch}
                disabled={disabled}
                // 占位是按用途推出来的分支名：不填就用它。
                placeholder={p && !choice.branch.trim() ? p.branch : ""}
                onChange={(event) => choice.setBranch(event.target.value)}
              />
            </label>
          </div>
          {p && (
            <div className="wt-opt-meta" data-testid="wt-meta">
              <span>{t.wtnew.base(p.base)}</span>
              <span className="wt-opt-path" data-tip={p.path}>{t.wtnew.path(p.path)}</span>
              {p.hasSetup && <span>{t.wtnew.hasSetup}</span>}
              {p.subDir && !p.subDirMissing && <span>{t.wtnew.subDir(p.subDir)}</span>}
            </div>
          )}
          {/* 选的是仓库里只在本地的子目录：新 worktree 从基点检出没有它，会话退回根目录，先说清楚。 */}
          {p?.subDir && p.subDirMissing && (
            <div className="wt-opt-warn" data-testid="wt-subdir-missing">{t.wtnew.subDirMissing(p.subDir)}</div>
          )}
          {conflict && (
            <div className="wt-opt-warn" data-testid="wt-conflict">
              <span>{conflict}</span>
              {suggestion && (
                <button type="button" className="wt-opt-btn" data-testid="wt-use-suggestion" disabled={disabled} onClick={choice.applySuggestion}>
                  {t.wtnew.useSuggestion(suggestion)}
                </button>
              )}
            </div>
          )}
          {choice.previewError && (
            <div className="wt-opt-warn" data-testid="wt-preview-error">
              {choice.previewError === "worktree/not_repo" ? t.wtnew.notRepo : formatBackendError(choice.previewError, t.locale)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
