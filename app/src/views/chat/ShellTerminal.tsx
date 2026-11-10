import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { UnicodeGraphemesAddon } from "@xterm/addon-unicode-graphemes";
import "@xterm/xterm/css/xterm.css";
import { attachShellTerminal, detachShellTerminal, openLink, resizeShellTerminal, writeShellTerminal } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { stripTerminalReplies, termStyleCache } from "../ManagedTerminal";

function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/**
 * 面板「终端」页签里的一个普通 shell。进程归后端（shell_terminal.rs），这里只负责画面：
 * 挂载时 attach（先回放历史输出），卸载时 detach，shell 不受影响。
 *
 * 外层沿用 .managed-terminal 的类名：对话窗里 Ctrl+F、Alt+↑↓ 这些窗口级快捷键都按它
 * 判断「焦点在终端里就让位」，普通 shell 同样需要把这些键留给自己。
 */
export function ShellTerminal({ id, visible }: { id: number; visible: boolean }) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const [exitCode, setExitCode] = useState<number | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const style = termStyleCache();
    // 字体、配色与托管终端一致，两种终端并排开着时看不出是两套东西。
    const terminal = new Terminal({
      cursorBlink: true,
      macOptionClickForcesSelection: true,
      fontFamily: '"JetBrains Mono", ui-monospace, SFMono-Regular, Consolas, "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", sans-serif',
      fontSize: style.size,
      lineHeight: style.line,
      scrollback: 5000,
      theme: { background: "#121212", foreground: "#e7e9e8", cursor: "#55d6ae", selectionBackground: "#31403a" },
    });
    termRef.current = terminal;
    const fit = new FitAddon();
    fitRef.current = fit;
    terminal.loadAddon(fit);
    // 终端惯例：Ctrl/Cmd+点击才打开链接，普通点击留给选区。
    terminal.loadAddon(new WebLinksAddon((event, uri) => {
      if (!event.ctrlKey && !event.metaKey) return;
      void openLink(uri).catch((e) => setError(formatBackendError(e, t.locale)));
    }));
    try {
      terminal.loadAddon(new UnicodeGraphemesAddon());
    } catch { /* 测试环境的哑实现不支持，维持默认宽表 */ }
    terminal.open(host);
    let webgl: WebglAddon | null = null;
    try {
      webgl = new WebglAddon();
      webgl.onContextLoss(() => { webgl?.dispose(); webgl = null; });
      terminal.loadAddon(webgl);
    } catch {
      webgl = null;
    }
    // 粘贴/复制的约定同托管终端：Ctrl+V 放行给原生 paste；有选区时 Ctrl+C 是复制，
    // 没选区才是发 ^C 中断。
    terminal.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      // Ctrl+` 是对话窗开合面板终端的键：xterm 吃掉 keydown 会拦住冒泡，放行才收得起来。
      if (event.ctrlKey && !event.altKey && !event.metaKey && event.code === "Backquote") return false;
      // 面板快捷键同理放行：Ctrl+Shift+B/G/E 开合面板、切改动 / 文件，Ctrl+(Shift+)Tab 切页签。
      // 这几个在 shell 里没有用处；Ctrl+W（删词）、Ctrl+T（换字）是 shell 的，不放。
      if (event.ctrlKey && !event.altKey && event.shiftKey && ["KeyB", "KeyG", "KeyE"].includes(event.code)) return false;
      if (event.ctrlKey && !event.altKey && event.code === "Tab") return false;
      const paste = ((event.ctrlKey || event.metaKey) && !event.altKey && event.code === "KeyV")
        || (event.shiftKey && !event.ctrlKey && !event.metaKey && event.code === "Insert");
      if (paste) return false;
      const copy = (event.ctrlKey || event.metaKey) && !event.altKey && event.code === "KeyC";
      if (copy && (event.shiftKey || terminal.hasSelection())) {
        const text = terminal.getSelection();
        if (text) void navigator.clipboard?.writeText(text).catch(() => {});
        return false;
      }
      return true;
    });

    let disposed = false;
    let generation: number | null = null;
    // 重放已显示过的历史时，里面 shell 当初发的终端查询（设备属性 ESC[c 等）会被 xterm
    // 再答一遍。那些早就答过了，再答的话答案会当成键盘输入落进 shell，提示符后面多出
    // 一串 `[?1;2c`（实拍：每切回一次页签多一串）。重放窗口里只剔应答，用户按键照发。
    let replaying = 0;
    const input = terminal.onData((data) => {
      const payload = replaying > 0 ? stripTerminalReplies(data) : data;
      if (!payload) return;
      void writeShellTerminal(id, payload).catch((e) => setError(formatBackendError(e, t.locale)));
    });
    const resize = terminal.onResize(({ cols, rows }) => {
      void resizeShellTerminal(id, cols, rows).catch(() => {});
    });
    attachShellTerminal(id, (event) => {
      if (disposed) return;
      if (event.kind === "output" && event.replay) {
        replaying += 1;
        // write 的回调在这段解析完才触发，期间 xterm 同步发出的应答都落在窗口里。
        terminal.write(decodeBase64(event.data), () => { replaying -= 1; });
      } else if (event.kind === "output") terminal.write(decodeBase64(event.data));
      else setExitCode(event.code);
    })
      .then((gen) => {
        if (disposed) void detachShellTerminal(id, gen);
        else generation = gen;
      })
      .catch((e) => setError(formatBackendError(e, t.locale)));

    // 面板宽度拖动、最大化、窗口缩放都会改尺寸；折叠时宽高为 0，跳过，免得把 shell 缩成 2 列。
    const observer = new ResizeObserver(() => {
      if (host.clientWidth === 0 || host.clientHeight === 0) return;
      try { fit.fit(); } catch { /* 刚卸载 */ }
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      resize.dispose();
      if (generation !== null) void detachShellTerminal(id, generation);
      try { webgl?.dispose(); } catch { /* 已释放 */ }
      terminal.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  // t 只用于错误文案，换语言不值得重建终端。
  // eslint-disable-next-line react-hooks/exhaustive-deps -- 终端按 id 只建一次；t.locale 只用于打开链接失败的报错文案，切语言不该重建终端
  }, [id]);

  // 切回这个页签时把焦点给终端，点「+」新开的也能直接打字。
  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => {
      try { fitRef.current?.fit(); } catch { /* 尺寸未就绪 */ }
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [visible]);

  return (
    <div className="managed-terminal shell-term" style={visible ? undefined : { display: "none" }}>
      <div ref={hostRef} className="managed-terminal-host" />
      {exitCode !== undefined && (
        <div className="shell-term-exit">{t.chat.shellExited.replace("{code}", exitCode === null ? "?" : String(exitCode))}</div>
      )}
      {error && <div className="shell-term-exit is-error">{error}</div>}
    </div>
  );
}
