// 手机远程页的根：ChatWindow 加上叠在它上面的新建会话浮层。从 main.tsx 拆出来是为了让
// 截图 harness（visual/mobile.tsx）挂同一棵树，新建浮层与底栏入口才拍得到。
// 这个模块自身不能有会 invoke 的副作用：ChatWindow / NewSessionPanel 用 React.lazy 延迟到
// 渲染期加载，那时 main.tsx 的 markRemoteUi + installRemoteTransport 早已就位。
import React from "react";
import { NEW_SESSION_EVENT, SELECT_SESSION_EVENT } from "../remoteMode";
import { useHistoryLayer } from "../hooks/useHistoryLayer";

const ChatWindow = React.lazy(() =>
  import("../views/ChatWindow").then((m) => ({ default: m.ChatWindow })),
);
const NewSessionPanel = React.lazy(() =>
  import("../views/NewSessionPanel").then((m) => ({ default: m.NewSessionPanel })),
);

export function RemoteApp() {
  // 新建会话在桌面开独立窗;远程转页内事件,这里叠加渲染 NewSessionPanel(会话列表/对话仍在底层)。
  // 事件 detail 里带卡片菜单的 cwd/provider 预填——桌面走 URL query / ns-prefill,远程两条都不通。
  const [newSession, setNewSession] = React.useState<null | { cwd?: string | null; provider?: string | null }>(null);
  React.useEffect(() => {
    const open = (e: Event) => {
      const d = (e as CustomEvent).detail as { cwd?: string | null; provider?: string | null } | undefined;
      setNewSession({ cwd: d?.cwd, provider: d?.provider });
    };
    window.addEventListener(NEW_SESSION_EVENT, open);
    return () => window.removeEventListener(NEW_SESSION_EVENT, open);
  }, []);
  // M-2（走查 2026-11-12）：新建浮层原先不压历史，右滑就是浏览器的上一页，扫码打开的页面会
  // 直接离开 Meowo，填了一半的提示词也没了。压一条，右滑只关浮层。
  const { handOffToChat } = useHistoryLayer(newSession != null, () => setNewSession(null));
  return (
    <>
      <ChatWindow />
      {newSession && (
        <div className="remote-new-session-overlay">
          <NewSessionPanel
            onClose={() => setNewSession(null)}
            prefill={newSession}
            // 启动成功:临时负 id 经页内事件送 ChatWindow 选中新会话(侧栏点选的同一通道;
            // 桥不 reveal,没有这一步用户会落回「去侧栏选会话」空态)。null 不导航。
            onLaunched={(tempId) => {
              if (typeof tempId === "number") {
                // 浮层那条历史原地换成对话记录：右滑从新会话回列表，而不是先「白退一步」关一个已经关掉的浮层。
                handOffToChat();
                window.dispatchEvent(new CustomEvent(SELECT_SESSION_EVENT, { detail: tempId }));
              }
            }}
          />
        </div>
      )}
    </>
  );
}
