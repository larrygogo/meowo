/**
 * 模型 / 思考档位（effort）/ 模式三类切换动作（从 ChatWindow.tsx 原样抽出，行为不变）。
 *
 * 为什么只抽动作、不抽守卫：三者都要走 ChatWindow 的 withSendGuard（占用检查、接管入口、
 * sending 与错误横幅），那套守卫与正文发送、排队、终端 attention 交织在一起，搬过来只会
 * 把半个 ChatWindow 拖进来。所以守卫、当前会话判定和 history 的 setter 都由调用方注入，
 * 这里只保留「写命令 → 读屏幕/输出确认 → 落值」的编排和它独有的状态（effortBusy、
 * 模式回显代际）。模型清单的数据合流另见 useModelPresets.ts。
 */
import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { managedTerminalSnapshot, setSessionLaunchSelection, writeManagedTerminal, type ChatHistory, type ChatUi, type ModelCatalogEntry, type ModelPreset, type ModeScreenMarker } from "../../api";
import { useT } from "../../i18n";
import { appendTerminalText, modeFromScreen, visibleTerminalText } from "../../terminalAttention";
import { currentPresetId } from "./modelLabels";
import { stashComposerDraft, submitToTerminal } from "./terminalSubmit";
import type { SendErrorExit } from "./SendErrorExits";

/** 屏幕上 CLI 的一行回话：去掉命令输出前的 `⎿` 引线与首尾空白，原话转给用户。 */
function cliLine(line: string): string {
  return line.replace(/^[\s⎿]+/, "").trim();
}

export interface ModelControlsDeps {
  sessionId: number;
  chatUi: ChatUi | null | undefined;
  /** history?.model：算当前模型可选的思考档位。 */
  model: string | null | undefined;
  modelCatalog: ModelCatalogEntry[];
  sending: boolean;
  /** 实时的会话色调（running 时一律拒绝切换，免得命令打进正在跑的回合）。 */
  toneRef: Readonly<{ current: string }>;
  /** 外部占用被拒后，接管成功要重放的动作。 */
  retryRef: { current: (() => void | Promise<void>) | null };
  withSendGuard: (body: () => Promise<boolean>) => Promise<boolean>;
  attentionAbort: () => string | null;
  isCurrentOperation: () => boolean;
  /** exit：错误条上的出路（重试 / 看终端画面），见 SendErrorExits。 */
  setSendError: (message: string, exit?: SendErrorExit) => void;
  setHistory: Dispatch<SetStateAction<ChatHistory | null>>;
  setStoredSelections: Dispatch<SetStateAction<Record<string, string>>>;
}

export function useModelControls({
  sessionId, chatUi, model, modelCatalog, sending, toneRef, retryRef,
  withSendGuard, attentionAbort, isCurrentOperation, setSendError, setHistory, setStoredSelections,
}: ModelControlsDeps) {
  const t = useT();
  const selectModel = async (preset: ModelPreset) => {
    const command = chatUi?.model_select_command;
    if (!command || toneRef.current === "running") { setSendError(t.chat.menuBusy); return; }
    await withSendGuard(async () => {
      const before = await managedTerminalSnapshot(sessionId);
      if (chatUi?.draft_stash) await stashComposerDraft(sessionId, chatUi.draft_stash);
      await submitToTerminal(sessionId, `${command} ${preset.id}`, attentionAbort);
      let since = before.endOffset;
      let tail = "";
      const decoder = new TextDecoder();
      // 确认/拒绝文案由插件声明（claude 2.1.322：确认 `Set model to …`，拒绝如 `Kept model as …`、
      // `Model switch to … was blocked by a PreModelSwitch hook`，见 plugins/claude/mod.rs 的
      // model_select_reply）。没声明的 CLI 用通用确认句兜底。
      const reply = chatUi?.model_select_reply;
      const confirmLine = new RegExp(reply?.confirm_pattern ?? "set model to|switched (?:the )?model to|model (?:changed|switched) to", "i");
      const rejectLine = reply ? new RegExp(reply.reject_pattern, "i") : null;
      // 比对时去掉 id 末尾的方括号变体（opus[1m] → opus）：Claude Code 2.1.322 起确认行只给显示名
      //「Set model to `Opus 5.5` and saved as your default for new sessions」，整串 id「opus1m」在里面
      // 永远找不到，于是 CLI 明明切过去了，这里却报未确认、也不记住选择（2026-11-12 leader 报
      //「切模型无法生效」，transcript 实证）。仍要求家族名出现：CLI 有自动降档路径，降到别的家族时
      // 也会打印「Set model to <实际模型>」，不比对就会把没生效的选择写进恢复配置（做空审查）。
      const family = preset.id.replace(/\[[^\]]*\]$/, "").toLowerCase().replace(/[^a-z0-9]/g, "");
      let outcome: { kind: "ok" } | { kind: "rejected"; line: string } | null = null;
      let otherModel: string | null = null;
      for (let attempt = 0; attempt < 12 && outcome === null; attempt += 1) {
        await new Promise((resolve) => window.setTimeout(resolve, 200));
        if (!isCurrentOperation()) return false;
        const snapshot = await managedTerminalSnapshot(sessionId, since);
        if (!snapshot.active || snapshot.endOffset < since) break;
        since = snapshot.endOffset;
        tail = appendTerminalText(tail, snapshot.data, decoder);
        // 只认本次命令之后 CLI 自己的回话，菜单列表中出现模型名不算切换成功。先整屏找确认行，
        // 再找拒绝行：重绘可能把旧行与新行一起带进来，确认优先才不会被旧的拒绝行抢先。
        const lines = visibleTerminalText(tail).split(/\r?\n/);
        const confirmed = lines.filter((line) => confirmLine.test(line));
        if (confirmed.some((line) => line.toLowerCase().replace(/[^a-z0-9]/g, "").includes(family))) { outcome = { kind: "ok" }; break; }
        // 确认成了别的模型：先记下，继续等本家族的那一行（可能是重绘带进来的旧确认）。
        if (confirmed.length > 0) otherModel = cliLine(confirmed[confirmed.length - 1]);
        const rejected = rejectLine ? lines.find((line) => rejectLine.test(line)) : undefined;
        if (rejected) outcome = { kind: "rejected", line: cliLine(rejected) };
      }
      if (outcome?.kind !== "ok") {
        // CLI 说了什么就转什么（2026-11-12 研究 C4：确认框被取消、hook 拦截、别名校验失败时
        // 以前一律只报「未确认」，用户看不到原因）。
        if (outcome?.kind === "rejected") setSendError(t.chat.modelSwitchRejected(outcome.line));
        else if (otherModel) setSendError(t.chat.modelSwitchOther(otherModel));
        // 没等到任何回话：切没切过去只有终端画面说得清，给「重试」与看终端画面两条出路，
        // 不再只说「请到终端检查」（走查 C-4，手机上那句是死路）。
        else setSendError(t.chat.modelSwitchUnconfirmed, { retry: () => void selectModel(preset), terminal: true });
        return false;
      }
      await setSessionLaunchSelection(sessionId, "model", preset.id);
      if (isCurrentOperation()) setStoredSelections((prev) => ({ ...prev, model: preset.id }));
      return true;
    });
  };
  /// 切换思考程度（对照 Kimi 模型菜单的「思考」分段）：写 `/effort <档位>`，只认本次之后
  /// CLI 自己的确认行 `Set effort level to <Level>`；被拒（如当前模型不支持 Max）时把 CLI
  /// 的原话带给用户。确认后乐观写进 history，statusline 下一帧会用同一个值覆盖。
  const [effortBusy, setEffortBusy] = useState(false);
  const selectEffort = async (level: string) => {
    const command = chatUi?.effort?.command;
    if (!command || toneRef.current === "running") { setSendError(t.chat.menuBusy); return; }
    setEffortBusy(true);
    try {
      await withSendGuard(async () => {
        const before = await managedTerminalSnapshot(sessionId);
        if (chatUi?.draft_stash) await stashComposerDraft(sessionId, chatUi.draft_stash);
        await submitToTerminal(sessionId, `${command} ${level}`, attentionAbort);
        let since = before.endOffset;
        let tail = "";
        const decoder = new TextDecoder();
        // 确认/拒绝文案由插件声明（claude `Set effort level to X`、kimi `Thinking set to X.`）。
        const confirm = new RegExp(chatUi.effort!.confirm_pattern, "i");
        const reject = new RegExp(chatUi.effort!.reject_pattern, "i");
        let outcome: { kind: "ok" } | { kind: "rejected"; line: string; actual: string | null } | null = null;
        for (let attempt = 0; attempt < 12 && outcome === null; attempt += 1) {
          await new Promise((resolve) => window.setTimeout(resolve, 200));
          if (!isCurrentOperation()) return false;
          const snapshot = await managedTerminalSnapshot(sessionId, since);
          if (!snapshot.active || snapshot.endOffset < since) break;
          since = snapshot.endOffset;
          tail = appendTerminalText(tail, snapshot.data, decoder);
          const lines = visibleTerminalText(tail).split(/\r?\n/);
          if (lines.some((line) => confirm.exec(line)?.[1]?.toLowerCase() === level)) { outcome = { kind: "ok" }; break; }
          for (const line of lines) {
            const hit = reject.exec(line);
            // 捕获组 1 = CLI 实际改成的档位（claude 2.1.322 封顶：`… set to 'high' instead`）。
            if (hit) { outcome = { kind: "rejected", line: cliLine(line), actual: hit[1]?.toLowerCase() ?? null }; break; }
          }
        }
        if (outcome === null) { setSendError(t.chat.effortUnconfirmed, { retry: () => void selectEffort(level), terminal: true }); return false; }
        if (outcome.kind === "rejected") {
          const actual = outcome.actual;
          if (actual) {
            if (isCurrentOperation()) setHistory((current) => current ? { ...current, effort: actual } : current);
            setSendError(t.chat.effortCapped(outcome.line));
          } else {
            setSendError(t.chat.effortRejected(outcome.line));
          }
          return false;
        }
        if (isCurrentOperation()) setHistory((current) => current ? { ...current, effort: level } : current);
        return true;
      });
    } finally {
      setEffortBusy(false);
    }
  };
  /// 当前模型可选的思考档位：插件给了静态表就用（claude），否则取模型目录里当前模型声明的
  /// （kimi 的 support_efforts，随模型变）。
  const effortLevels: readonly string[] = !chatUi?.effort ? []
    : chatUi.effort.levels.length > 0 ? chatUi.effort.levels
    : modelCatalog.find((entry) => entry.id === currentPresetId(modelCatalog, model))?.efforts ?? [];
  /// 把某维度的显示值写进 history（乐观更新与屏幕回显共用）。transcript 增量随后到达时
  /// 会覆盖为同一个值（或纠正我们），合并逻辑见轮询处。
  const applyModeValue = (dimension: string, value: string) => {
    setHistory((current) => {
      if (!current) return current;
      const agentModes = [...current.agentModes];
      const index = agentModes.findIndex((mode) => mode.dimension === dimension);
      const update = { dimension, value };
      if (index >= 0) agentModes[index] = update;
      else agentModes.push(update);
      return { ...current, agentModes };
    });
  };
  /// 屏幕回显的代际计数：新一次模式操作作废还在跑的旧轮询，避免旧屏幕的滞后帧倒灌。
  const modeEchoSeqRef = useRef(0);
  // 换会话/卸载同样作废：旧会话的回显不能写进新会话的 history。
  useEffect(() => () => { modeEchoSeqRef.current += 1; }, [sessionId]);
  /// cycle 是盲切，而 claude 只在活跃回合往 transcript 写模式记录——空闲时切换（恰是用户
  /// 切权限模式的典型时机）没有任何增量可等，标签会一直冻在旧值上，看起来就是「点了没效果」。
  /// 补救：写入后短暂轮询 PTY 屏幕，用插件声明的指示文案（provider 文档承诺的稳定文本）
  /// 识别落点。屏幕就是 CLI 自己画的权威状态，识别到什么就显示什么；识别不到则保持现状，
  /// 等 transcript 下一条记录兜底。
  /// 读一次屏幕回显：在 `budgetMs` 内轮询 PTY，识别到模式值就立刻返回（识别不到返回 null）。
  /// 供 echoModeFromScreen（观察）与 cycleToMode（逐步逼近目标）共用同一套识别口径。
  const readModeEcho = async (markers: ModeScreenMarker[], baseline: number, budgetMs: number): Promise<string | null> => {
    if (markers.length === 0) return null;
    const decoder = new TextDecoder();
    let tail = "";
    let since = baseline;
    const startedAt = Date.now();
    while (Date.now() - startedAt < budgetMs) {
      let snapshot;
      try { snapshot = await managedTerminalSnapshot(sessionId, since); } catch { return null; }
      if (!snapshot?.active) return null;
      if (snapshot.endOffset < since) { since = 0; tail = ""; continue; }
      since = snapshot.endOffset;
      tail = appendTerminalText(tail, snapshot.data, decoder);
      const value = modeFromScreen(visibleTerminalText(tail), markers);
      if (value) return value;
      await new Promise((resolve) => window.setTimeout(resolve, 120));
    }
    return null;
  };
  /// 循环按 cycle 键直到落到目标模式：claude 的权限模式只有「切下一个」的快捷键，没有
  /// 直达某个值的办法，但用户要的是「选那个模式」而不是「按 N 次」。每按一次读一次屏幕
  /// 回显确认落点，最多绕一圈（模式数）——转满一圈还没到说明该模式在当前账号/启动参数下
  /// 不可用（插件注释已声明模式集合会变），此时停手并说明，不再空转。
  const cycleToMode = async (dimension: string, target: string, cycleInput: string, markers: ModeScreenMarker[]) => {
    if (sending) return;
    await withSendGuard(async () => {
      const seq = ++modeEchoSeqRef.current;
      for (let step = 0; step < Math.max(1, markers.length); step += 1) {
        if (modeEchoSeqRef.current !== seq) return true; // 换会话/新操作作废本轮
        let base = 0;
        try { base = (await managedTerminalSnapshot(sessionId, 0))?.endOffset ?? 0; } catch { /* 从 0 起扫 */ }
        await writeManagedTerminal(sessionId, cycleInput);
        const value = await readModeEcho(markers, base, 1_500);
        if (value) applyModeValue(dimension, value);
        if (value === target) return true;
      }
      setSendError(t.chat.modeUnreachable);
      return true;
    });
  };
  const echoModeFromScreen = async (dimension: string, markers: ModeScreenMarker[], baseline: number) => {
    if (markers.length === 0) return;
    const seq = ++modeEchoSeqRef.current;
    const decoder = new TextDecoder();
    let tail = "";
    // 从**按键之前**的偏移起扫(baseline 由 changeMode 在写入前采):回显必然落在其后,
    // 旧指示天然出局——扫 backlog 会把切换前的旧指示当回显,把乐观值/transcript 刚落的
    // 正确模式覆写回去;而「循环启动时」当基线又会吞掉写入与启动之间已到达的重绘。
    let since = baseline;
    const startedAt = Date.now();
    while (Date.now() - startedAt < 3_000) {
      let snapshot;
      try { snapshot = await managedTerminalSnapshot(sessionId, since); } catch { return; }
      if (modeEchoSeqRef.current !== seq) return;
      if (!snapshot?.active) return;
      // 偏移回绕(PTY 被就地重启复位):与发送/探测循环同款守卫,从头扫——新进程的
      // 首屏重绘就带着当前模式指示,不会误认旧进程残影。
      if (snapshot.endOffset < since) {
        since = 0;
        tail = "";
        continue;
      }
      since = snapshot.endOffset;
      tail = appendTerminalText(tail, snapshot.data, decoder);
      const value = modeFromScreen(visibleTerminalText(tail), markers);
      if (value) applyModeValue(dimension, value);
      await new Promise((resolve) => window.setTimeout(resolve, 250));
      if (modeEchoSeqRef.current !== seq) return;
    }
  };
  /// 模式动作完全由插件描述：快捷键原样发送，命令则用和人工输入一致的 paste + Enter 序列。
  const changeMode = async (dimension: string, inputs: { data: string; submit: boolean }[], optimisticValue?: string, screenMarkers: ModeScreenMarker[] = []) => {
    if (inputs.length === 0 || sending) return;
    if (toneRef.current === "running") { setSendError(t.chat.menuBusy); return; }
    // 若因外部占用被拒，接管后重放的是这同一个动作。
    retryRef.current = () => changeMode(dimension, inputs, optimisticValue, screenMarkers);
    // 守卫与发送同一套(withSendGuard):交后端权威判定,被拒才给接管入口。
    await withSendGuard(async () => {
      // 回显基线:按键**之前**的输出末尾偏移。在写入前采,回显循环从它之后扫——
      // 见 echoModeFromScreen 的注释。拿不到就从 0 起扫,宁可多扫不可漏扫。
      let echoBase = 0;
      try { echoBase = (await managedTerminalSnapshot(sessionId, 0))?.endOffset ?? 0; } catch { /* noop */ }
      for (const input of inputs) {
        const cancelled = attentionAbort();
        if (cancelled) throw new Error(cancelled);
        // submit 的动作（如 `/plan on`）走同一套提交逻辑，消除「只换行不提交」的竞态；
        // 非 submit 的是原始按键序列（如循环快捷键），原样写、不追回车。
        if (input.submit) await submitToTerminal(sessionId, input.data, attentionAbort);
        else await writeManagedTerminal(sessionId, input.data);
      }
      if (!isCurrentOperation()) return false;

      // 不 await：回显是后台观察，不该把发送按钮按住 3 秒。
      void echoModeFromScreen(dimension, screenMarkers, echoBase);
      return true;
    });
  };
  return { selectModel, selectEffort, effortBusy, effortLevels, cycleToMode, changeMode };
}
