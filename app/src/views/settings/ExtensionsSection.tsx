/// 设置 › 扩展（对照 Kimi 的「插件」页）：各 agent 在用户级装了哪些技能、MCP 服务、
/// 自定义命令、子代理、插件、钩子。只读——增删改别家工具的配置有写坏的风险，这一版只看。
///
/// 和账号页一样一次只看一个 agent（顶部下拉切换、记住上次选的）：每家几十个技能摞在一起
/// 要滚很久。位置与扫描规则在后端 meowo_agent::extensions。
import { useCallback, useEffect, useState } from "react";
import { listAgentExtensions, revealPathInFileManager } from "../../api";
import type { AgentExtensionsDto } from "../../generated/contracts/AgentExtensionsDto";
import type { ExtensionGroupDto } from "../../generated/contracts/ExtensionGroupDto";
import type { ExtensionKind } from "../../generated/contracts/ExtensionKind";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { agentAssets } from "../../providers";
import { Dropdown } from "../menu";
import { SettingsError } from "./widgets";

const PICKED_KEY = "meowo-extensions-agent";

/** 组的显示顺序：先是最常用的技能和 MCP，钩子放最后（大多是 Meowo 自己的）。 */
const KIND_ORDER: ExtensionKind[] = ["skill", "mcp", "command", "agent", "plugin", "hook"];

function readPicked(): string | null {
  try { return localStorage.getItem(PICKED_KEY); } catch { return null; }
}

/** 路径拆成「所在目录 + 名字」：定位一个目录 / 配置文件 = 在它的上级里选中它。 */
function splitPath(path: string): [string, string] {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at < 0 ? [path, ""] : [path.slice(0, at), path.slice(at + 1)];
}

/** 摘要里计数的条目：Meowo 自己装的（状态上报钩子、上报插件）不算，那不是用户装的。 */
function ownCount(groups: ExtensionGroupDto[], kind: ExtensionKind): number {
  return groups.filter((g) => g.kind === kind).reduce((sum, g) => sum + g.items.filter((i) => !i.managed).length, 0);
}

export function ExtensionsSection() {
  const t = useT();
  const [list, setList] = useState<AgentExtensionsDto[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(readPicked);
  const load = useCallback(() => {
    listAgentExtensions()
      .then((next) => { setList(next); setFailed(false); })
      .catch(() => setFailed(true));
  }, []);
  // 窗口聚焦重读：常见用法是切去终端装个技能、改个 MCP，再切回来看。
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);

  const pick = (id: string) => {
    setPicked(id);
    try { localStorage.setItem(PICKED_KEY, id); } catch { /* 隐私模式：只是不记 */ }
  };
  const reveal = (dir: string, name: string) => {
    revealPathInFileManager(dir, name).catch((e) => setError(formatBackendError(e, t.locale)));
  };

  if (failed && !list) {
    return (
      <div className="sec-hint proxy-err" role="alert">
        {t.extensions.loadFailed}
        <button type="button" className="provider-card-action" style={{ marginLeft: 8 }} onClick={load}>{t.sticker.retry}</button>
      </div>
    );
  }
  if (!list) return null;
  if (list.length === 0) return <div className="sec-hint">{t.extensions.noAgents}</div>;

  const cur = list.find((a) => a.provider === picked) ?? list[0];
  const groups = cur.groups
    .filter((g) => g.items.length > 0)
    .sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind));
  const summary = KIND_ORDER
    .map((kind) => [kind, ownCount(cur.groups, kind)] as const)
    .filter(([, n]) => n > 0)
    .map(([kind, n]) => t.extensions.counts[kind](n))
    .join(" · ");
  const [dataParent, dataName] = splitPath(cur.dataDir);

  return (
    <>
      <SettingsError error={error} onDismiss={() => setError(null)} />
      <div className="account-agent-switch">
        {/* 一次只渲染当前 agent：别家的名字挂一行读屏隐藏文本，设置搜索搜「codex」也能命中本页
            （同账号页的 7S-4 做法；搜索的行级过滤只认 .row）。 */}
        <div className="row chat-sronly" aria-hidden="true">{list.map((a) => a.displayName).join(" ")}</div>
        <Dropdown
          value={cur.provider}
          options={list.map((a) => {
            const { Icon, tint } = agentAssets(a.provider);
            return {
              value: a.provider,
              label: a.displayName,
              icon: <span style={{ display: "flex", color: tint ? `var(${tint})` : undefined }}><Icon /></span>,
            };
          })}
          onChange={pick}
        />
      </div>
      <div className="ext-summary">
        <span>{summary || t.extensions.empty}</span>
        <button type="button" className="ext-link" onClick={() => reveal(dataParent, dataName)}>{t.extensions.openDataDir}</button>
      </div>

      {groups.map((group) => {
        const [parent, name] = splitPath(group.source);
        const own = group.items.filter((i) => !i.managed);
        const managed = group.items.filter((i) => i.managed);
        return (
          <div key={`${group.kind}-${group.source}`} className="ext-group">
            <div className="sec-caption ext-caption">
              <span>
                {t.extensions.kinds[group.kind]}
                {group.shared && <span className="ext-faint"> · {t.extensions.shared}</span>}
              </span>
              <button type="button" className="ext-link" data-tip={group.source} onClick={() => reveal(parent, name)}>{t.extensions.reveal}</button>
            </div>
            <div className="row-card ext-card">
              {own.map((item) => (
                <div className="row" key={`${item.name}-${item.rel ?? ""}`}>
                  <div className="row-text">
                    <div className="row-label">{item.name}</div>
                    {/* 钩子没有描述，说明挂了几条命令（命令本身不显示：可能带 token）。 */}
                    {group.kind === "hook"
                      ? <div className="row-desc">{t.extensions.hookCommands(Number(item.detail ?? 1))}</div>
                      : item.description && <div className="row-desc">{item.description}</div>}
                  </div>
                  <div className="ext-meta">
                    {group.kind !== "hook" && item.detail && <span className="ext-chip">{item.detail}</span>}
                    {item.enabled === false && <span className="ext-chip is-off">{t.extensions.off}</span>}
                    {item.rel && (
                      <button
                        type="button"
                        className="ext-icon-btn"
                        aria-label={t.extensions.reveal}
                        data-tip={t.extensions.reveal}
                        onClick={() => reveal(group.source, item.rel!)}
                      >
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                          <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
                        </svg>
                      </button>
                    )}
                  </div>
                </div>
              ))}
              {/* Meowo 自己装的（看板靠它们拿会话状态）合成一行，标明别删，免得被当成第三方清掉。 */}
              {managed.length > 0 && (
                <div className="row">
                  <div className="row-text">
                    <div className="row-label">{t.extensions.managedName}<span className="ext-chip is-meowo">{t.extensions.managedTag}</span></div>
                    <div className="row-desc">
                      {group.kind === "hook" ? t.extensions.managedHooksDesc(managed.length) : t.extensions.managedDesc}
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
        );
      })}
      <div className="sec-hint">{t.extensions.readOnlyHint}</div>
    </>
  );
}
