import { memo, useMemo, useState } from "react";
import { useT } from "../../i18n";
import { agentAssets, tintStyle } from "../../providers";
import { AppModal } from "../AppModal";

export type ModelPickerItem = {
  id: string;
  label: string;
  desc?: string;
  provider: string;
  capabilities?: string[];
};

/// 模型市场（对照 Kimi 的 ModelPicker）：独立对话框、按提供商分组、搜索、收藏。
/// 收藏只是本机展示偏好，不写后端设置，也不随会话目录迁移。
const STARRED_KEY = "meowo-starred-models";

export function readStarred(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(STARRED_KEY) || "[]");
    return new Set(Array.isArray(raw) ? raw.filter((id): id is string => typeof id === "string") : []);
  } catch {
    return new Set();
  }
}

export const ModelPicker = memo(function ModelPicker({ items, current, currentProvider, onSelect, onClose }: {
  items: ModelPickerItem[];
  current?: string;
  currentProvider?: string;
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [starred, setStarred] = useState<Set<string>>(readStarred);
  const toggleStar = (id: string) => {
    setStarred((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      // 隐私模式禁写时仍保留本次打开期间的收藏态，不能让整颗按钮点击崩溃。
      try { localStorage.setItem(STARRED_KEY, JSON.stringify([...next])); } catch { /* 仅本次会话生效 */ }
      return next;
    });
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = items.filter((item) =>
      !q || item.label.toLowerCase().includes(q) || item.provider.toLowerCase().includes(q) || item.id.toLowerCase().includes(q),
    );
    return list.sort((a, b) => {
      const sa = starred.has(a.id) ? 0 : 1;
      const sb = starred.has(b.id) ? 0 : 1;
      if (sa !== sb) return sa - sb;
      return a.label.localeCompare(b.label);
    });
  }, [items, query, starred]);

  const groups = useMemo(() => {
    const map = new Map<string, ModelPickerItem[]>();
    for (const item of filtered) {
      const list = map.get(item.provider) ?? [];
      list.push(item);
      map.set(item.provider, list);
    }
    return [...map.entries()];
  }, [filtered]);

  return (
    <AppModal label={t.chat.switchModel} className="chat-model-picker" onClose={onClose}>
      <div className="chat-model-picker-head">
        <h3>{t.chat.switchModel}</h3>
        <button type="button" className="chat-modal-close" aria-label={t.chat.close} onClick={onClose}>×</button>
      </div>
      <div className="chat-model-picker-search">
        <input
          autoFocus
          value={query}
          placeholder={t.chat.modelSearchPlaceholder}
          aria-label={t.chat.modelSearchPlaceholder}
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>
      <div className="chat-model-picker-list">
        {groups.map(([provider, models]) => {
          const { Icon } = agentAssets(provider);
          return (
            <section key={provider} className="chat-model-picker-group">
              <div className="chat-model-picker-provider">
                <span className="chat-model-picker-provider-icon" style={tintStyle(provider)}><Icon /></span>
                <span>{provider === currentProvider ? provider : `${t.chat.switchProvider} · ${provider}`}</span>
              </div>
              {models.map((model) => (
                <div key={model.id} className={"chat-model-picker-item" + (current === model.label ? " is-current" : "")}>
                  <button
                    type="button"
                    role="menuitem"
                    className="chat-model-picker-item-select"
                    aria-label={model.label}
                    onClick={() => onSelect(model.id)}
                  >
                    <span className="chat-model-picker-item-main">
                      <span className="chat-model-picker-item-name">{model.label}</span>
                      {model.desc && <span className="chat-model-picker-item-desc">{model.desc}</span>}
                      {model.capabilities && model.capabilities.length > 0 && (
                        <span className="chat-model-picker-item-caps">
                          {model.capabilities.map((cap) => <span key={cap} className="chat-model-picker-cap">{cap}</span>)}
                        </span>
                      )}
                    </span>
                  </button>
                  <button
                    type="button"
                    className={"chat-model-picker-star" + (starred.has(model.id) ? " is-starred" : "")}
                    aria-label={starred.has(model.id) ? t.chat.modelUnstar : t.chat.modelStar}
                    onClick={() => toggleStar(model.id)}
                  >
                    {starred.has(model.id) ? "★" : "☆"}
                  </button>
                </div>
              ))}
            </section>
          );
        })}
        {filtered.length === 0 && <div className="chat-model-picker-empty">{t.chat.modelSearchEmpty}</div>}
      </div>
    </AppModal>
  );
});
