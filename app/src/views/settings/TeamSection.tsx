import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  createTeamTaste,
  openTeamPersonal,
  saveTeamPersonalConfig,
  teamPersonalConfig,
  type SaveTeamPersonalConfigArgs,
  type TeamPersonalConfigDto,
} from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { Icon } from "../../icons/Icon";
import { icAlert, icPlus } from "../../icons/glyphs";
import { icArrowUpRight, icChevronLeft, icChevronsUpDown, icFileText, icShieldLock } from "../../icons/glyphs.chat";
import { ActionMenu } from "../menu";

/**
 * 团队设置子页（设计稿 designer-1 的 C 版「一句话 + 权限梯」，leader 2026-11-25「先这样做」）。
 *
 * - 数据只有一份：个人层 config.json。界面每改一处立即写文件（没有「保存」按钮），写前比对读到时的
 *   指纹；文件在这之后被别处改过（编辑器、team-learn），不覆盖，给出「用文件里的 / 载入后叠上我的改动」。
 *   每次只有一步改动，叠上去就是在新内容上重放这一步。
 * - 底线五条按内置清单渲染、锁死，不从文件读文字；文件里把底线写进宽松档的，那一条标「这里不生效」。
 * - 写坏的文件只读展示原因和原文，不提供任何会写文件的按钮。
 */

type Tier = "doAndReport" | "askAtEnd" | "askFirst";
const TIERS: Tier[] = ["doAndReport", "askAtEnd", "askFirst"];
type Edit = { describe: string; apply: (c: SaveTeamPersonalConfigArgs) => SaveTeamPersonalConfigArgs };

function argsOf(dto: TeamPersonalConfigDto): SaveTeamPersonalConfigArgs {
  return {
    token: dto.token,
    taste: dto.taste,
    protected: [...dto.protected],
    doAndReport: [...dto.doAndReport],
    askAtEnd: [...dto.askAtEnd],
    askFirst: [...dto.askFirst],
  };
}

/** 两次读到的内容差在哪（冲突提示里的「文件里」那一行）。只比界面管的几项。 */
function diffSummary(before: TeamPersonalConfigDto, after: TeamPersonalConfigDto, t: ReturnType<typeof useT>): string {
  const p = t.settings.teamPage;
  const parts: string[] = [];
  const added = after.protected.filter((x) => !before.protected.includes(x));
  const removed = before.protected.filter((x) => !after.protected.includes(x));
  if (added.length) parts.push(p.diffDirsAdded(added.join("、")));
  if (removed.length) parts.push(p.diffDirsRemoved(removed.join("、")));
  if ((before.taste ?? "") !== (after.taste ?? "")) parts.push(p.diffTaste);
  const tierChanged = (["doAndReport", "askAtEnd", "askFirst"] as const).some((k) => before[k].join("\n") !== after[k].join("\n"));
  if (tierChanged) parts.push(p.diffTiers);
  if (!after.exists) parts.push(p.diffDeleted);
  return parts.length ? parts.join(t.locale.startsWith("zh") ? "，" : ", ") : p.diffOther;
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
function dirname(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : "";
}

export function TeamSection({ onBack }: { onBack: () => void }) {
  const t = useT();
  const p = t.settings.teamPage;
  const [dto, setDto] = useState<TeamPersonalConfigDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{ edit: Edit; theirs: TeamPersonalConfigDto; mine: TeamPersonalConfigDto } | null>(null);
  const busy = useRef(false);

  const reload = useCallback(async () => {
    try {
      setDto(await teamPersonalConfig());
    } catch (e) {
      setError(formatBackendError(e, t.locale));
    }
  }, [t.locale]);

  useEffect(() => { void reload(); }, [reload]);
  // 文件可能在编辑器里被改（写坏后修好、team-learn 追加）：窗口回到前台时重新读，不用手动刷新。
  useEffect(() => {
    const onFocus = () => { if (!busy.current && !conflict) void reload(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload, conflict]);

  const commit = async (edit: Edit, base: TeamPersonalConfigDto) => {
    if (busy.current) return;
    busy.current = true;
    setError(null);
    try {
      setDto(await saveTeamPersonalConfig(edit.apply(argsOf(base))));
    } catch (e) {
      if (String(e) === "team/config_changed") {
        try {
          const theirs = await teamPersonalConfig();
          setConflict({ edit, theirs, mine: base });
        } catch (inner) {
          setError(formatBackendError(inner, t.locale));
        }
      } else {
        setError(formatBackendError(e, t.locale));
      }
    } finally {
      busy.current = false;
    }
  };
  const change = (describe: string, apply: Edit["apply"]) => { if (dto && !dto.error) void commit({ describe, apply }, dto); };

  const sections = { tiers: useRef<HTMLElement>(null), dirs: useRef<HTMLElement>(null), taste: useRef<HTMLElement>(null) };
  const jump = (key: keyof typeof sections) => sections[key].current?.scrollIntoView({ behavior: "smooth", block: "start" });

  const head = (
    <div className="tp-head">
      <button type="button" className="tp-back" aria-label={p.back} data-tip={p.back} onClick={onBack}>
        <Icon glyph={icChevronLeft} size={14} />
      </button>
      <span className="tp-crumb">{t.settings.nav.general} /</span>
      <span className="tp-title">{p.title}</span>
      <span className="tp-head-sp" />
      {dto?.exists && (
        <button type="button" className="tp-src" title={dto.configPath} onClick={() => void openTeamPersonal("config")}>
          <Icon glyph={icFileText} size={12} />
          <span className="mono">config.json</span>
          <Icon glyph={icArrowUpRight} size={12} />
        </button>
      )}
    </div>
  );

  if (!dto) {
    return <div className="tp-page">{head}{error && <div className="row-desc is-error is-full" role="alert">{error}</div>}</div>;
  }

  const advanced = (
    <section>
      <div className="sec-caption">{p.advanced}</div>
      <div className="row-card">
        <div className="row">
          <div className="row-text">
            <div className="row-label">{p.blocked}</div>
            <div className="row-desc">{p.blockedDesc(dto.blockedCommandCount)}</div>
          </div>
          <button type="button" className="sbtn" onClick={() => void openTeamPersonal("config")}>{p.editInFile}</button>
        </div>
        <div className="row">
          <div className="row-text">
            <div className="row-label">{p.folder}</div>
            <div className="tp-path">{dto.personalDir}</div>
          </div>
          <button type="button" className="sbtn" onClick={() => void openTeamPersonal("folder")}>{p.open}</button>
        </div>
      </div>
    </section>
  );

  // 写坏了：只读，摆出原因和原文，不给任何会写文件的按钮。
  if (dto.error) {
    const first = dto.errorFirstLine;
    return (
      <div className="tp-page">
        {head}
        <p className="tpc-say is-muted">{p.brokenSay}</p>
        <div className="tp-note is-err" role="alert">
          <div className="tp-note-title"><Icon glyph={icAlert} size={16} /><span>{dto.errorLine ? p.brokenAt(dto.errorLine) : dto.error}</span></div>
          {dto.errorLines.length > 0 && (
            <pre className="tp-code">
              {dto.errorLines.map((line, i) => (
                <div key={i} className={first + i === dto.errorLine ? "is-bad" : undefined}><i>{first + i}</i>{line}</div>
              ))}
            </pre>
          )}
          <div className="tp-note-body tp-mono-reason">{dto.error}</div>
          <div className="tp-note-body">{p.brokenBody}</div>
          <div className="tp-note-actions">
            <button type="button" className="sbtn btn-primary" onClick={() => void openTeamPersonal("config")}>{p.openToFix}</button>
            <button type="button" className="sbtn" onClick={() => void reload()}>{p.reload}</button>
          </div>
        </div>
        {advanced}
      </div>
    );
  }

  const misplaced = new Set(dto.floorMisplaced);
  const lists: Record<Tier, string[]> = { doAndReport: dto.doAndReport, askAtEnd: dto.askAtEnd, askFirst: dto.askFirst };
  const live = (tier: Tier) => lists[tier].filter((x) => !misplaced.has(x));
  const floor = p.floorItems;

  const count = (n: number, zero: string, some: (n: number) => string) => (n === 0 ? zero : some(n));
  const tok = (label: ReactNode, target: keyof typeof sections, cls = "") => (
    <button type="button" className={"tpc-tok" + (cls ? " " + cls : "")} onClick={() => jump(target)}>{label}</button>
  );
  const extraFirst = live("askFirst").length;
  const sentence = p.say({
    direct: tok(count(live("doAndReport").length, p.noneThings, p.things), "tiers"),
    end: tok(count(live("askAtEnd").length, p.noneThings, p.things), "tiers"),
    floor: tok(<><Icon glyph={icShieldLock} size={12} />{extraFirst ? p.floorPlus(floor.length, extraFirst) : p.floorN(floor.length)}</>, "tiers", "is-lock"),
    taste: tok(dto.taste ? basename(dto.taste) : p.noTaste, "taste", dto.taste && dto.tasteExists ? "" : "is-warn"),
    dirs: tok(count(dto.protected.length, p.anyDir, p.dirsN), "dirs"),
    misplaced: misplaced.size ? tok(<><Icon glyph={icAlert} size={12} />{p.misplacedN(misplaced.size)}</>, "tiers", "is-warn") : null,
    hasTaste: !!dto.taste,
    hasDirs: dto.protected.length > 0,
  });

  const moveItems = (item: string, from: Tier) =>
    TIERS.filter((to) => to !== from).map((to) => ({
      key: `to-${to}`,
      label: p.moveTo(p.tierName[to]),
      onSelect: () => change(p.movedDesc(item, p.tierName[to]), (c) => ({ ...c, [from]: c[from].filter((x) => x !== item), [to]: [...c[to], item] })),
    }));

  return (
    <div className="tp-page">
      {head}
      <p className="tpc-say">{sentence}</p>

      {!dto.exists && (
        <div className="tp-note is-info">
          <div className="tp-note-title"><Icon glyph={icFileText} size={16} /><span>{p.missingTitle}</span></div>
          <div className="tp-note-body">{p.missingBody}</div>
          <div className="tp-note-actions">
            <button type="button" className="sbtn btn-primary" onClick={() => change(p.createdDesc, (c) => c)}>{p.create}</button>
          </div>
        </div>
      )}
      {misplaced.size > 0 && (
        <div className="tp-note is-warn" role="status">
          <div className="tp-note-title"><Icon glyph={icShieldLock} size={16} /><span>{p.misplacedTitle(misplaced.size)}</span></div>
          <div className="tp-note-body">
            <ul>{dto.floorMisplaced.map((item) => <li key={item}>{p.misplacedItem(item, p.tierName[dto.doAndReport.includes(item) ? "doAndReport" : "askAtEnd"])}</li>)}</ul>
          </div>
          <div className="tp-note-actions">
            <button type="button" className="sbtn" onClick={() => change(p.moveBackDesc, (c) => ({ ...c, doAndReport: c.doAndReport.filter((x) => !misplaced.has(x)), askAtEnd: c.askAtEnd.filter((x) => !misplaced.has(x)) }))}>{p.moveBack}</button>
          </div>
        </div>
      )}

      <section ref={sections.tiers}>
        <div className="sec-caption">{p.tiers}</div>
        <div className="tpc-ladder">
          {TIERS.map((tier, idx) => (
            <div key={tier} className={"tpc-rung" + (idx === 1 ? " is-mid" : idx === 2 ? " is-strict" : "")}>
              <div className="tpc-rung-head">
                <span className="tpc-rung-name">{p.tierName[tier]}</span>
                <span className="tpc-rung-desc">{p.tierDesc[tier]}</span>
              </div>
              <div className="tpc-items">
                {lists[tier].map((item) =>
                  misplaced.has(item) ? (
                    <div key={item} className="tpc-it is-void" title={p.voidTip}>
                      <span className="tpc-grip"><Icon glyph={icShieldLock} size={12} /></span>
                      <span className="tp-item-text">{item}</span>
                      <span className="tp-tag is-warn">{p.voidTag}</span>
                    </div>
                  ) : (
                    <div key={item} className="tpc-it">
                      <span className="tp-item-text">{item}</span>
                      <ActionMenu
                        glyph={icChevronsUpDown}
                        label={p.move}
                        items={[
                          ...moveItems(item, tier),
                          { key: "del", label: p.remove, danger: true, onSelect: () => change(p.removedDesc(item), (c) => ({ ...c, [tier]: c[tier].filter((x) => x !== item) })) },
                        ]}
                      />
                    </div>
                  ),
                )}
                {tier === "askFirst" && (
                  <div className="tpc-bed">
                    <div className="tpc-bed-head"><Icon glyph={icShieldLock} size={12} /><span><b>{p.floorHead}</b> {p.floorHeadDesc}</span></div>
                    {floor.map((b) => (
                      <div key={b} className="tpc-it" title={p.lockTip}>
                        <span className="tpc-grip"><Icon glyph={icShieldLock} size={12} /></span>
                        <span className="tp-item-text">{b}</span>
                      </div>
                    ))}
                  </div>
                )}
                <AddItem label={p.addItem} placeholder={p.addItemPlaceholder} onAdd={(text) => change(p.addedDesc(text, p.tierName[tier]), (c) => ({ ...c, [tier]: [...c[tier], text] }))} />
              </div>
            </div>
          ))}
        </div>
        <p className="tp-hint">{p.tiersHint}</p>
      </section>

      <section ref={sections.dirs}>
        <div className="sec-caption tp-cap">{p.dirs}<span className="tp-count">{dto.protected.length}</span></div>
        <div className="row-card">
          <div className="row tp-dirs">
            {dto.protected.length > 0 ? (
              <div className="tp-chips">
                {dto.protected.map((d) => (
                  <span key={d} className="tp-chip">
                    {d}
                    <button type="button" className="tp-chip-x" aria-label={p.removeDir(d)} onClick={() => change(p.dirRemovedDesc(d), (c) => ({ ...c, protected: c.protected.filter((x) => x !== d) }))}>×</button>
                  </span>
                ))}
              </div>
            ) : (
              <div className="tp-empty">{p.dirsEmpty}</div>
            )}
            <AddDir placeholder={p.dirPlaceholder} button={p.add} onAdd={(d) => change(p.dirAddedDesc(d), (c) => ({ ...c, protected: [...c.protected, d] }))} />
          </div>
        </div>
        <p className="tp-hint">{p.dirsHint}</p>
      </section>

      <section ref={sections.taste}>
        <div className="sec-caption">{p.taste}</div>
        <div className="row-card">
          <div className="row">
            <div className="tp-file">
              <span className="tp-file-ico"><Icon glyph={icFileText} size={16} /></span>
              <div className="row-text">
                <div className="row-label">{dto.taste ? basename(dto.taste) : p.noTaste}</div>
                {dto.taste && <div className="tp-path">{dirname(dto.taste)}</div>}
                <div className={"row-desc" + (dto.taste && !dto.tasteExists ? " is-error" : "")}>
                  {dto.taste ? (dto.tasteExists ? p.tasteDesc : p.tasteMissing) : p.tasteNone}
                </div>
              </div>
            </div>
            <div className="tp-btns">
              {dto.taste && dto.tasteExists && <button type="button" className="sbtn" onClick={() => void openTeamPersonal("taste")}>{p.open}</button>}
              {!dto.taste && <button type="button" className="sbtn btn-primary" onClick={() => { const base = dto; void createTeamTaste(base.token).then(setDto, (e) => setError(formatBackendError(e, t.locale))); }}>{p.newTaste}</button>}
              <button
                type="button"
                className="sbtn"
                onClick={() => {
                  void openDialog({ multiple: false, directory: false, filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }] }).then((picked) => {
                    if (typeof picked === "string" && picked) change(p.tasteChangedDesc(basename(picked)), (c) => ({ ...c, taste: picked }));
                  });
                }}
              >{dto.taste ? p.changeTaste : p.pickTaste}</button>
            </div>
          </div>
        </div>
      </section>

      {advanced}

      {error && <div className="row-desc is-error is-full" role="alert">{error}</div>}

      {conflict && (
        <div className="tp-savebar tp-conflict" role="alertdialog" aria-label={p.conflictTitle}>
          <div className="tp-note-title"><Icon glyph={icAlert} size={16} /><span>{p.conflictTitle}</span></div>
          <dl>
            <dt>{p.conflictTheirs}</dt><dd>{diffSummary(conflict.mine, conflict.theirs, t)}</dd>
            <dt>{p.conflictMine}</dt><dd>{conflict.edit.describe}</dd>
          </dl>
          <div className="tp-note-actions">
            <button type="button" className="sbtn" onClick={() => { setDto(conflict.theirs); setConflict(null); }}>{p.useFile}</button>
            <button
              type="button"
              className="sbtn btn-primary"
              disabled={!!conflict.theirs.error}
              onClick={() => { const { edit, theirs } = conflict; setConflict(null); setDto(theirs); void commit(edit, theirs); }}
            >{p.reapply}</button>
          </div>
        </div>
      )}
    </div>
  );
}

function AddDir({ placeholder, button, onAdd }: { placeholder: string; button: string; onAdd: (d: string) => void }) {
  const [text, setText] = useState("");
  const submit = () => { const v = text.trim(); if (v) { onAdd(v); setText(""); } };
  return (
    <div className="tp-add">
      <input className="ns-input" value={text} placeholder={placeholder} aria-label={placeholder} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !(e.nativeEvent as KeyboardEvent).isComposing) submit(); }} />
      <button type="button" className="sbtn" disabled={!text.trim()} onClick={submit}>{button}</button>
    </div>
  );
}

function AddItem({ label, placeholder, onAdd }: { label: string; placeholder: string; onAdd: (text: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  if (!editing) {
    return (
      <button type="button" className="tp-addrow" onClick={() => setEditing(true)}>
        <Icon glyph={icPlus} size={12} />{label}
      </button>
    );
  }
  const done = (save: boolean) => { const v = text.trim(); if (save && v) onAdd(v); setText(""); setEditing(false); };
  return (
    <div className="tp-add tp-add-item">
      <input
        className="ns-input"
        autoFocus
        value={text}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if ((e.nativeEvent as KeyboardEvent).isComposing) return;
          if (e.key === "Enter") done(true);
          if (e.key === "Escape") { e.preventDefault(); done(false); }
        }}
        onBlur={() => done(true)}
      />
    </div>
  );
}
