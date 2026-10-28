import { agentAssets, tintStyle } from "../../providers";
import { useT } from "../../i18n";

/// 欢迎面板里优先露出的内置命令：按这个顺序挑当前 agent 确实支持的（slash_commands
/// 由插件声明，opencode 没有 /model 之类的差异由它兜住），不在清单里的不画死按钮。
const STARTER_COMMANDS = ["/resume", "/model", "/init", "/help"];

export type WelcomeInfo = {
  provider: string;
  name: string;
  version: string | null;
  model: string | null;
  branch: string | null;
  changes: number;
};

/// 空会话的欢迎面板（A+B 改版定稿）：agent 标识 + 会话事实条（等宽，终端腔）+ 起手入口。
/// 起手入口只做「往输入框预填」，不直接发送——用户还能改，也不会误触就开跑一轮。
export function WelcomePanel({ info, commands, mention, onPick }: {
  info: WelcomeInfo;
  commands: string[];
  mention: boolean;
  onPick: (text: string) => void;
}) {
  const t = useT();
  const { Icon } = agentAssets(info.provider);
  const starters = STARTER_COMMANDS.filter((name) => commands.includes(name));
  return (
    <div className="chat-welcome">
      <span className="chat-welcome-mark" style={tintStyle(info.provider)} aria-hidden="true"><Icon /></span>
      <div className="chat-welcome-title">{info.name}</div>
      <div className="chat-welcome-sub">{t.chat.welcomeSub}</div>
      <div className="chat-welcome-facts">
        <span>agent<b>{info.version ? `${info.provider} ${info.version}` : info.provider}</b></span>
        {info.model && <span>model<b>{info.model}</b></span>}
        {info.branch && (
          <span>branch<b>{info.branch}</b>{info.changes > 0 && <em>{t.chat.welcomeChanges(info.changes)}</em>}</span>
        )}
      </div>
      {(starters.length > 0 || mention || info.changes > 0) && (
        <div className="chat-welcome-chips">
          {info.changes > 0 && (
            <button type="button" className="is-primary" onClick={() => onPick(t.chat.welcomeReviewPrompt)}>{t.chat.welcomeReview}</button>
          )}
          {starters.map((name) => (
            <button type="button" key={name} onClick={() => onPick(`${name} `)}>
              <code>{name}</code>{t.chat.slashDesc[name] ?? ""}
            </button>
          ))}
          {mention && <button type="button" onClick={() => onPick("@")}><code>@</code>{t.chat.welcomeMention}</button>}
        </div>
      )}
    </div>
  );
}
