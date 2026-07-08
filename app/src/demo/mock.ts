// demo 专用:用 @tauri-apps/api/mocks 拦截全部 IPC,数据源是内存 store。
// 舞台状态(窗口形态/字幕/收尾)也放这里,分镜动作改完 notify() 即驱动 React 重渲染。
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { Settings, type ProviderAccountPayload, type ProviderUsage } from "../api";
import { Item } from "./data";

export type StageMode = "normal" | "docking" | "strip" | "expanded";

export type Store = {
  sessions: Item[];
  stage: { mode: StageMode; caption: string | null; finale: boolean };
  settings: Settings;
};

export const store: Store = {
  sessions: [],
  stage: { mode: "normal", caption: null, finale: false },
  settings: {
    archive_hide_days: 0,
    notifications_enabled: true,
    theme: "dark",
    opacity: 97,
    ui_scale: 100,
    resume_terminal: "wt",
    language: "zh",
    terminal_open_mode: "card",
    card_menu_mode: "context",
    preview_enabled: true,
    sticker_style: "elevated",
    sticker_color: "classic",
    sticker_quota_providers: ["claude"],
    default_agent: "claude",
  },
};

const subs = new Set<() => void>();
export function subscribe(fn: () => void): () => void {
  subs.add(fn);
  return () => subs.delete(fn);
}
export function notify(): void {
  subs.forEach((f) => f());
}

export function installMocks(): void {
  mockWindows("main");
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "host_os":
        return "windows";
      case "get_settings":
        return store.settings;
      case "get_accounts": {
        // demo 假数据：仅 claude 有账号与用量
        const claudePayload: ProviderAccountPayload = {
          provider: "claude",
          account: {
            email: "demo@example.com",
            display_name: "Demo User",
            organization: null,
            plan: "Pro",
            login_label: null,
          },
          usage: {
            lanes: [
              { kind: "five_hour", used_pct: 62, used: null, limit: null, unit: null, resets_at: "2026-06-18T20:00:00Z" },
              { kind: "seven_day", used_pct: 38, used: null, limit: null, unit: null, resets_at: "2026-06-24T08:00:00Z" },
            ],
            note: null,
          },
          usage_supported: true,
        };
        return [claudePayload];
      }
      case "refresh_usage": {
        const a = args as { provider: string };
        if (a.provider === "claude") {
          const claudeUsage: ProviderUsage = {
            lanes: [
              { kind: "five_hour", used_pct: 62, used: null, limit: null, unit: null, resets_at: "2026-06-18T20:00:00Z" },
              { kind: "seven_day", used_pct: 38, used: null, limit: null, unit: null, resets_at: "2026-06-24T08:00:00Z" },
            ],
            note: null,
          };
          return claudeUsage;
        }
        return { lanes: [], note: null };
      }
      case "get_live_sessions":
        return store.sessions;
      case "rename_session": {
        const a = args as { sessionId: string; title: string };
        const s = store.sessions.find((x) => x.session.cc_session_id === a.sessionId);
        if (s) s.task_title = a.title;
        notify();
        return null;
      }
      case "set_archived": {
        const a = args as { sessionId: number; archived: boolean };
        const s = store.sessions.find((x) => x.session.id === a.sessionId);
        if (s) {
          s.archived = a.archived;
          s.archived_at = a.archived ? Date.now() : null;
        }
        notify();
        return null;
      }
      case "set_session_note": {
        const a = args as { sessionId: string; note: string };
        const s = store.sessions.find((x) => x.session.cc_session_id === a.sessionId);
        if (s) s.note = a.note;
        notify();
        return null;
      }
      case "plugin:event|listen":
        return 1;
      default:
        // focus_session / resume_session / snap_* / plugin:window|* 等一律 no-op
        return null;
    }
  });
}
