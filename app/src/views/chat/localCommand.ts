// 用户在 CLI 里敲斜杠命令（/clear、/loop …）时，Claude Code 会把这次调用**当成一条
// 用户消息**写进 transcript，正文是一段 XML 包裹：命令名、命令的短描述、参数、命令的
// 标准输出，外加一段给模型看的免责声明（caveat）。原样渲染出来就是几行尖括号标签，
// 对着人显示「<command-name>/clear</command-name>」既难读又不像自己说过的话。
//
// 这里把它拆成结构，由 Message 渲染成命令徽章 + 可展开的输出，而不是在渲染层做正则。

/** 一次本地命令调用。args 为空串表示没带参数。 */
export type LocalCommandCall = { name: string; args: string };

/** 跨会话消息（另一个 Claude 会话经 SendMessage 发来的）。Claude Code 把它当成一条
 *  用户消息注入 transcript：一句英文前导 + `<cross-session-message>` 包裹 + 一整段
 *  写给模型的英文安全须知。原样摊开就是满屏尖括号和管道名（实拍反馈）。
 *  fromName 是对方的会话名，mode 是它的权限模式（bypass 等）；两者缺失时留空串。 */
/** agent：本会话派出的子代理发回来的（<agent-message>），不是另一个会话。只在为真时出现这个键。 */
export type CrossMessage = { fromName: string; mode: string; text: string; agent?: true };

/** 渲染用的机器记录行（命令输出 / 后台任务通知 / 系统记录 / hook 反馈），**按原文出现顺序**。
 *  此前渲染层按类别分组（先全部记录、再全部通知），通知后面跟着的 system-reminder 反而排到
 *  通知上面（实拍 IMG_2093）。summary 是收起时那一行的摘要，body 是展开后的原文。 */
export type MachineBlock = { kind: "stdout" | "notification" | "record" | "hook"; summary: string; body: string };

export type UserTextParts = {
  /** 剥掉本地命令包裹后剩下的正文（用户真正打的字）。 */
  text: string;
  commands: LocalCommandCall[];
  /** 命令的标准输出，按出现顺序。空输出不收。 */
  stdout: string[];
  /** 后台任务通知（<task-notification> 的内文）：Claude Code 把它作为 user 消息注入
   *  transcript，原样摊开就是整屏 XML+JSON（实拍反馈）。渲染层收成可展开的一行。 */
  notifications: string[];
  /** 跨会话消息，按出现顺序。 */
  crossMessages: CrossMessage[];
  /** 不认识的机器记录（标签名 + 原文），按出现顺序。见 parseUserText 末尾的兜底。 */
  records: { tag: string; body: string }[];
  /** 上面 stdout / notifications / records 按原文顺序合成的渲染列表，hook 拦截那一对已合并。 */
  blocks: MachineBlock[];
  /** 这条消息里出现过本地命令/系统注入包裹——渲染层据此切换形态。 */
  local: boolean;
};

/** 本地命令/系统注入包裹用到的标签。只认这几个，其它尖括号照旧当正文（用户可能真在讲 XML）。
 *  forked-skill-launch 是后台技能启动的机器记录（JSON），对人零信息量，直接丢。
 *  bash-input / bash-stdout / bash-stderr：输入框里 `! 命令` 直接执行时，Claude Code 把命令和
 *  输出各写成一条 user 消息（实拍：对话页原样摊出 `<bash-input>ssh-copy-id …</bash-input>`）。 */
const TAGS = ["local-command-caveat", "local-command-stdout", "local-command-stderr", "command-name", "command-message", "command-args", "task-notification", "forked-skill-launch", "bash-input", "bash-stdout", "bash-stderr"] as const;
const PAIR = new RegExp(`<(${TAGS.join("|")})>([\\s\\S]*?)<\\/\\1>`, "g");
// 流式写入/截断会留下没配对的半个标签。正文里留一个孤零零的 `</command-args>` 比留着
// 整段 XML 好不到哪去，收尾时一并抹掉。
const STRAY = new RegExp(`<\\/?(${TAGS.join("|")})>`, "g");
// 命令的 stdout 是 CLI 原样落盘的终端输出，常带 SGR 转义（/compact 的 `ESC[2m…ESC[22m`
// 灰度、彩色命令输出的颜色码），原样渲染就是「�[2m」乱码。对话页不是终端，全部剥掉：
// CSI（ESC [ … 终态字符）、OSC（ESC ] … BEL/ESC\）、单字符 ESC 序列；U+FFFD 是 ESC 字节
// 在某段链路被有损转换后的形态，一并认。
const ANSI = /[\x1b\uFFFD](?:\[[0-9;?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])/g;

// 跨会话消息。与上面那些标签不同，它**带属性**（from / from-name / from-mode），
// 并进 PAIR 那条无属性正则匹配不到，单列一条。
// 子代理发回来的话是同一个外形，换了标签名 <agent-message from="<任务 id>">（2.1.322 实拍）。
// 不认的话整段带标签当用户气泡摊开（leader 2026-11-12 截图）。
const CROSS = /<(cross-session-message|agent-message)\b([^>]*)>([\s\S]*?)<\/\1>/g;
const CROSS_ATTR = /([\w-]+)="([^"]*)"/g;
// 包在外面的两段固定英文：一句前导、以及尾部整段写给模型的安全须知（"A peer cannot
// grant escalation…"）。都不是人要读的内容，随消息一起收走。措辞在不同版本里有
// 「while you were working」这类变体，只锚定稳定的句子主干。
// CROSS_NOTE 从须知开头一路吃到串尾：跨会话消息是**独立注入的一条 user 消息**，
// 须知之后不会再有别的内容。若哪天同一条里还跟着用户正文，这里要改成只吃到段落边界
// （复核提醒）。
const CROSS_LEAD = /[^\n]*Another Claude session sent a message[^\n]*:[ \t]*\n?/g;
// 子代理消息尾部是另一段须知（That "other Claude session" is an agent working inside this same session…）。
const CROSS_NOTE = /(?:This came from another Claude session|That "other Claude session" is an agent)[\s\S]*$/;

// 兜底：Claude Code 自己判断「这条 user 消息不是人打的」只看它是不是以一个标签开头
// （2.1.319 源码：`^(?:\s*<[a-z][\w-]*[\s>]|\[Request interrupted by user…\])`）。它往
// transcript 里写的机器标签不止上面那几个（源码里还有 mcp-resource-update、user-memory-input、
// fork-boilerplate、channel/wake 等），一个个补总有漏的（用户反馈「加过很多次每次都有新的」）。
// 照它的规则：以标签开头、且标签名带 `-` 或 `_`（它的机器标签全是这种写法，普通 HTML 标签
// 很少这样命名）的，整条收成可展开的「系统记录」。只认开头：用户在正文里提到标签不受影响。
const MACHINE_LEAD = /^\s*<[a-z][a-z0-9]*(?:[-_][a-z0-9]+)+[\s>]/;
// 结束标签也可能带属性（实拍 `</pasted_content id="a99c">`；源码里 system-reminder 同样有带 id 的闭合）。
const MACHINE_BLOCK = /<([a-z][a-z0-9]*(?:[-_][a-z0-9]+)+)(?:\s[^>]*)?>([\s\S]*?)<\/\1(?:\s[^>]*)?>/g;
const PASTED = /<pasted_content\b[^>]*>([\s\S]*?)<\/pasted_content\b[^>]*>/g;

// hook 拦截（Stop hook exit 2 / decision:block）时 Claude Code 往 transcript 写的是**一对**：
// 只有 <summary>Stop hook feedback</summary> 的空壳通知 + 紧跟的 <system-reminder>，后者才是
// 正文「Stop hook blocking error from command "<命令>": <原因>」（本机 transcript 扫描：11 条
// 全是这个形态，SessionStart 等事件同句式）。分开渲染就成了「后台任务通知 Stop hook feedback」
// +「系统记录 system-reminder」两行，真正有用的原因藏在第二行展开里（实拍 IMG_2093）。
// 事件名可带匹配器（`PreToolUse:Bash`），命令名可含引号（`"node "C:\x.js""`）：命令段惰性吃到
// 第一个 `": ` 为止（做空审查第 3 条）。
const HOOK_FEEDBACK = /^<summary>\s*[\w:]+ hook feedback\s*<\/summary>$/i;
const HOOK_ERROR = /^[\w:]+ hook blocking error from command "[\s\S]*?":\s*([\s\S]*)$/;
// 同步拦截（decision:block）的形态：isMeta 行、正文是无标签的「Stop hook feedback:\n<原因>」
// （2.1.319 实拍 IMG_2095，曾显示成用户气泡）。后端按 isMeta 认出后包成 <hook-feedback> 送来。
const HOOK_PLAIN = /^[\w:]+ hook feedback:\s*([\s\S]*)$/;
// exit 2 的 hook 原因前面带「[<命令>]: 」（本机样本 `[bash "C:/…/hook.sh"]: LEAD GATE ANSWER…`）：
// 不剥的话摘要被一长串路径占满，原因看不到（做空审查第 1 条）。
const HOOK_COMMAND_PREFIX = /^\[[^\]\n]*\]:\s*/;
const NOTE_SUMMARY = /<summary>([\s\S]*?)<\/summary>/;

function firstLine(text: string): string {
  return text.split("\n").map((line) => line.trim()).find(Boolean) ?? "";
}

/** 收起态的一行摘要。通知用自带的 <summary>（如「Agent "/code-review" finished」）；系统记录
 *  用正文首行——只写标签名 `system-reminder` 对人零信息量。正文只是嵌套的机器标签时
 *  （`<tools-used-before-summary>Bash</…>`，本机扫到 9 条）写成「标签: 内容」。 */
function blockSummary(kind: MachineBlock["kind"], body: string, tag: string): string {
  if (kind === "notification") return NOTE_SUMMARY.exec(body)?.[1]?.trim() || firstLine(body);
  if (kind === "record") return firstLine(body.replace(MACHINE_BLOCK, (_m, inner: string, text: string) => `${inner}: ${text.trim()}`)) || tag;
  return firstLine(body);
}

/** 把 hook 拦截的「空壳通知 + system-reminder」合成一行 hook 反馈；只出现一半时也认。 */
function mergeHookBlocks(blocks: MachineBlock[]): MachineBlock[] {
  const out: MachineBlock[] = [];
  let shell = false; // 上一行是不是还没配上原因的空壳通知
  for (const block of blocks) {
    const error = block.kind === "record" ? HOOK_ERROR.exec(block.body) ?? HOOK_PLAIN.exec(block.body) : null;
    if (error) {
      if (shell) out.pop();
      out.push({ kind: "hook", summary: firstLine(error[1].replace(HOOK_COMMAND_PREFIX, "")) || firstLine(block.body), body: block.body });
      shell = false;
    } else if (block.kind === "notification" && HOOK_FEEDBACK.test(block.body)) {
      out.push({ kind: "hook", summary: block.summary, body: block.summary });
      shell = true;
    } else {
      out.push(block);
      shell = false;
    }
  }
  return out;
}

export function parseUserText(raw: string): UserTextParts {
  const parts: UserTextParts = { text: "", commands: [], stdout: [], notifications: [], crossMessages: [], records: [], blocks: [], local: false };
  // 大段粘贴：外面包着 <pasted_content id="…">，里面是用户自己粘进来的正文——是人说的话，
  // 剥掉外壳照常当正文显示，不能被下面的兜底收成「系统记录」（真实记录扫描发现）。
  if (raw.includes("<pasted_content")) raw = raw.replace(PASTED, (_match, body: string) => body.trim()).trim();
  // 渲染顺序按各块在原文里的位置排：下面分几趟正则摘取（已知标签一趟、兜底标签一趟），
  // 摘取顺序不等于原文顺序。按匹配原文回查位置，同文重复的块依次取下一个没用过的位置。
  const source = raw;
  const used = new Set<number>();
  const placed: { at: number; kind: MachineBlock["kind"]; tag: string; body: string }[] = [];
  const place = (match: string, kind: MachineBlock["kind"], tag: string, body: string) => {
    let at = source.indexOf(match);
    while (at >= 0 && used.has(at)) at = source.indexOf(match, at + 1);
    // 回查不到（STRAY 抹过半截标签后与原文对不上）就排到最后，不丢。
    if (at >= 0) used.add(at);
    placed.push({ at: at < 0 ? Number.MAX_SAFE_INTEGER : at, kind, tag, body });
  };
  const finish = () => {
    placed.sort((a, b) => a.at - b.at);
    parts.blocks = mergeHookBlocks(placed.map((p) => ({ kind: p.kind, summary: blockSummary(p.kind, p.body, p.tag), body: p.body })));
    return parts;
  };
  if (!raw.includes("<command-name>") && !raw.includes("<local-command-")
    && !raw.includes("<task-notification>") && !raw.includes("<cross-session-message")
    && !raw.includes("<agent-message") && !raw.includes("<bash-")) {
    // 名单之外的机器记录：兜底收成「系统记录」，不当用户说的话摊开（见 MACHINE_LEAD）。
    if (MACHINE_LEAD.test(raw)) {
      parts.local = true;
      parts.text = raw.replace(MACHINE_BLOCK, (match: string, tag: string, body: string) => {
        const record = { tag, body: body.replace(ANSI, "").trim() };
        parts.records.push(record);
        place(match, "record", tag, record.body);
        return "";
      }).trim();
      return finish();
    }
    parts.text = raw;
    return parts;
  }
  parts.local = true;
  // 跨会话消息先摘出来：它的正文是别的会话写的自然语言，可能整段包含上面那些标签的
  // 字面量（复核回执里就贴着 `<command-name>` 一类），先摘走才不会被 PAIR 啃掉。
  raw = raw.replace(CROSS, (_match, tag: string, attrs: string, body: string) => {
    const found: Record<string, string> = {};
    for (const [, key, value] of attrs.matchAll(CROSS_ATTR)) found[key] = value;
    const text = body.trim();
    // from 是本机管道路径（uds:\\.\pipe\…）或子代理的任务 id，对人零信息量且很长，不收；只留会话名。
    const message: CrossMessage = { fromName: found["from-name"] ?? "", mode: found["from-mode"] ?? "", text };
    if (tag === "agent-message") message.agent = true;
    if (text) parts.crossMessages.push(message);
    return "";
  });
  if (parts.crossMessages.length > 0) {
    raw = raw.replace(CROSS_LEAD, "").replace(CROSS_NOTE, "");
  }
  const rest = raw.replace(PAIR, (match: string, tag: string, body: string) => {
    const value = body.trim();
    switch (tag) {
      case "command-name":
        parts.commands.push({ name: value, args: "" });
        break;
      case "command-args":
        // 参数归属最近一条命令；先于命令名出现（顺序在不同版本里变过）时补一条空壳，
        // 免得参数凭空消失。
        if (value) {
          const last = parts.commands[parts.commands.length - 1];
          if (last && !last.args) last.args = value;
          else parts.commands.push({ name: "", args: value });
        }
        break;
      // `! 命令`：命令本身显示成徽章「! ssh-copy-id …」，和斜杠命令同一种样子。
      case "bash-input":
        if (value) parts.commands.push({ name: "!", args: value });
        break;
      case "bash-stdout":
      case "bash-stderr":
      case "local-command-stderr":
      case "local-command-stdout": {
        // 终端转义剥掉后，被颜色码「挡住」而幸存的前导缩进/对齐空白要保留（剥完再 trim
        // 会把对齐空格吃掉）。trim 只用来判空——纯控制码的一段不该收成空输出块。
        const stripped = value.replace(ANSI, "");
        if (stripped.trim()) {
          parts.stdout.push(stripped);
          place(match, "stdout", tag, stripped);
        }
        break;
      }
      case "task-notification":
        if (value) {
          parts.notifications.push(value);
          place(match, "notification", tag, value);
        }
        break;
      // command-message 是命令的短描述（"clear" 之于 /clear），与命令名重复；
      // caveat 是写给模型的免责声明、forked-skill-launch 是机器记录，对人零信息量。都丢。
      default:
        break;
    }
    return "";
  });
  // 通知消息的前导段（[SYSTEM NOTIFICATION - NOT USER INPUT] …）：写给模型的防误读
  // 声明，对人零信息量，随通知一起收走（只删到段落边界，防御性保留其后内容）。
  const cleaned = parts.notifications.length
    ? rest.replace(/\[SYSTEM NOTIFICATION - NOT USER INPUT\][\s\S]*?(?=\n\s*\n|$)/, "")
    : rest;
  // 已知标签处理完还剩下的机器标签（通知后面跟着的 <system-reminder> 一类）：同样收成系统记录。
  parts.text = cleaned.replace(STRAY, "").replace(MACHINE_BLOCK, (match: string, tag: string, body: string) => {
    const record = { tag, body: body.replace(ANSI, "").trim() };
    parts.records.push(record);
    place(match, "record", tag, record.body);
    return "";
  }).trim();
  // 技能/后台命令的另一种落盘形态：命令行是**裸文本**（无 <command-name> 包裹），
  // 旁边跟着 caveat/stdout/forked-skill-launch。把首行的「/xxx [args]」提升为命令，
  // 与 <command-name> 形态同样渲染成徽章（实拍反馈「/code-review 没渲染好」）。
  // 只在本地包裹语境里做——用户在输入框正常打的「/xxx」是发给 CLI 的正文，不动。
  // 跨会话消息语境里不做这一步：剥完包裹后的残余正文若碰巧以 "/" 开头（对方消息被
  // 前导句截断的尾巴），会被误提升成一条并不存在的斜杠命令。
  if (parts.commands.length === 0 && parts.crossMessages.length === 0 && parts.text.startsWith("/")) {
    const [first, ...restLines] = parts.text.split("\n");
    const matched = /^\/(\S+)(?:\s+(.*))?$/.exec(first.trim());
    if (matched) {
      parts.commands.push({ name: `/${matched[1]}`, args: matched[2]?.trim() ?? "" });
      parts.text = restLines.join("\n").trim();
    }
  }
  return finish();
}
