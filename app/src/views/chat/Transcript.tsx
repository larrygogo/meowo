import { memo, useRef } from "react";
import { type ChatItem } from "../../api";
import { collectSubagentReceipts } from "../../chat/reducer";
import { useT } from "../../i18n";
import type { Dict } from "../../i18n/zh";

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
import { TurnFold } from "./TurnFold";
import { FoldScopeContext, foldScope } from "./foldMemory";
import { isSubagentDelegation, type ToolResultItem, type ToolUseItem } from "./shared";

type SubagentOutcomeInfo = NonNullable<ToolResultItem["subagent"]>;

/// 增量缓存的块记录。start = 该块首条 item 的下标;lastDayBefore = 进入该块时的日期
/// 分隔游标(复算从此块起步时要还原它);subagentId = 子任务块对应的 tool_use id——
/// 该块的徽标读的是整个 items 里的回执,复用前必须核对 outcomes/settled 没变;
/// nodes = 该块产出的元素(块本体,可能前置一条日期分隔条)。
type BlockRec = {
  start: number;
  lastDayBefore: string | null;
  subagentId: string | null;
  nodes: JSX.Element[];
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
export const Transcript = memo(function Transcript({ sessionId, items, working = false }: { sessionId: number; items: ChatItem[]; agent?: TranscriptAgent | null; working?: boolean }) {
  const t = useT();
  // 委派的结局写在**主链的工具回执**上，而回执往往排在委派之后若干条（TaskOutput 拉取
  // 的结局甚至挂在别的调用上，靠 task_id 归回原委派）。关联规则收敛在
  // collectSubagentReceipts——与标题栏进度面板同一份。没有回执 = 派出去了还没回来 = 在跑。
  // 每轮全量重扫:O(n) 的纯 Map 构建很便宜,贵的是按块生成 JSX——那部分走下面的缓存。
  const { outcomes, settledAt, finishedTs } = collectSubagentReceipts(items);
  const cacheRef = useRef<BlockCache | null>(null);
  const cache = cacheRef.current;
  let reuse: BlockRec[] = [];
  if (cache && cache.sessionId === sessionId && cache.t === t && cache.blocks.length > 0) {
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
    record.nodes.push(
      <div className="chat-day-sep" role="separator" key={`day-${item.id}`}>
        <span>{dayLabel(date, t)}</span>
      </div>,
    );
  };
  for (let index = resume ? resume.start : 0; index < items.length;) {
    const item = items[index];
    const record: BlockRec = { start: index, lastDayBefore: lastDay, subagentId: null, nodes: [] };
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
          nodes.push(<Message key={entry.id} item={entry} nextStamp={nextStamp(items, globalIndex)} />);
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
        // 连续工具调用/回执：保持原有配对，逐行渲染。
        const tools: Array<ToolUseItem | ToolResultItem> = [];
        while (cursor < processItems.length) {
          const candidate = processItems[cursor];
          if (candidate.type === "tool_use" && isSubagentDelegation(candidate, outcomes)) break;
          if (candidate.type !== "tool_use" && candidate.type !== "tool_result") break;
          tools.push(candidate);
          cursor += 1;
        }
        if (tools.length === 0) continue;
        const results = new Map<string, ToolResultItem>();
        for (const tool of tools) {
          if (tool.type === "tool_result" && tool.tool_use_id) results.set(tool.tool_use_id, tool);
        }
        const consumed = new Set<string>();
        for (const tool of tools) {
          if (tool.type !== "tool_use") continue;
          const result = results.get(tool.id);
          if (result) consumed.add(result.id);
        }
        nodes.push(
          <div className="chat-flat-steps" key={`tools-${tools[0].id}`}>
            {tools.map((tool) => {
              if (tool.type === "tool_use") {
                return <ToolActivity key={tool.id} item={tool} result={results.get(tool.id)} />;
              }
              return consumed.has(tool.id) ? null : <Message key={tool.id} item={tool} />;
            })}
          </div>,
        );
      }
      return nodes;
    };

    // 只有一条思考/工具时原样平铺，≥2 条才收成汇总块（Kimi 的 ActivityRun 分组规则）：
    // 单条工具外面再套一层「已运行 1 条命令」是重复信息。工具与它的回执算一条。
    const units = processItems.filter((entry) => entry.type !== "tool_result").length;
    // 运行中 = 会话在工作、这一段在末尾、后面还没有正文（正文一到，这段过程就算结束）。
    const running = working && isLive && bodyItems.length === 0;
    if (units === 1) {
      record.nodes.push(...renderProcess());
    } else if (processItems.length > 0) {
      record.nodes.push(
        <TurnFold
          key={`fold-${turnItems[0].id}`}
          id={turnItems[0].id}
          items={processItems}
          durationMs={durationMs}
          running={running}
        >
          {renderProcess()}
        </TurnFold>,
      );
    }
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
    }
    index = next;
  }
  cacheRef.current = { sessionId, t, items, blocks, outcomes, settledAt };
  // 折叠块的开合按会话各记一份（最近 4 个会话），切走再回来原样还原，见 foldMemory。
  return <FoldScopeContext.Provider value={foldScope(sessionId)}>{blocks.flatMap((block) => block.nodes)}</FoldScopeContext.Provider>;
});
