/// 把一条消息写进托管终端的 I/O 流程：正文与回车分两次写、写后回显验证、发送前暂存终端
/// 输入行里的草稿、清掉上次贴图残留、等新拉起的终端就绪。
///
/// 为什么从 ChatWindow 搬出来：这些都是「对某个会话的终端做什么、等什么」，不碰对话窗的
/// 状态，却和五千行的组件混在一个文件里，改发送流程时规则散在各处。会话身份、交互门控、
/// 界面提示仍归调用方（ChatWindow 的 withSendGuard / ensureWritableTerminal）。
/// 同会话多条消息的先后顺序见 createSendQueue。

import { managedTerminalScreen, managedTerminalSnapshot, writeManagedTerminal, type ChatUi } from "../../api";
import { appendTerminalText, terminalAttention as detectTerminalAttention, visibleTerminalText, type AttentionGrammar, type TerminalAttention } from "../../terminalAttention";

export function terminalInput(content: string): string {
  // 多行内容必须作为一次 bracketed paste 交给 TUI composer，否则附件列表中的换行可能被当成
  // 多次 Enter，导致第一行提前提交。单行保持原协议，兼容不启用 bracketed paste 的旧 CLI。
  return content.includes("\n") ? `\x1b[200~${content}\x1b[201~` : content;
}

/// 正文与 Enter 之间的基础间隔。实测（probe_enter.rs 对真实 kimi）：
/// 20ms / 60ms 失败——正文刚落，斜杠命令的**补全菜单还在异步渲染**，此时的 Enter 被菜单
/// 吃掉（当成选中候选）而不是提交 composer，正文就留在框里只换了行；
/// 150ms / 400ms 稳定成功。取 250ms：在 150ms 的成功线之上留足余量，人也感知不到。
const SUBMIT_GAP_MS = 250;
/// 超长粘贴要按长度追加消化时间：bracketed paste 的正文由 TUI 逐段渲染，几十 KB 的粘贴
/// 250ms 内消化不完，此时的回车会被当成正文换行、或落在还没挂稳的 composer 上。每 KB
/// 追加 50ms（10KB 粘贴 ≈ 750ms），封顶 2s——再长的粘贴也不该让人干等。
const SUBMIT_GAP_PER_KB_MS = 50;
const SUBMIT_GAP_MAX_MS = 2_000;
export function submitGapMs(content: string): number {
  return Math.min(SUBMIT_GAP_MS + Math.floor(content.length / 1024) * SUBMIT_GAP_PER_KB_MS, SUBMIT_GAP_MAX_MS);
}
/// 把 content 写进 composer 并提交：正文与回车**必须分两次写**。
///
/// 实测依据（app/src-tauri/tests/probe_enter.rs 对真实 kimi 的 PTY 探针）：
/// - 分两次写 `/plan on` → `\r`：状态栏切到 plan、输入框清空 = **提交成功**
/// - 合并成一次写 `"/plan on\r"`：`/plan on` 留在输入框里 = **只换行不提交**
///
/// 原因是 TUI 把一次写入当成一个输入事件/粘贴块处理，块内的 `\r` 只是内容里的换行，
/// 而不是「按下 Enter」。两次写才符合真实键盘的语义。多行内容同理，且必须先包成
/// bracketed paste（见 terminalInput），否则中间的换行会被当成多次 Enter 提前提交。
///
/// `abortIf`:回车前的最后复核。发送守卫只在开头查一次终端占用,而屏幕识别有 150ms
/// 节流、正文与回车之间又隔 submitGapMs(随粘贴长度加长)——交互提示恰在这窗口里弹出时,这个 `\r`
/// 会替用户回答它(如命令审批的默认焦点项)。复核命中则不发回车,并 Ctrl-U 尽力清掉
/// 已写进对方界面的正文,抛错让调用方把「终端在等交互」呈现出来。
///
/// `verify`:写后回显验证(恢复/接管后的险区才传,见 RESUME_VERIFY_WINDOW_MS)。刚拉起
/// 的 CLI 可能还压着 resume 确认选择器、或 composer 尚未挂载——waitForTerminalReady 的
/// 「可见 + 安静 700ms」对这两种形态都会误判成就绪。此时正文被吞,而无条件回车会替用户
/// 按下选择器的默认焦点项:消息蒸发、恢复继续、没有任何报错(实拍:接管后对话页发送,
/// transcript 里根本没有这条 user 消息)。写完正文先确认它真的出现在屏幕上,再发回车。
export async function submitToTerminal(sessionId: number, content: string, abortIf?: () => string | null, verify?: { message: string }, submitInput = "\r"): Promise<void> {
  const initialAbort = abortIf?.();
  if (initialAbort) throw new Error(initialAbort);
  const before = verify ? await managedTerminalSnapshot(sessionId) : null;
  await writeManagedTerminal(sessionId, terminalInput(content));
  if (before && verify) {
    // 匹配规则:正文与可见文本都压掉空白与框线字符后做前缀包含——回显会被 composer
    // 边框/自动折行打散;长粘贴认 claude 的占位形态(Pasted text)。只扫**写入之后的
    // 增量**输出:屏幕旧内容(上一回合恰好含同样字句)不作数。
    const squash = (text: string) => text.replace(/[\s│┃❯>|]/g, "").toLowerCase();
    const needle = squash(content).slice(0, 16);
    const decoder = new TextDecoder();
    let tail = "";
    let since = Number.isFinite(before.endOffset) ? before.endOffset : 0;
    let seen = needle.length === 0;
    for (let attempt = 0; attempt < 10 && !seen; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 200));
      const cancelled = abortIf?.();
      if (cancelled) throw new Error(cancelled);
      const snapshot = await managedTerminalSnapshot(sessionId, since);
      if (!snapshot.active) break;
      if (Number.isFinite(snapshot.endOffset) && snapshot.endOffset < since) {
        // 偏移回绕(PTY 被重启复位):归零重对齐,别拿旧偏移空等(同 sendWithClipboardImages)。
        since = 0;
        tail = "";
        continue;
      }
      since = snapshot.endOffset;
      tail = appendTerminalText(tail, snapshot.data, decoder);
      const visible = visibleTerminalText(tail);
      if (squash(visible).includes(needle) || /pasted text/i.test(visible)) seen = true;
    }
    if (!seen) {
      // 回显没来 = 正文没进 composer(被恢复确认/启动提示拦下,或被初始化丢弃)。
      // **绝不能发回车**;Ctrl-U 尽力清掉可能已落进对方界面的残余,抛错让消息退回输入框。
      const cancelled = abortIf?.();
      if (cancelled) throw new Error(cancelled);
      await writeManagedTerminal(sessionId, "\x15").catch(() => {});
      throw new Error(verify.message);
    }
  }
  await new Promise((resolve) => window.setTimeout(resolve, submitGapMs(content)));
  const abortReason = abortIf?.() ?? null;
  if (abortReason) throw new Error(abortReason);
  await writeManagedTerminal(sessionId, submitInput);
}

/// 定位 composer 首行,返回提示符之后的文本(已 trim);认不出返回 null。提示行的上一行须
/// 整行是边框:历史区回显的用户消息也可能以同一提示符开头。
export function composerDraftText(screen: string[] | null, stash: NonNullable<ChatUi["draft_stash"]>): string | null {
  if (!screen) return null;
  const isBorder = (line: string) => {
    const trimmed = line.trim();
    return trimmed !== "" && [...trimmed].every((char) => char === stash.composer_border);
  };
  for (let index = screen.length - 1; index > 0; index -= 1) {
    const line = screen[index].trimStart();
    if (isBorder(screen[index - 1]) && line.startsWith(stash.composer_prompt)) {
      return line.slice(stash.composer_prompt.length).trim();
    }
  }
  return null;
}

/// 发送前收起 composer 里用户在终端输入行留下的未提交草稿(issue #72:残留会拼进
/// 消息)。CLI 在下一次提交后自己还原草稿,终端里的输入原样回来。判断全看仿真后的整屏
/// (规格与取证见后端 DraftStash / tests/probe_draft_residual.rs):
/// - 认不出 composer(无屏幕仿真、提示符不在屏上)→ 不按:画面不在预期形态时盲按
///   暂存键,可能触发对方别的功能;
/// - composer 空 → 不按:暂存键是切换语义,空 composer 上按会把旧暂存放出来;
/// - 非空才按;按前有暂存标记、按后消失 = 放出了用户手动暂存的内容,再按一次收回。
/// 任何环节失败都静默放行:最坏退回修复前的行为(草稿拼进消息),不能挡住发送。
/// 已知代价:composer 非空且用户**另有**手动暂存时,新暂存会覆盖旧的(CLI 只存一份)。
export async function stashComposerDraft(sessionId: number, stash: NonNullable<ChatUi["draft_stash"]>): Promise<void> {
  try {
    const screen = await managedTerminalScreen(sessionId);
    if (!composerDraftText(screen, stash)) return;
    const hasMarker = (lines: string[] | null) => !!lines?.some((line) => line.includes(stash.stashed_marker));
    const hadStash = hasMarker(screen);
    await writeManagedTerminal(sessionId, stash.input);
    // 成功条件是 composer 变空,不看标记出现:用户已有手动暂存时标记按前按后都在,
    // 靠标记判定会白等满整个轮询窗(PR #73 评审)。
    for (let attempt = 0; attempt < 8; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 50));
      const now = await managedTerminalScreen(sessionId);
      if (composerDraftText(now, stash) === "") return;
      if (hadStash && !hasMarker(now)) {
        await writeManagedTerminal(sessionId, stash.input);
        return;
      }
    }
  } catch {
    // 放行:见函数注释。
  }
}

/// 上次发图失败留在 composer 里的占位符(`[Image #1]`)自动清掉,返回是否已确认干净。
/// 失败那次只贴了图、正文没写(写前草稿已暂存),故 composer 里只有残图,Ctrl-U 整行删
/// 不会误伤用户内容;原稿与附件仍在对话页输入框里,重发会重新贴。claude 2.1.303 实测:
/// Ctrl-U 后再提交的 user 行不带 image 块,图随占位符一起丢弃。空 composer 显示灰色
/// 提示语(`Try "…"`),所以判干净看「不再含占位符」而不是「为空」。
/// 认不出 composer 或清后仍在 → false,调用方退回人工清理的拦截。
export async function clearPartialImageDraft(sessionId: number, stash: NonNullable<ChatUi["draft_stash"]>, marker: string): Promise<boolean> {
  const pattern = new RegExp(marker, "i");
  const residual = async () => {
    const draft = composerDraftText(await managedTerminalScreen(sessionId), stash);
    return draft === null ? null : pattern.test(draft);
  };
  try {
    const before = await residual();
    if (before === null) return false;
    if (!before) return true;
    await writeManagedTerminal(sessionId, "\x15");
    for (let attempt = 0; attempt < 10; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 100));
      if (await residual() === false) return true;
    }
  } catch {
    // 落到下面的人工清理拦截。
  }
  return false;
}

/// 恢复/接管完成后的险区时间窗:这段时间内的发送启用 submitToTerminal 的写后回显验证。
/// 稳态终端不验证——占用检查、attention 识别与 abortIf 复核已经覆盖,逐条轮询屏幕只会
/// 拖慢每次发送。
export const RESUME_VERIFY_WINDOW_MS = 15_000;

/// 贴图后等占位符（`[Image #N]`）落出的轮询次数，每次 250ms。claude 在 Windows 上读剪贴板图片
/// 是串行起两个 PowerShell（ContainsImage 检查 + GetImage 存 PNG，2.1.304 源码），之后还要
/// sharp 缩放：本机实测两个进程合计约 2.7s，机器忙时更久。原先只等 1.5s，图片晚到时被判
/// 「附加未完成」，迟到的 [Image #N] 又留在终端输入框里，下一次发送被要求去终端清理。
/// 放宽到 10s；测试环境保持 1.5s，免得超时用例每条多等十秒。
export const IMAGE_PASTE_LANDING_POLLS = import.meta.env.MODE === "test" ? 6 : 40;
/// 放弃原生贴图、清掉残图之后再观察多久:迟到的 [Image #N] 可能在 Ctrl-U 之后才落出,
/// 落出就再清一次,免得它粘进下一条消息。
export const LATE_IMAGE_GRACE_MS = import.meta.env.MODE === "test" ? 100 : 1500;

export type TerminalStartupResult = "ready" | TerminalAttention;

export type TerminalReadyMessages = {
  exited: (code: number | null, tail?: string) => string;
  failed: string;
  timeout: string;
};

export async function waitForTerminalReady(sessionId: number, attentionMarkers: string[], messages: TerminalReadyMessages, grammar?: AttentionGrammar, onWaitProgress?: (elapsedSeconds: number) => void): Promise<TerminalStartupResult> {
  const startedAt = Date.now();
  const decoder = new TextDecoder();
  let outputTail = "";
  let lastOutputAt = 0;
  let hasVisible = false;
  // 带 since 只拉增量；保留一小段解码后的尾部，让跨 IPC 分片的提示仍可识别。
  let since = 0;
  // 等待进度的上次报送值：只在整秒翻动时回调，80ms 轮询不刷爆 setState。
  // 头 2 秒不报——快启动是常态，等待条不该一闪而过。
  let lastReportedSecond = 1;
  while (Date.now() - startedAt < 45_000) {
    const elapsedSeconds = Math.floor((Date.now() - startedAt) / 1000);
    if (elapsedSeconds > lastReportedSecond) {
      lastReportedSecond = elapsedSeconds;
      onWaitProgress?.(elapsedSeconds);
    }
    const snapshot = await managedTerminalSnapshot(sessionId, since);
    if (!snapshot.active) {
      if (snapshot.exited) {
        // 失败带 tail:CLI 拒绝启动的原因只打在 PTY 输出里(banner 后报错退出的 CLI
        // 会骗过后端 1s 秒退探测),随快照带上来,别只甩一句「退出码 X」。
        throw new Error(messages.exited(snapshot.exitCode ?? null, snapshot.exitTail?.trim() || undefined));
      }
      throw new Error(messages.failed);
    }
    // 判「有新输出」看 endOffset 而不是 data：data 现在是增量，首帧之后的轮次
    // 常常为空，用它判断会把已经就绪的终端误判成还没输出。
    const grew = snapshot.endOffset > since;
    since = snapshot.endOffset;
    outputTail = appendTerminalText(outputTail, snapshot.data, decoder);
    const attention = detectTerminalAttention(outputTail, attentionMarkers, false, false, grammar);
    if (attention) return attention;
    if (grew) lastOutputAt = Date.now();
    if (!hasVisible && visibleTerminalText(outputTail)) hasVisible = true;
    // 就绪 = 已画出可见内容（--resume 启动阶段只有清屏/光标序列，不算）且输出安静了
    // 700ms。回放长 transcript 时输出持续、计时随之顺延，不会把消息写进还在初始化的
    // composer；固定「首字节后 700ms」正是之前吞消息的根因。
    if (hasVisible && lastOutputAt && Date.now() - lastOutputAt >= 700) return "ready";
    // 极端情况：TUI 常驻动画让输出永不安静。已有可见画面且没识别到阻塞提示时，
    // 20 秒后按就绪处理——比直接超时报错对用户更有用。
    if (hasVisible && Date.now() - startedAt >= 20_000) return "ready";
    await new Promise((resolve) => window.setTimeout(resolve, 80));
  }
  throw new Error(messages.timeout);
}

/// 同一会话的正文消息依次写进终端：前一条写完（成败都算）才写下一条，连发几条不会互相
/// 插键。不同会话互不等待。切走会话不影响已排上的发送（先上屏再发送，用户要求）。
/// `busy(sid)`：该会话还有发送没落定——撤回、切模式这类同样要写终端的动作据此避让。
export function createSendQueue() {
  const chains = new Map<number, Promise<boolean>>();
  return {
    enqueue(sessionId: number, deliver: () => Promise<boolean>): Promise<boolean> {
      const previous = chains.get(sessionId) ?? Promise.resolve(true);
      const next = previous.then(() => deliver(), () => deliver()).catch(() => false);
      chains.set(sessionId, next);
      void next.then(() => { if (chains.get(sessionId) === next) chains.delete(sessionId); });
      return next;
    },
    busy: (sessionId: number) => chains.has(sessionId),
  };
}
