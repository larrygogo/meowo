import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  createTeamTaste,
  openTeamPersonal,
  saveTeamPersonalConfig,
  teamPersonalConfig,
  type SaveTeamPersonalConfigArgs,
  type TeamPersonalConfigDto,
} from "../../api";
import { busyProps } from "../../busyButton";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { Icon } from "../../icons/Icon";
import { icAlert, icPlus } from "../../icons/glyphs";
import { icArrowUpRight, icChevronLeft, icChevronsUpDown, icFileText, icShieldLock } from "../../icons/glyphs.chat";
import { ActionMenu } from "../menu";

/**
 * 团队设置子页（设计稿 designer-1 的 C 版「一句话 + 权限梯」，leader 2026-11-25「先这样做」）。
 *
 * - 数据只有一份：个人层 config.json。改动先留在页面上，底部出现「改了 N 处，还没写进文件」，点「保存」
 *   才写（品味清单 I2：设置页要有明确的保存和放弃）。写前比对读到时的指纹；文件在这之后被别处改过
 *   （编辑器、team-learn），不覆盖，给出「用文件里的 / 载入后叠上我的改动」，叠上去就是在新内容上把
 *   这几处改动按顺序重放一遍。
 * - 底线五条按内置清单渲染、锁死，不从文件读文字；文件里把底线写进宽松档的，那一条标「这里不生效」，
 *   照样能换档、能删。
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

function applyAll(dto: TeamPersonalConfigDto, edits: Edit[]): SaveTeamPersonalConfigArgs {
  return edits.reduce((c, e) => e.apply(c), argsOf(dto));
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
  const tierChanged = TIERS.some((k) => before[k].join("\n") !== after[k].join("\n"));
  if (tierChanged) parts.push(p.diffTiers);
  if (!after.exists) parts.push(p.diffDeleted);
  return parts.length ? parts.join(sep(t)) : p.diffOther;
}

function sep(t: ReturnType<typeof useT>): string {
  return t.locale.startsWith("zh") ? "，" : ", ";
}

/** 冲突提示里「你这里」那一行：列出前两处改动，其余合成「等 N 处」。 */
function describeEdits(edits: Edit[], t: ReturnType<typeof useT>): string {
  const shown = edits.slice(0, 2).map((e) => e.describe).join(sep(t));
  return edits.length > 2 ? shown + " " + t.settings.teamPage.andMore(edits.length) : shown;
}

/**
 * 缺逗号时 serde 报的是下一行开头（「expected `,` or `}`」），真正漏写的是上一行末尾。
 * 这种情况把上一处非空行也标出来，免得用户盯着没错的那一行找。
 */
function badLines(dto: TeamPersonalConfigDto): Set<number> {
  const bad = new Set<number>();
  if (!dto.errorLine) return bad;
  bad.add(dto.errorLine);
  if (/expected `,`/.test(dto.error ?? "")) {
    for (let n = dto.errorLine - 1; n >= dto.errorFirstLine; n--) {
      if (dto.errorLines[n - dto.errorFirstLine]?.trim()) {
        bad.add(n);
        break;
      }
    }
  }
  return bad;
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
  const [pending, setPending] = useState<Edit[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{ theirs: TeamPersonalConfigDto; mine: TeamPersonalConfigDto } | null>(null);
  const quiet = useRef(true);
  quiet.current = pending.length === 0 && !saving && !conflict;

  const reload = useCallback(async () => {
    try {
      setDto(await teamPersonalConfig());
      setError(null);
    } catch (e) {
      setError(formatBackendError(e, t.locale));
    }
  }, [t.locale]);

  useEffect(() => { void reload(); }, [reload]);
  // 文件可能在编辑器里被改（写坏后修好、team-learn 追加）：窗口回到前台时重新读。
  // 页面上有没保存的改动时不读，免得把用户正看着的东西换掉；保存时指纹比对会发现冲突。
  useEffect(() => {
    const onFocus = () => { if (quiet.current) void reload(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [reload]);

  /** 把 edits 叠在 base 上写进文件。成功后只清掉写进去的那几处，保存期间新做的改动留着。 */
  const save = async (base: TeamPersonalConfigDto, edits: Edit[]) => {
    if (saving) return;
    setSaving(true);
    setError(null);
    try {
      const next = await saveTeamPersonalConfig(applyAll(base, edits));
      setDto(next);
      setPending((cur) => cur.slice(edits.length));
    } catch (e) {
      if (String(e) === "team/config_changed") {
        try {
          setConflict({ theirs: await teamPersonalConfig(), mine: base });
        } catch (inner) {
          setError(formatBackendError(inner, t.locale));
        }
      } else {
        setError(formatBackendError(e, t.locale));
      }
    } finally {
      setSaving(false);
    }
  };
  const change = (describe: string, apply: Edit["apply"]) => {
    if (dto && !dto.error) setPending((cur) => [...cur, { describe, apply }]);
  };

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
    const bad = badLines(dto);
    return (
      <div className="tp-page">
        {head}
        <p className="tpc-say is-muted">{p.brokenSay}</p>
        <div className="tp-note is-err" role="alert">
          <div className="tp-note-title"><Icon glyph={icAlert} size={16} /><span>{dto.errorLine ? p.brokenAt(dto.errorLine) : dto.error}</span></div>
          {dto.errorLines.length > 0 && (
            <pre className="tp-code">
              {dto.errorLines.map((line, i) => (
                <div key={i} className={bad.has(first + i) ? "is-bad" : undefined}><i>{first + i}</i>{line}</div>
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

  // 页面上显示的是「文件内容 + 还没保存的改动」。
  const view = applyAll(dto, pending);
  const misplaced = new Set(dto.floorMisplaced);
  const lists: Record<Tier, string[]> = { doAndReport: view.doAndReport, askAtEnd: view.askAtEnd, askFirst: view.askFirst };
  const voidIn = (tier: Tier, item: string) => tier !== "askFirst" && misplaced.has(item);
  const live = (tier: Tier) => lists[tier].filter((x) => !voidIn(tier, x));
  const stillMisplaced = [...lists.doAndReport, ...lists.askAtEnd].filter((x) => misplaced.has(x));
  const floor = p.floorItems;
  const taste = view.taste ?? null;
  const tasteExists = taste === dto.taste ? dto.tasteExists : true;

  const count = (n: number, zero: string, some: (n: number) => string) => (n === 0 ? zero : some(n));
  const tok = (label: ReactNode, target: keyof typeof sections, cls = "") => (
    <button type="button" className={"tpc-tok" + (cls ? " " + cls : "")} onClick={() => jump(target)}>{label}</button>
  );
  const extraFirst = live("askFirst").length;
  const sentence = p.say({
    direct: tok(count(live("doAndReport").length, p.noneThings, p.things), "tiers"),
    end: tok(count(live("askAtEnd").length, p.noneThings, p.things), "tiers"),
    floor: tok(<><Icon glyph={icShieldLock} size={12} />{extraFirst ? p.floorPlus(floor.length, extraFirst) : p.floorN(floor.length)}</>, "tiers", "is-lock"),
    taste: tok(taste ? basename(taste) : p.noTaste, "taste", taste && tasteExists ? "" : "is-warn"),
    dirs: tok(count(view.protected.length, p.anyDir, p.dirsN), "dirs"),
    misplaced: stillMisplaced.length ? tok(<><Icon glyph={icAlert} size={12} />{p.misplacedN(stillMisplaced.length)}</>, "tiers", "is-warn") : null,
    hasTaste: !!taste,
    hasDirs: view.protected.length > 0,
  });

  const moveTo = (item: string, from: Tier, to: Tier) =>
    change(p.movedDesc(item, p.tierName[to]), (c) => {
      const i = c[from].indexOf(item);
      if (i < 0) return c;
      return { ...c, [from]: c[from].filter((_, j) => j !== i), [to]: [...c[to], item] };
    });
  const removeAt = (item: string, tier: Tier, index: number) =>
    change(p.removedDesc(item), (c) => ({ ...c, [tier]: c[tier].filter((x, j) => !(j === index && x === item)) }));

  return (
    <div className="tp-page">
      {head}
      <p className="tpc-say">{sentence.map((part, i) => <Fragment key={i}>{part}</Fragment>)}</p>

      {!dto.exists && pending.length === 0 && (
        <div className="tp-note is-info">
          <div className="tp-note-title"><Icon glyph={icFileText} size={16} /><span>{p.missingTitle}</span></div>
          <div className="tp-note-body">{p.missingBody}</div>
          <div className="tp-note-actions">
            <button type="button" className="sbtn btn-primary" {...busyProps(saving, () => void save(dto, []))}>{p.create}</button>
          </div>
        </div>
      )}
      {stillMisplaced.length > 0 && (
        <div className="tp-note is-warn" role="status">
          <div className="tp-note-title"><Icon glyph={icShieldLock} size={16} /><span>{p.misplacedTitle(stillMisplaced.length)}</span></div>
          <div className="tp-note-body">
            <ul>{stillMisplaced.map((item, i) => <li key={i}>{p.misplacedItem(item, p.tierName[lists.doAndReport.includes(item) ? "doAndReport" : "askAtEnd"])}</li>)}</ul>
          </div>
          <div className="tp-note-actions">
            <button
              type="button"
              className="sbtn"
              onClick={() =>
                change(p.moveBackDesc, (c) => {
                  const moved = [...c.doAndReport, ...c.askAtEnd].filter((x) => misplaced.has(x));
                  return {
                    ...c,
                    doAndReport: c.doAndReport.filter((x) => !misplaced.has(x)),
                    askAtEnd: c.askAtEnd.filter((x) => !misplaced.has(x)),
                    askFirst: [...c.askFirst, ...moved.filter((x) => !c.askFirst.includes(x))],
                  };
                })
              }
            >{p.moveBack}</button>
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
                {lists[tier].map((item, i) => {
                  const isVoid = voidIn(tier, item);
                  return (
                    <div key={`${i}:${item}`} className={"tpc-it" + (isVoid ? " is-void" : "")} title={isVoid ? p.voidTip : undefined}>
                      {isVoid && <span className="tpc-grip"><Icon glyph={icShieldLock} size={12} /></span>}
                      <span className="tp-item-text">{item}</span>
                      {isVoid && <span className="tp-tag is-warn">{p.voidTag}</span>}
                      <ActionMenu
                        glyph={icChevronsUpDown}
                        label={p.move}
                        items={[
                          ...TIERS.filter((to) => to !== tier).map((to) => ({ key: `to-${to}`, label: p.moveTo(p.tierName[to]), onSelect: () => moveTo(item, tier, to) })),
                          { key: "del", label: p.remove, danger: true, onSelect: () => removeAt(item, tier, i) },
                        ]}
                      />
                    </div>
                  );
                })}
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
        <div className="sec-caption tp-cap">{p.dirs}<span className="tp-count">{view.protected.length}</span></div>
        <div className="row-card">
          <div className="row tp-dirs">
            {view.protected.length > 0 ? (
              <div className="tp-chips">
                {view.protected.map((d, i) => (
                  <span key={`${i}:${d}`} className="tp-chip">
                    {d}
                    <button type="button" className="tp-chip-x" aria-label={p.removeDir(d)} onClick={() => change(p.dirRemovedDesc(d), (c) => ({ ...c, protected: c.protected.filter((x, j) => !(j === i && x === d)) }))}>×</button>
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
                <div className="row-label">{taste ? basename(taste) : p.noTaste}</div>
                {taste && <div className="tp-path">{dirname(taste)}</div>}
                <div className={"row-desc" + (taste && !tasteExists ? " is-error" : "")}>
                  {taste ? (tasteExists ? p.tasteDesc : p.tasteMissing) : p.tasteNone}
                </div>
              </div>
            </div>
            <div className="tp-btns">
              {taste && tasteExists && taste === dto.taste && <button type="button" className="sbtn" onClick={() => void openTeamPersonal("taste")}>{p.open}</button>}
              {!taste && (
                <button
                  type="button"
                  className="sbtn btn-primary"
                  {...busyProps(saving, () => {
                    // 新建品味清单会马上写文件；页面上没保存的改动留着，叠在新读到的内容上。
                    void createTeamTaste(dto.token).then(setDto, (e) => setError(formatBackendError(e, t.locale)));
                  })}
                >{p.newTaste}</button>
              )}
              <button
                type="button"
                className="sbtn"
                onClick={() => {
                  void openDialog({ multiple: false, directory: false, filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }] }).then((picked) => {
                    if (typeof picked === "string" && picked) change(p.tasteChangedDesc(basename(picked)), (c) => ({ ...c, taste: picked }));
                  });
                }}
              >{taste ? p.changeTaste : p.pickTaste}</button>
            </div>
          </div>
        </div>
      </section>

      {advanced}

      {error && <div className="row-desc is-error is-full" role="alert">{error}</div>}

      {conflict ? (
        <div className="tp-savebar tp-conflict" role="alertdialog" aria-label={p.conflictTitle}>
          <div className="tp-note-title"><Icon glyph={icAlert} size={16} /><span>{p.conflictTitle}</span></div>
          <dl>
            <dt>{p.conflictTheirs}</dt><dd>{diffSummary(conflict.mine, conflict.theirs, t)}</dd>
            {pending.length > 0 && <><dt>{p.conflictMine}</dt><dd>{describeEdits(pending, t)}</dd></>}
          </dl>
          <div className="tp-note-actions">
            <button type="button" className="sbtn" onClick={() => { setDto(conflict.theirs); setPending([]); setConflict(null); }}>{p.useFile}</button>
            <button
              type="button"
              className="sbtn btn-primary"
              disabled={!!conflict.theirs.error}
              {...busyProps(saving, () => { const { theirs } = conflict; setConflict(null); setDto(theirs); void save(theirs, pending); })}
            >{p.reapply}</button>
          </div>
        </div>
      ) : pending.length > 0 ? (
        <div className="tp-savebar" role="status">
          <span className="tp-savebar-text"><b>{p.dirty(pending.length)}</b></span>
          <button type="button" className="sbtn" {...busyProps(saving, () => setPending([]))}>{p.discard}</button>
          <button type="button" className="sbtn btn-primary" {...busyProps(saving, () => void save(dto, pending))}>{saving ? p.saving : p.save}</button>
        </div>
      ) : null}
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

/** 只在按回车时加入；失焦不加入（半句话被意外加进权限档，做空 redteam-1 问题 3）。写了字又失焦的保留输入框。 */
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
  const close = () => { setText(""); setEditing(false); };
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
          if (e.key === "Enter") { const v = text.trim(); if (v) onAdd(v); close(); }
          if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); }
        }}
        onBlur={() => { if (!text.trim()) close(); }}
      />
    </div>
  );
}
