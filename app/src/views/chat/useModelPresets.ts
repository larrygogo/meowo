/**
 * 模型清单的三个来源合流（从 ChatWindow.tsx 原样抽出，行为不变）：
 * 1) CLI 自举清单（`opencode models` 一类非交互子命令，agentModels）——最优先；
 * 2) 从 CLI 菜单学到的真实标签（modelLabels，随版本存取）；
 * 3) 插件内置别名（chatUi.model_presets）作为没学到之前的兜底。
 * 静默探测/菜单编排仍在 ChatWindow（与 sendText、终端 attention 交织），这里只管数据。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { agentModelCatalog, agentModels, type ChatUi, type ModelCatalogEntry, type ModelPreset } from "../../api";
import type { TerminalAttentionOption } from "../../terminalAttention";
import { applyLearnedLabels, loadLearnedLabels, saveLearnedLabels } from "./modelLabels";

export function useModelPresets(provider: string | null, chatUi: ChatUi | null, sessionId?: number) {
  // 模型清单：别名来自插件（CLI 文档化的稳定契约），**标签**优先用从 CLI 菜单学到的真实
  // 文案（见 modelLabels）——插件里的内置标签只是没学到之前的兜底，它必随 CLI 改版漂移。
  const [learnedModelLabels, setLearnedModelLabels] = useState<string[] | null>(null);
  const version = chatUi?.version ?? null;
  useEffect(() => {
    setLearnedModelLabels(loadLearnedLabels(provider, version));
  }, [provider, version]);
  const learnModelLabels = useCallback((options: TerminalAttentionOption[]) => {
    const labels = options.map((option) => option.label).filter(Boolean);
    saveLearnedLabels(provider, version, labels);
    setLearnedModelLabels(labels);
  }, [provider, version]);
  // CLI 能自己列举模型时优先用它（`opencode models` 这类非交互子命令）：一问就有，
  // 不必往会话 PTY 发命令弹 TUI 菜单、也不依赖屏幕识别取证。id 即 CLI 接受的模型串，
  // 标签直接用它（这些 CLI 的清单本就是 `provider/model` 形式的标识，没有另一套展示名）。
  const listKey = `${provider ?? ""}\0${version ?? ""}\0${sessionId ?? ""}`;
  const [listing, setListing] = useState<{ key: string; models: string[] }>({ key: "", models: [] });
  const listedModels = listing.key === listKey ? listing.models : [];
  useEffect(() => {
    // OpenCode 的 models 子命令只能列举，不能因此把选择操作改成 Claude 的 /model id。
    if (!provider || !chatUi?.model_select_command) return;
    let cancelled = false;
    agentModels(provider, sessionId)
      .then((models) => { if (!cancelled) setListing({ key: listKey, models }); })
      .catch(() => { if (!cancelled) setListing({ key: listKey, models: [] }); });
    return () => { cancelled = true; };
  }, [provider, sessionId, listKey, chatUi?.model_select_command]);
  // 菜单型 CLI（kimi：/model 不接受参数）的模型目录直接读它的配置文件（对照 Kimi 桌面版）：
  // 菜单一打开就列出，不必先让 CLI 弹一次菜单、再靠屏幕识别读回来。选中时仍走 CLI 菜单。
  const [catalog, setCatalog] = useState<{ key: string; models: ModelCatalogEntry[] }>({ key: "", models: [] });
  const modelCatalog = catalog.key === listKey ? catalog.models : [];
  const menuOnly = !!chatUi && !chatUi.model_select_command && !!chatUi.model_menu_command;
  // 能内联选择的（claude）也读目录：不是为了列清单，是为了把内置标签换成装着的那一版的
  // 真实显示名（插件写着 Opus 5，CLI 早已是 Opus 5.5——用户实拍「模型名不准」）。
  const wantsCatalog = menuOnly || !!chatUi?.model_select_command;
  useEffect(() => {
    if (!provider || !wantsCatalog) return;
    let cancelled = false;
    agentModelCatalog(provider, sessionId)
      .then((models) => { if (!cancelled) setCatalog({ key: listKey, models }); })
      .catch(() => { if (!cancelled) setCatalog({ key: listKey, models: [] }); });
    return () => { cancelled = true; };
  }, [provider, sessionId, listKey, wantsCatalog]);
  const modelPresets = useMemo<ModelPreset[]>(() => {
    if (listedModels.length > 0) return listedModels.map((id) => ({ id, label: id }));
    return applyCatalogLabels(
      applyLearnedLabels(chatUi?.model_presets ?? [], loadLearnedLabels(provider, version)),
      menuOnly ? [] : modelCatalog,
    );
  // eslint-disable-next-line react-hooks/exhaustive-deps -- learnedModelLabels 是学到新标签后的重算信号（标签本身从 localStorage 读）；listedModels/modelCatalog 未命中时是新的空数组，多算一次很便宜
  }, [listedModels, chatUi?.model_presets, learnedModelLabels, provider, version, menuOnly, modelCatalog]);
  const modelMenuCommand = chatUi?.model_menu_command ?? null;
  // 能直接下拉 = 清单已知（内置别名或学到的真实清单），且标签已是真的/该 CLI 无菜单可学。
  const modelDropdownReady = !!chatUi?.model_select_command && modelPresets.length > 0
    && (listedModels.length > 0 || learnedModelLabels !== null || !modelMenuCommand);
  // 目录只对菜单型 CLI 当清单用；能内联选择的已经折进 modelPresets 的标签里，再单列会重复。
  return { modelPresets: chatUi?.model_select_command ? modelPresets : [], modelCatalog: menuOnly ? modelCatalog : [], modelMenuCommand, modelDropdownReady, learnModelLabels };
}

/// 用目录里的家族显示名改写预设标签（claude：`opus` → `Opus 5.5`，`opus[1m]` → `Opus 5.5 (1M)`）。
/// 目录里没有的预设（opusplan 这类组合别名）保留原标签。
export function applyCatalogLabels<T extends { id: string; label: string }>(presets: T[], catalog: readonly ModelCatalogEntry[]): T[] {
  if (catalog.length === 0) return presets;
  const byFamily = new Map(catalog.map((entry) => [entry.id, entry.label]));
  return presets.map((preset) => {
    const long = /\[1m\]$/i.test(preset.id);
    const name = byFamily.get(long ? preset.id.replace(/\[1m\]$/i, "") : preset.id);
    return name ? { ...preset, label: long ? `${name} (1M)` : name } : preset;
  });
}
