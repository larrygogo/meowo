// 渲染崩溃兜底。此前全 app 没有一个 ErrorBoundary：任何组件渲染时抛错（实测 3000 层嵌套引用
// 让 react-markdown 栈溢出），React 会卸掉整棵树，窗口直接白屏，无边框窗连拖动、关闭都做不到。
//
// 两档：
// - 窗口级（各窗口入口的根，WindowCrash）：一句「出错了」+「重新加载」，整页 reload 即恢复。
// - 消息级（对话里每条消息、每段 markdown）：退回纯文本显示原文，别的消息照常。
// 两档都 console.error 记下错误与组件栈，排障有线索。
import { Component, type ErrorInfo, type ReactNode } from "react";
import { useT } from "./i18n";

type Props = {
  children: ReactNode;
  /** 出错时渲染什么。 */
  fallback: (error: Error) => ReactNode;
  /** 这个值一变就清掉错误重试（流式消息的正文每帧都在变，后一帧可能就正常了）。 */
  resetKey?: unknown;
  /** 记日志时标明是哪一层。 */
  scope: string;
};

type State = { error: Error | null; resetKey: unknown };

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, resetKey: this.props.resetKey };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    if (props.resetKey !== state.resetKey) return { error: null, resetKey: props.resetKey };
    return null;
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error(`[${this.props.scope}] 渲染出错`, error, info.componentStack);
  }

  render() {
    return this.state.error ? this.props.fallback(this.state.error) : this.props.children;
  }
}

/** 窗口级兜底页。整窗可拖（无边框窗没有系统标题栏，白屏时原本连挪都挪不动）。 */
export function WindowCrash() {
  const t = useT();
  return (
    <div className="app-crash" role="alert" data-tauri-drag-region>
      <p className="app-crash-title">{t.crash.title}</p>
      <p className="app-crash-hint">{t.crash.hint}</p>
      <button type="button" className="app-crash-reload" onClick={() => window.location.reload()}>
        {t.crash.reload}
      </button>
    </div>
  );
}

/** 窗口根节点包一层；放在 I18nProvider 里面，兜底页才有字典可用。 */
export function WindowErrorBoundary({ children }: { children: ReactNode }) {
  return <ErrorBoundary scope="window" fallback={() => <WindowCrash />}>{children}</ErrorBoundary>;
}
