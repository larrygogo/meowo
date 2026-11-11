import { useEffect, useRef, useState } from "react";
import { managedTerminalScreen, writeManagedTerminal } from "../../api";
import { useT } from "../../i18n";

/// 手机上「认不出来的终端交互」的兜底作答面：直接给出终端此刻的画面 + 一排按键。
///
/// 为什么需要：审批/提问卡依赖 broker 题面或屏幕识别，两者都落空时，桌面端还能
/// 「去终端处理」，手机上没有终端视图，此前只能写一句「请在桌面端作答」——人在手机
/// 上远程，这句话等于死路（用户实拍反馈）。画面取自后端的终端仿真
/// （managed_terminal_screen），按键走 write_managed_terminal，两者都已在远程白名单里，
/// 没有放开任何新的能力。
const KEYS: { label: string; data: string }[] = [
  { label: "↑", data: "\x1b[A" },
  { label: "↓", data: "\x1b[B" },
  { label: "1", data: "1" },
  { label: "2", data: "2" },
  { label: "3", data: "3" },
  { label: "Tab", data: "\t" },
  { label: "Esc", data: "\x1b" },
];

/** 比对两份画面时的行分隔。 */
const LINE_SEP = String.fromCharCode(10);

/** 画面轮询间隔：按键后的回显要跟得上手指，又不至于把远程桥打满。 */
const SCREEN_POLL_MS = 900;

export function RemoteTerminalKeys({ sessionId, checkScreen = false }: {
  sessionId: number;
  /**
   * 发键前重取一次画面，和眼前这份不一样就不发，只把新画面换上来。主窗口名单行内用：那里的
   * 会话不是正开着的那个，画面只靠 900ms 一次的轮询，人按下去时光标可能已经不在看到的那一项上。
   * 手机页不开：它的画面和按键走同一条远程桥，多一次往返手感明显变钝。
   */
  checkScreen?: boolean;
}) {
  const t = useT();
  const [lines, setLines] = useState<string[] | null>(null);
  const linesRef = useRef(lines);
  linesRef.current = lines;
  const [text, setText] = useState("");
  const [error, setError] = useState(false);
  const [moved, setMoved] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    const tick = () => {
      void managedTerminalScreen(sessionId)
        .then((screen) => { if (!cancelled) setLines(screen); })
        .catch(() => {})
        .finally(() => { if (!cancelled) timer = window.setTimeout(tick, SCREEN_POLL_MS); });
    };
    tick();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [sessionId]);

  /** 画面还是人看到的那一份吗。不开 checkScreen 恒为是。变了就换上新画面并提示，返回否。 */
  const screenUnchanged = async (): Promise<boolean> => {
    if (!checkScreen) return true;
    const seen = (linesRef.current ?? []).join(LINE_SEP);
    const screen = await managedTerminalScreen(sessionId);
    // 取不到画面（终端已不在 Meowo 手里）：无法确认按下去落在哪，不发。
    if (!screen) { setError(true); return false; }
    if (screen.join(LINE_SEP) === seen) return true;
    // 画面变了：这一下不发。把新画面换上来，让人看清再按。
    setLines(screen);
    setMoved(true);
    return false;
  };
  const send = (data: string) => {
    setError(false);
    setMoved(false);
    // 不比对画面时当场就写（手机页）：多绕一个微任务也是手感上的延迟，现有测试也钉着这一点。
    if (!checkScreen) {
      void writeManagedTerminal(sessionId, data).catch(() => setError(true));
      return;
    }
    void screenUnchanged()
      .then((ok) => (ok ? writeManagedTerminal(sessionId, data) : undefined))
      .catch(() => setError(true));
  };
  /**
   * 文字作答：文字与回车分两次写、中间隔一拍（同 ChatWindow 的 SUBMIT_GAP_MS：同一块里的回车
   * 会被 TUI 当成粘贴内容的一部分，只换行不提交）。画面只在写文字之前比对一次，回车跟着文字走：
   * 两次各比各的话，文字被拦下时回车会单独发出去，落在人没看过的新提示上；文字发出去了，它自己
   * 的回显又会让回车被拦（做空审查 H2，两个方向都复现过）。
   */
  const sendLine = (line: string) => {
    setError(false);
    setMoved(false);
    void screenUnchanged()
      .then(async (ok) => {
        if (!ok) return;
        await writeManagedTerminal(sessionId, line);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
        await writeManagedTerminal(sessionId, "\r");
      })
      .catch(() => setError(true));
  };

  // 只留有字的行，并去掉首尾空行：终端满屏多是空白，手机上要省竖向空间。
  const visible = (lines ?? []).map((line) => line.replace(/\s+$/, ""));
  while (visible.length && !visible[0]) visible.shift();
  while (visible.length && !visible[visible.length - 1]) visible.pop();

  return (
    <div className="chat-remote-keys">
      <pre className="chat-remote-screen" aria-label={t.chat.remoteScreenLabel}>
        {visible.length ? visible.join("\n") : t.chat.remoteScreenEmpty}
      </pre>
      <div className="chat-remote-keyrow">
        {KEYS.map((key) => (
          <button type="button" key={key.label} onClick={() => send(key.data)}>{key.label}</button>
        ))}
        <button type="button" className="is-enter" onClick={() => send("\r")}>{t.chat.remoteKeyEnter}</button>
      </div>
      <form
        className="chat-remote-type"
        onSubmit={(event) => {
          event.preventDefault();
          if (!text) return;
          sendLine(text);
          setText("");
        }}
      >
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder={t.chat.remoteTypePlaceholder}
          enterKeyHint="send"
        />
        <button type="submit" disabled={!text}>{t.chat.remoteTypeSend}</button>
      </form>
      {moved && <span className="chat-remote-keys-moved" role="status">{t.chat.remoteScreenMoved}</span>}
      {error && <span className="chat-remote-keys-error" role="alert">{t.chat.remoteKeyFailed}</span>}
    </div>
  );
}
