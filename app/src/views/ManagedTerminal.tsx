import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import {
  isExternallyHeld,
  managedTerminalSnapshot,
  openAttachedTerminal,
  resizeManagedTerminal,
  startManagedTerminal,
  stopManagedTerminal,
  takeoverManagedTerminal,
  writeManagedTerminal,
} from "../api";
import { useT } from "../i18n";
import type { PtyExitEvent as ExitEvent } from "../generated/contracts/PtyExitEvent";
import type { PtyOutputEvent as OutputEvent } from "../generated/contracts/PtyOutputEvent";
import { terminalAttention, visibleTerminalText, type TerminalAttention } from "../terminalAttention";

function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

/// 这段字节里是否有**画得出来的东西**，而不只是控制序列。
///
/// 全屏 TUI（claude/codex）启动时先甩一大串清屏与光标控制序列，真正的界面要等
/// `--resume` 把 transcript 读完才画得出来——长会话可以是几十秒。若把「收到字节」
/// 当成「初始化完成」，遮罩会在这段空窗期就撤掉，用户面对的是一块没有任何提示的纯黑屏。
function hasVisibleOutput(bytes: Uint8Array): boolean {
  let i = 0;
  while (i < bytes.length) {
    const byte = bytes[i];
    if (byte === 0x1b) {
      i += 1;
      const kind = bytes[i];
      if (kind === 0x5b) {
        // CSI：参数字节之后以 @~ 区间的最终字节收尾。
        i += 1;
        while (i < bytes.length && (bytes[i] < 0x40 || bytes[i] > 0x7e)) i += 1;
      } else if (kind === 0x5d) {
        // OSC：到 BEL 或 ST 为止，串里的标题文本不算可见内容。
        i += 1;
        while (i < bytes.length && bytes[i] !== 0x07 && bytes[i] !== 0x1b) i += 1;
      }
      i += 1;
      continue;
    }
    // 空格与制表符构不成画面——清屏后的空行全是它们。
    if (byte > 0x20 && byte !== 0x7f) return true;
    i += 1;
  }
  return false;
}

/// TUI 迟迟不画东西时的保底：宁可把黑屏交给用户，也不要让 spinner 永远转下去。
const INITIALIZING_TIMEOUT_MS = 25_000;

type ManagedTerminalProps = {
  sessionId: number;
  status?: string;
  visible?: boolean;
  onUserSubmit?: () => void;
  attentionMarkers?: string[];
  interactivePrompt?: boolean;
  /// 刚发出会弹菜单的命令（如 `/model`）：这段窗口里额外识别光标菜单。
  expectMenu?: boolean;
  onAttention?: (attention: TerminalAttention | null) => void;
  /// 供父组件在自己重启 PTY 后触发偏移复位（对话页发送/切模式也会重启 PTY，
  /// 不止组件内部的 start/takeover 按钮）。
  rearmRef?: MutableRefObject<(() => void) | null>;
};

export function ManagedTerminal({ sessionId, status, visible = true, onUserSubmit, attentionMarkers = [], interactivePrompt = false, expectMenu = false, onAttention, rearmRef: externalRearmRef }: ManagedTerminalProps) {
  const t = useT();
  const hostRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [active, setActive] = useState(false);
  const [snapshotReady, setSnapshotReady] = useState(false);
  const [initialized, setInitialized] = useState(false);
  const [starting, setStarting] = useState(false);
  // 结束终端是异步的（杀进程 + 等 pty-exit 回传，Windows 上有几百毫秒延迟）。
  // 没有这个状态，按钮点完毫无变化，用户以为没反应。
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const [exitCode, setExitCode] = useState<number | null | undefined>(undefined);
  const externalRunning = isExternallyHeld(status);
  /// 就地重启（结束终端 → 再接管）后重新对齐输出偏移。新 PTY 的 output_end 从 0 重新计数，
  /// 而 nextOffset 还停在上一个进程的高位，writeOutput 会把新输出全部当成「已写过」丢掉，
  /// 终端就永远定格在旧内容上。effect 内部把重置逻辑挂到这里，供 start/takeover 调用。
  const rearmRef = useRef<(() => void) | null>(null);
  const onUserSubmitRef = useRef(onUserSubmit);
  const attentionMarkersRef = useRef(attentionMarkers);
  const interactivePromptRef = useRef(interactivePrompt);
  const expectMenuRef = useRef(expectMenu);
  const onAttentionRef = useRef(onAttention);
  const visibleRef = useRef(visible);
  const attentionTailRef = useRef("");
  const attentionReportedRef = useRef<string | null>(null);
  const lastScreenRef = useRef("");
  onUserSubmitRef.current = onUserSubmit;
  attentionMarkersRef.current = attentionMarkers;
  interactivePromptRef.current = interactivePrompt;
  expectMenuRef.current = expectMenu;
  onAttentionRef.current = onAttention;
  visibleRef.current = visible;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      // "JetBrains Mono" 由 styles.css 的 @font-face 打包提供（不依赖本机安装），管拉丁+符号；
      // 它不含 CJK，中文逐字回退到各平台**真实存在**的好看字体：微软雅黑 / 苹方 / Noto。
      // （曾误写 "Microsoft YaHei Mono"——该字体名不存在，导致中文掉到 Courier New 的宋体兜底，
      //  正是「中文看着奇怪」的来源。）xterm 用等宽网格定位，中文按双宽对齐，非等宽也整齐。
      fontFamily: '"JetBrains Mono", ui-monospace, SFMono-Regular, Consolas, "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", sans-serif',
      fontSize: 12,
      lineHeight: 1.22,
      scrollback: 5000,
      // 绿色只留给光标这一格宽的点缀；选区是成片色块，用低饱和灰绿保持清爽。
      theme: { background: "#151617", foreground: "#e7e9e8", cursor: "#55d6ae", selectionBackground: "#31403a" },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    terminalRef.current = terminal;
    fitRef.current = fit;
    requestAnimationFrame(() => fit.fit());

    const input = terminal.onData((data) => {
      if (data.includes("\r")) {
        onUserSubmitRef.current?.();
      }
      // 写失败必须可见：典型场景是整段粘贴超过后端单次输入上限被拒——
      // 静默吞掉的话，粘贴无声消失，终端画面纹丝不动。
      void writeManagedTerminal(sessionId, data).catch((e) => setError(String(e)));
    });
    let unOutput: (() => void) | undefined;
    let unExit: (() => void) | undefined;
    let cancelled = false;
    let hasWrittenOutput = false;
    let painted = false;
    let snapshotApplied = false;
    let nextOffset = 0;
    const bufferedOutput: OutputEvent[] = [];
    let bufferedExit: ExitEvent | null = null;
    let snapshotTimer = 0;
    const attentionDecoder = new TextDecoder();
    // 保底：TUI 一直不画东西也不能永远停在 spinner 上。
    const giveUpTimer = window.setTimeout(() => {
      if (!cancelled) { painted = true; setInitialized(true); }
    }, INITIALIZING_TIMEOUT_MS);
    // 只有画得出东西的输出才算初始化完成；清屏/光标序列不算（见 hasVisibleOutput）。
    const markPainted = (bytes: Uint8Array) => {
      if (painted || !hasVisibleOutput(bytes)) return;
      painted = true;
      window.clearTimeout(giveUpTimer);
      setInitialized(true);
    };
    const reportAttention = (text: string) => {
      if (text) lastScreenRef.current = text;
      const attention = terminalAttention(text, attentionMarkersRef.current, interactivePromptRef.current, expectMenuRef.current);
      if (!attention) return;
      const signature = `${attention.id}\0${attention.text}\0${JSON.stringify(attention.options)}`;
      if (signature === attentionReportedRef.current) return;
      // 信任页本身就是当前需要展示的有效画面，不能继续被“正在初始化”遮罩盖住。
      if (!painted) {
        painted = true;
        window.clearTimeout(giveUpTimer);
        setInitialized(true);
      }
      attentionReportedRef.current = signature;
      onAttentionRef.current?.(attention);
    };
    const renderedScreen = () => {
      // 原始 PTY 流里的光标回退、逐行清除无法靠正则完整还原。xterm 已经替我们执行了
      // 这些控制序列，直接读它的当前 viewport 才是用户此刻真正看到的画面。
      const buffer = terminal.buffer?.active;
      if (!buffer) return visibleTerminalText(attentionTailRef.current);
      const first = Math.max(0, buffer.viewportY);
      const lines: string[] = [];
      for (let row = first; row < Math.min(buffer.length, first + terminal.rows); row += 1) {
        const line = buffer.getLine(row)?.translateToString(true).trimEnd();
        if (line) lines.push(line);
      }
      return lines.slice(-80).join("\n").trim();
    };
    const inspectAttention = (bytes: Uint8Array) => {
      attentionTailRef.current = (attentionTailRef.current + attentionDecoder.decode(bytes, { stream: true })).slice(-16_384);
    };
    // 整屏抓取 + 多条回溯正则不便宜，不能每个输出 chunk 都跑一遍——构建/日志刷屏时
    // 事件很密，逐帧扫描会拖垮主线程。合并成至多每 150ms 一次的尾随节流：持续输出时
    // 有界地扫，输出停下后最后一批也保证在 150ms 内被扫到（审批/信任页正是这种停帧画面）。
    let attentionScanTimer = 0;
    const scheduleAttentionScan = () => {
      if (attentionScanTimer) return;
      attentionScanTimer = window.setTimeout(() => {
        attentionScanTimer = 0;
        if (cancelled) return;
        const screen = renderedScreen();
        if (screen) reportAttention(screen);
      }, 150);
    };
    const writeOutput = (payload: OutputEvent) => {
      const bytes = decodeBase64(payload.data);
      const offset = Number.isFinite(payload.offset) ? payload.offset : nextOffset;
      const end = offset + bytes.length;
      if (end <= nextOffset) return;
      const visible = offset < nextOffset ? bytes.slice(nextOffset - offset) : bytes;
      if (visible.length === 0) return;
      hasWrittenOutput = true;
      nextOffset = end;
      inspectAttention(visible);
      markPainted(visible);
      terminal.write(visible, scheduleAttentionScan);
    };
    const applyExit = (payload: ExitEvent) => {
      window.clearTimeout(snapshotTimer);
      window.clearTimeout(giveUpTimer);
      painted = true;
      setActive(false);
      setSnapshotReady(true);
      // 进程没了就没有下一帧可等：必须离开初始化态，把退出结果交给遮罩。
      setInitialized(true);
      setExitCode(payload.code);
      terminal.write(`\r\n\x1b[90m[Meowo: process exited${payload.code == null ? "" : ` (${payload.code})`}]\x1b[0m\r\n`);
    };
    // 首帧传 0 拿全量（要完整回放历史），补查轮询带 nextOffset 只取增量。
    // writeOutput 本来就按 startOffset 做区间裁剪，增量返回天然兼容。
    const inspectSnapshot = () => managedTerminalSnapshot(sessionId, hasWrittenOutput ? nextOffset : 0).then((snapshot) => {
      if (cancelled) return;
      setSnapshotReady(true);
      setActive(snapshot.active);
      setExitCode(snapshot.exited ? snapshot.exitCode : undefined);
      if (snapshot.data) {
        writeOutput({
          sessionId,
          offset: Number.isFinite(snapshot.startOffset) ? snapshot.startOffset : 0,
          data: snapshot.data,
        });
      }
      // data 是从 startOffset 起的增量，兜底算末尾要从 startOffset 加起，
      // 直接拿长度当绝对末尾会把偏移算小，之后的事件会被重复写一遍。
      const start = Number.isFinite(snapshot.startOffset) ? snapshot.startOffset : 0;
      nextOffset = Math.max(
        nextOffset,
        Number.isFinite(snapshot.endOffset)
          ? snapshot.endOffset
          : start + (snapshot.data ? decodeBase64(snapshot.data).length : 0),
      );
      snapshotApplied = true;
      bufferedOutput.sort((a, b) => a.offset - b.offset).forEach(writeOutput);
      bufferedOutput.length = 0;
      if (bufferedExit) { applyExit(bufferedExit); bufferedExit = null; }
      // PTY 可能先 active、后输出首屏；在此期间保持初始化遮罩并补查快照，避免监听器
      // 尚未注册完成时漏掉极早的一段输出，最终永远停在黑屏或加载态。
      // 补查只为等**第一批**字节：拿到就停，之后的帧走 pty-output 事件——每次快照都会
      // 把整个 backlog（可达 1MB）编码重传一遍，不能拿它轮询到界面画出来为止。
      if ((snapshot.active || sessionId < 0) && !hasWrittenOutput && !snapshot.exited) {
        snapshotTimer = window.setTimeout(() => void inspectSnapshot(), 120);
      }
    }).catch(() => {
      if (!cancelled) {
        snapshotApplied = true;
        bufferedOutput.sort((a, b) => a.offset - b.offset).forEach(writeOutput);
        bufferedOutput.length = 0;
        if (bufferedExit) { applyExit(bufferedExit); bufferedExit = null; }
        setSnapshotReady(true);
      }
    });

    // 就地重启后把偏移归零并重新拉一次快照。新 PTY 从 0 重新计数，沿用旧的 nextOffset
    // 会让 writeOutput 把所有新输出判成「已写过」而丢弃（终端定格在旧内容）。
    rearmRef.current = () => {
      if (cancelled) return;
      window.clearTimeout(snapshotTimer);
      // 排程中的扫描读的是旧进程的画面，重启后不再有意义。
      window.clearTimeout(attentionScanTimer);
      attentionScanTimer = 0;
      nextOffset = 0;
      hasWrittenOutput = false;
      painted = false;
      attentionReportedRef.current = null;
      attentionTailRef.current = "";
      lastScreenRef.current = "";
      snapshotApplied = false;
      bufferedOutput.length = 0;
      bufferedExit = null;
      terminal.reset();
      setInitialized(false);
      setExitCode(undefined);
      void inspectSnapshot();
    };
    if (externalRearmRef) externalRearmRef.current = () => rearmRef.current?.();
    const outputListener = listen<OutputEvent>("pty-output", ({ payload }) => {
      if (payload.sessionId === sessionId) {
        window.clearTimeout(snapshotTimer);
        setActive(true);
        setSnapshotReady(true);
        setExitCode(undefined);
        if (snapshotApplied) writeOutput(payload);
        else bufferedOutput.push(payload);
      }
    });
    const exitListener = listen<ExitEvent>("pty-exit", ({ payload }) => {
      if (payload.sessionId === sessionId) {
        if (snapshotApplied) applyExit(payload);
        else bufferedExit = payload;
      }
    });
    Promise.all([outputListener, exitListener]).then(([outputUnlisten, exitUnlisten]) => {
      if (cancelled) {
        outputUnlisten();
        exitUnlisten();
        return;
      }
      unOutput = outputUnlisten;
      unExit = exitUnlisten;
      // 监听器就绪后再取快照；期间到达的帧按 offset 在快照之后去重回放。
      void inspectSnapshot();
    }).catch(() => {
      if (!cancelled) void inspectSnapshot();
    });

    let resizeTimer = 0;
    const observer = new ResizeObserver(() => {
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        fit.fit();
        if (visibleRef.current && terminal.cols > 1 && terminal.rows > 1) {
          void resizeManagedTerminal(sessionId, terminal.cols, terminal.rows).catch(() => {});
        }
      }, 80);
    });
    observer.observe(host);
    return () => {
      cancelled = true;
      window.clearTimeout(snapshotTimer);
      window.clearTimeout(giveUpTimer);
      window.clearTimeout(resizeTimer);
      window.clearTimeout(attentionScanTimer);
      observer.disconnect();
      input.dispose();
      unOutput?.();
      unExit?.();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
      rearmRef.current = null;
      if (externalRearmRef) externalRearmRef.current = null;
    };
  }, [sessionId, externalRearmRef]);

  // capability 查询可能比 PTY 首屏稍晚返回。提示文字先到、markers 后到时也要立刻补判，
  // 不能等一个可能永远不会来的后续输出 chunk。
  const attentionMarkerKey = attentionMarkers.join("\0");
  useEffect(() => {
    const attention = terminalAttention(lastScreenRef.current || attentionTailRef.current, attentionMarkers, interactivePrompt, expectMenu);
    if (!attention) return;
    const signature = `${attention.id}\0${attention.text}\0${JSON.stringify(attention.options)}`;
    if (signature === attentionReportedRef.current) return;
    attentionReportedRef.current = signature;
    setInitialized(true);
    onAttentionRef.current?.(attention);
  }, [attentionMarkerKey, interactivePrompt, expectMenu]);

  // 隐藏期间容器尺寸为 0，xterm 会按 0 列算布局。切回来立刻 fit 一次并聚焦——
  // ResizeObserver 虽然也会触发，但带 80ms 防抖，中间会闪一帧错位的画面。
  useEffect(() => {
    if (!visible) return;
    const terminal = terminalRef.current;
    const fit = fitRef.current;
    if (!terminal || !fit) return;
    const raf = requestAnimationFrame(() => {
      fit.fit();
      if (terminal.cols > 1 && terminal.rows > 1) {
        void resizeManagedTerminal(sessionId, terminal.cols, terminal.rows).catch(() => {});
      }
      terminal.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [visible, sessionId]);

  const initializing = !snapshotReady || ((active || sessionId < 0) && !initialized);

  const start = async () => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    setStarting(true);
    setStopping(false);
    setError("");
    terminal.focus();
    try {
      await startManagedTerminal(sessionId, terminal.cols || 80, terminal.rows || 24);
      setActive(true);
      // 新 PTY 的偏移从 0 重新计数，必须归零重拉，否则新输出会被当成旧数据丢弃。
      rearmRef.current?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  };

  const takeover = async () => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    // 确认框走 `@tauri-apps/plugin-dialog` 的 `confirm`，**不是 `window.confirm`**：后者在 Tauri 的
    // webview（尤其 macOS WKWebView）里会被直接吞掉、恒返回 false——按钮看着能点，点了却什么都不发生。
    const yes = await confirm(t.chat.terminalTakeoverConfirm, {
      title: t.chat.terminalTakeover,
      kind: "warning",
    }).catch(() => false);
    if (!yes) return;
    setStarting(true);
    setStopping(false);
    setError("");
    terminal.focus();
    try {
      await takeoverManagedTerminal(sessionId, terminal.cols || 80, terminal.rows || 24);
      setActive(true);
      rearmRef.current?.();
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  };

  const stop = async () => {
    // 结束是破坏性操作（直接杀掉正在跑的 Agent 进程），与接管同款确认——
    // 同样必须走 plugin-dialog 的 confirm，理由见 takeover。
    const yes = await confirm(t.chat.terminalStopConfirm, {
      title: t.chat.terminalStop,
      kind: "warning",
    }).catch(() => false);
    if (!yes) return;
    setError("");
    // 立刻给反馈：按钮转「正在结束…」并禁用，直到进程退出（pty-exit 把 active 设 false，
    // 整个操作区随之卸载）。成功后不清 stopping——active 变 false 时这块就消失了。
    setStopping(true);
    try {
      await stopManagedTerminal(sessionId);
    } catch (e) {
      // 失败要能重试：终端看着还活着，得把状态退回可点。
      setError(String(e));
      setStopping(false);
    }
  };

  return (
    <div className="managed-terminal">
      <div className="managed-terminal-host" ref={hostRef} />
      {initializing && (
        <div className="managed-terminal-cover is-initializing" role="status">
          <i className="managed-terminal-spinner" />
          <div>{t.chat.terminalInitializing}</div>
        </div>
      )}
      {!initializing && !active && (
        <div className="managed-terminal-cover">
          <div>{error || (exitCode !== undefined ? t.chat.terminalExited(exitCode) : externalRunning ? t.chat.terminalExternal : t.chat.terminalReady)}</div>
          <button type="button" onClick={() => void (externalRunning ? takeover() : start())} disabled={starting}>
            {starting ? t.chat.terminalStarting : externalRunning ? t.chat.terminalTakeover : t.chat.terminalStart}
          </button>
        </div>
      )}
      {active && (
        <div className="managed-terminal-actions">
          {/* 后端刻意让 attach 失败可见（不静默回退 GUI），前端吞掉就前功尽弃；
              结束失败同理——终端看起来还活着，用户会以为已经停了。 */}
          <button type="button" onClick={() => { setError(""); void openAttachedTerminal(sessionId).catch((e) => setError(String(e))); }}>{t.chat.terminalAttach}</button>
          <button type="button" disabled={stopping} onClick={() => void stop()}>
            {stopping ? t.chat.terminalStopping : t.chat.terminalStop}
          </button>
        </div>
      )}
      {active && error && (
        // 容器只挂 role="alert"，关闭动作收进内嵌按钮——同一元素身兼 button 与 alert 两个角色会冲突。
        <div className="managed-terminal-error" role="alert">
          <span>{error}</span>
          <button type="button" aria-label={t.chat.close} onClick={() => setError("")}>×</button>
        </div>
      )}
    </div>
  );
}
