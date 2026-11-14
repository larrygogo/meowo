// 截图验收（任务 20261114-tui-model-switch）：四家原生 TUI 会话的模型 / 模式数据。
// 能力表与启动选项直接用 src/test/agents.ts（与后端插件声明同源的那份），模型清单与 CLI 默认值
// 照本机 CLI 配置与后端解析器的真实形状造（codex models_cache.json、kimi config.toml、
// gemini models.rs FULL 表、opencode `models --verbose` 的 provider/model）。
// ?state=idle|running|lead|open|notnow|ready 决定会话状态与切换结果；?modes=approval:yolo,collaboration:plan
// 覆盖会话当前档（第四轮 L6 拍手机各档图标）；?cfg=proxy 让 CLI 配置的模型不在清单里（第四轮 M1）。
import { chatUi, descriptors } from "../src/test/agents";
import { makeSession } from "../src/demo/data";
import { CHAT_SESSION_ID } from "./fixtures";
import frames from "../src/test/fixtures/composer-frames.json";

/** 输入框为空的真机帧（重启型切换前要先确认 TUI 输入框是空的）。 */
const EMPTY_FRAME: Partial<Record<TmsProvider, string>> = { codex: "codex-0.167-03", kimi: "kimi-2.6-k2-1", gemini: "gemini-0.65-empty" };

export type TmsProvider = "codex" | "kimi" | "gemini" | "opencode";
export const TMS_PROVIDERS: TmsProvider[] = ["codex", "kimi", "gemini", "opencode"];

const entry = (id: string, label: string, extra: Record<string, unknown> = {}) =>
  ({ id, label, group: null, efforts: [], default_effort: null, is_default: false, uncertain: false, ...extra });

const CATALOG: Record<TmsProvider, ReturnType<typeof entry>[]> = {
  codex: [
    entry("gpt-6.1-sol", "GPT-6.1-Sol"),
    entry("gpt-6-astra", "GPT-6-Astra"),
    entry("gpt-6.1-terra", "GPT-6.1 Terra"),
    entry("gpt-6-sol", "GPT-6-Sol"),
    entry("gpt-6-luna", "GPT-6-Luna"),
    entry("gpt-5.6-sol", "GPT-5.6-Sol"),
    entry("gpt-5.6-terra", "GPT-5.6-Terra", { is_default: true, default_effort: "high" }),
    entry("gpt-5.6-luna", "GPT-5.6-Luna"),
  ],
  kimi: [
    entry("kimi-code/kimi-for-coding", "K2.8 Preview", { group: "managed:kimi-code" }),
    entry("kimi-code/kimi-for-coding-highspeed", "K2.7 Code Highspeed", { group: "managed:kimi-code" }),
    entry("kimi-code/k3", "K3", { group: "managed:kimi-code", efforts: ["low", "medium", "high"] }),
    entry("kimi-code/k3-256k", "K3-256k", { group: "managed:kimi-code" }),
    entry("kimi-code/k3.1-pro", "K3.1 Pro", { group: "managed:kimi-code", efforts: ["low", "medium", "high", "max"], default_effort: "high", is_default: true }),
    entry("kimi-code/k3.1-pro-highspeed", "K3.1 Pro Highspeed 2x Usage Cost", { group: "managed:kimi-code" }),
  ],
  gemini: [
    entry("auto", "Auto", { is_default: true }),
    entry("gemini-3.1-pro-preview", "gemini-3.1-pro-preview"),
    entry("gemini-3-flash-preview", "gemini-3-flash-preview"),
    entry("gemini-2.5-pro", "gemini-2.5-pro"),
    entry("gemini-3.8-flash", "gemini-3.8-flash"),
    entry("gemini-3.5-flash-lite", "gemini-3.5-flash-lite"),
    entry("gemma-4-31b-it", "gemma-4-31b-it"),
    entry("gemma-4-26b-a4b-it", "gemma-4-26b-a4b-it"),
  ],
  opencode: [
    entry("tensorfold/Qwen3.8-Flash-Next", "Qwen3.8 Flash Next", { group: "tensorfold", is_default: true }),
    entry("tensorfold/Qwen3.8-Max", "Qwen3.8 Max", { group: "tensorfold" }),
    entry("opencode/big-pickle", "Big Pickle", { group: "opencode" }),
    entry("opencode/grok-code", "Grok Code Fast 1", { group: "opencode" }),
    entry("opencode/gpt-5.6-nano", "GPT-5.6 Nano", { group: "opencode" }),
  ],
};

const DEFAULTS: Record<TmsProvider, { model: string | null; choices: Record<string, string> }> = {
  codex: { model: "gpt-5.6-terra", choices: { approval: "ask" } },
  // Kimi 配置默认 yolo：「每步都问」没有启动参数，应显示为置灰（configLocked）。
  kimi: { model: "kimi-code/k3.1-pro", choices: { permission: "yolo" } },
  gemini: { model: null, choices: { approval: "default" } },
  opencode: { model: "tensorfold/Qwen3.8-Flash-Next", choices: { agent: "build" } },
};

const MODES: Record<TmsProvider, { dimension: string; value: string }[]> = {
  codex: [{ dimension: "approval", value: "ask" }, { dimension: "collaboration", value: "default" }],
  kimi: [{ dimension: "permission", value: "yolo" }, { dimension: "work", value: "default" }],
  gemini: [{ dimension: "approval", value: "default" }],
  opencode: [{ dimension: "agent", value: "build" }],
};

const MODEL_LABEL: Record<TmsProvider, string> = {
  codex: "GPT-5.6-Terra",
  kimi: "K3.1 Pro",
  gemini: "Auto",
  opencode: "Qwen3.8 Flash Next",
};

export function tmsProviderOf(scene: string): TmsProvider {
  const hit = TMS_PROVIDERS.find((p) => scene.endsWith("-" + p));
  if (!hit) throw new Error("未知 provider：" + scene);
  return hit;
}

/** 新建页 / 对话页共用：能力表、启动选项、模型清单、CLI 默认值、自定义 agent。 */
export function tmsCommonMock(provider: TmsProvider, cfg?: string | null) {
  return (cmd: string, args: unknown): unknown => {
    const p = (args as { provider?: string } | undefined)?.provider ?? provider;
    switch (cmd) {
      case "list_agents":
        return descriptors(["claude", "codex", "kimi", "gemini", "opencode"]);
      case "agent_chat_ui":
        return chatUi(p);
      case "agent_model_catalog":
        return CATALOG[p as TmsProvider] ?? [];
      case "agent_launch_defaults": {
        const defaults = DEFAULTS[p as TmsProvider] ?? { model: null, choices: {} };
        // 第三方 / 旧模型写在 CLI 配置里、不在清单里（做空 M1 的 my-proxy-model 用例）。
        return cfg === "proxy" ? { ...defaults, model: "my-proxy-model" } : defaults;
      }
      case "agent_dynamic_mode_choices":
        return p === "opencode" ? [{ option: "agent", id: "review", label: "review" }] : [];
      case "check_provider_hooks":
        return "installed";
      case "recent_cwds":
        return ["C:\\Users\\demo\\workspace\\web-app", "C:\\Users\\demo\\workspace\\billing-service"];
      default:
        return undefined;
    }
  };
}

/** 对话页：会话状态 + 切换结果。chat 是 chatMock(lang)。 */
export function tmsChatMock(provider: TmsProvider, state: string, chat: (cmd: string) => unknown, modes?: string | null) {
  const common = tmsCommonMock(provider);
  // ?modes=dim:value,dim:value 覆盖会话当前档。
  const overrides = Object.fromEntries((modes ?? "").split(",").filter(Boolean).map((pair) => pair.split(":") as [string, string]));
  const agentModes = MODES[provider].map((mode) => ({ ...mode, value: overrides[mode.dimension] ?? mode.value }));
  return (cmd: string, args: unknown): unknown => {
    const hit = common(cmd, args);
    if (hit !== undefined) return hit;
    switch (cmd) {
      case "get_chat_history": {
        const base = chat(cmd) as Record<string, unknown>;
        return {
          ...base,
          provider,
          status: state === "running" ? "running" : "waiting",
          pendingReview: null,
          connected: state !== "open",
          model: MODEL_LABEL[provider],
          effort: provider === "kimi" ? "high" : null,
          agentModes,
          contextPct: provider === "codex" || provider === "kimi" ? 18 : null,
          contextWindow: provider === "kimi" ? 262_144 : provider === "codex" ? 400_000 : null,
          items: (base.items as unknown[]).slice(0, -1),
        };
      }
      case "pending_interaction":
        return { approval: null, question: null };
      case "get_live_sessions_page": {
        const rows = chat(cmd) as ReturnType<typeof makeSession>[];
        return rows.map((row) => row.session.id === CHAT_SESSION_ID
          ? { ...row, provider, pending_review: null, session: { ...row.session, status: state === "running" ? "running" : "waiting" }, connected: state !== "open" }
          : row);
      }
      case "session_live_settings":
        return null;
      case "queue_session_settings": {
        const request = (args as { request: Record<string, unknown> }).request;
        const waitFor = state === "lead" ? "lead" : state === "open" ? "open" : undefined;
        return waitFor ? { ...request, waitFor } : request;
      }
      case "queued_session_settings":
      case "take_queued_session_settings":
        return null;
      case "apply_session_settings":
        if (state === "lead" || state === "open") return { kind: "queued", sessionId: CHAT_SESSION_ID, confirmed: false, reason: null };
        // 会话服务还没握手（刚打开的 Codex）：前端放回宿主队列，提示「会话就绪后切换到「X」」（做空 R5）。
        if (state === "ready") throw "settings/server_not_ready";
        if (state === "notnow") return { kind: "not_now", sessionId: CHAT_SESSION_ID, confirmed: false, reason: "settings/background_busy" };
        return { kind: "applied", sessionId: CHAT_SESSION_ID, confirmed: true, reason: null };
      case "managed_terminal_frame": {
        const frame = (frames as { frames: { name: string; lines: string[] }[] }).frames.find((f) => f.name === EMPTY_FRAME[provider]);
        return frame ? { lines: frame.lines, cursorRow: 0, cursorCol: 0, cursorVisible: true } : null;
      }
      case "managed_terminal_screen":
        return [];
      default:
        return undefined;
    }
  };
}
