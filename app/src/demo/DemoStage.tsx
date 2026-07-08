// demo 专用:桌面舞台——渐变背景 + 带阴影的贴纸窗口 + 字幕/收尾/假光标。
// 复用真实 Sticker / CollapsedStrip 组件,数据与形态由 mock store 驱动。
import { useEffect, useReducer } from "react";
import { Sticker } from "../views/Sticker";
import { CollapsedStrip } from "../views/CollapsedStrip";
import { store, subscribe } from "./mock";
import logoUrl from "../../src-tauri/icons/128x128@2x.png";

export function DemoStage() {
  const [, force] = useReducer((x: number) => x + 1, 0);
  useEffect(() => subscribe(force), []);
  const { mode, caption, finale } = store.stage;
  const strip = mode === "strip";
  return (
    <div className="demo-desktop">
      <div className="demo-blob demo-blob-a" />
      <div className="demo-blob demo-blob-b" />
      <div className="demo-grain" />
      <div className={"demo-window demo-mode-" + mode}>
        {strip ? (
          <CollapsedStrip data={store.sessions} edge="right" onExpand={() => {}} />
        ) : (
          <Sticker filter="all" data={store.sessions} />
        )}
      </div>
      {caption && (
        <div className="demo-caption" key={caption}>
          {caption}
        </div>
      )}
      {finale && (
        <div className="demo-finale">
          <img src={logoUrl} width={88} height={88} alt="" />
          <div className="demo-finale-name">Meowo</div>
          <div className="demo-finale-slogan">你所有的 Claude Code 会话,一眼看全</div>
        </div>
      )}
      <div id="demo-cursor">
        <svg width="20" height="22" viewBox="0 0 20 22">
          <path
            d="M2 1 L2 17 L6.5 13.5 L9.5 20 L12.5 18.7 L9.6 12.3 L15.5 11.8 Z"
            fill="#fff"
            stroke="#1b1917"
            strokeWidth="1.4"
          />
        </svg>
      </div>
    </div>
  );
}
