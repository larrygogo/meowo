/**
 * 对话窗标题栏右侧的工具（对照 Kimi Code 标题栏）：
 * - OpenInEditor：「用编辑器打开」分体按钮——主钮用上次选的编辑器打开会话目录，▾ 换一个。
 * - GitStatusChip：分支 + 领先远程的提交数 + 未提交改动的增删行，点开改动面板。
 * 文件面板里「打开方式」是按单个文件的关联走的；这里打开的是整个目录，清单由后端按安装位置探测。
 */
import { useEffect, useState } from "react";
import { listFolderEditors, openFolderIn, openLink, openProjectDir, type GitDiffSummaryDto, type PullRequestDto } from "../../api";
import type { FileOpenerDto } from "../../generated/contracts/FileOpenerDto";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { isMac } from "../../platform";
import { useMenuPopup } from "../menu";
import { CheckIcon, ChevronDownIcon } from "../sticker/icons";

/** 上次选的打开方式（编辑器 id 或 "files"）。只是界面偏好，存本机即可。 */
const OPENER_KEY = "meowo-folder-opener";
/** 系统文件管理器这一项的 id：走 open_project_dir，前端恒有。 */
const FILE_MANAGER = "files";

function readOpener(): string | null {
  try { return localStorage.getItem(OPENER_KEY); } catch { return null; }
}
function saveOpener(id: string) {
  try { localStorage.setItem(OPENER_KEY, id); } catch { /* 隐私模式：只是不记 */ }
}

// 一个窗口探测一次：装没装编辑器在一次使用期间基本不变，每次切会话都扫磁盘取图标是浪费。
let editorsOnce: Promise<FileOpenerDto[]> | null = null;
// 旧后端（手机页先于安装版更新等）没有这条命令时拿不到数组，按「一个编辑器都没装」处理。
const loadEditors = () => (editorsOnce ??= listFolderEditors()
  .then((list) => (Array.isArray(list) ? list : []))
  .catch(() => []));

function FolderGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    </svg>
  );
}
function CodeGlyph() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m8 7-5 5 5 5M16 7l5 5-5 5" />
    </svg>
  );
}
function OpenerIcon({ opener }: { opener: FileOpenerDto }) {
  if (opener.icon) return <img src={opener.icon} width={16} height={16} alt="" draggable={false} />;
  return opener.id === FILE_MANAGER ? <FolderGlyph /> : <CodeGlyph />;
}

export function OpenInEditor({ cwd, onError }: { cwd: string; onError: (message: string) => void }) {
  const t = useT();
  const [editors, setEditors] = useState<FileOpenerDto[]>([]);
  useEffect(() => {
    let alive = true;
    void loadEditors().then((list) => { if (alive) setEditors(list); });
    return () => { alive = false; };
  }, []);
  const [preferred, setPreferred] = useState(readOpener);
  const { open, setOpen, pos, ref, btnRef, menuRef, toggle, onKeyDown } = useMenuPopup({});
  const options: FileOpenerDto[] = [
    ...editors,
    { id: FILE_MANAGER, name: isMac() ? t.chat.fileManagerMac : t.chat.fileManager, icon: null },
  ];
  // 没选过（或选的那个已卸载）时默认第一个编辑器；一个都没装就是文件管理器。
  const current = options.find((option) => option.id === preferred) ?? options[0];
  const openWith = (id: string) => {
    const opening = id === FILE_MANAGER ? openProjectDir(cwd) : openFolderIn(cwd, id);
    opening.catch((error) => onError(formatBackendError(error, t.locale)));
  };
  return (
    <div className="dd tb-open" ref={ref} onKeyDown={onKeyDown}>
      <button
        type="button"
        className="tb-open-main"
        data-tip={t.chat.openWith(current.name)}
        aria-label={t.chat.openWith(current.name)}
        onClick={() => openWith(current.id)}
      >
        <OpenerIcon opener={current} />
      </button>
      <button
        ref={btnRef}
        type="button"
        className={"tb-open-chev" + (open ? " open" : "")}
        data-tip={t.chat.chooseOpener}
        aria-label={t.chat.chooseOpener}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={toggle}
      >
        <ChevronDownIcon />
      </button>
      {open && (
        <div className="dd-menu tb-open-menu" role="menu" ref={menuRef} style={{ position: "fixed", top: pos.top, bottom: pos.bottom, right: pos.right }}>
          {options.map((option) => (
            <button
              type="button"
              role="menuitem"
              key={option.id}
              className="dd-item"
              onClick={() => {
                // 选了就记住并直接打开（对照 Kimi）：下次点主钮就是它。
                setPreferred(option.id);
                saveOpener(option.id);
                setOpen(false);
                openWith(option.id);
              }}
            >
              <span className="dd-val">
                <span className="dd-ico"><OpenerIcon opener={option} /></span>
                <span className="dd-label">{option.name}</span>
              </span>
              {option.id === current.id && <CheckIcon className="dd-check" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/// 当前分支的 Pull Request（对照 Kimi 标题栏的「打开 Pull Request」+ 状态）：编号 + 状态，
/// 按状态配色（开着绿、草稿灰、已合并紫、已关闭红），点一下在浏览器打开。
export function PullRequestChip({ pr }: { pr: PullRequestDto }) {
  const t = useT();
  const tip = t.chat.prOpenTip(pr.number, pr.title);
  return (
    <button type="button" className={`tb-pr is-${pr.state}`} aria-label={tip} data-tip={tip} onClick={() => void openLink(pr.url).catch(() => {})}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <circle cx="6" cy="6" r="2.2" /><circle cx="6" cy="18" r="2.2" /><circle cx="18" cy="18" r="2.2" />
        <path d="M6 8.2v7.6M18 15.8V9a3 3 0 0 0-3-3h-4M13 3.5 10.5 6 13 8.5" />
      </svg>
      <span>#{pr.number}</span>
      <span className="tb-pr-state">{t.chat.prState[pr.state] ?? pr.state}</span>
    </button>
  );
}

export function GitStatusChip({ summary, active, onToggle }: {
  summary: GitDiffSummaryDto;
  active: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  const files = summary.files.length;
  const additions = summary.files.reduce((sum, file) => sum + (file.additions ?? 0), 0);
  const deletions = summary.files.reduce((sum, file) => sum + (file.deletions ?? 0), 0);
  const ahead = summary.ahead ?? 0;
  const behind = summary.behind ?? 0;
  const tip = [
    summary.branch,
    ahead > 0 ? t.chat.branchAhead(ahead) : null,
    behind > 0 ? t.chat.branchBehind(behind) : null,
    files > 0 ? t.chat.uncommitted(files) : null,
  ].filter(Boolean).join(" · ") || t.chat.diff;
  return (
    <button
      type="button"
      className={"tb-git" + (active ? " is-active" : "")}
      aria-label={t.chat.diff}
      aria-pressed={active}
      data-tip={tip}
      onClick={onToggle}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" />
        <path d="M6 7v10" /><path d="M18 9c0 5-6 5-11.5 8" />
      </svg>
      {summary.branch && <span className="tb-git-branch">{summary.branch}</span>}
      {ahead > 0 && <span className="tb-git-pill is-ahead">↑{ahead}</span>}
      {behind > 0 && <span className="tb-git-pill is-behind">↓{behind}</span>}
      {/* 二进制文件等没有行数时只报文件数，不写一个误导的 +0 -0。 */}
      {files > 0 && (additions + deletions > 0
        ? <span className="tb-git-pill is-diff"><span className="tb-git-add">+{additions}</span><span className="tb-git-del">-{deletions}</span></span>
        : <span className="tb-git-pill">{files}</span>)}
    </button>
  );
}
