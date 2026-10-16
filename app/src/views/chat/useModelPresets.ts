/**
 * 模型清单的三个来源合流（从 ChatWindow.tsx 原样抽出，行为不变）：
 * 1) CLI 自举清单（`opencode models` 一类非交互子命令，agentModels）——最优先；
 * 2) 从 CLI 菜单学到的真实标签（modelLabels，随版本存取）；
 * 3) 插件内置别名（chatUi.model_presets）作为没学到之前的兜底。
 * 静默探测/菜单编排仍在 ChatWindow（与 sendText、终端 attention 交织），这里只管数据。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { agentModels, type ChatUi, type ModelPreset } from "../../api";
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
  const modelPresets = useMemo<ModelPreset[]>(() => {
    if (listedModels.length > 0) return listedModels.map((id) => ({ id, label: id }));
    return applyLearnedLabels(chatUi?.model_presets ?? [], loadLearnedLabels(provider, version));
  }, [listedModels, chatUi?.model_presets, learnedModelLabels, provider, version]);
  const modelMenuCommand = chatUi?.model_menu_command ?? null;
  // 能直接下拉 = 清单已知（内置别名或学到的真实清单），且标签已是真的/该 CLI 无菜单可学。
  const modelDropdownReady = !!chatUi?.model_select_command && modelPresets.length > 0
    && (listedModels.length > 0 || learnedModelLabels !== null || !modelMenuCommand);
  return { modelPresets: chatUi?.model_select_command ? modelPresets : [], modelMenuCommand, modelDropdownReady, learnModelLabels };
}
