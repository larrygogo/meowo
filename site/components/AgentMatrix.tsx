// 功能页「五家 Agent 能力对照」。每一格取自 0.8.1 的插件声明，改之前先核对出处：
// - 多账号 / 切账号会话跟着搬 / 上下文 / SOCKS / 代理写进配置 / API Key 登录：
//   app/src-tauri/crates/meowo-agent/src/descriptor.rs 的字段，逐家取值见 app/src/test/agents.ts
//   （那份夹具与后端矩阵有绊线测试钉着同源）
// - 额度读数：各插件 account.rs 的 usage_supported（claude 要 OAuth 登录、codex 要 ChatGPT 登录、
//   gemini / opencode 恒 false）
// - Agent 操作浏览器：声明了 mcp_attach 的 claude / codex / kimi（0.8.1 发版说明同）
// - 提问卡作答：claude（0.6.1）、kimi（0.8.0）
// - 登录入口：claude/codex/kimi 的 `login`、opencode 的 `auth login`；gemini 只有 API Key（OAuth 被官方停用）
// 做空审查（2026-11-09）抓到官网把五家写成一样，这张表就是为了把差别摆明。
import type { Lang } from "@/lib/i18n";
import { AgentLogo, type ProviderId } from "./SupportedAgents";

type Cell = boolean | string;
type Row = { label: string; cells: [Cell, Cell, Cell, Cell, Cell] };

const AGENTS: { id: ProviderId; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex" },
  { id: "kimi", name: "Kimi Code" },
  { id: "gemini", name: "Gemini CLI" },
  { id: "opencode", name: "OpenCode" },
];

const ROWS: Record<Lang, { groups: { title: string; rows: Row[] }[]; note: string; yes: string; no: string; caption: string }> = {
  zh: {
    caption: "五家 Agent 能力对照",
    yes: "支持",
    no: "不支持",
    note: "取自 Meowo 0.8.1 里各家插件的能力声明。CLI 自己更新后可能变化，以应用设置页的显示为准。",
    groups: [
      {
        title: "接入与登录",
        rows: [
          { label: "怎么接入", cells: ["hooks + 状态栏", "hooks", "hooks", "hooks", "桥接插件"] },
          { label: "在 Meowo 里登录", cells: ["一键登录", "一键登录", "一键登录", "填 API Key", "一键登录"] },
        ],
      },
      {
        title: "对话窗",
        rows: [
          { label: "提问卡直接作答", cells: [true, false, true, false, false] },
          { label: "Agent 操作面板浏览器", cells: [true, true, true, false, false] },
          { label: "上下文用量", cells: [true, true, true, false, false] },
        ],
      },
      {
        title: "账号与额度",
        rows: [
          { label: "额度读数", cells: ["官方账号登录时", "ChatGPT 登录时", true, false, false] },
          { label: "多个账号", cells: [true, true, true, false, true] },
          { label: "切账号时运行中的会话一起切", cells: [true, true, true, false, false] },
          { label: "API 中转", cells: [true, true, true, true, true] },
        ],
      },
      {
        title: "网络",
        rows: [
          { label: "HTTP 代理", cells: [true, true, true, true, true] },
          { label: "SOCKS 代理", cells: [false, false, true, true, false] },
          { label: "代理对你自己在终端里开的会话也生效", cells: [true, false, false, false, false] },
        ],
      },
    ],
  },
  en: {
    caption: "Agent capability comparison",
    yes: "Supported",
    no: "Not supported",
    note: "Taken from each agent plugin's declared capabilities in Meowo 0.8.1. Things may change as the CLIs update; the Settings page shows what applies to you.",
    groups: [
      {
        title: "Connect & sign in",
        rows: [
          { label: "How it connects", cells: ["hooks + statusline", "hooks", "hooks", "hooks", "bridge plugin"] },
          { label: "Sign in from Meowo", cells: ["one click", "one click", "one click", "API key", "one click"] },
        ],
      },
      {
        title: "Chat window",
        rows: [
          { label: "Answer questions on the card", cells: [true, false, true, false, false] },
          { label: "Agent drives the panel browser", cells: [true, true, true, false, false] },
          { label: "Context usage", cells: [true, true, true, false, false] },
        ],
      },
      {
        title: "Accounts & quota",
        rows: [
          { label: "Quota readout", cells: ["with official sign-in", "with ChatGPT sign-in", true, false, false] },
          { label: "Multiple accounts", cells: [true, true, true, false, true] },
          { label: "Running sessions switch with the account", cells: [true, true, true, false, false] },
          { label: "API relay", cells: [true, true, true, true, true] },
        ],
      },
      {
        title: "Network",
        rows: [
          { label: "HTTP proxy", cells: [true, true, true, true, true] },
          { label: "SOCKS proxy", cells: [false, false, true, true, false] },
          { label: "Proxy also applies to sessions you start in your own terminal", cells: [true, false, false, false, false] },
        ],
      },
    ],
  },
};

function CellView({ cell, yes, no }: { cell: Cell; yes: string; no: string }) {
  if (cell === true) return <span className="am-yes" role="img" aria-label={yes}>●</span>;
  if (cell === false) return <span className="am-no" role="img" aria-label={no}>–</span>;
  return <span className="am-text">{cell}</span>;
}

export default function AgentMatrix({ lang }: { lang: Lang }) {
  const t = ROWS[lang];
  return (
    <div className="am">
      <div className="am-scroll">
        <table>
          <caption className="sr-only">{t.caption}</caption>
          <thead>
            <tr>
              <th scope="col" />
              {AGENTS.map((a) => (
                <th key={a.id} scope="col">
                  <span className="am-agent">
                    <AgentLogo id={a.id} size={26} />
                    {a.name}
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          {t.groups.map((g) => (
            <tbody key={g.title}>
              <tr className="am-group">
                <th scope="colgroup" colSpan={6}>{g.title}</th>
              </tr>
              {g.rows.map((r) => (
                <tr key={r.label}>
                  <th scope="row">{r.label}</th>
                  {r.cells.map((cell, i) => (
                    <td key={AGENTS[i].id}>
                      <CellView cell={cell} yes={t.yes} no={t.no} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          ))}
        </table>
      </div>
      <p className="am-note">{t.note}</p>
    </div>
  );
}
