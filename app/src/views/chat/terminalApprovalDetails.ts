/// 终端里审批提示的详情解析（屏幕文法由各 agent 真机/源码取证）：从屏幕文本里拆出工具、
/// 命令、说明与问句，交给审批卡展示。用哪种解析器由插件随 AttentionPattern.details 声明。
/// 纯函数，从 ChatWindow 搬出以便单独测试。

const PROCEED_QUESTION = /^do you want to proceed\?$/i;

/// 「proceed_box」框式审批详情（claude 真机取证的屏幕文法）。用哪种解析器由插件随
/// AttentionPattern.details 声明，前端不再按 pattern id 猜。
export function proceedBoxApprovalDetails(text: string) {
  // TUI 把标题行用横线铺满整屏宽（"Do you want to proceed? ──────"）。这些框线是
  // 排版，不是内容：行尾的要剥掉，整行都是框线的整行丢弃。
  const lines = text.split("\n")
    // U+2500–U+257F 是 Unicode 的制表符块（─ ━ │ ┌ ╌ … TUI 画框全用它）。
    .map((line) => line.replace(/[\s─-╿]+$/u, "").trim())
    .filter((line) => line && !/^[\s─-╿|+-]+$/u.test(line));
  // 问句是审批框的末行，它之后的东西不属于这次请求。同样取**最后一次**出现——重绘
  // 残留会让同一句问话在缓冲里出现好几遍，取第一处就会把两屏内容都收进命令区。
  const questionIndex = lines.reduce((found, line, index) => (PROCEED_QUESTION.test(line) ? index : found), -1);
  const question = questionIndex >= 0 ? lines[questionIndex] : "";
  let before = questionIndex >= 0 ? lines.slice(0, questionIndex) : lines;
  const marker = before.findIndex((line) => /this command requires approval/i.test(line));
  if (marker >= 0) before = before.slice(0, marker);
  // 审批框以工具头（"Bash command"）开头；只取最后一个工具头之后的内容，
  // 避免把上一屏残留的输出并进命令文本。
  // 标题有变体(2.1.304 源码):「Bash command (unsandboxed)」「Bash command (runs on …)」,
  // Windows 上还有「PowerShell command」——只认裸「Bash command」会让这些框认不出边界。
  const HEADER = /^(bash|powershell) command(?: \(.*\))?$/i;
  const header = before.reduce((found, line, index) => (HEADER.test(line) ? index : found), -1);
  const tool = header >= 0 && /^powershell/i.test(before[header]) ? "PowerShell" : "Bash";
  if (header >= 0) before = before.slice(header + 1);
  before = before.filter((line) => !PROCEED_QUESTION.test(line));
  // 末行是「用途说明」这个判断，只有认出了框的边界（工具头或 requires-approval 行）
  // 才成立。认不出时整段进命令区，不去猜哪一行是说明——猜错就是把半条命令当成说明
  // 单独拎出来显示，比不显示更误导。
  const framed = header >= 0 || marker >= 0;
  const description = framed && before.length >= 2 ? before[before.length - 1] : "";
  return {
    tool,
    // 长命令会按终端宽度硬换行成多行；除末行（用途说明）外全部并入命令整段显示，
    // 不能按「倒数第二行是命令」取——那只会摘到换行后的最后一个片段。
    command: (description ? before.slice(0, -1) : before).join("\n"),
    description,
    question,
  };
}

/// 「arrow_panel」面板式审批详情（kimi 官方源码取证 apps/kimi-code/.../approval-panel.ts @ 0.29）：
/// 标题行 ▶ <按工具定制的问题>（Run this command? / Write this file? / Approve X?），
/// 之下是命令/diff 等 display 块。详情区剥掉框线与标题，正文整段进 pre；
/// 工具名从标题反推，反推不到就留空（工具行整体不显示，比显示一个猜的名字诚实）。
export function arrowPanelApprovalDetails(text: string) {
  const lines = text.split("\n").map((line) => line.trim()).filter(Boolean)
    .filter((line) => !/^[─━═|-]+$/.test(line));
  const headerIndex = lines.findIndex((line) => /^▶\s*[^\n]+\?$/.test(line));
  const question = headerIndex >= 0 ? lines[headerIndex].replace(/^▶\s*/, "") : "";
  const command = (headerIndex >= 0 ? lines.slice(headerIndex + 1) : lines).join("\n");
  const tool = /run this command/i.test(question) ? "Bash"
    : /write this file/i.test(question) ? "Write"
    : /apply these edits/i.test(question) ? "Edit"
    : /stop this task/i.test(question) ? "TaskStop"
    : /^approve ([^\n?]+)\?$/i.exec(question)?.[1] ?? "";
  return { tool, command, description: "", question };
}

/// 「framed_box」框线对话框式审批详情（gemini 0.6x 真机取证 + 源码 ToolConfirmationMessage）：
/// 外框首行「? <工具名>  <说明>」，内框是命令/diff 正文，其后是问句（Allow execution of
/// [Shell]? / Apply this change? / Do you want to proceed?），再往下是编号选项（另由按钮承载）。
export function framedBoxApprovalDetails(text: string) {
  const lines = text.split("\n")
    .filter((line) => /^\s*│/.test(line))
    .map((line) => line.replace(/^\s*│ ?/, "").replace(/\s*│\s*$/, ""));
  const header = lines.findIndex((line) => /^\s*\?\s+\S/.test(line));
  const tool = header >= 0 ? lines[header].trim().replace(/^\?\s+/, "").split(/\s{2,}/)[0] : "";
  const questionIndex = lines.findIndex((line, index) => index > header
    && /^\s*(?:allow execution of|apply this change\?|do you want to proceed\?)/i.test(line));
  const question = questionIndex >= 0 ? lines[questionIndex].trim() : "";
  // 正文在工具头与问句之间；内框的上下边整行丢，左右边逐行剥。
  const command = lines.slice(header + 1, questionIndex >= 0 ? questionIndex : undefined)
    .filter((line) => !/^\s*[╭╰]/.test(line))
    .map((line) => line.replace(/^\s*│ ?/, "").replace(/\s*│\s*$/, ""))
    .join("\n")
    .trim();
  return { tool, command, description: "", question };
}
