import { useEffect, useState } from "react";
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
  // 退格：发图没贴完时终端输入框里留着残图占位（claude `[Image #1]`、kimi `[image #1 (W×H)]`，整块一删），
  // Meowo 认不出那个输入框就清不了，手机上得能自己删掉（第三轮验收 13：此前手机上只有一个点了没反应的「去终端页」）。
  { label: "⌫", data: "\x7f" },
];

/** 画面轮询间隔：按键后的回显要跟得上手指，又不至于把远程桥打满。 */
const SCREEN_POLL_MS = 900;

export function RemoteTerminalKeys({ sessionId }: { sessionId: number }) {
  const t = useT();
  const [lines, setLines] = useState<string[] | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState(false);

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

  const send = (data: string) => {
    setError(false);
    void writeManagedTerminal(sessionId, data).catch(() => setError(true));
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
          // 文字与回车分两次写、中间隔一拍（同 ChatWindow 的 SUBMIT_GAP_MS）：同一块里的
          // 回车会被 TUI 当成粘贴内容的一部分，只换行不提交。
          send(text);
          window.setTimeout(() => send("\r"), 250);
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
      {error && <span className="chat-remote-keys-error" role="alert">{t.chat.remoteKeyFailed}</span>}
    </div>
  );
}
