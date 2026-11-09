<div align="center">
  <img src="docs/images/logo.png" width="104" alt="Meowo logo" />
  <h1>Meowo / 喵呜</h1>
  <p><b>A desktop workbench for running AI coding agents in parallel. Claude Code, Codex, Kimi, Gemini CLI and OpenCode sessions, all in one place.</b></p>
  <p>
    <a href="https://github.com/larrygogo/meowo/actions/workflows/ci.yml"><img src="https://github.com/larrygogo/meowo/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
    <a href="https://github.com/larrygogo/meowo/releases/latest"><img src="https://img.shields.io/github/v/release/larrygogo/meowo?label=release&color=d97757" alt="Release" /></a>
    <a href="https://github.com/larrygogo/meowo/releases"><img src="https://img.shields.io/github/downloads/larrygogo/meowo/total?color=4ec9a5" alt="Downloads" /></a>
    <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS-555" alt="Platform: Windows | macOS" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
  </p>
  <p><a href="README.md">中文</a> · <b>English</b> · <a href="https://meowo.io/en">Website meowo.io</a></p>
  <p>Start sessions, follow the conversation, approve commands, review changes, open a terminal or a browser — all inside Meowo's chat window.<br/>The island at the top of your screen shows who's running and who's waiting on you. Away from your desk? Keep things moving from your phone.</p>
  <img src="docs/images/island.en.webp" alt="Meowo island: quota and per-session status when collapsed, an alert when a session needs approval, and the session list when expanded" width="760" />
</div>

## Download

Get it from **[meowo.io](https://meowo.io/en)** — the page picks the right installer for your system. Or download straight from [GitHub Releases](https://github.com/larrygogo/meowo/releases/latest):

| Platform | Installer | Notes |
|----------|-----------|-------|
| **Windows 10 / 11** | `Meowo_x.y.z_x64-installer.exe` | Double-click to install; falls back to the classic installer when WebView2 isn't available |
| **macOS 14+** | `_aarch64.dmg` for Apple silicon, `_x64.dmg` for Intel | Signed and notarized. Not sure which chip? Apple menu → About This Mac |

No AI CLI needed upfront: once Meowo is open, install and sign in from Settings in one click, and the hooks are wired for you. Updates are checked and installed from inside the app.

> **Windows says "Windows protected your PC"**: the Windows installer isn't code-signed yet. Choose "More info" → "Run anyway".
> Installers are built by GitHub Actions from the public source (see [release.yml](.github/workflows/release.yml)), and auto-updates are verified with a minisign signature.

## What it does

### Chat window: where sessions run

<img src="docs/images/panel.en.png" alt="Meowo chat window: session sidebar on the left, the conversation and a pending command approval in the middle, and the diff of this change in the right panel" width="760" />

- **Structured conversation**: messages, tool calls and subagents each take one line; expand for the full input and output. The todo list and progress update live.
- **Approvals and questions become buttons**: approval cards show what the action touches (paths outside the workspace, file deletion, network), and allow / deny is one click. Answer the agent's multiple-choice questions right on the card. Trust prompts and model menus in the terminal turn into buttons too.
- **Start a session right here**: pick an agent and a folder in the chat window and type your first message. Attach images, or take a screenshot and mark it up before sending.
- **Managed terminal**: sessions run in a terminal hosted by Meowo. Chat and terminal are two views of the same session; switch over and type yourself anytime.
- **Right panel**: Changes, Files, Terminal and Browser share one row of tabs.
  - The Changes tab shows the diff. Comment on any line, then drop the comments into the composer for the agent.
  - The Browser tab opens localhost pages. claude, codex and kimi sessions can drive this browser themselves; you see their cursor and can take over anytime.
- **Title bar**: shows the branch and pull request, and opens the project in your editor in one click.

### Island: status at the top of your screen

- Collapsed, it shows a quota ring and one icon per session: a green arc spins around running ones, a yellow dot marks those waiting on you.
- When a session needs approval, asks a question, errors out or finishes, the island expands for a few seconds; click an icon to open that session.
- Expand it for your most urgent sessions and every agent's quota, or start a new session from there.
- On Windows you can drag it sideways or switch to minimal mode, which leaves just a thin strip. On a Mac with a notch it wraps the notch; without one it lives in the menu bar.

### Phone access: keep going away from your desk

<img src="docs/images/mobile.en.png" alt="Meowo phone access page: reading the conversation and handling a command approval in a mobile browser" width="280" align="right" />

- Turn on "Phone access" in Settings and scan the QR code. No app to install; add it to your home screen if you like.
- Read conversations, send messages, **approve commands** and answer the agent's questions from your phone. You can also start and fork sessions, and see the terminal and changes.
- Get a push on your phone through [Bark](https://github.com/Finb/Bark) when a session needs you.
- The computer stays awake while sessions run; you can flip that switch from your phone too.
- Works over your LAN or Tailscale; binding to the Tailscale interface only is recommended.

<br clear="right" />

### Accounts, quota and network

- Keep several official accounts per agent and switch in one click; running sessions switch with you.
- No official account? Connect an API relay per model.
- Shows 5-hour / 7-day quota plus per-model weekly quota (such as Opus), turning red near the limit.
- Set a global default proxy or one per agent; HTTP, SOCKS5 and authenticated proxies are supported.

### Local-first

Session data and settings live only in `~/.meowo/` on your machine (SQLite). Meowo has no server of its own, uploads no conversations, and works offline. Sessions you start in your own terminal show up too.

## How it works

Each CLI hands its events to `meowo-reporter` through its own hook mechanism (OpenCode has no hooks, so Meowo generates a bridge plugin for it), and everything lands in one local database:

```
 AI CLI session (claude / codex / kimi / gemini / opencode)
   │  fires hooks; claude's statusline also reports Context usage
   ▼
 meowo-reporter (one-shot CLI process, exits after writing)
   ▼
 ~/.meowo/board.db (SQLite, WAL)
   ▲  file watch, refresh on change
 meowo-app (Tauri desktop app)
   ├─ hosts each session's terminal (PTY); chat and terminal show the same session
   └─ with phone access on, runs an embedded HTTP server that only allows whitelisted calls
```

On startup Meowo wires hooks into the CLIs it detects: it backs up first, then writes atomically, so your existing config stays intact. To connect by hand or troubleshoot, see the [manual setup guide](https://meowo.io/en/docs#manual). For where data and config files live, see [Data & config](https://meowo.io/en/docs#data).

Architecture notes are in [`docs/architecture/`](docs/architecture/).

## Build from source

You need [Rust](https://rustup.rs/) (stable), [Bun](https://bun.sh/) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) (Windows: WebView2 + MSVC build tools; macOS: Xcode command line tools).

```bash
cd app
bun install
bun run tauri dev     # dev mode; the first run compiles Rust and takes a while
bun run tauri build   # build installers into app/src-tauri/target/release/bundle/
```

Tests and checks:

```bash
cd app && bunx tsc --noEmit && bun run test
cd app/src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test
```

Layout: the front end is in `app/src/` (React + TypeScript); the Rust workspace is in `app/src-tauri/` with four crates: `meowo-protocol` (cross-process contracts), `meowo-store` (SQLite), `meowo-agent` (per-agent adapters) and `meowo-reporter` (hook reporter). The website is in `site/`. The README images are generated with `cd app && bun run demo:webp`.

## Why "Meowo"?

It comes from a cat's **meow**, written 喵呜 in Chinese.

## License

[MIT](LICENSE) © larrygogo
