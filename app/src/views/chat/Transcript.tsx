import { memo, useContext, useRef } from "react";
import { type ChatItem } from "../../api";
import { collectSubagentReceipts } from "../../chat/reducer";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";
import type { ToolUseItem } from "./shared";

/** 日期分隔条文案：今天/昨天沿用侧栏日期分组的同一对键，更早按界面语言格式化。 */
function dayLabel(date: Date, t: Dict): string {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const day = new Date(date);
  day.setHours(0, 0, 0, 0);
  const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000);
  if (diffDays === 0) return t.chat.dateToday;
  if (diffDays === 1) return t.chat.dateYesterday;
  return date.toLocaleDateString(t.locale);
}
import { imageOnlyPaths, Message, MessageCopyButton, UserImageGroup } from "./Message";
import { SubagentBlock } from "./SubagentBlock";
import { ToolActivity } from "./ToolActivity";
import { TurnChanges } from "./TurnChanges";
import { ReplyFold, TurnFold } from "./TurnFold";
import { FoldScopeContext, foldScope } from "./foldMemory";
import { isSubagentDelegation, type ToolResultItem } from "./shared";
import { liveTurnStart, SessionAliveContext, TurnLiveContext, type TurnActivity } from "./toolState";

type SubagentOutcomeInfo = NonNullable<ToolResultItem["subagent"]>;

/// 增量缓存的块记录。start = 该块首条 item 的下标;lastDayBefore = 进入该块时的日期
/// 分隔游标(复算从此块起步时要还原它);subagentId = 子任务块对应的 tool_use id——
/// 该块的徽标读的是整个 items 里的回执,复用前必须核对 outcomes/settled 没变;
/// nodes = 该块产出的元素(块本体,可能前置一条日期分隔条,daySep 标记有没有)。
/// reply = 所属回复的起点下标(assistant 段才有;一段回复被正文切成多段时各段同值);
/// fold = 回合级折叠的切点,只挂在「已结束且开着消息自动折叠」那段回复的最后一段上:
/// nodes[0..at) 与同回复前面各段一起收进 ReplyFold。见文件末尾的拼装。
type BlockRec = {
  start: number;
  lastDayBefore: string | null;
  subagentId: string | null;
  /// 该块里还没等到回执的调用。回执可能隔着别的块才到（见 collectSubagentReceipts 的 results），到了就得
  /// 从这块重算，不然它会一直显示成没回执的样子。
  unresolved: string[];
  /// 这块的节点是按哪个活动态生成的（工具行与子任务块的运行态、回合级折叠要不要收，都取决于它）。
  /// null = 这块与活动态无关（用户消息、分隔条）。复用前核对：按现在的活动态与回合起点再算一遍，
  /// 不一样就从这块起重算。
  activity: TurnActivity | null;
  nodes: JSX.Element[];
  daySep: boolean;
  reply: number | null;
  fold: { at: number; id: string; durationMs: number | null } | null;
};

/// 时间与整段复制统一收在回合末尾，对齐 Kimi 的大段回复结构。
export type TranscriptAgent = { provider: string; name: string; model: string | null };

function TurnFooter({ text, timestamp }: { text: string; timestamp: string | null }) {
  const date = timestamp ? new Date(timestamp) : null;
  const time = date && !Number.isNaN(date.getTime())
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })
    : null;
  return (
    <div className="chat-turn-foot">
      {time && <time className="chat-turn-time">{time}</time>}
      <MessageCopyButton text={text} className="chat-turn-copy" />
    </div>
  );
}

/// 该条目之后第一条非 meta 条目的时间戳——思考块用它算「想了多久」。思考只在收尾时留下
/// 一条带戳的记录，自己没有结束时间，就近取下一条的。到底还没有（流式尾条）返回 null，
/// 由组件按秒自己 tick。
function nextStamp(items: ChatItem[], index: number): string | null {
  for (let next = index + 1; next < items.length; next += 1) {
    const candidate = items[next];
    if (candidate.type === "meta") continue;
    return candidate.timestamp;
  }
  return null;
}

type BlockCache = {
  sessionId: number;
  t: Dict;
  foldTurns: boolean;
  items: ChatItem[];
  blocks: BlockRec[];
  outcomes: Map<string, SubagentOutcomeInfo>;
  settledAt: Map<string, number>;
};

/// items 引用不变就不重算：稳态下历史轮询每轮都让父组件重渲染，
/// 但 items 往往原样不动（reduceChatEvents 无新消息时返回同一引用）。没有这层
/// memo 时，每一轮都要重跑下面的分组循环、重建全部 JSX——长会话上千条时很贵。
///
/// 流式期间 items 每批(200ms 的 pty-output 合流)都换新引用,memo 帮不上——此前每批
/// 都从头全量重建整棵块数组,长会话是 O(n²)。现在按块做增量缓存:reducer 的写时复制
/// 保证未变条目同引用,据此找到首个变化下标,之前的块原样复用,只重算尾部。
export const Transcript = memo(function Transcript({ sessionId, items, activity = "idle", alive, foldTurns = false }: {
  sessionId: number;
  items: ChatItem[];
  agent?: TranscriptAgent | null;
  /** 当前回合此刻的活动态（对话窗按会话状态给，见 turnActivityOf）。缺省 = 已不在进行。 */
  activity?: TurnActivity;
  /** 会话进程是否还活着（后台子任务的「进行中」只在活着时成立）。缺省沿用外层。 */
  alive?: boolean;
  /** 设置「消息自动折叠」：已结束的回复只露最终回答，过程收成一行。 */
  foldTurns?: boolean;
}) {
  const t = useT();
  const outerAlive = useContext(SessionAliveContext);
  const sessionAlive = alive ?? outerAlive;
  const working = activity !== "idle";
  // 委派的结局写在**主链的工具回执**上，而回执往往排在委派之后若干条（TaskOutput 拉取
  // 的结局甚至挂在别的调用上，靠 task_id 归回原委派）。关联规则收敛在
  // collectSubagentReceipts——与标题栏进度面板同一份。没有回执 = 派出去了还没回来 = 在跑。
  // 每轮全量重扫:O(n) 的纯 Map 构建很便宜,贵的是按块生成 JSX——那部分走下面的缓存。
  // results：调用与回执按整份时间线配对，与委派结局同一趟扫描建出来。
  const { outcomes, settledAt, finishedTs, results } = collectSubagentReceipts(items);
  const cacheRef = useRef<BlockCache | null>(null);
  const cache = cacheRef.current;
  let reuse: BlockRec[] = [];
  // 当前回合的起点：最后一条开启回合的用户消息，或最后一条中断分隔（见 liveTurnStart）。
  // 回合中途送进去的话不算：agent 读到后接着干同一回合。以前把它当回合边界，运行中的回复一遇插话就被当成
  // 「已结束」折起来，十几分钟的进度只剩「工作了 16m21s」一行（2026-11-12 用户实拍：这个会话的消息都没在
  // 对话里反馈出来）。
  const lastTurnStart = liveTurnStart(items);
  // 一段回复此刻的活动态：会话在进行，且这一段在当前回合起点之后。更早回合里没回执的调用不会再有结果了，
  // 一律按已不在进行算。
  const activityAt = (start: number): TurnActivity => working && start > lastTurnStart ? activity : "idle";
  // 折叠开关一变,旧块里记的切点全部作废,整体重算(开关很少动,不值得做细)。
  //
  // 活动态一变**不**整体重算，只重算受它影响的块：当前回合起点之后的那几段（见下面对 block.activity 的核对）。
  // 之前回合的块按「已不在进行」生成，活动态怎么翻它们都不变。此前这里把活动态放进了整体失效的条件，
  // 一份 2432 条的真实会话每翻一次重渲染 296 行（main 是 1 行），而等审批、放行、回合起止都会翻
  // （做空指出，数字见任务目录 progress-fix-display.md）。会话进程是否活着（alive）走 context，不进节点，
  // 不需要为它重算。
  if (cache && cache.sessionId === sessionId && cache.t === t && cache.foldTurns === foldTurns && cache.blocks.length > 0) {
    const common = Math.min(cache.items.length, items.length);
    let firstChanged = 0;
    while (firstChanged < common && cache.items[firstChanged] === items[firstChanged]) firstChanged += 1;
    // 最后一个缓存块永不复用:纯追加时它可能被新条目延长(工具组/连续图片组会吸收
    // 紧随其后的同类条目),必须从它的起点重算。
    let keep = 0;
    while (keep < cache.blocks.length - 1) {
      const block = cache.blocks[keep];
      const end = cache.blocks[keep + 1].start;
      if (end > firstChanged) break;
      const id = block.subagentId;
      // 尾部新到的回执会改写早先子任务块的结局徽标;outcomes 的值来自未变条目时是
      // 同一个对象引用,直接比引用即可。
      if (id !== null && (outcomes.get(id) !== cache.outcomes.get(id) || settledAt.has(id) !== cache.settledAt.has(id))) break;
      // 早先块里没回执的调用，这一批等到了回执：从它起重算。
      if (block.unresolved.some((callId) => results.has(callId))) break;
      // 这块是按另一个活动态生成的（活动态翻了，或回合起点挪了）：从它起重算。能走到这里复用的都不是
      // 末尾那块，所以只看它在不在当前回合里。
      if (block.activity !== null && block.activity !== activityAt(block.start)) break;
      keep += 1;
    }
    reuse = cache.blocks.slice(0, keep);
  }
  const blocks: BlockRec[] = [...reuse];
  const resume = reuse.length > 0 ? cache!.blocks[reuse.length] : null;
  // 日期分隔条：跨天处插一条（含首个带时间的块，IM 惯例——每天的开头都有标签）。
  // timestamp 为 null 的条目（乐观回显/部分 provider 不带时间）不参与也不重置游标，
  // 混在带时间的消息中间不会催生重复的分隔条。
  let lastDay: string | null = resume ? resume.lastDayBefore : null;
  const pushDaySep = (record: BlockRec, item: ChatItem) => {
    if (!item.timestamp) return;
    const date = new Date(item.timestamp);
    if (Number.isNaN(date.getTime())) return;
    const key = date.toDateString();
    if (key === lastDay) return;
    lastDay = key;
    record.daySep = true;
    record.nodes.push(
      <div className="chat-day-sep" role="separator" key={`day-${item.id}`}>
        <span>{dayLabel(date, t)}</span>
      </div>,
    );
  };
  for (let index = resume ? resume.start : 0; index < items.length;) {
    const item = items[index];
    const previous = blocks[blocks.length - 1];
    const record: BlockRec = { start: index, lastDayBefore: lastDay, subagentId: null, unresolved: [], activity: null, nodes: [], daySep: false, reply: null, fold: null };
    blocks.push(record);
    pushDaySep(record, item);
    // 连续的纯图片用户消息合成一行：Claude Code 把一次多图粘贴记成连续多条独立消息，
    // 逐条渲染是竖着摞一列大图；用户视角那是「一次发的几张图」，该排在一起。
    if (item.type === "user_text") {
      const paths = imageOnlyPaths(item.text);
      if (paths) {
        const merged = [...paths];
        let next = index + 1;
        while (next < items.length) {
          const candidate = items[next];
          if (candidate.type !== "user_text") break;
          const more = imageOnlyPaths(candidate.text);
          if (!more) break;
          merged.push(...more);
          next += 1;
        }
        record.nodes.push(<UserImageGroup key={item.id} paths={merged} anchorId={item.id} />);
        index = next;
        continue;
      }
      record.nodes.push(<Message key={item.id} item={item} />);
      index += 1;
      continue;
    }
    if (item.type === "meta") {
      // 压缩摘要挂到紧邻在前的分隔条上（「查看摘要」），不单独成行；前面不是分隔条的孤立
      // 摘要（读窗口恰好从它开始）直接略过——没有分隔条它就没有落脚处。
      if (item.kind === "compact_summary") {
        index += 1;
        continue;
      }
      const following = items[index + 1];
      const summary = item.kind === "compact" && following?.type === "meta" && following.kind === "compact_summary"
        ? following.compaction?.summary ?? null
        : null;
      record.nodes.push(<Message key={item.id} item={item} compactSummary={summary} />);
      index += summary !== null ? 2 : 1;
      continue;
    }
    // assistant 回合：从当前条目一直收集到下一个 user/meta 或结尾。思考、工具、正文
    // 同属一个回合；不再额外显示 provider/model 回合头。
    const turnItems: ChatItem[] = [];
    let next = index;
    while (next < items.length && items[next].type !== "user_text" && items[next].type !== "meta") {
      const entry = items[next];
      // 一个回合只装一段 assistant 正文：遇到下一段正文就结束当前回合。
      // 思考/工具围绕这段正文归属同一回合，已完成回合才能折叠过程。
      if ((entry.type === "assistant_text" || entry.type === "assistant_delta")
        && turnItems.some((existing) => existing.type === "assistant_text" || existing.type === "assistant_delta")) {
        break;
      }
      turnItems.push(entry);
      next += 1;
    }
    const processItems = turnItems.filter((entry) =>
      entry.type === "reasoning" || entry.type === "reasoning_delta" || entry.type === "tool_use" || entry.type === "tool_result",
    );
    const bodyItems = turnItems.filter((entry) =>
      entry.type === "assistant_text" || entry.type === "assistant_delta" || entry.type === "turn_error",
    );
    const isLive = next >= items.length;
    // 这一段此刻的活动态：会话在进行，且这一段属于当前回合（在末尾，或在当前回合起点之后）。
    const blockActivity: TurnActivity = working && isLive ? activity : activityAt(index);
    record.activity = blockActivity;
    // 所属回复:紧挨着的前一条不是用户消息/meta,说明还在同一段回复里,沿用上一段的起点。
    const before = items[index - 1];
    record.reply = before && before.type !== "user_text" && before.type !== "meta" && previous?.reply != null
      ? previous.reply
      : index;
    const firstStamp = Date.parse(turnItems[0]?.timestamp ?? "");
    const lastStamp = Date.parse(turnItems[turnItems.length - 1]?.timestamp ?? "");
    const durationMs = Number.isFinite(firstStamp) && Number.isFinite(lastStamp) ? Math.max(0, lastStamp - firstStamp) : null;

    const renderProcess = () => {
      const nodes: JSX.Element[] = [];
      let cursor = 0;
      while (cursor < processItems.length) {
        const entry = processItems[cursor];
        if (entry.type === "reasoning" || entry.type === "reasoning_delta") {
          const globalIndex = items.indexOf(entry);
          // 「思考中」只在真的还在想时出现：逐块流进来的思考（reasoning_delta），排在整份时间线
          // 的最后，且会话在跑。后面一旦有了新内容，或回合停了，它就是想完的那条。
          const live = entry.type === "reasoning_delta" && activity === "running" && globalIndex === items.length - 1;
          nodes.push(<Message key={entry.id} item={entry} nextStamp={nextStamp(items, globalIndex)} live={live} />);
          cursor += 1;
          continue;
        }
        if (entry.type === "tool_use" && isSubagentDelegation(entry, outcomes)) {
          nodes.push(<SubagentBlock
            key={entry.id}
            sessionId={sessionId}
            item={entry}
            outcome={outcomes.get(entry.id)}
            settled={settledAt.has(entry.id)}
            finishedAt={finishedTs.get(entry.id) ?? null}
          />);
          cursor += 1;
          continue;
        }
        // 连续工具调用：逐行渲染，回执挂在各自的调用行上（配对见 collectSubagentReceipts 的 results）。
        // 回执本身不单独成行：它要么已经挂在调用上，要么是找不到调用的孤儿（委派的晚到结局
        // 归子任务块管；调用落在已加载窗口之外的，等「加载更早」把调用读进来再配上）。
        // 此前孤儿回执按旧样式画成一行「工具结果」，正文为空时是一个空行（走查 A10、C6）。
        const tools: ToolUseItem[] = [];
        const groupStart = cursor;
        while (cursor < processItems.length) {
          const candidate = processItems[cursor];
          if (candidate.type === "tool_use" && isSubagentDelegation(candidate, outcomes)) break;
          if (candidate.type !== "tool_use" && candidate.type !== "tool_result") break;
          if (candidate.type === "tool_use") tools.push(candidate);
          cursor += 1;
        }
        if (tools.length === 0) continue;
        nodes.push(
          <div className="chat-flat-steps" key={`tools-${processItems[groupStart].id}`}>
            {tools.map((tool) => <ToolActivity key={tool.id} item={tool} result={results.get(tool.id)} />)}
          </div>,
        );
      }
      // 这一段的活动态经 context 给到每条工具行与子任务块。
      return nodes.map((node) => (
        <TurnLiveContext.Provider key={node.key} value={blockActivity}>{node}</TurnLiveContext.Provider>
      ));
    };
    for (const entry of processItems) {
      if (entry.type === "tool_use" && !results.has(entry.id)) record.unresolved.push(entry.id);
    }

    // 只有一条思考/工具时原样平铺，≥2 条才收成汇总块（Kimi 的 ActivityRun 分组规则）：
    // 单条工具外面再套一层「已运行 1 条命令」是重复信息。工具与它的回执算一条。
    const units = processItems.filter((entry) => entry.type !== "tool_result").length;
    // 这一段过程还在进行 = 会话在进行、这一段在末尾、过程之后还没有正文（正文一到，这段过程就算结束）。
    // 看的是「末条是不是正文」，不是「这一段有没有正文」：先说要做什么再调用工具是最常见的顺序，那段正文
    // 排在工具之前。只看有没有正文时，命令还在跑、还在等批准，过程块却收起并写着「1 次已中断」
    // （2026-11-14 截图自验，kimi 与 opencode 的样本都是这个顺序）。
    const tail = turnItems[turnItems.length - 1];
    const processOpen = bodyItems.length === 0 || (tail.type !== "assistant_text" && tail.type !== "assistant_delta" && tail.type !== "turn_error");
    const foldActivity: TurnActivity = isLive && processOpen ? blockActivity : "idle";
    // 一条调用都没有、只剩回执的段（调用落在已加载窗口之外）什么都不画：不单列这一档的话，
    // 它会被套成一个空的过程块，摘要写着「思考过程」。为它插的日期分隔条也撤掉，游标还原，
    // 留给下一段去插。
    if (units === 0 && bodyItems.length === 0) {
      if (record.daySep) lastDay = record.lastDayBefore;
      record.nodes = [];
      record.daySep = false;
      index = next;
      continue;
    }
    if (units === 1) {
      record.nodes.push(...renderProcess());
    } else if (units > 1) {
      record.nodes.push(
        <TurnFold
          key={`fold-${turnItems[0].id}`}
          id={turnItems[0].id}
          items={processItems}
          results={results}
          durationMs={durationMs}
          activity={foldActivity}
        >
          {renderProcess()}
        </TurnFold>,
      );
    }
    // 正文之前的节点都是过程:回合级折叠从这里切开,之后的正文/改动/页脚留在外面。
    const bodyAt = record.nodes.length;
    for (const entry of bodyItems) {
      // 出错卡只有在它就是时间线末尾时才给「继续」（后面已有新内容的旧错误不必再续）。
      const latest = entry.type === "turn_error" && next >= items.length && entry === bodyItems[bodyItems.length - 1];
      record.nodes.push(<Message key={entry.id} item={entry} latest={latest} />);
    }
    // 页脚只挂在整段回复的末尾（下一条是用户消息/meta 或到底）：一段回复被工具调用切成
    // 多节时，Kimi 也只在最后给一组时间与复制，逐节挂会把一屏刷成一串时间戳。
    // 复制取整段回复的全部正文、改动卡片汇总整段回复的编辑——往回扫到这段回复的起点，
    // 增量缓存从中间重算时也拿得全。
    const replyEnds = next >= items.length || items[next].type === "user_text" || items[next].type === "meta";
    if (replyEnds) {
      let start = index;
      while (start > 0 && items[start - 1].type !== "user_text" && items[start - 1].type !== "meta") start -= 1;
      const reply = items.slice(start, next);
      record.nodes.push(<TurnChanges key={`changes-${turnItems[0].id}`} items={reply} />);
      const copyText = reply
        .filter((entry) => entry.type === "assistant_text" || entry.type === "assistant_delta")
        .map((entry) => entry.text)
        .filter((text) => text.trim())
        .join("\n\n");
      if (copyText) {
        record.nodes.push(<TurnFooter key={`foot-${turnItems[0].id}`} text={copyText} timestamp={turnItems[turnItems.length - 1]?.timestamp ?? null} />);
      }
      // 回合级折叠(对照 Kimi):回复已结束(运行中不折,过程要看得见)、最后一段有正文、
      // 正文之前确实有东西可收(前面还有别的段,或本段有思考/工具)。
      // 运行中不折：这段回复在末尾，或者它后面只隔着插话、仍属于正在进行的回合。
      const settled = blockActivity === "idle";
      const finalText = bodyItems.some((entry) => (entry.type === "assistant_text" || entry.type === "assistant_delta") && entry.text.trim());
      const replyStart = record.reply ?? index;
      const hasEarlier = replyStart < index;
      if (foldTurns && settled && finalText && (hasEarlier || bodyAt > (record.daySep ? 1 : 0))) {
        // 时长从用户发出那句算起(没有就从回复第一条),到这段回复最后一条。
        const asked = items[replyStart - 1];
        const fromStamp = Date.parse((asked?.type === "user_text" ? asked.timestamp : null) ?? items[replyStart].timestamp ?? "");
        const toStamp = Date.parse(items[next - 1]?.timestamp ?? "");
        const spanMs = Number.isFinite(fromStamp) && Number.isFinite(toStamp) ? Math.max(0, toStamp - fromStamp) : null;
        record.fold = { at: bodyAt, id: items[replyStart].id, durationMs: spanMs };
      }
    }
    index = next;
  }
  cacheRef.current = { sessionId, t, foldTurns, items, blocks, outcomes, settledAt };
  // 拼装:同一段回复的各块连在一起;最后一块带 fold 的,把它切点之前和前面各块的节点收进
  // ReplyFold。日期分隔条提到折叠外面:它标的是日期,收进去就看不出跨了天。
  const nodes: JSX.Element[] = [];
  for (let at = 0; at < blocks.length;) {
    const block = blocks[at];
    let end = at + 1;
    if (block.reply !== null) {
      while (end < blocks.length && blocks[end].reply === block.reply) end += 1;
    }
    const last = blocks[end - 1];
    if (!last.fold) {
      for (let k = at; k < end; k += 1) nodes.push(...blocks[k].nodes);
      at = end;
      continue;
    }
    const seps: JSX.Element[] = [];
    const folded: JSX.Element[] = [];
    for (let k = at; k < end; k += 1) {
      const own = k === end - 1 ? blocks[k].nodes.slice(0, last.fold.at) : blocks[k].nodes;
      if (blocks[k].daySep) {
        seps.push(own[0]);
        folded.push(...own.slice(1));
      } else {
        folded.push(...own);
      }
    }
    nodes.push(...seps);
    if (folded.length > 0) {
      nodes.push(<ReplyFold key={`reply-${last.fold.id}`} id={last.fold.id} durationMs={last.fold.durationMs}>{folded}</ReplyFold>);
    }
    nodes.push(...last.nodes.slice(last.fold.at));
    at = end;
  }
  // 折叠块的开合按会话各记一份（最近 4 个会话），切走再回来原样还原，见 foldMemory。
  return (
    <SessionAliveContext.Provider value={sessionAlive}>
      <FoldScopeContext.Provider value={foldScope(sessionId)}>{nodes}</FoldScopeContext.Provider>
    </SessionAliveContext.Provider>
  );
});
