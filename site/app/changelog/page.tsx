import type { Metadata } from "next";
import { RELEASES } from "@/lib/site";
import Reveal from "@/components/Reveal";

export const metadata: Metadata = {
  title: "更新日志 · Meowo",
  description:
    "Meowo 各版本更新记录，包含新功能、优化与修复。",
};

type Kind = "new" | "imp" | "fix";
type Release = {
  ver: string;
  date: string;
  title: string;
  items: { kind: Kind; text: string }[];
};

// 版本与日期取自仓库真实 git tag；条目为对应里程碑的概述，完整明细见 GitHub Releases。
const RELEASE_LOG: Release[] = [
  {
    ver: "v0.4.2",
    date: "2026-07-05",
    title: "更多终端支持",
    items: [
      { kind: "new", text: "新增 WezTerm 终端支持（跳转 / 恢复会话）" },
      { kind: "imp", text: "跨平台通知代码收敛，macOS / Linux 构建对齐" },
      { kind: "fix", text: "修复若干终端聚焦与恢复的边缘情形" },
    ],
  },
  {
    ver: "v0.4.0",
    date: "2026-07-01",
    title: "macOS 正式打包",
    items: [
      { kind: "new", text: "macOS universal DMG（Intel / Apple Silicon 通用）" },
      { kind: "new", text: "已签名公证，双击直接打开；支持自动更新" },
      { kind: "imp", text: "菜单栏面板与计数图标细节打磨" },
    ],
  },
  {
    ver: "v0.3.0",
    date: "2026-06-18",
    title: "用量读数 & 会话搜索",
    items: [
      { kind: "new", text: "贴纸底栏常显 5 小时 / 7 天配额读数" },
      { kind: "new", text: "会话搜索：按标题 / 仓库名即时过滤" },
      { kind: "new", text: "账号页显示各模型用量、重置时间与配额开关" },
    ],
  },
  {
    ver: "v0.2.0",
    date: "2026-06-10",
    title: "在线更新与发布流水线",
    items: [
      { kind: "new", text: "应用内检查 / 自动更新（tauri-plugin-updater）" },
      { kind: "imp", text: "tag 触发的 GitHub Releases 发布流程" },
      { kind: "fix", text: "更新失败 / 无更新后保留可点的「检查更新」入口" },
    ],
  },
  {
    ver: "v0.1.0",
    date: "2026-06-05",
    title: "首个公开版本",
    items: [
      { kind: "new", text: "实时会话看板：卡片、状态分类、待交互提醒" },
      { kind: "new", text: "点击直达终端、断线 claude --resume 续聊" },
      { kind: "new", text: "Windows 吸边缩略、托盘计数、自动接入 Claude Code hooks" },
    ],
  },
];

const KIND_LABEL: Record<Kind, string> = { new: "新增", imp: "优化", fix: "修复" };

export default function ChangelogPage() {
  return (
    <main>
      <section className="pagehead">
        <div className="container">
          <span className="eyebrow">更新日志</span>
          <h1 className="h1">一路的变化</h1>
          <p className="lead">
            版本与日期取自仓库真实发布记录；完整明细见{" "}
            <a
              href={RELEASES}
              target="_blank"
              rel="noopener noreferrer"
              style={{ color: "var(--accent-ink)", textDecoration: "underline" }}
            >
              GitHub Releases
            </a>
            。
          </p>
        </div>
      </section>

      <section className="section-sm">
        <div className="container">
          <div className="timeline">
            {RELEASE_LOG.map((r) => (
              <Reveal key={r.ver}>
                <div className="rel">
                  <div className="rel-meta">
                    <span className="ver">{r.ver}</span>
                    <div className="date">{r.date}</div>
                  </div>
                  <div className="rel-body">
                    <h3>{r.title}</h3>
                    <ul>
                      {r.items.map((it, i) => (
                        <li key={i}>
                          <span className={`tag ${it.kind}`}>{KIND_LABEL[it.kind]}</span>
                          <span>{it.text}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                </div>
              </Reveal>
            ))}
          </div>
        </div>
      </section>
    </main>
  );
}
